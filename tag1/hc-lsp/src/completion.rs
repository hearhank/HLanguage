//! Auto-completion engine for LSP
//!
//! This module provides completion suggestions for H language code.

use crate::symbol::SymbolTable;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind};

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
    "using",
    "script",
    "comptime",
    "test",
    "extern",
    "export",
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
                let kind = match symbol.kind {
                    crate::symbol::SymbolKind::Function => CompletionItemKind::FUNCTION,
                    crate::symbol::SymbolKind::Class => CompletionItemKind::CLASS,
                    crate::symbol::SymbolKind::Enum => CompletionItemKind::ENUM,
                    crate::symbol::SymbolKind::Interface => CompletionItemKind::INTERFACE,
                    crate::symbol::SymbolKind::Variable => CompletionItemKind::VARIABLE,
                    crate::symbol::SymbolKind::Constant => CompletionItemKind::CONSTANT,
                    crate::symbol::SymbolKind::Namespace => CompletionItemKind::MODULE,
                    crate::symbol::SymbolKind::Field => CompletionItemKind::FIELD,
                    crate::symbol::SymbolKind::Method => CompletionItemKind::METHOD,
                };

                CompletionItem {
                    label: symbol.name.clone(),
                    kind: Some(kind),
                    detail: Some(format!("{:?}", symbol.kind)),
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
}

impl Default for CompletionEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyword_completions() {
        let engine = CompletionEngine::new();
        let completions = engine.get_keyword_completions("f");

        // Should include "fn", "for", "f16", "f32", "f64", "f128", "false"
        assert!(!completions.is_empty());
        assert!(completions.iter().any(|c| c.label == "fn"));
        assert!(completions.iter().any(|c| c.label == "for"));
        assert!(completions.iter().any(|c| c.label == "false"));
    }

    #[test]
    fn test_keyword_completions_empty_prefix() {
        let engine = CompletionEngine::new();
        let completions = engine.get_keyword_completions("");

        // Should include all keywords
        assert_eq!(completions.len(), KEYWORDS.len());
    }

    #[test]
    fn test_keyword_completions_no_match() {
        let engine = CompletionEngine::new();
        let completions = engine.get_keyword_completions("xyz");

        // Should be empty
        assert!(completions.is_empty());
    }
}
