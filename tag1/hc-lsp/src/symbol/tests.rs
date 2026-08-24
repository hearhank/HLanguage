//! hc-lsp/src/symbol/tests.rs
//!
//! 定义：枚举：Color

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
