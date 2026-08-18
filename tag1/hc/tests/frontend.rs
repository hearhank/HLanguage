//! hc 编译器前端单元测试（M1/M2 验收：词法、解析、诊断）

use hc::ast::Decl;
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
interface IShape { fn area(self: *Self) f32; }
class Rect: IShape { w: f32, h: f32, fn area(self: *Self) f32 { return self.w * self.h; } }
"#;
    let program = parse_source(src).expect("parse types");
    assert_eq!(program.decls.len(), 4);
}

#[test]
fn parse_union_decl() {
    // K1（ADR-0014）：无标签 union 声明解析——仅字段，无方法
    let src = r#"
union Kind { player: i32, enemy: f32, active: bool }
pub union Num { i: i32, f: f64 }
"#;
    let program = parse_source(src).expect("parse unions");
    assert_eq!(program.decls.len(), 2);
    assert!(matches!(&program.decls[0], hc::ast::Decl::Union { name, .. } if name == "Kind"));
}

#[test]
fn parse_test_fn_and_assertions() {
    let src = r#"
fn add(a: i32, b: i32) i32 { return a + b; }
[test] fn check() !void {
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
[test] fn t() !void {
    var x = error.NotFound;
}
"#;
    let program = parse_source(src).expect("parse");
    let table = error_code_table(&program);
    assert_eq!(table.len(), 2, "同名错误应合并为一条 + 内建 OutOfMemory");
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
    assert_eq!(table.len(), 4, "3 用户错误 + 内建 OutOfMemory");
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
    assert_eq!(table.len(), 3, "2 用户错误 + 内建 OutOfMemory");
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
    assert_eq!(table.name_of(2), Some("OutOfMemory"), "内建错误已注册");
    assert_eq!(table.name_of(3), None, "未分配码无名字");
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
    assert_eq!(
        v1,
        vec![
            ("X".into(), 0),
            ("Y".into(), 1),
            ("Z".into(), 2),
            ("OutOfMemory".into(), 3)
        ]
    );
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
        "fn take(y: o String) void {}\n[test] fn t() !void {\n    var arena = Arena.init(alloc);\n    var buf = arena.alloc(64);\n    take(move buf);\n}\n",
        "allocated by Arena",
    );
}

#[test]
fn m24_move_global_rejected() {
    // move global → 编译错误（所有权归根作用域）
    check_has_error(
        "global g: String = String.from(\"x\", alloc);\n[test] fn t() !void {\n    take(move g);\n}\nfn take(y: o String) void {}\n",
        "cannot move global",
    );
}

#[test]
fn m24_move_value_type_rejected() {
    // move 值类型（无所有权）→ 编译错误
    check_has_error(
        "fn take(n: i32) void {}\n[test] fn t() !void {\n    var n = 42;\n    take(move n);\n}\n",
        "value type has no ownership",
    );
}

#[test]
fn m24_move_owned_ok() {
    // move 有所有权对象（非 Arena 分配）→ 合法
    check_clean(
        "fn make() o String {\n    var s = String.from(\"made\", alloc);\n    return move s;\n}\n[test] fn t() !void {}\n",
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
    check_clean("fn f(y: o String) o String {\n    return move y;\n}\n[test] fn t() !void {}\n");
}

#[test]
fn m24_return_global_ref_ok() {
    // 返回 global 引用 → 合法（global 归根作用域，比函数长命）
    check_clean("global g: i32 = 1;\nfn f() *i32 {\n    return &g;\n}\n[test] fn t() !void {}\n");
}

// ---------- K1 无标签 union（ADR-0014）语义 ----------

#[test]
fn k1_union_scalar_only_rejected() {
    // union 字段仅限标量（内存双关工具；引用类型编译错误）
    check_has_error(
        "union Bad { s: String }\n[test] fn t() !void {}\n",
        "必须为标量类型",
    );
    check_has_error(
        "union Bad { v: Vec(i32) }\n[test] fn t() !void {}\n",
        "必须为标量类型",
    );
}

#[test]
fn k1_union_literal_single_field() {
    // union 字面量恰好接受一个字段
    check_has_error(
        "union U { a: i32, b: i32 }\n[test] fn t() !void {\n    var u = U{ a = 1, b = 2 };\n}\n",
        "expects exactly one field",
    );
}

#[test]
fn k1_union_literal_unknown_field() {
    // union 字面量字段必须存在
    check_has_error(
        "union U { a: i32 }\n[test] fn t() !void {\n    var u = U{ x = 1 };\n}\n",
        "has no field",
    );
}

#[test]
fn k1_union_field_access_clean() {
    // 合法 union：构造 + 字段读/写（写同步重解释在运行时，语义层通过）
    check_clean(
        "union Num { i: i32, f: f32, b: bool }\n\
         [test] fn t() !void {\n\
             var n = Num{ i = 1 };\n\
             var x = n.b;\n\
             n.i = 2;\n\
         }\n",
    );
}

// ---------- K2 @volatileLoad/@volatileStore（ADR-0014）语义 ----------

#[test]
fn k2_volatile_load_returns_pointee() {
    // @volatileLoad(ptr) 返回 pointee 类型——赋给 i32 无诊断
    check_clean(
        "[test] fn t() !void {\n\
             var mut x: i32 = 5;\n\
             var p = &mut x;\n\
             var y: i32 = @volatileLoad(p);\n\
             @volatileStore(p, y);\n\
         }\n",
    );
}

#[test]
fn k2_volatile_load_non_pointer_rejected() {
    // @volatileLoad(非指针) → 编译错误
    check_has_error(
        "[test] fn t() !void {\n    var x = @volatileLoad(5);\n}\n",
        "@volatileLoad expects a pointer argument",
    );
}

// ---------- M1.4 跨文件模块验收 ----------

#[test]
fn m14_extern_symbols_enable_crossfile_check() {
    // 外部（兄弟文件）namespace 符号并入语义检查——限定类型字段校验生效
    let ext =
        parse_source("namespace Orders {\n    pub struct Line { item: String, price: f64 }\n}\n")
            .expect("parse ext");
    let main = parse_source(
        "using Orders;\n[test] fn t() !void {\n    var l = Orders.Line{ item = String.from(\"a\", alloc), price = 3.0 };\n    var x = l.itemm;\n}\n",
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
        "using Orders;\n[test] fn t() !void {\n    var l = Line{ item = String.from(\"a\", alloc) };\n}\n",
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
    let main =
        parse_source("using Math as M;\n[test] fn t() !void {\n    var r = M.square(5);\n}\n")
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
    check_clean("fn square(x: i32) i32 { return x * x; }\n[test] fn t() !void {\n    try expect_eq(square(5), 25);\n}\n");
}

// ---------- ADR-0010：import 语句（A1：词法与解析） ----------

#[test]
fn a1_lex_import_keyword() {
    let toks = lex("import H.std.{io as my};");
    assert!(matches!(toks[0].kind, TokenKind::KwImport));
}

#[test]
fn a1_parse_import_symbol_selection() {
    // 符号选择 + as 别名：`import H.std.{io as my};`
    let program = parse_source("import H.std.{io as my};").expect("parse");
    let Decl::Import {
        path,
        alias,
        select,
        ..
    } = &program.decls[0]
    else {
        panic!("预期 Decl::Import，实际 {:?}", program.decls[0]);
    };
    assert_eq!(path, &vec!["H".to_string(), "std".to_string()]);
    assert_eq!(alias, &None);
    assert_eq!(
        select,
        &Some(vec![("io".to_string(), Some("my".to_string()))])
    );
}

#[test]
fn a1_parse_import_multi_symbol_no_alias() {
    // 多符号（无别名）：`import H.std.net.{http, tcp};`
    let program = parse_source("import H.std.net.{http, tcp};").expect("parse");
    let Decl::Import { path, select, .. } = &program.decls[0] else {
        panic!("预期 Decl::Import");
    };
    assert_eq!(
        path,
        &vec!["H".to_string(), "std".to_string(), "net".to_string()]
    );
    assert_eq!(
        select,
        &Some(vec![("http".to_string(), None), ("tcp".to_string(), None),])
    );
}

#[test]
fn a1_parse_import_whole_module() {
    // 整模块：`import pkg.mod;`
    let program = parse_source("import pkg.mod;").expect("parse");
    let Decl::Import {
        path,
        alias,
        select,
        ..
    } = &program.decls[0]
    else {
        panic!("预期 Decl::Import");
    };
    assert_eq!(path, &vec!["pkg".to_string(), "mod".to_string()]);
    assert_eq!(alias, &None);
    assert_eq!(select, &None);
}

#[test]
fn a1_parse_import_whole_module_alias() {
    // 整模块 + 别名：`import pkg.mod as m;`
    let program = parse_source("import pkg.mod as m;").expect("parse");
    let Decl::Import {
        path,
        alias,
        select,
        ..
    } = &program.decls[0]
    else {
        panic!("预期 Decl::Import");
    };
    assert_eq!(path, &vec!["pkg".to_string(), "mod".to_string()]);
    assert_eq!(alias, &Some("m".to_string()));
    assert_eq!(select, &None);
}

#[test]
fn a1_parse_study_example() {
    // study.hc 全例可解析（ADR-0010 形态）
    let src = r#"
import H.std.{io as my};
import H.std.net.{http,tcp};

fn main(args: o Vec(String)) !void {
    my.print("hello, world\n");
    io.print("x = {}, y = {}\n", 42, 3.14);
    http.print();
}
"#;
    let program = parse_source(src).expect("parse study.hc");
    // 2 import + 1 fn（+ 忽略 [test] 仅注释形态）
    assert!(matches!(program.decls[0], Decl::Import { .. }));
    assert!(matches!(program.decls[1], Decl::Import { .. }));
    assert!(matches!(&program.decls[2], Decl::Fn { name, .. } if name == "main"));
}

#[test]
fn a1_parse_import_missing_semi_is_error() {
    // 缺分号 → 解析失败
    assert!(parse_source("import pkg.mod").is_err());
}

// ---------- ADR-0010：import 语义（A2a——符号登记/冲突/模块识别前置） ----------

#[test]
fn a2_sem_import_io_alias_no_error() {
    // `import H.std.{io as my}; my.print(...)`——io 族环境模块 → 别名绑定，语义无错
    let program =
        parse_source("import H.std.{io as my};\nfn main() !void {\n    my.print(\"hi\\n\");\n}\n")
            .expect("parse");
    let diags = hc::check_semantics(&program);
    assert!(
        !diags.iter().any(|d| d.is_error()),
        "import io 别名语义不应报错: {:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn a2_sem_import_whole_module_no_error() {
    // `import H.std.io;` 整模块——绑定名 = 末段 `io`，语义无错
    let program =
        parse_source("import H.std.io;\nfn main() !void {\n    io.print(\"hi\\n\");\n}\n")
            .expect("parse");
    let diags = hc::check_semantics(&program);
    assert!(
        !diags.iter().any(|d| d.is_error()),
        "import 整模块语义不应报错: {:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn a2_sem_import_user_pkg_selection() {
    // 用户包符号选择：`import jsonlib.{double as dbl}; dbl(21)`——语义无错
    let dep = parse_source("pub fn double(x: i32) i32 { return x * 2; }\n").expect("parse dep");
    let main = parse_source(
        "import jsonlib.{double as dbl};\n[test] fn t() !void {\n    var a = dbl(21);\n    try expect_eq(a, 42);\n}\n",
    )
    .expect("parse main");
    let diags = hc::check_semantics_deps(&main, &[("jsonlib", &dep)]);
    assert!(
        !diags.iter().any(|d| d.is_error()),
        "import 用户包符号选择语义不应报错: {:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn a2_sem_import_user_pkg_whole_module() {
    // 用户包整模块 + 别名：`import jsonlib as j; j.double(21)`——语义无错
    let dep = parse_source("pub fn double(x: i32) i32 { return x * 2; }\n").expect("parse dep");
    let main = parse_source(
        "import jsonlib as j;\n[test] fn t() !void {\n    var a = j.double(21);\n    try expect_eq(a, 42);\n}\n",
    )
    .expect("parse main");
    let diags = hc::check_semantics_deps(&main, &[("jsonlib", &dep)]);
    assert!(
        !diags.iter().any(|d| d.is_error()),
        "import 用户包整模块语义不应报错: {:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

// ---------- A2b：模块识别（[module]）+ 命名规范（PascalCase） ----------

#[test]
fn a2b_parse_module_trait() {
    // `[module] namespace` → is_module=true
    let program = parse_source("[module] namespace Orders { pub fn f() i32 { return 1; } }\n")
        .expect("parse");
    let Decl::Namespace {
        name, is_module, ..
    } = &program.decls[0]
    else {
        panic!("预期 Decl::Namespace: {:?}", program.decls[0]);
    };
    assert_eq!(name, "Orders");
    assert!(*is_module, "[module] 标注应置 is_module");
}

#[test]
fn a2b_sem_pascal_case_class_error() {
    // 类型名 PascalCase：class 首字母小写 → 编译期诊断
    check_has_error("class point { x: i32 }\n", "必须首字母大写");
}

#[test]
fn a2b_sem_pascal_case_enum_error() {
    // 类型名 PascalCase：enum 首字母小写 → 编译期诊断
    check_has_error("enum color { red, green }\n", "必须首字母大写");
}

#[test]
fn a2b_sem_pascal_case_namespace_error() {
    // 命名空间名 PascalCase：首字母小写 → 编译期诊断
    check_has_error(
        "namespace math { fn f() i32 { return 1; } }\n",
        "必须首字母大写",
    );
}

#[test]
fn a2b_sem_pascal_case_ok() {
    // 合规命名（类型/命名空间 PascalCase）不报错
    check_clean(
        "class Point { x: f32 }\nenum Color { red }\nnamespace Math { fn square(x: i32) i32 { return x * x; } }\n",
    );
}

// 注：模块隔离（`[module]` 成员仅限定名、扁平访问失败）由**运行时**落实——
// 语义检查器对未知符号保守放行（准确优先），见 hc-rt/tests/import.rs a2b_* 运行时用例。

#[test]
fn m14_sibling_top_level_fn_is_file_private() {
    // 兄弟文件顶层函数文件私有：不污染主文件重载池（25/26 各自 load_config 不误报 ambiguous）
    let ext = parse_source("fn load_config(x: i32) i32 { return x * 2; }\n").expect("parse ext");
    let main = parse_source(
        "fn load_config(x: i32) i32 { return x + 1; }\n[test] fn t() !void {\n    try expect_eq(load_config(1), 2);\n}\n",
    )
    .expect("parse main");
    let diags = hc::check_semantics_extern(&main, &[&ext]);
    assert!(
        !diags
            .iter()
            .any(|d| d.is_error() && d.message.contains("ambiguous")),
        "兄弟文件同名顶层函数不应误报歧义: {:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}
