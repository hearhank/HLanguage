//! hc 编译器前端单元测试（M1/M2 验收：词法、解析、诊断）

use hc::lexer::lex;
use hc::parse_source;
use hc::token::TokenKind;

#[test]
fn lex_keywords_and_punct() {
    let toks = lex("fn main(io: Io) !void { var x: i32 = 42; }");
    let kinds: Vec<&TokenKind> = toks.iter().map(|t| &t.kind).collect();
    assert!(matches!(kinds[0], TokenKind::KwFn));
    assert!(matches!(kinds[1], TokenKind::Ident(n) if n == "main"));
    assert!(matches!(kinds[2], TokenKind::LParen));
    assert!(kinds.contains(&&TokenKind::KwVar));
    assert!(kinds.contains(&&TokenKind::KwVoid));
    assert!(kinds.contains(&&TokenKind::LBrace));
    assert!(kinds.contains(&&TokenKind::RBrace));
    assert!(kinds.contains(&&TokenKind::Semi));
}

#[test]
fn lex_number_bases_and_suffix() {
    let toks = lex("0xFF 0b1010 0o17 42i32 255u8 3.14f64 1_000");
    let texts: Vec<String> = toks
        .iter()
        .filter_map(|t| match &t.kind {
            TokenKind::Int(s) | TokenKind::Float(s) => Some(s.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        texts,
        vec!["0xFF", "0b1010", "0o17", "42i32", "255u8", "3.14f64", "1_000"]
    );
}

#[test]
fn lex_strings_and_comments() {
    let toks = lex("// line\n\"hello\\n\" \"\"\"raw\"\"\" 'x' /* block */");
    assert!(toks
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Str(s) if s == "hello\n")));
    assert!(toks
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::RawStr(s) if s == "raw")));
    assert!(toks
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Char(b) if *b == b'x')));
    // 注释不产生 token（除 EOF）
    assert_eq!(toks.last().map(|t| &t.kind), Some(&TokenKind::Eof));
}

#[test]
fn lex_at_builtin() {
    let toks = lex("@sizeOf(x) @intCast(u8, y)");
    assert!(toks
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::AtBuiltin(n) if n == "sizeOf")));
    assert!(toks
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::AtBuiltin(n) if n == "intCast")));
}

#[test]
fn lex_errors_unterminated() {
    let toks = lex("\"unterminated");
    assert!(toks.iter().any(|t| matches!(&t.kind, TokenKind::Error(_))));
}

#[test]
fn parse_hello_program() {
    let src = "fn main(io: Io) !void { io.print(\"hi\\n\"); }";
    let program = parse_source(src).expect("parse hello");
    assert_eq!(program.decls.len(), 1);
}

#[test]
fn parse_variables_and_ops() {
    let src = r#"
fn main(io: Io) !void {
    var mut x: i32 = 5;
    x += 1;
    var y = x * 2;
    io.print("{}\n", y);
}
"#;
    parse_source(src).expect("parse variables");
}

#[test]
fn parse_control_flow() {
    let src = r#"
fn f(n: i32) i32 {
    var total = 0;
    var i = 0;
    while (i < n) : (i += 1) {
        if (i % 2 == 0) total += i;
        else total -= i;
    }
    for (0..n) |k| {
        total += k;
    }
    switch (n) {
        1 => 10,
        else => 20,
    };
    return total;
}
"#;
    parse_source(src).expect("parse control flow");
}

#[test]
fn parse_class_enum_interface() {
    let src = r#"
[continuous]
class Point { x: f32, y: f32, fn dist(a: *Point, b: *Point) f32 { return 0; } }
enum Kind { player, enemy }
interface Shape { fn area(self: *Self) f32; }
class Rect: Shape { w: f32, h: f32, fn area(self: *Self) f32 { return self.w * self.h; } }
"#;
    let program = parse_source(src).expect("parse types");
    assert_eq!(program.decls.len(), 4);
}

#[test]
fn parse_test_fn_and_assertions() {
    let src = r#"
fn add(a: i32, b: i32) i32 { return a + b; }
test fn check() !void {
    try expect_eq(add(1, 2), 3);
    try expect(add(2, 3) == 5);
}
"#;
    parse_source(src).expect("parse test fn");
}

#[test]
fn parse_error_reports_position() {
    let src = "fn main(io: Io) !void {\n    var x: i32 = ;\n}";
    let err = parse_source(src).unwrap_err();
    assert!(err.iter().any(|d| d.span.line >= 2));
}

#[test]
fn parse_closures() {
    let src = r#"
fn apply(f: Fn1(i32) i32, x: i32) i32 { return f(x); }
fn main(io: Io) !void {
    var a = 10;
    var add_a = |v| v + a;
    io.print("{}\n", apply(add_a, 5));
}
"#;
    parse_source(src).expect("parse closures");
}

#[test]
fn parse_alloc_init_forms() {
    let src = r#"
class Foo { mut a: i32, b: i32 }
fn main(io: Io) !void {
    var x = alloc.init(Foo);
    x.a = 10;
    var y = alloc.init(Foo{ a = 1, b = 2 });
}
"#;
    parse_source(src).expect("parse alloc.init dual forms");
}
