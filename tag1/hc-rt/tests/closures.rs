//! M2.7 闭包（move 捕获 + 按值返回规则）
//!
//! tag1 近似：闭包捕获整个作用域链（自由变量精确分析留后续）；只读/mut 捕获 =
//! 共享槽快照（原绑定变更对闭包可见），`move` 捕获 = 深拷贝独立副本（闭包脱离
//! 原作用域生命周期，原绑定后续变更/销毁不影响闭包）。

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

#[test]
fn move_closure_captures_copy() {
    // move 捕获：闭包持有 a 的副本，原绑定后续变更不影响闭包
    run_ok(
        r#"
[test] fn t() !void {
    var a = 10;
    var f = move |v| v + a;
    a = 100;
    try expect_eq(f(5), 15);
}
"#,
    );
}

#[test]
fn read_closure_shares_slot() {
    // 只读捕获：共享槽，原绑定后续变更对闭包可见（与 move 捕获对照）
    run_ok(
        r#"
[test] fn t() !void {
    var a = 10;
    var f = |v| v + a;
    a = 100;
    try expect_eq(f(5), 105);
}
"#,
    );
}

#[test]
fn mut_closure_writes_captured() {
    // mut 捕获：闭包内写入被捕获变量，对原绑定可见
    run_ok(
        r#"
[test] fn t() !void {
    var total = 0;
    var acc = mut |v| { total = total + v; return total; };
    try expect_eq(acc(3), 3);
    try expect_eq(acc(4), 7);
    try expect_eq(total, 7);
}
"#,
    );
}

#[test]
fn move_closure_returns_by_value() {
    // 按值返回规则：move 闭包捕获 base，脱离 make 作用域后仍可用（副本随返回值转移）
    run_ok(
        r#"
fn make() Fn1(i32) i32 {
    var base = 10;
    return move |v| v + base;
}
[test] fn t() !void {
    var f = make();
    try expect_eq(f(5), 15);
    try expect_eq(f(100), 110);
}
"#,
    );
}
