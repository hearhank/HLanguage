//! 组 G1：`spawn(f, args...) owned Thread(T)` 协作式延迟执行 + Q8 每线程 alloc。
//!
//! tree-walking interp 侧验证：spawn 立即返回句柄（不并发运行）、join 运行到完成并返回
//! 结果、is_done 状态转移（false → true）、每线程独立 alloc 实例（bump 到自身 arena，
//! 不进全局泄漏跟踪）。G2 在 interp.rs 中扩展 cancel/detach/错误 union 传播矩阵。

use hc_rt::{Interp, Value};

/// 运行源码中所有 test fn；断言全部通过
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
fn spawn_join_returns_value() {
    // 基本 spawn/join：spawn(add, 6, 7) 立即返回句柄；join 运行 add 到完成返回 13
    run_ok(
        r#"
fn add(a: i32, b: i32) i32 { return a + b; }
[test] fn t() !void {
    var th = spawn(add, 6, 7);
    try expect_eq(try th.join(), 13);
}
"#,
    );
}

#[test]
fn thread_is_done_transitions() {
    // is_done 状态转移：spawn 后 false（未运行）→ join 后 true（已完成）
    run_ok(
        r#"
fn add(a: i32, b: i32) i32 { return a + b; }
[test] fn t() !void {
    var th = spawn(add, 6, 7);
    try expect_eq(th.is_done(), false);
    var r = try th.join();
    try expect_eq(r, 13);
    try expect_eq(th.is_done(), true);
}
"#,
    );
}

#[test]
fn thread_own_alloc_isolation() {
    // Q8：子任务绑定每线程独立 alloc 实例——worker 的 alloc.alloc(8) bump 到自身
    // arena（alloc.bytes() == 8），且不进全局泄漏跟踪（根 alloc.leaks() 不变）
    run_ok(
        r#"
fn worker() usize {
    var buf = alloc.alloc(8);
    return alloc.bytes();
}
[test] fn t() !void {
    var n0 = alloc.leaks();
    var th = spawn(worker);
    try expect_eq(try th.join(), 8);
    try expect_eq(alloc.leaks(), n0);
}
"#,
    );
}

#[test]
fn join_error_propagates() {
    // G2：join 透传子任务错误 union——may_fail 返回 error.Boom，join 返回同名错误
    run_ok(
        r#"
fn may_fail() !i32 { return error.Boom; }
[test] fn t() !void {
    var th = spawn(may_fail);
    try expect_error(error.Boom, th.join());
    try expect_eq(th.is_done(), true);
}
"#,
    );
}

#[test]
fn cancel_then_join_returns_cancelled() {
    // G2：cancel 置协作标志；未运行线程 join 返回 error.Cancelled 并置 done
    run_ok(
        r#"
fn work() i32 { return 42; }
[test] fn t() !void {
    var th = spawn(work);
    try expect_eq(th.is_done(), false);
    th.cancel();
    try expect_error(error.Cancelled, th.join());
    try expect_eq(th.is_done(), true);
}
"#,
    );
}

#[test]
fn detach_runs_side_effects() {
    // G2：detach 立即运行到完成并丢弃结果——全局副作用发生、句柄置 done
    run_ok(
        r#"
global g: i32 = 0;
fn bump() void { g = g + 1; }
[test] fn t() !void {
    var th = spawn(bump);
    th.detach();
    try expect_eq(g, 1);
    try expect_eq(th.is_done(), true);
}
"#,
    );
}

#[test]
fn unjoined_thread_runs_at_program_end() {
    // G2：未 join/未 detach 的线程在作用域退出时提升到根回收队列，
    // 程序（全部测试）结束时运行到完成——测试内 g 仍为 0（延迟），drain 后 g == 1
    let src = r#"
global g: i32 = 0;
fn bump() void { g = g + 1; }
[test] fn t() !void {
    {
        var th = spawn(bump);
    }
    try expect_eq(g, 0);
}
"#;
    let program = hc::parse_source(src).unwrap_or_else(|d| panic!("parse: {:?}", d));
    let mut interp = Interp::new(src);
    interp
        .load(&program)
        .unwrap_or_else(|e| panic!("load: {} {}", e.name, e.message));
    let (p, f, _s) = interp.run_tests();
    assert_eq!(f, 0, "failed: {:?}", interp.test_out);
    assert!(p >= 1, "no tests ran");
    // 根回收：run_tests 结束后 drain → bump 运行到完成 → 全局 g 由 0 变为 1
    match interp.global_value("g") {
        Some(Value::Int(v)) => assert_eq!(v, 1, "根回收线程应已运行到完成"),
        other => panic!("global g 应为 i32，实际 {other:?}"),
    }
}
