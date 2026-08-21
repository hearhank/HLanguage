//! Symbol table for LSP features (go-to-definition, hover, completion)
//!
//! This module provides symbol table construction and management for the LSP server.

use hc::ast::{Decl, Expr, Program, Stmt};
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

    /// Build symbol table from AST
    pub fn build_from_ast(program: &Program, uri: Url) -> Self {
        let mut table = SymbolTable::new();

        // Collect symbols from declarations
        for decl in &program.decls {
            collect_decl_symbols(decl, &uri, None, &mut table);
        }

        table
    }
}

/// Collect symbols from a declaration
fn collect_decl_symbols(decl: &Decl, uri: &Url, container: Option<&str>, table: &mut SymbolTable) {
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
            });

            // Add parameter symbols (if we want to track them)
            for param in params {
                table.add(Symbol {
                    name: param.name.clone(),
                    kind: SymbolKind::Variable,
                    location: span_to_location(&param.span, uri.clone()),
                    container: Some(name.clone()),
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
            });

            // Add field symbols
            for field in fields {
                table.add(Symbol {
                    name: field.name.clone(),
                    kind: SymbolKind::Field,
                    location: span_to_location(&field.span, uri.clone()),
                    container: Some(name.clone()),
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
            });

            // Add variant symbols
            for variant in variants {
                table.add(Symbol {
                    name: variant.name.clone(),
                    kind: SymbolKind::Constant,
                    location: span_to_location(&variant.span, uri.clone()),
                    container: Some(name.clone()),
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
            });
        }
        Decl::Global { name, span, .. } => {
            // Add global variable symbol
            table.add(Symbol {
                name: name.clone(),
                kind: SymbolKind::Variable,
                location: span_to_location(span, uri.clone()),
                container: container.map(|s| s.to_string()),
            });
        }
        Decl::Const { name, span, .. } => {
            // Add constant symbol
            table.add(Symbol {
                name: name.clone(),
                kind: SymbolKind::Constant,
                location: span_to_location(span, uri.clone()),
                container: container.map(|s| s.to_string()),
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
            });

            // Recursively collect symbols from namespace members
            for member_decl in decls {
                collect_decl_symbols(member_decl, uri, Some(name), table);
            }
        }
        _ => {}
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
mod tests {
    use super::*;
    use hc::parse_source;

    fn make_url() -> Url {
        Url::parse("file:///test.hc").unwrap()
    }

    #[test]
    fn test_symbol_table_function() {
        let source = r#"
            fn add(a: i32, b: i32) i32 {
                return a + b;
            }
        "#;

        let program = parse_source(source).unwrap();
        let table = SymbolTable::build_from_ast(&program, make_url());

        // Should find function symbol
        let symbols = table.find("add").unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].kind, SymbolKind::Function);
        assert_eq!(symbols[0].name, "add");

        // Should find parameter symbols
        let param_a = table.find("a").unwrap();
        assert_eq!(param_a.len(), 1);
        assert_eq!(param_a[0].kind, SymbolKind::Variable);

        let param_b = table.find("b").unwrap();
        assert_eq!(param_b.len(), 1);
        assert_eq!(param_b[0].kind, SymbolKind::Variable);
    }

    #[test]
    fn test_symbol_table_class() {
        let source = r#"
            class Point {
                x: f32,
                y: f32,

                fn dist(self: *Point, other: *Point) f32 {
                    return 0.0;
                }
            }
        "#;

        let program = parse_source(source).unwrap();
        let table = SymbolTable::build_from_ast(&program, make_url());

        // Should find class symbol
        let symbols = table.find("Point").unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].kind, SymbolKind::Class);

        // Should find field symbols
        let field_x = table.find("x").unwrap();
        assert_eq!(field_x.len(), 1);
        assert_eq!(field_x[0].kind, SymbolKind::Field);

        let field_y = table.find("y").unwrap();
        assert_eq!(field_y.len(), 1);
        assert_eq!(field_y[0].kind, SymbolKind::Field);
    }

    #[test]
    fn test_symbol_table_enum() {
        let source = r#"
            enum Color {
                red,
                green,
                blue,
            }
        "#;

        let program = parse_source(source).unwrap();
        let table = SymbolTable::build_from_ast(&program, make_url());

        // Should find enum symbol
        let symbols = table.find("Color").unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].kind, SymbolKind::Enum);

        // Should find variant symbols
        let red = table.find("red").unwrap();
        assert_eq!(red.len(), 1);
        assert_eq!(red[0].kind, SymbolKind::Constant);
    }

    #[test]
    fn test_symbol_table_namespace() {
        let source = r#"
            namespace math {
                fn sqrt(x: f64) f64 {
                    return 0.0;
                }
            }
        "#;

        let program = parse_source(source).unwrap();
        let table = SymbolTable::build_from_ast(&program, make_url());

        // Should find namespace symbol
        let symbols = table.find("math").unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].kind, SymbolKind::Namespace);

        // Should find function symbol inside namespace
        let sqrt = table.find("sqrt").unwrap();
        assert_eq!(sqrt.len(), 1);
        assert_eq!(sqrt[0].kind, SymbolKind::Function);
        assert_eq!(sqrt[0].container, Some("math".to_string()));
    }
}
