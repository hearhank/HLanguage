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

// ---------- M2.6 错误码表验收 ----------

use hc::error_code_table;

#[test]
fn m26_encode_decode_roundtrip() {
    // 编码 = 高位 16 位包 ID + 低位 16 位包内码（L5 定案）
    let code = hc::ErrorCodeTable::encode(0x0007, 0x002A);
    assert_eq!(code, 0x0007_002A);
    assert_eq!(hc::ErrorCodeTable::package_of(code), 0x0007);
    assert_eq!(hc::ErrorCodeTable::index_of(code), 0x002A);
    // 包 ID 0：码 = 包内码（tag1 单包）
    assert_eq!(hc::ErrorCodeTable::encode(0, 3), 3);
    assert_eq!(hc::ErrorCodeTable::package_of(3), 0);
    assert_eq!(hc::ErrorCodeTable::index_of(3), 3);
}

#[test]
fn m26_same_name_same_code() {
    // 同名错误 → 同一码（全局唯一，Q13）
    let src = r#"
const FileError = error{ NotFound };
fn f() FileError!i32 {
    return error.NotFound;
}
fn g() FileError!i32 {
    return error.NotFound;
}
test fn t() !void {
    var x = error.NotFound;
}
"#;
    let program = parse_source(src).expect("parse");
    let table = error_code_table(&program);
    assert_eq!(table.len(), 1, "同名错误应合并为一条");
    let c1 = table.code_of("NotFound").expect("NotFound 有码");
    // 声明序 = 0（首个出现）
    assert_eq!(c1, 0);
}

#[test]
fn m26_distinct_names_distinct_codes() {
    // 不同错误名 → 不同码；包内序按声明序递增
    let src = r#"
const E1 = error{ A, B, C };
fn f() E1!i32 {
    return error.B;
}
"#;
    let program = parse_source(src).expect("parse");
    let table = error_code_table(&program);
    assert_eq!(table.len(), 3);
    assert_eq!(table.code_of("A"), Some(0));
    assert_eq!(table.code_of("B"), Some(1));
    assert_eq!(table.code_of("C"), Some(2));
}

#[test]
fn m26_collects_all_three_sources() {
    // 三类来源全收集：错误集声明成员 / error.X 字面量 / switch 模式
    let src = r#"
const E1 = error{ NotFound };
fn f() E1!i32 {
    return error.NotFound;
}
fn g(e: anyerror!i32) i32 {
    return e catch |err| switch (err) {
        error.Timeout => 0,
        else => 1,
    };
}
"#;
    let program = parse_source(src).expect("parse");
    let table = error_code_table(&program);
    assert!(table.code_of("NotFound").is_some(), "错误集声明成员收集");
    assert!(table.code_of("Timeout").is_some(), "switch 模式收集");
    assert_eq!(table.len(), 2);
}

#[test]
fn m26_span_recorded() {
    // 每个错误记录首次出现位置（原始错误定位）
    let src = "const E1 = error{ NotFound };\nfn f() E1!i32 {\n    return error.NotFound;\n}\n";
    let program = parse_source(src).expect("parse");
    let table = error_code_table(&program);
    // NotFound 首次出现在第 1 行（错误集声明）
    let sp = table.span_of("NotFound").expect("有位置");
    assert_eq!(sp.line, 1);
    // 位置有效（行列非零）
    assert!(sp.line >= 1 && sp.col >= 1);
}

#[test]
fn m26_span_uses_first_occurrence() {
    // 同名多处以首次出现为准（先字面量后声明）
    let src =
        "fn f() anyerror!i32 {\n    return error.First;\n}\nconst E2 = error{ First, Second };\n";
    let program = parse_source(src).expect("parse");
    let table = error_code_table(&program);
    let sp = table.span_of("First").expect("有位置");
    assert_eq!(sp.line, 2, "首次出现位置 = return 所在行");
    let sp2 = table.span_of("Second").expect("Second 有位置");
    assert_eq!(sp2.line, 4, "Second 首次出现在声明行");
}

#[test]
fn m26_reverse_lookup() {
    let src = "const E1 = error{ Alpha, Beta };\n";
    let program = parse_source(src).expect("parse");
    let table = error_code_table(&program);
    assert_eq!(table.name_of(0), Some("Alpha"));
    assert_eq!(table.name_of(1), Some("Beta"));
    assert_eq!(table.name_of(2), None, "未分配码无名字");
    // 跨包码（高位非本包）→ 无名
    assert_eq!(table.name_of(0x0001_0000), None);
}

#[test]
fn m26_stable_order_across_visits() {
    // 声明序稳定：同一程序重复收集结果一致（可复现/回归基准）
    let src = r#"
const E1 = error{ X, Y };
fn f() E1!i32 { return error.Y; }
const E2 = error{ Z };
"#;
    let p1 = parse_source(src).expect("parse");
    let p2 = parse_source(src).expect("parse 2");
    let t1 = error_code_table(&p1);
    let t2 = error_code_table(&p2);
    let v1: Vec<(String, u32)> = t1.entries().map(|e| (e.name.clone(), e.code)).collect();
    let v2: Vec<(String, u32)> = t2.entries().map(|e| (e.name.clone(), e.code)).collect();
    assert_eq!(v1, v2);
    assert_eq!(v1, vec![("X".into(), 0), ("Y".into(), 1), ("Z".into(), 2)]);
}

#[test]
fn m26_package_id_encoded() {
    // 包 ID 参与编码（高位）——跨包不冲突（L5）
    let mut t = hc::ErrorCodeTable::new(0x0002);
    t.register("A", &hc::token::Span::new(1, 1, 1, 1));
    t.register("B", &hc::token::Span::new(1, 1, 1, 1));
    assert_eq!(t.code_of("A"), Some(0x0002_0000));
    assert_eq!(t.code_of("B"), Some(0x0002_0001));
    assert_eq!(
        hc::ErrorCodeTable::package_of(t.code_of("A").unwrap()),
        0x0002
    );
}
