//! Symbol table for LSP features (go-to-definition, hover, completion)
//!
//! This module provides symbol table construction and management for the LSP server.

use hc::ast::{Decl, Program, Type};
use hc::Span;
use std::collections::HashMap;
use tower_lsp::lsp_types::{Location, Position, Range, Url};

/// Symbol kind (for LSP SymbolKind)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Class,
    Enum,
    Interface,
    Variable,
    Constant,
    Namespace,
    Field,
    Method,
}

impl From<SymbolKind> for tower_lsp::lsp_types::SymbolKind {
    fn from(kind: SymbolKind) -> Self {
        use tower_lsp::lsp_types::SymbolKind as LspSymbolKind;
        match kind {
            SymbolKind::Function => LspSymbolKind::FUNCTION,
            SymbolKind::Class => LspSymbolKind::CLASS,
            SymbolKind::Enum => LspSymbolKind::ENUM,
            SymbolKind::Interface => LspSymbolKind::INTERFACE,
            SymbolKind::Variable => LspSymbolKind::VARIABLE,
            SymbolKind::Constant => LspSymbolKind::CONSTANT,
            SymbolKind::Namespace => LspSymbolKind::NAMESPACE,
            SymbolKind::Field => LspSymbolKind::FIELD,
            SymbolKind::Method => LspSymbolKind::METHOD,
        }
    }
}

/// Doc comment run: a consecutive block of `///` lines
struct DocRun {
    end: usize,
    text: String,
}

/// Collect all `///` doc comment runs from source
fn collect_doc_runs(src: &str) -> Vec<DocRun> {
    let mut runs = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    let mut cur_end = 0usize;
    let mut pos = 0usize;
    for line in src.split_inclusive('\n') {
        let t = line.trim();
        pos += line.len();
        if let Some(rest) = t.strip_prefix("///") {
            cur.push(rest.trim_start_matches(' ').to_string());
            cur_end = pos;
        } else if !cur.is_empty() {
            runs.push(DocRun {
                end: cur_end,
                text: cur.join("\n"),
            });
            cur = Vec::new();
        }
    }
    if !cur.is_empty() {
        runs.push(DocRun {
            end: cur_end,
            text: cur.join("\n"),
        });
    }
    runs
}

/// Check if the gap between a doc comment end and a declaration start
/// contains only whitespace, `pub`, and `[...]` attributes.
fn gap_is_doc_prefix(gap: &str) -> bool {
    let b = gap.as_bytes();
    let mut pos = 0usize;
    loop {
        while pos < b.len() && (b[pos] as char).is_whitespace() {
            pos += 1;
        }
        if pos >= b.len() {
            return true;
        }
        if gap[pos..].starts_with("pub")
            || gap[pos..].starts_with("export")
            || gap[pos..].starts_with("extern")
        {
            pos += 3;
            continue;
        }
        if gap[pos..].starts_with("export") {
            pos += 6;
            continue;
        }
        if gap[pos..].starts_with("extern") {
            pos += 6;
            continue;
        }
        // Skip attribute annotations like [test], [Continuous], etc.
        if b[pos] == b'[' {
            pos += 1;
            let mut depth = 1;
            while pos < b.len() && depth > 0 {
                if b[pos] == b'[' {
                    depth += 1;
                } else if b[pos] == b']' {
                    depth -= 1;
                }
                pos += 1;
            }
            continue;
        }
        return false;
    }
}

/// Find the doc comment right before a declaration start position
fn doc_before(src: &str, runs: &[DocRun], decl_start: usize) -> Option<String> {
    let mut best: Option<usize> = None;
    for (i, r) in runs.iter().enumerate() {
        if r.end <= decl_start && gap_is_doc_prefix(&src[r.end..decl_start]) {
            if best.map_or(true, |b| runs[b].end < r.end) {
                best = Some(i);
            }
        }
    }
    best.map(|i| runs[i].text.clone())
}

/// Generate a signature string for a declaration
fn decl_signature(decl: &Decl) -> Option<String> {
    match decl {
        Decl::Fn {
            name, params, ret, ..
        } => {
            let params_str: Vec<String> = params
                .iter()
                .map(|p| {
                    if let Type::Infer = &p.ty {
                        format!("{}: anytype", p.name)
                    } else {
                        format!("{}: {}", p.name, format_type(&p.ty))
                    }
                })
                .collect();
            let ret_str = match ret {
                Some(ty) => format!(" {}", format_type(ty)),
                None => String::new(),
            };
            Some(format!("fn {}({}){}", name, params_str.join(", "), ret_str))
        }
        Decl::Class { name, .. } => Some(format!("class {}", name)),
        Decl::Enum { name, .. } => Some(format!("enum {}", name)),
        Decl::Interface { name, .. } => Some(format!("interface {}", name)),
        Decl::Namespace { name, .. } => Some(format!("namespace {}", name)),
        Decl::Global { name, ty, .. } => {
            if let Some(ty) = ty {
                Some(format!("global {}: {}", name, format_type(ty)))
            } else {
                Some(format!("global {}", name))
            }
        }
        Decl::Const { name, ty, .. } => {
            if let Some(ty) = ty {
                Some(format!("const {}: {}", name, format_type(ty)))
            } else {
                Some(format!("const {}", name))
            }
        }
        _ => None,
    }
}

/// Generate a type info string for a declaration
fn decl_type_info(decl: &Decl) -> Option<String> {
    match decl {
        Decl::Fn {
            name: _,
            params,
            ret,
            ..
        } => {
            let params_str: Vec<String> = params
                .iter()
                .map(|p| {
                    if let Type::Infer = &p.ty {
                        "anytype".to_string()
                    } else {
                        format_type(&p.ty)
                    }
                })
                .collect();
            let ret_str = match ret {
                Some(ty) => format!(" -> {}", format_type(ty)),
                None => String::new(),
            };
            Some(format!("fn({}){}", params_str.join(", "), ret_str))
        }
        Decl::Class { name, fields, .. } => {
            let fields_str: Vec<String> = fields
                .iter()
                .map(|f| format!("{}: {}", f.name, format_type(&f.ty)))
                .collect();
            Some(format!("class {} {{ {} }}", name, fields_str.join(", ")))
        }
        Decl::Enum { name, variants, .. } => {
            let variants_str: Vec<String> = variants.iter().map(|v| v.name.clone()).collect();
            Some(format!("enum {} {{ {} }}", name, variants_str.join(", ")))
        }
        Decl::Interface { name, .. } => Some(format!("interface {}", name)),
        Decl::Namespace { name, .. } => Some(format!("namespace {}", name)),
        Decl::Global { name, ty, .. } => {
            if let Some(ty) = ty {
                Some(format!("global {}: {}", name, format_type(ty)))
            } else {
                None
            }
        }
        Decl::Const { name, ty, .. } => {
            if let Some(ty) = ty {
                Some(format!("const {}: {}", name, format_type(ty)))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Symbol information
#[derive(Debug, Clone)]
pub struct Symbol {
    /// Symbol name
    pub name: String,
    /// Symbol kind
    pub kind: SymbolKind,
    /// Definition location (file URI, range)
    pub location: Location,
    /// Containing symbol (for scoped symbols)
    pub container: Option<String>,
    /// Doc comment (/// lines)
    pub doc: Option<String>,
    /// Type information (e.g. "fn(i32, i32) -> i32")
    pub type_info: Option<String>,
    /// Full signature (e.g. "fn foo(x: i32, y: i32) i32")
    pub signature: Option<String>,
}

impl Symbol {
    /// Get a human-readable kind name
    pub fn kind_name(&self) -> String {
        match self.kind {
            SymbolKind::Function => "function".to_string(),
            SymbolKind::Class => "class".to_string(),
            SymbolKind::Enum => "enum".to_string(),
            SymbolKind::Interface => "interface".to_string(),
            SymbolKind::Variable => "variable".to_string(),
            SymbolKind::Constant => "constant".to_string(),
            SymbolKind::Namespace => "namespace".to_string(),
            SymbolKind::Field => "field".to_string(),
            SymbolKind::Method => "method".to_string(),
        }
    }
}

/// Symbol table (maps symbol name to symbol info)
#[derive(Debug, Clone, Default)]
pub struct SymbolTable {
    /// Symbols indexed by name
    symbols: HashMap<String, Vec<Symbol>>,
}

impl SymbolTable {
    /// Create a new empty symbol table
    pub fn new() -> Self {
        Self {
            symbols: HashMap::new(),
        }
    }

    /// Add a symbol to the table
    pub fn add(&mut self, symbol: Symbol) {
        self.symbols
            .entry(symbol.name.clone())
            .or_default()
            .push(symbol);
    }

    /// Find symbols by name
    pub fn find(&self, name: &str) -> Option<&Vec<Symbol>> {
        self.symbols.get(name)
    }

    /// Get all symbols
    pub fn all_symbols(&self) -> Vec<&Symbol> {
        self.symbols.values().flat_map(|v| v.iter()).collect()
    }

    /// Build symbol table from source (parses source, extracts doc comments)
    pub fn build_from_source(source: &str, uri: Url) -> Self {
        let mut table = SymbolTable::new();

        // Parse the source
        let program = match hc::parse_source(source) {
            Ok(p) => p,
            Err(_) => return table, // Return empty table on parse error
        };

        // Collect doc comment runs
        let runs = collect_doc_runs(source);

        // Collect symbols from declarations
        for decl in &program.decls {
            collect_decl_symbols(decl, &uri, None, &mut table, source, &runs);
        }

        table
    }

    /// Build symbol table from AST (backward compatible, no doc comments)
    pub fn build_from_ast(program: &Program, uri: Url) -> Self {
        let mut table = SymbolTable::new();
        let runs = Vec::new(); // Empty runs = no doc comments

        for decl in &program.decls {
            collect_decl_symbols(decl, &uri, None, &mut table, "", &runs);
        }

        table
    }
}

/// Get the start byte position of a declaration
fn decl_start(decl: &Decl) -> usize {
    match decl {
        Decl::Fn { span, .. }
        | Decl::Class { span, .. }
        | Decl::Enum { span, .. }
        | Decl::Interface { span, .. }
        | Decl::Namespace { span, .. }
        | Decl::Global { span, .. }
        | Decl::Const { span, .. } => span.start,
        Decl::Import { span, .. } => span.start,
        Decl::Using { span, .. } => span.start,
        Decl::Script { span, .. } => span.start,
        Decl::Comptime { span, .. } => span.start,
        _ => 0,
    }
}

/// Collect symbols from a declaration
fn collect_decl_symbols(
    decl: &Decl,
    uri: &Url,
    container: Option<&str>,
    table: &mut SymbolTable,
    source: &str,
    runs: &[DocRun],
) {
    let start = decl_start(decl);
    let doc = if !source.is_empty() {
        doc_before(source, runs, start)
    } else {
        None
    };
    let signature = decl_signature(decl);
    let type_info = decl_type_info(decl);

    match decl {
        Decl::Fn {
            name, span, params, ..
        } => {
            // Add function symbol
            table.add(Symbol {
                name: name.clone(),
                kind: SymbolKind::Function,
                location: span_to_location(span, uri.clone()),
                container: container.map(|s| s.to_string()),
                doc: doc.clone(),
                type_info: type_info.clone(),
                signature: signature.clone(),
            });

            // Add parameter symbols
            for param in params {
                let param_doc = if !source.is_empty() {
                    doc_before(source, runs, param.span.start)
                } else {
                    None
                };
                let param_sig = if let Type::Infer = &param.ty {
                    Some(format!("{}: anytype", param.name))
                } else {
                    Some(format!("{}: {}", param.name, format_type(&param.ty)))
                };
                table.add(Symbol {
                    name: param.name.clone(),
                    kind: SymbolKind::Variable,
                    location: span_to_location(&param.span, uri.clone()),
                    container: Some(name.clone()),
                    doc: param_doc,
                    type_info: Some(format_type(&param.ty)),
                    signature: param_sig,
                });
            }
        }
        Decl::Class {
            name, span, fields, ..
        } => {
            // Add class symbol
            table.add(Symbol {
                name: name.clone(),
                kind: SymbolKind::Class,
                location: span_to_location(span, uri.clone()),
                container: container.map(|s| s.to_string()),
                doc: doc.clone(),
                type_info: type_info.clone(),
                signature: signature.clone(),
            });

            // Add field symbols
            for field in fields {
                let field_doc = if !source.is_empty() {
                    doc_before(source, runs, field.span.start)
                } else {
                    None
                };
                table.add(Symbol {
                    name: field.name.clone(),
                    kind: SymbolKind::Field,
                    location: span_to_location(&field.span, uri.clone()),
                    container: Some(name.clone()),
                    doc: field_doc,
                    type_info: Some(format_type(&field.ty)),
                    signature: Some(format!("{}: {}", field.name, format_type(&field.ty))),
                });
            }
        }
        Decl::Enum {
            name,
            span,
            variants,
            ..
        } => {
            // Add enum symbol
            table.add(Symbol {
                name: name.clone(),
                kind: SymbolKind::Enum,
                location: span_to_location(span, uri.clone()),
                container: container.map(|s| s.to_string()),
                doc: doc.clone(),
                type_info: type_info.clone(),
                signature: signature.clone(),
            });

            // Add variant symbols
            for variant in variants {
                table.add(Symbol {
                    name: variant.name.clone(),
                    kind: SymbolKind::Constant,
                    location: span_to_location(&variant.span, uri.clone()),
                    container: Some(name.clone()),
                    doc: None,
                    type_info: Some(format!("enum {}::{}", name, variant.name)),
                    signature: Some(format!("{}::{}", name, variant.name)),
                });
            }
        }
        Decl::Interface { name, span, .. } => {
            // Add interface symbol
            table.add(Symbol {
                name: name.clone(),
                kind: SymbolKind::Interface,
                location: span_to_location(span, uri.clone()),
                container: container.map(|s| s.to_string()),
                doc: doc.clone(),
                type_info: type_info.clone(),
                signature: signature.clone(),
            });
        }
        Decl::Global { name, span, .. } => {
            table.add(Symbol {
                name: name.clone(),
                kind: SymbolKind::Variable,
                location: span_to_location(span, uri.clone()),
                container: container.map(|s| s.to_string()),
                doc: doc.clone(),
                type_info: type_info.clone(),
                signature: signature.clone(),
            });
        }
        Decl::Const { name, span, .. } => {
            table.add(Symbol {
                name: name.clone(),
                kind: SymbolKind::Constant,
                location: span_to_location(span, uri.clone()),
                container: container.map(|s| s.to_string()),
                doc: doc.clone(),
                type_info: type_info.clone(),
                signature: signature.clone(),
            });
        }
        Decl::Namespace {
            name, span, decls, ..
        } => {
            // Add namespace symbol
            table.add(Symbol {
                name: name.clone(),
                kind: SymbolKind::Namespace,
                location: span_to_location(span, uri.clone()),
                container: container.map(|s| s.to_string()),
                doc: doc.clone(),
                type_info: type_info.clone(),
                signature: signature.clone(),
            });

            // Recursively collect symbols from namespace members
            for member_decl in decls {
                collect_decl_symbols(member_decl, uri, Some(name), table, source, runs);
            }
        }
        _ => {}
    }
}

/// Format a Type for display (since Type doesn't implement Display)
fn format_type(ty: &Type) -> String {
    match ty {
        Type::Named(name, args) => {
            if args.is_empty() {
                name.clone()
            } else {
                let args_str: Vec<String> = args.iter().map(format_type).collect();
                format!("{}<{}>", name, args_str.join(", "))
            }
        }
        Type::Ptr(inner, mut_) => {
            if *mut_ {
                format!("*mut {}", format_type(inner))
            } else {
                format!("*{}", format_type(inner))
            }
        }
        Type::Slice(inner, mut_) => {
            if *mut_ {
                format!("&mut [{}]", format_type(inner))
            } else {
                format!("&[{}]", format_type(inner))
            }
        }
        Type::Optional(inner) => format!("?{}", format_type(inner)),
        Type::ErrorUnion(err, inner) => match err {
            Some(e) => format!("{}!{}", format_type(e), format_type(inner)),
            None => format!("!{}", format_type(inner)),
        },
        Type::Tuple(items) => {
            let items_str: Vec<String> = items.iter().map(format_type).collect();
            format!("({})", items_str.join(", "))
        }
        Type::Array(n, inner) => format!("[{}]{}", n, format_type(inner)),
        Type::ComptimeInt(n) => format!("{}", n),
        Type::Infer => "_".to_string(),
        Type::Owned(inner) => format!("o {}", format_type(inner)),
    }
}

/// Convert hc::Span to LSP Location
fn span_to_location(span: &Span, uri: Url) -> Location {
    // Convert 1-based to 0-based
    let start = Position::new(span.line.saturating_sub(1), span.col.saturating_sub(1));
    let end = Position::new(
        span.line.saturating_sub(1),
        span.col.saturating_sub(1) + (span.end - span.start) as u32,
    );

    Location::new(uri, Range::new(start, end))
}

#[cfg(test)]
mod tests;
