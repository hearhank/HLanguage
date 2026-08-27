//! hc-rt/tests/dep.rs

use hc_rt::Interp;

fn parse(s: &str) -> hc::Program {
    hc::parse_source(s).unwrap_or_else(|d| panic!("parse: {:?}", d))
}

#[test]
fn dep_pub_fn_accessible_via_qualified_and_using() {
    let dep = parse(
        r#"
pub fn double(x: i32) i32 { return x * 2; }
fn hidden(x: i32) i32 { return x + 100; }
"#,
    );
    let main = parse(
        r#"
using jsonlib;
[test] fn dep_pub_fn_accessible_via_qualified_and_using() !void {
    var a = jsonlib.double(21);
    try expect_eq(a, 42);
    var b = double(10);
    try expect_eq(b, 20);
}
"#,
    );
    let mut interp = Interp::new("");
    interp
        .load_dep("jsonlib", &[&dep])
        .unwrap_or_else(|e| panic!("load_dep: {} {}", e.name, e.message));
    interp
        .load(&main)
        .unwrap_or_else(|e| panic!("load: {} {}", e.name, e.message));
    let (p, f, _s) = interp.run_tests();
    assert_eq!(f, 0, "failed: {:?}", interp.test_out);
    assert!(p >= 1, "no tests ran");
}

#[test]
fn dep_non_pub_fn_not_visible() {
    // 非 pub 顶层函数不登记 → 调用失败（运行期诊断；语义层保守放行）
    let dep = parse(
        r#"
fn hidden(x: i32) i32 { return x + 100; }
"#,
    );
    let main = parse(
        r#"
using jsonlib;
[test] fn dep_non_pub_fn_not_visible() !void {
    var x = jsonlib.hidden(1);
    try expect_eq(x, 101);
}
"#,
    );
    let mut interp = Interp::new("");
    interp.load_dep("jsonlib", &[&dep]).unwrap();
    interp
        .load(&main)
        .unwrap_or_else(|e| panic!("load should pass (lenient): {} {}", e.name, e.message));
    let (_p, f, _s) = interp.run_tests();
    assert!(f > 0, "非 pub 依赖符号应不可见: {:?}", interp.test_out);
}

#[test]
fn dep_pub_fn_in_namespace_via_prefix() {
    // pub namespace 内 pub 函数：`jsonlib.util.triple(...)` 限定访问
    let dep = parse(
        r#"
pub namespace Util {
    pub fn triple(x: i32) i32 { return x * 3; }
    fn secret(x: i32) i32 { return x; }
}
"#,
    );
    let main = parse(
        r#"
[test] fn dep_pub_fn_in_namespace_via_prefix() !void {
    var a = jsonlib.Util.triple(5);
    try expect_eq(a, 15);
}
"#,
    );
    let mut interp = Interp::new("");
    interp.load_dep("jsonlib", &[&dep]).unwrap();
    interp
        .load(&main)
        .unwrap_or_else(|e| panic!("load: {} {}", e.name, e.message));
    let (p, f, _s) = interp.run_tests();
    assert_eq!(f, 0, "failed: {:?}", interp.test_out);
    assert!(p >= 1);
}
