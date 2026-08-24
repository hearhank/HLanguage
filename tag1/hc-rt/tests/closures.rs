//! hc-rt/tests/closures.rs

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

/// 期望运行失败：至少一个 [test] FAIL 且输出含指定错误名（未检查具体测试数）。
fn run_fail_contains(src: &str, err: &str) {
    let program = hc::parse_source(src).unwrap_or_else(|d| panic!("parse: {:?}", d));
    let mut interp = Interp::new(src);
    interp
        .load(&program)
        .unwrap_or_else(|e| panic!("load: {} {}", e.name, e.message));
    let (_p, f, _s) = interp.run_tests();
    assert!(f >= 1, "expected FAIL, got: {:?}", interp.test_out);
    let joined = interp.test_out.join("\n");
    assert!(
        joined.contains(err),
        "expected error `{err}` in test_out: {joined}"
    );
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
fn make() Fn1<i32> i32 {
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

// ---------- Phase 8：捕获精确化 + is_mut 只读强制 + move 深拷贝补全 ----------

#[test]
fn non_mut_closure_cannot_rebind_capture() {
    // 只读强制：非 `mut` 闭包内重绑定被捕获变量 → ReadonlyCapture
    run_fail_contains(
        r#"
[test] fn t() void {
    var total = 0;
    var acc = |v| { total = total + v; return total; };
    acc(3);
}
"#,
        "error.ReadonlyCapture",
    );
}

#[test]
fn non_mut_closure_cannot_compound_assign_capture() {
    // 只读强制同样覆盖复合赋值（x += v → x = x + v）
    run_fail_contains(
        r#"
[test] fn t() void {
    var n = 10;
    var f = |v| { n += v; };
    f(5);
}
"#,
        "error.ReadonlyCapture",
    );
}

#[test]
fn non_mut_closure_can_rebind_shadowed_local() {
    // 只读限制只作用于捕获变量：体内部署的遮蔽局部变量可自由重绑定
    run_ok(
        r#"
[test] fn t() void {
    var x = 1;
    var f = | | { var x = 10; x = 20; return x; };
    expect_eq(f(), 20);
    expect_eq(x, 1);
}
"#,
    );
}

#[test]
fn mut_closure_writes_shared_capture_visible_to_nested() {
    // mut 闭包写被捕获变量对原绑定可见；且嵌套非 mut 闭包读同一捕获仍只读
    run_ok(
        r#"
[test] fn t() void {
    var total = 0;
    var acc = mut |v| { total = total + v; return total; };
    var read = |v| total + v;      // 只读嵌套闭包共享 total
    expect_eq(acc(3), 3);
    expect_eq(read(1), 4);
    expect_eq(total, 3);
}
"#,
    );
}

#[test]
fn move_closure_deep_copies_closure_capture() {
    // move 捕获闭包值：深拷贝其环境——原闭包捕获的变量后续变更不影响 move 副本
    // （若仅共享 Rc，`inner` 的捕获 cell 被复用 → outer_move 会看到新值 101）。
    run_ok(
        r#"
[test] fn t() void {
    var x = 1;
    var inner = |v| v + x;         // inner 捕获 x（共享）
    var outer_move = move | | inner(1);  // move 捕获 inner → 深拷贝其环境副本
    x = 100;
    expect_eq(outer_move(), 2);    // 深拷贝 env 的 x 仍为 1 → 1+1=2
}
"#,
    );
}

#[test]
fn read_closure_shared_capture_visible_to_nested() {
    // 对照：只读闭包值（非 move）捕获共享 → 原闭包捕获变量变更对闭包可见
    run_ok(
        r#"
[test] fn t() void {
    var x = 1;
    var inner = |v| v + x;
    var outer = | | inner(1);      // 非 move：inner 值共享（捕获 cell 未复制）
    x = 100;
    expect_eq(outer(), 101);
}
"#,
    );
}

#[test]
fn nested_closure_transitive_capture() {
    // 嵌套闭包传递：外层闭包体只在内层闭包体内引用外部变量 → 仍须捕获
    // （外层创建内层时需提供该变量）；未被内层引用的无关变量不捕获。
    run_ok(
        r#"
[test] fn t() void {
    var a = 1;
    var b = 2;
    var f = | | {
        var g = |v| v + a;         // 只引用 a
        return g(10);
    };
    a = 100;
    expect_eq(f(), 110);           // f 捕获 a（共享），经嵌套 g 读取
    expect_eq(b, 2);
}
"#,
    );
}

#[test]
fn move_closure_isolation_after_external_change() {
    // 捕获后外部变量再变（对照）：只读捕获共享（闭包可见变更），
    // move 捕获独立（闭包持创建时副本）——与 read_closure_shares_slot 对照。
    run_ok(
        r#"
[test] fn t() void {
    var s = "hello";
    var shared = | | s.len();      // 只读捕获：共享槽
    var copied = move | | s.len(); // move 捕获：深拷贝独立 Str
    s = "hello world";             // 重绑定原变量
    expect_eq(shared(), 11);       // 共享 → 新值
    expect_eq(copied(), 5);        // 副本 → 创建时值
}
"#,
    );
}
