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

#[test]
fn parse_array_trailing_comma() {
    // ArrayLit 尾逗号（M7.2 build.zon `files`/`deps` 数组依赖）
    let src = r#"
const build = Build{
    files = [ "a.hc", "b.hc", ],
    deps = [ Pkg{ name = "x", path = "../x" }, ],
};
"#;
    parse_source(src).expect("parse array trailing comma");
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

// ---------- M2.4 所有权编译时检查验收 ----------

/// 断言语义检查产生含指定片段的错误
fn check_has_error(src: &str, frag: &str) {
    let program = parse_source(src).expect("parse should succeed");
    let diags = hc::check_semantics(&program);
    assert!(
        diags
            .iter()
            .any(|d| d.is_error() && d.message.contains(frag)),
        "期望错误含 `{frag}`，实际: {:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

/// 断言语义检查通过（无 error 诊断）
fn check_clean(src: &str) {
    let program = parse_source(src).expect("parse should succeed");
    let diags = hc::check_semantics(&program);
    assert!(
        !diags.iter().any(|d| d.is_error()),
        "不应有错误诊断: {:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn m24_move_arena_rejected() {
    // move Arena 分配对象 → 编译错误（所有权归 Arena）
    check_has_error(
        "fn take(y: o String) void {}\ntest fn t() !void {\n    var arena = Arena.init(alloc);\n    var buf = arena.alloc(64);\n    take(move buf);\n}\n",
        "allocated by Arena",
    );
}

#[test]
fn m24_move_global_rejected() {
    // move global → 编译错误（所有权归根作用域）
    check_has_error(
        "global g: String = String.from(\"x\", alloc);\ntest fn t() !void {\n    take(move g);\n}\nfn take(y: o String) void {}\n",
        "cannot move global",
    );
}

#[test]
fn m24_move_value_type_rejected() {
    // move 值类型（无所有权）→ 编译错误
    check_has_error(
        "fn take(n: i32) void {}\ntest fn t() !void {\n    var n = 42;\n    take(move n);\n}\n",
        "value type has no ownership",
    );
}

#[test]
fn m24_move_owned_ok() {
    // move 有所有权对象（非 Arena 分配）→ 合法
    check_clean(
        "fn make() o String {\n    var s = String.from(\"made\", alloc);\n    return move s;\n}\ntest fn t() !void {}\n",
    );
}

#[test]
fn m24_return_local_ref_rejected() {
    // Q18：返回局部变量引用 → 编译错误（引用逃逸）
    check_has_error(
        "fn f() *i32 {\n    var x: i32 = 1;\n    return &x;\n}\n",
        "escapes function scope",
    );
}

#[test]
fn m24_return_owned_param_must_move() {
    // 带所有权参数：返回引用 → 错误；必须 `return move param`
    check_has_error(
        "fn f(y: o String) *String {\n    return &y;\n}\n",
        "escapes function scope",
    );
    // move 返回所有权 → 合法
    check_clean("fn f(y: o String) o String {\n    return move y;\n}\ntest fn t() !void {}\n");
}

#[test]
fn m24_return_global_ref_ok() {
    // 返回 global 引用 → 合法（global 归根作用域，比函数长命）
    check_clean("global g: i32 = 1;\nfn f() *i32 {\n    return &g;\n}\ntest fn t() !void {}\n");
}

// ---------- M1.4 跨文件模块验收 ----------

#[test]
fn m14_extern_symbols_enable_crossfile_check() {
    // 外部（兄弟文件）namespace 符号并入语义检查——限定类型字段校验生效
    let ext =
        parse_source("namespace Orders {\n    pub struct Line { item: String, price: f64 }\n}\n")
            .expect("parse ext");
    let main = parse_source(
        "using Orders;\ntest fn t() !void {\n    var l = Orders.Line{ item = String.from(\"a\", alloc), price = 3.0 };\n    var x = l.itemm;\n}\n",
    )
    .expect("parse main");
    let diags = hc::check_semantics_extern(&main, &[&ext]);
    assert!(
        diags
            .iter()
            .any(|d| d.is_error() && d.message.contains("has no field")),
        "跨文件类型字段校验应报未知字段: {:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn m14_using_imports_type() {
    // using 导入类型：`Line` 不限定直接引用（扁平名）
    let ext =
        parse_source("namespace Orders { pub struct Line { item: String } }\n").expect("parse ext");
    let main = parse_source(
        "using Orders;\ntest fn t() !void {\n    var l = Line{ item = String.from(\"a\", alloc) };\n}\n",
    )
    .expect("parse main");
    let diags = hc::check_semantics_extern(&main, &[&ext]);
    assert!(
        !diags.iter().any(|d| d.is_error()),
        "using 导入后扁平类型可用: {:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn m14_using_alias_qualified_call() {
    // using NS as M：M.member 限定调用（语义可解析）
    let ext = parse_source("namespace Math { pub fn square(x: i32) i32 { return x * x; } }\n")
        .expect("parse ext");
    let main = parse_source("using Math as M;\ntest fn t() !void {\n    var r = M.square(5);\n}\n")
        .expect("parse main");
    let diags = hc::check_semantics_extern(&main, &[&ext]);
    assert!(
        !diags.iter().any(|d| d.is_error()),
        "using as 别名限定调用合法: {:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn m14_single_file_unchanged() {
    // 无外部符号时行为不变（check_semantics 兼容）
    check_clean("fn square(x: i32) i32 { return x * x; }\ntest fn t() !void {\n    try expect_eq(square(5), 25);\n}\n");
}

#[test]
fn m14_sibling_top_level_fn_is_file_private() {
    // 兄弟文件顶层函数文件私有：不污染主文件重载池（25/26 各自 load_config 不误报 ambiguous）
    let ext = parse_source("fn load_config(x: i32) i32 { return x * 2; }\n").expect("parse ext");
    let main = parse_source(
        "fn load_config(x: i32) i32 { return x + 1; }\ntest fn t() !void {\n    try expect_eq(load_config(1), 2);\n}\n",
    )
    .expect("parse main");
    let diags = hc::check_semantics_extern(&main, &[&ext]);
    assert!(
        !diags.iter().any(|d| d.is_error() && d.message.contains("ambiguous")),
        "兄弟文件同名顶层函数不应误报歧义: {:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}
