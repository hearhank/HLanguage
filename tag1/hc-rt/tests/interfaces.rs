//! M2.1 接口三用途：真实实现（去占位）
//!
//! ① implements 标注 = 方法契约验证（class 冒号标注须实现接口方法，含超接口）；
//! ② where T: 约束调用点验证；③ 类型参数编译可验证（签名兼容，能精确判定才报错）。
//! 内建接口（ICompare/INumber/IIterable/Io）由编译器内建实现，跳过契约检查。

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

/// 严格断言：load 阶段编译错误信息必须包含 err_frag（确保命中接口契约诊断）
fn run_compile_error(src: &str, err_frag: &str) {
    let program = hc::parse_source(src).expect("parse should succeed");
    let mut interp = Interp::new(src);
    let e = interp
        .load(&program)
        .expect_err("semantic check should reject");
    assert!(
        e.message.contains(err_frag),
        "expected error containing `{err_frag}`, got: {} ({})",
        e.name,
        e.message
    );
}

// ---------- ① implements 标注：方法契约验证 ----------

#[test]
fn implements_missing_method_rejected() {
    run_compile_error(
        "interface Shape {
    fn area(self: *Self) f32;
}
[continuous]
class Rect: Shape {
    w: f32,
}
test fn t() !void { var r = Rect{ w = 1.0 }; }\n",
        "does not implement method",
    );
}

#[test]
fn implements_wrong_return_type_rejected() {
    // 接口返回 f32，实现返回 i32 → 签名不兼容 → 契约违反
    run_compile_error(
        "interface Shape {
    fn area(self: *Self) f32;
}
[continuous]
class Rect: Shape {
    w: f32,
    fn area(self: *Self) i32 { return 1; }
}
test fn t() !void { var r = Rect{ w = 1.0 }; }\n",
        "does not implement method",
    );
}

#[test]
fn implements_wrong_arity_rejected() {
    run_compile_error(
        "interface Shape {
    fn area(self: *Self) f32;
}
[continuous]
class Rect: Shape {
    w: f32,
    fn area(self: *Self, extra: i32) f32 { return 1.0; }
}
test fn t() !void { var r = Rect{ w = 1.0 }; }\n",
        "does not implement method",
    );
}

#[test]
fn implements_super_interface_missing_rejected() {
    // B: A——实现 B 须同时实现 A 与 B 的方法契约
    run_compile_error(
        "interface A {
    fn x(self: *Self) void;
}
interface B: A {
    fn y(self: *Self) void;
}
[continuous]
class C: B {
    fn x(self: *Self) void {}
}
test fn t() !void { var c = C{}; }\n",
        "does not implement method `y",
    );
}

#[test]
fn implements_full_contract_ok() {
    run_ok(
        r#"
interface Shape {
    fn area(self: *Self) f32;
}
[continuous]
class Rect: Shape {
    w: f32,
    h: f32,
    fn area(self: *Self) f32 { return self.w * self.h; }
}
test fn t() !void {
    var r = Rect{ w = 3.0, h = 4.0 };
    try expect(r.area() > 11.99 and r.area() < 12.01);
}
"#,
    );
}

#[test]
fn implements_multiple_interfaces_ok() {
    run_ok(
        r#"
interface Drawable {
    fn draw(self: *Self) void;
}
interface Saveable {
    fn save(self: *Self) void;
}
[continuous]
class Document: Drawable, Saveable {
    title: i32,
    fn draw(self: *Self) void { var _ = self.title; }
    fn save(self: *Self) void { var _ = self.title; }
}
test fn t() !void {
    var d = Document{ title = 1 };
    d.draw();
    d.save();
    try expect_eq(d.title, 1);
}
"#,
    );
}

// ---------- ②③ where 约束 + 编译可验证（静态分发） ----------

#[test]
fn where_constraint_static_dispatch_ok() {
    // 接口约束参数：describe(shape: *T) where T: Shape → 单态化调用 shape.area()
    run_ok(
        r#"
interface Shape {
    fn area(self: *Self) f32;
}
[continuous]
class Rect: Shape {
    w: f32,
    h: f32,
    fn area(self: *Self) f32 { return self.w * self.h; }
}
fn describe(shape: *T) f32 where T: Shape {
    return shape.area();
}
test fn t() !void {
    var r = Rect{ w = 3.0, h = 4.0 };
    try expect(describe(&r) > 11.99 and describe(&r) < 12.01);
}
"#,
    );
}
