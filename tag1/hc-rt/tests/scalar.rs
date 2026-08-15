//! M4.5 标量接口族（ICompare/INumber 族方法 + 运算符绑定）
//!
//! 86-scalar-interfaces 已覆盖 Int 方法形式与泛型 `.add`；此处锁定浮点方法族
//! （IFloat：abs/pow/mod/neg/eq/lt）与泛型约束下的**运算符形式**（`+` ≡ `.add`）。

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
fn float_method_family() {
    // IFloat 完整方法族（INumber: ICompare + abs/pow）
    run_ok(
        r#"
test fn t() !void {
    try expect_eq(1.5.add(2.5), 4.0);
    try expect_eq(2.0.sub(0.5), 1.5);
    try expect_eq(3.0.mul(2.0), 6.0);
    try expect_eq(5.0.div(2.0), 2.5);
    try expect_eq(2.0.pow(3.0), 8.0);
    try expect_eq((-3.5).abs(), 3.5);
    try expect_eq(2.0.neg(), -2.0);
    try expect_eq(5.5.mod(2.0), 1.5);
    try expect_eq(1.5.eq(1.5), true);
    try expect_eq(1.5.lt(2.5), true);
}
"#,
    );
}

#[test]
fn generic_operator_plus() {
    // 泛型约束 where T: INumber 下，运算符 `+` ≡ `.add`（类型擦除到 Int/Float）
    run_ok(
        r#"
fn sum_plus(items: &[T]) T where T: INumber {
    var total = items[0];
    for (items[1..]) |v| {
        total = total + v;
    }
    return total;
}
test fn t() !void {
    var ints = [10, 20, 30];
    try expect_eq(sum_plus(&ints), 60);
    var floats = [1.5, 2.5, 3.0];
    try expect_eq(sum_plus(&floats), 7.0);
}
"#,
    );
}
