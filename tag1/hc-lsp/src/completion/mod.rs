//! LSP 自动补全：关键字、类型、`@` 内建、特性标注、符号点限定补全
//!
//! 依据：docs/SPEC/syntax/（01 §1.2.1 关键字表、03 §3.2 类型、04 §4.9 特性、13 内建全集）
//! 定义：结构体：CompletionEngine

use crate::symbol::SymbolTable;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, Documentation, InsertTextFormat, MarkupContent, MarkupKind,
};

/// H 语言关键字（01 §1.2.1，冻结的 45 个；与 tag1/hc/src/lexer 一致，
/// 含词法保留但声明级报错的 `script`——补全引擎会过滤它）
pub const KEYWORDS: &[&str] = &[
    // 声明
    "var",
    "const",
    "fn",
    "global",
    // 控制流
    "if",
    "else",
    "while",
    "for",
    "break",
    "continue",
    "return",
    "switch",
    "defer",
    "errdefer",
    // 类型构造
    "class",
    "struct",
    "enum",
    "union",
    "tree",
    "interface",
    "where",
    // 模块
    "namespace",
    "import",
    "pub",
    "export",
    // 所有权
    "owned",
    "move",
    "mut",
    // 操作
    "and",
    "or",
    "try",
    "catch",
    "orelse",
    // 元编程（script：块已移除，词法保留 + 声明级报错指引 .hs）
    "script",
    "comptime",
    "anytype",
    "type",
    // 并发
    "async",
    "await",
    "spawn",
    // 外部接口
    "extern",
    // 字面量
    "void",
    "null",
    "true",
    "false",
];

/// 补全时排除的关键字（词法保留但功能已移除/废弃）
pub const KEYWORD_BLOCKLIST: &[&str] = &["script"];

/// 内建类型名（03 §3.2 标量 + 04 §4.7 String + 内建容器/并发句柄）
pub const TYPES: &[&str] = &[
    // 整数（03 §3.2.1）
    "i8",
    "i16",
    "i32",
    "i64",
    "i128",
    "isize",
    "u8",
    "u16",
    "u32",
    "u64",
    "u128",
    "usize",
    // 浮点（03 §3.2.2，F1：f16/f32/f64 真实宽度）
    "f16",
    "f32",
    "f64",
    // 其它标量
    "bool",
    // comptime 类型名（03 §3.2.5）
    "comptime_int",
    "comptime_float",
    // 扩展/内建类型（04 §4.7/§4.8、11 §11.5）
    "String",
    "Vec",
    "Map",
    "Deque",
    "Table",
    "List",
    "Pair",
    "LinkedList",
    "Opt",
    "Chan",
    "Mutex",
    "Thread",
    "Future",
    "ExitType",
    "Allocator",
    "Arena",
];

/// `@` 内建函数全集（13-builtins.md）：名称 + 签名说明
pub const BUILTINS: &[(&str, &str)] = &[
    // 内省（13 §13.2）
    ("@sizeOf", "@sizeOf(T) usize — 类型字节大小"),
    ("@alignOf", "@alignOf(T) usize — 自然对齐"),
    ("@offsetOf", "@offsetOf(T, field) usize — 字段偏移"),
    ("@typeOf", "@typeOf(expr) String — 表达式类型名"),
    ("@intFromEnum", "@intFromEnum(e) 整数 — 枚举 → 底层值"),
    ("@enumFromInt", "@enumFromInt(i) 枚举 — 整数 → 枚举"),
    // 转换（13 §13.3）
    ("@intCast", "@intCast(T, v) T — 整数跨宽度显式转换"),
    ("@ptrCast", "@ptrCast(T, p) T — 指针类型转换"),
    ("@alignCast", "@alignCast(T, p) T — 指针对齐提升"),
    ("@ptrFromInt", "@ptrFromInt(i) 指针 — 整数 → 指针"),
    ("@intFromPtr", "@intFromPtr(p) 整数 — 指针 → 地址"),
    // 诊断（13 §13.4）
    (
        "@panic",
        "@panic(消息) never — 不可恢复终止（abort，无 unwind）",
    ),
    ("@compileError", "@compileError(消息) never — 编译期错误"),
    // 原子/volatile（13 §13.5，Q-S3）
    ("@atomicLoad", "@atomicLoad(T, p, order) T — 原子载入"),
    ("@atomicStore", "@atomicStore(T, p, v, order) — 原子存储"),
    (
        "@atomicRmw",
        "@atomicRmw(T, p, op, v, order) T — 原子读改写（返回旧值）",
    ),
    ("@volatileLoad", "@volatileLoad(p) — 易失载入"),
    ("@volatileStore", "@volatileStore(p, v) — 易失存储"),
    // 溢出算术（13 §13.6，Q-S6）
    (
        "@addWithOverflow",
        "@addWithOverflow(a, b) (T, bool) — 加法溢出原语",
    ),
    (
        "@subWithOverflow",
        "@subWithOverflow(a, b) (T, bool) — 减法溢出原语",
    ),
    (
        "@mulWithOverflow",
        "@mulWithOverflow(a, b) (T, bool) — 乘法溢出原语",
    ),
];

/// 特性标注（04 §4.9，ADR-0022 §5 + H2）——已实现集合
pub const ATTRIBUTES: &[(&str, &str, &str)] = &[
    (
        "test",
        "[test] / [test(\"名称\")] / [test(async)] / [test(thread)] / [test(timeout=N)]",
        "[test($1)] fn $2() !void {\n\t$0\n}",
    ),
    (
        "Inline",
        "[Inline] — 内联函数：所有调用点编译期展开",
        "[Inline] ",
    ),
    (
        "Extension",
        "[Extension(类型名)] — 为任意类型扩展方法（不能访问私有字段）",
        "[Extension($1)] ",
    ),
    (
        "Align",
        "[Align(n)] — 对齐（n ∈ 1, 2, 4, 8），struct 类型级/字段级",
        "[Align($1)] ",
    ),
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
    keywords: Vec<String>,
}

impl CompletionEngine {
    /// Create a new completion engine
    pub fn new() -> Self {
        Self {
            keywords: KEYWORDS.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Get keyword completions（排除 KEYWORD_BLOCKLIST：script 已移除等）
    pub fn get_keyword_completions(&self, prefix: &str) -> Vec<CompletionItem> {
        self.keywords
            .iter()
            .filter(|kw| kw.starts_with(prefix) && !KEYWORD_BLOCKLIST.contains(&kw.as_str()))
            .map(|kw| CompletionItem {
                label: kw.clone(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("keyword".to_string()),
                ..Default::default()
            })
            .collect()
    }

    /// Get builtin type completions (03 §3.2 / 04)
    pub fn get_type_completions(&self, prefix: &str) -> Vec<CompletionItem> {
        TYPES
            .iter()
            .filter(|ty| ty.starts_with(prefix))
            .map(|ty| CompletionItem {
                label: ty.to_string(),
                kind: Some(CompletionItemKind::STRUCT),
                detail: Some("builtin type".to_string()),
                ..Default::default()
            })
            .collect()
    }

    /// Get `@`-builtin completions (13-builtins.md) — `@` 触发后按名称/签名过滤
    pub fn get_builtin_completions(&self, prefix: &str) -> Vec<CompletionItem> {
        BUILTINS
            .iter()
            .filter(|(name, _)| name.starts_with(prefix))
            .map(|(name, detail)| CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::FUNCTION),
                detail: Some(detail.to_string()),
                documentation: Some(Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: format!("```hc\n{detail}\n```\n依据：`docs/SPEC/syntax/13-builtins.md`"),
                })),
                ..Default::default()
            })
            .collect()
    }

    /// Get attribute completions (04 §4.9) — `[` 触发后提供 snippet
    pub fn get_attribute_completions(&self, prefix: &str) -> Vec<CompletionItem> {
        ATTRIBUTES
            .iter()
            .filter(|(name, _, _)| name.starts_with(prefix))
            .map(|(name, detail, snippet)| CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::PROPERTY),
                detail: Some(detail.to_string()),
                insert_text: Some(snippet.to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
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

    /// Get all completions (keywords + types + symbols)
    pub fn get_completions(
        &self,
        symbol_table: Option<&SymbolTable>,
        prefix: &str,
    ) -> Vec<CompletionItem> {
        let mut completions = self.get_keyword_completions(prefix);
        completions.extend(self.get_type_completions(prefix));

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
