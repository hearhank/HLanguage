//! 梯队 1 语义完整验收测试（M2.2 类型检查 / M2.5 definite / M2.4 所有权 / M4.3 @ 内建）

use hc_rt::Interp;

/// 运行单个 .hc 源码所有 test fn；断言全部通过
fn run_ok(src: &str) {
    let program = hc::parse_source(src).unwrap_or_else(|d| panic!("parse: {:?}", d));
    let mut interp = Interp::new(src);
    interp
        .load(&program)
        .unwrap_or_else(|e| panic!("load: {} {}", e.name, e.message));
    let (p, f, _s) = interp.run_tests();
    assert_eq!(f, 0, "failed tests: {:?}", interp.test_out);
    assert!(p >= 1, "no tests ran");
}

/// 断言 load 阶段编译错误（语义检查拦截）
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

#[test]
fn width_check_u8_overflow() {
    // 06-integers：var g: u8 = 256 → 编译期报错（Q24/Q39）
    run_compile_error(
        "fn main(io: Io) !void { var g: u8 = 256; }\n",
        "out of range",
    );
}

#[test]
fn width_check_i32_ok() {
    run_ok("test fn t() !void { var a: i32 = 42; try expect_eq(a, 42); }\n");
}

#[test]
fn width_check_hex_ok() {
    // 0xFF 在 u8 范围内
    run_ok("test fn t() !void { var a: u8 = 0xFF; try expect_eq(a, 255); }\n");
}

#[test]
fn width_check_u64_ok() {
    // xorshift 种子（84-rng）
    run_ok(
        "test fn t() !void {
    var s: u64 = 0x1234_5678_9abc_def0;
    try expect_eq(s, 1311768467463790320);
}\n",
    );
}

#[test]
fn reference_assignment_rejected() {
    // 引用类型（Vec）赋值 = 编译错误（Q1'：显式 copy(&x) 或指针）
    run_compile_error(
        "class Foo { a: i32 }
fn main(io: Io) !void {
    var v: Vec(i32) = Vec(i32).init(alloc);
    var w: Vec(i32) = v;
}\n",
        "cannot assign",
    );
}

#[test]
fn continuous_assignment_allowed() {
    // [continuous] 值类型：赋值即复制（允许）
    run_ok(
        "[continuous]
class Point { x: f32, y: f32 }
test fn t() !void {
    var p1 = Point{ x = 1.0, y = 2.0 };
    var p2 = p1;
    p2.x = 99.0;
    try expect_eq(p1.x, 1.0);
}\n",
    );
}

#[test]
fn table_construct_and_index() {
    // M8：Table(T).init(alloc, rows, cols, init) + t[i, j] 多参索引
    run_ok(
        "test fn t() !void {
    var tbl = Table(i32).init(alloc, 3, 4, 0);
    try expect_eq(tbl[1, 2], 0);
    var t2 = Table(i32).init(alloc, 2, 2, 7);
    try expect_eq(t2[0, 0], 7);
    try expect_eq(t2[1, 1], 7);
}\n",
    );
}

#[test]
fn at_int_from_enum() {
    run_ok(
        "enum Kind { player, enemy, item }
test fn t() !void {
    var k = Kind.enemy;
    try expect_eq(@intFromEnum(k), 1);
    var k2 = @enumFromInt(Kind, 2);
    try expect_eq(@intFromEnum(k2), 2);
}\n",
    );
}

#[test]
fn copy_shallow_mode() {
    // L1：copy(&x, .shallow) ≡ copy(&x, CopyMode.shallow)
    run_ok(
        "test fn t() !void {
    var v1 = Vec(i32).init(alloc);
    v1.append(1);
    var v2 = copy(&v1, .shallow);
    try expect_eq(v2.len, 1);
}\n",
    );
}

#[test]
fn copy_deep_mode_default() {
    run_ok(
        "test fn t() !void {
    var v1 = Vec(i32).init(alloc);
    v1.append(1);
    var v2 = copy(&v1);
    v2.append(2);
    try expect_eq(v1.len, 1);
    try expect_eq(v2.len, 2);
}\n",
    );
}

#[test]
fn definite_assignment_rejects_partial_return() {
    // C7：alloc.init(T) 无参构造后字段未全赋值即 return → 编译错误
    run_compile_error(
        "class Order { id: i32, amount: f64 }
fn make() Order {
    var ord = alloc.init(Order);
    ord.id = 42;
    return ord;   // amount 未赋值
}
fn main(io: Io) !void {}",
        "partially-initialized",
    );
}

#[test]
fn definite_assignment_allows_complete() {
    // 全字段赋值后返回 → 通过
    run_ok(
        "class Order { id: i32, amount: f64 }
fn make() Order {
    var ord = alloc.init(Order);
    ord.id = 42;
    ord.amount = 3.5;
    return ord;
}
test fn t() !void {
    var ord = make();
    try expect_eq(ord.id, 42);
}\n",
    );
}

#[test]
fn definite_assignment_ignores_continuous() {
    // [continuous] 值类型走字面量构造，无需字段跟踪
    run_ok(
        "[continuous]
class Point { x: f32, y: f32 }
test fn t() !void {
    var p = Point{ x = 1.0, y = 2.0 };
    try expect_eq(p.x, 1.0);
}\n",
    );
}
