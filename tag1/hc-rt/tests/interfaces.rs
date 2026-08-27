//! hc-rt/tests/interfaces.rs

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
        "interface IShape {
    fn area(self: *Self) f32;
}
class Rect: IShape {
    w: f32,
}
[test] fn t() !void { var r = Rect{ w = 1.0 }; }\n",
        "does not implement method",
    );
}

#[test]
fn implements_wrong_return_type_rejected() {
    // 接口返回 f32，实现返回 i32 → 签名不兼容 → 契约违反
    run_compile_error(
        "interface IShape {
    fn area(self: *Self) f32;
}
class Rect: IShape {
    w: f32,
    fn area(self: *Self) i32 { return 1; }
}
[test] fn t() !void { var r = Rect{ w = 1.0 }; }\n",
        "does not implement method",
    );
}

#[test]
fn implements_wrong_arity_rejected() {
    run_compile_error(
        "interface IShape {
    fn area(self: *Self) f32;
}
class Rect: IShape {
    w: f32,
    fn area(self: *Self, extra: i32) f32 { return 1.0; }
}
[test] fn t() !void { var r = Rect{ w = 1.0 }; }\n",
        "does not implement method",
    );
}

#[test]
fn implements_super_interface_missing_rejected() {
    // IB: IA——实现 IB 须同时实现 IA 与 IB 的方法契约
    run_compile_error(
        "interface IA {
    fn x(self: *Self) void;
}
interface IB: IA {
    fn y(self: *Self) void;
}
class C: IB {
    fn x(self: *Self) void {}
}
[test] fn t() !void { var c = C{}; }\n",
        "does not implement method `y",
    );
}

#[test]
fn implements_full_contract_ok() {
    run_ok(
        r#"
interface IShape {
    fn area(self: *Self) f32;
}
class Rect: IShape {
    w: f32,
    h: f32,
    fn area(self: *Self) f32 { return self.w * self.h; }
}
[test] fn t() !void {
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
interface IDrawable {
    fn draw(self: *Self) void;
}
interface ISaveable {
    fn save(self: *Self) void;
}
class Document: IDrawable, ISaveable {
    title: i32,
    fn draw(self: *Self) void { var _ = self.title; }
    fn save(self: *Self) void { var _ = self.title; }
}
[test] fn t() !void {
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
    // 接口约束参数：describe(shape: *T) where T: IShape → 单态化调用 shape.area()
    run_ok(
        r#"
interface IShape {
    fn area(self: *Self) f32;
}
class Rect: IShape {
    w: f32,
    h: f32,
    fn area(self: *Self) f32 { return self.w * self.h; }
}
fn describe(shape: *T) f32 where T: IShape {
    return shape.area();
}
[test] fn t() !void {
    var r = Rect{ w = 3.0, h = 4.0 };
    try expect(describe(&r) > 11.99 and describe(&r) < 12.01);
}
"#,
    );
}

// ---------- 命名约定：接口名必须以 I 开头 ----------

#[test]
fn interface_name_must_start_with_i() {
    run_compile_error(
        "interface Foo {
    fn x(self: *Self) void;
}
[test] fn t() !void {}\n",
        "必须以 I 开头",
    );
}

// ---------- [test("名称")] 特性标记：显示名 = 名称 ?? 函数名 ----------

#[test]
fn test_attr_name_used_as_display() {
    let src = "[test(\"格式化输入运行\")] fn format_entry_runs() !void { try expect(true); }\n";
    let program = hc::parse_source(src).unwrap();
    let mut interp = Interp::new(src);
    interp.load(&program).unwrap();
    let (p, f, _s) = interp.run_tests();
    assert_eq!(f, 0, "failed: {:?}", interp.test_out);
    assert_eq!(p, 1, "one test should run: {:?}", interp.test_out);
    assert!(
        interp.test_out.iter().any(|l| l.contains("格式化输入运行")),
        "名称应作为显示名，got: {:?}",
        interp.test_out
    );
    assert!(
        !interp
            .test_out
            .iter()
            .any(|l| l.contains("format_entry_runs")),
        "函数名不应出现在显示名，got: {:?}",
        interp.test_out
    );
}

#[test]
fn test_attr_no_name_falls_back_to_fn_name() {
    // 无参 [test]：显示名回退为函数名
    let src = "[test] fn hello() !void { try expect(true); }\n[test(\"单参名称\")] fn b() !void { try expect(true); }\n";
    let program = hc::parse_source(src).unwrap();
    let mut interp = Interp::new(src);
    interp.load(&program).unwrap();
    let (p, f, _s) = interp.run_tests();
    assert_eq!(f, 0, "failed: {:?}", interp.test_out);
    assert_eq!(p, 2, "two tests should run: {:?}", interp.test_out);
    assert!(interp.test_out.iter().any(|l| l.contains("hello")));
    assert!(interp.test_out.iter().any(|l| l.contains("单参名称")));
}
