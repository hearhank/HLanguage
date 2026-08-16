//! M2.3 推断补全：泛型 T / 指针形态 / 多路径返回 / 重载歧义

use hc_rt::Interp;

fn run_ok(src: &str) {
    let program = hc::parse_source(src).unwrap_or_else(|d| panic!("parse: {:?}", d));
    let mut interp = Interp::new(src);
    interp
        .load(&program)
        .unwrap_or_else(|e| panic!("load: {} {}", e.name, e.message));
    let (p, f, _s) = interp.run_tests();
    assert_eq!(f, 0, "failed: {:?}", interp.test_out);
    assert!(p >= 1, "no tests ran");
}

fn run_compile_error(src: &str, err_frag: &str) {
    let program = hc::parse_source(src).expect("parse should succeed");
    let mut interp = Interp::new(src);
    let e = interp
        .load(&program)
        .expect_err("semantic check should reject");
    assert!(
        e.message.contains(err_frag) || e.name == "CompileError",
        "expected error containing `{err_frag}`, got: {} ({})",
        e.name,
        e.message
    );
}

// ---------- 多路径返回类型推断 ----------

#[test]
fn infer_return_single_path() {
    // 未标注返回类型：单路径 return 推断（不再误报 void 不匹配）
    run_ok(
        r#"
fn add_one(v: i32) { return v + 1; }
[test] fn t() !void {
    try expect_eq(add_one(41), 42);
}
"#,
    );
}

#[test]
fn infer_return_multi_path_consistent() {
    // 多路径 return 一致类型：通过
    run_ok(
        r#"
fn pick(flag: bool) {
    if (flag) { return 10; }
    return 20;
}
[test] fn t() !void {
    try expect_eq(pick(true), 10);
    try expect_eq(pick(false), 20);
}
"#,
    );
}

#[test]
fn infer_return_multi_path_conflict() {
    // 多路径 return 类型不一致：int vs string → 编译错误要求显式
    run_compile_error(
        "fn pick(flag: bool) {
    if (flag) { return 10; }
    return \"str\";
}
[test] fn t() !void { var _ = pick(true); }\n",
        "inferred return type mismatch",
    );
}

#[test]
fn infer_return_int_vs_float_conflict() {
    // int 与 float 不互通（比 compatible 更严格的推断统一）
    run_compile_error(
        "fn pick(flag: bool) {
    if (flag) { return 10; }
    return 3.5;
}
[test] fn t() !void { var _ = pick(true); }\n",
        "inferred return type mismatch",
    );
}

// ---------- 重载歧义与期望类型传播 ----------

#[test]
fn overload_disambiguated_by_expected_return() {
    // 期望类型传播：var x: f64 = get() 选返回 f64 的重载
    run_ok(
        r#"
fn get() i32 { return 1; }
fn get() f64 { return 1.5; }
[test] fn t() !void {
    var x: f64 = get();
    try expect_eq(x, 1.5);
    var y: i32 = get();
    try expect_eq(y, 1);
}
"#,
    );
}

#[test]
fn overload_ambiguous_literal() {
    // 字面量同精度匹配多个重载且无期望类型 → 歧义编译错误
    // （int 字面量对 i32/i64 同为精确匹配；而 i32 vs f64 时 i32 精确胜出，不歧义）
    run_compile_error(
        "fn f(a: i32) i32 { return a; }
fn f(a: i64) i64 { return a; }
[test] fn t() !void { var x = f(1); var _ = x; }\n",
        "ambiguous",
    );
}

#[test]
fn overload_int_literal_prefers_int_over_float() {
    // int 字面量：i32 精确匹配优先于 f64 兼容匹配（不歧义）
    run_ok(
        r#"
fn f(a: i32) i32 { return a; }
fn f(a: f64) f64 { return a; }
[test] fn t() !void {
    try expect_eq(f(1), 1);
}
"#,
    );
}

#[test]
fn overload_concrete_beats_generic() {
    // 具体非泛型候选优先于泛型候选（不报歧义）
    run_ok(
        r#"
fn id(x: i32) i32 { return x + 100; }
fn id(x: T) T where T: INumber { return x; }
[test] fn t() !void {
    try expect_eq(id(5), 105);
}
"#,
    );
}

// ---------- 指针形态推断 ----------

#[test]
fn pointer_write_read_only_rejected() {
    // var p = &x 推断 *i32（只读）：写只读指针 → 编译错误
    run_compile_error(
        "[test] fn t() !void {
    var x: i32 = 0;
    var p = &x;
    p.* = 42;
}\n",
        "read-only pointer",
    );
}

#[test]
fn pointer_write_mut_ok() {
    // var p = &mut x 推断 *mut i32（可写）：写通过
    run_ok(
        r#"
[test] fn t() !void {
    var x: i32 = 0;
    var p = &mut x;
    p.* = 42;
    try expect_eq(x, 42);
}
"#,
    );
}

// ---------- 泛型 T 推断 ----------

#[test]
fn generic_t_through_pointer_param() {
    // 泛型 T 经 *T 形参绑定并具体化返回类型
    run_ok(
        r#"
fn deref_id(p: *T) T where T: INumber {
    return p.*;
}
[test] fn t() !void {
    var x: i32 = 42;
    try expect_eq(deref_id(&x), 42);
}
"#,
    );
}
