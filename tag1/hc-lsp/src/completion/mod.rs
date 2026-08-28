//! LSP 自动补全：关键字、类型、符号点限定补全
//!
//! 定义：结构体：CompletionEngine

use crate::symbol::SymbolTable;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, Documentation, MarkupContent, MarkupKind,
};

/// H language keywords
pub const KEYWORDS: &[&str] = &[
    "fn",
    "var",
    "const",
    "global",
    "class",
    "enum",
    "interface",
    "namespace",
    "if",
    "else",
    "while",
    "for",
    "switch",
    "case",
    "default",
    "return",
    "break",
    "continue",
    "try",
    "catch",
    "defer",
    "errdefer",
    "true",
    "false",
    "null",
    "self",
    "i8",
    "i16",
    "i32",
    "i64",
    "i128",
    "iSize",
    "u8",
    "u16",
    "u32",
    "u64",
    "u128",
    "uSize",
    "f16",
    "f32",
    "f64",
    "f128",
    "bool",
    "void",
    "type",
    "anytype",
    "pub",
    "script",
    "comptime",
    "test",
    "extern",
    "export",
    "import",
    "namespace",
    "struct",
    "union",
    "move",
    "mut",
    "and",
    "or",
    "orelse",
    "async",
    "await",
    "spawn",
];

/// Standard library module names
pub const STD_MODULES: &[&str] = &[
    "io",
    "fs",
    "net",
    "time",
    "mem",
    "text",
    "rng",
    "serialize",
    "archive",
    "ipc",
    "storage",
];

/// Standard library module members (common functions/types)
pub const STD_MODULE_MEMBERS: &[(&str, &[&str])] = &[
    (
        "io",
        &[
            "print", "println", "read", "write", "stdin", "stdout", "stderr", "File", "Io",
        ],
    ),
    (
        "fs",
        &[
            "open", "create", "read", "write", "append", "rename", "remove", "list_dir", "exists",
            "File",
        ],
    ),
    (
        "net",
        &["Tcp", "Udp", "Http", "connect", "listen", "accept"],
    ),
    ("time", &["now", "sleep", "Duration", "Instant"]),
    ("mem", &["Allocator", "Arena", "alloc", "free", "realloc"]),
    (
        "text",
        &[
            "contains",
            "starts_with",
            "ends_with",
            "split",
            "trim",
            "to_upper",
            "to_lower",
            "replace",
        ],
    ),
    ("rng", &["xorshift64", "seed", "next"]),
    (
        "serialize",
        &[
            "json",
            "csv",
            "fmt_int",
            "fmt_float",
            "parse_int",
            "parse_float",
        ],
    ),
    ("archive", &["rle_encode", "rle_decode"]),
    ("ipc", &["pipe", "shm", "open", "close"]),
    ("storage", &["Store", "load", "save"]),
];

/// Completion engine
#[derive(Debug)]
pub struct CompletionEngine {
    /// Keywords
    keywords: Vec<String>,
}

impl CompletionEngine {
    /// Create a new completion engine
    pub fn new() -> Self {
        Self {
            keywords: KEYWORDS.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Get keyword completions
    pub fn get_keyword_completions(&self, prefix: &str) -> Vec<CompletionItem> {
        self.keywords
            .iter()
            .filter(|kw| kw.starts_with(prefix))
            .map(|kw| CompletionItem {
                label: kw.clone(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("keyword".to_string()),
                ..Default::default()
            })
            .collect()
    }

    /// Get symbol completions from symbol table
    pub fn get_symbol_completions(
        &self,
        symbol_table: &SymbolTable,
        prefix: &str,
    ) -> Vec<CompletionItem> {
        symbol_table
            .all_symbols()
            .iter()
            .filter(|symbol| symbol.name.starts_with(prefix))
            .map(|symbol| {
                let kind = symbol_kind_to_completion_kind(symbol.kind);
                CompletionItem {
                    label: symbol.name.clone(),
                    kind: Some(kind),
                    detail: Some(symbol.kind_name()),
                    documentation: symbol.doc.as_ref().map(|d| {
                        Documentation::MarkupContent(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: d.clone(),
                        })
                    }),
                    ..Default::default()
                }
            })
            .collect()
    }

    /// Get all completions (keywords + symbols)
    pub fn get_completions(
        &self,
        symbol_table: Option<&SymbolTable>,
        prefix: &str,
    ) -> Vec<CompletionItem> {
        let mut completions = self.get_keyword_completions(prefix);

        if let Some(table) = symbol_table {
            completions.extend(self.get_symbol_completions(table, prefix));
        }

        completions
    }

    /// Get dot-qualified completions for a namespace/class/module
    pub fn get_dot_qualified_completions(
        &self,
        namespace: &str,
        symbol_tables: &[&SymbolTable],
    ) -> Vec<CompletionItem> {
        let mut completions = Vec::new();

        // 1. Check standard library modules
        if let Some(members) = STD_MODULE_MEMBERS
            .iter()
            .find(|(name, _)| *name == namespace)
        {
            for member in members.1 {
                completions.push(CompletionItem {
                    label: member.to_string(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some(format!("std::{}", namespace)),
                    ..Default::default()
                });
            }
            return completions;
        }

        // 2. Check symbol tables for namespace/class members
        for table in symbol_tables {
            // Find the namespace/class symbol
            if let Some(symbols) = table.find(namespace) {
                for symbol in symbols {
                    match symbol.kind {
                        crate::symbol::SymbolKind::Namespace
                        | crate::symbol::SymbolKind::Class
                        | crate::symbol::SymbolKind::Enum
                        | crate::symbol::SymbolKind::Interface => {
                            // Found a namespace/class/enum/interface - collect its members
                            for member in table.all_symbols() {
                                if member.container.as_deref() == Some(namespace) {
                                    let kind = symbol_kind_to_completion_kind(member.kind);
                                    completions.push(CompletionItem {
                                        label: member.name.clone(),
                                        kind: Some(kind),
                                        detail: Some(format!("{}::{}", namespace, member.name)),
                                        documentation: member.doc.as_ref().map(|d| {
                                            Documentation::MarkupContent(MarkupContent {
                                                kind: MarkupKind::Markdown,
                                                value: d.clone(),
                                            })
                                        }),
                                        ..Default::default()
                                    });
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        completions
    }
}

impl Default for CompletionEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert SymbolKind to CompletionItemKind
fn symbol_kind_to_completion_kind(kind: crate::symbol::SymbolKind) -> CompletionItemKind {
    match kind {
        crate::symbol::SymbolKind::Function => CompletionItemKind::FUNCTION,
        crate::symbol::SymbolKind::Class => CompletionItemKind::CLASS,
        crate::symbol::SymbolKind::Enum => CompletionItemKind::ENUM,
        crate::symbol::SymbolKind::Interface => CompletionItemKind::INTERFACE,
        crate::symbol::SymbolKind::Variable => CompletionItemKind::VARIABLE,
        crate::symbol::SymbolKind::Constant => CompletionItemKind::CONSTANT,
        crate::symbol::SymbolKind::Namespace => CompletionItemKind::MODULE,
        crate::symbol::SymbolKind::Field => CompletionItemKind::FIELD,
        crate::symbol::SymbolKind::Method => CompletionItemKind::METHOD,
    }
}

#[cfg(test)]
mod tests;
