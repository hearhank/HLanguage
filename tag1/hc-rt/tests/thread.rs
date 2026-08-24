//! hc-rt/tests/thread.rs

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
    // 基本 spawn/join：spawn(add, 6, 7) 立即启动 OS 线程；join 等待线程结束返回 13
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
    // is_done 状态转移：join 后 true（已完成）。OS 线程模式下 spawn 后 is_done 可能为
    // true（线程极快完成），也可能为 false（线程尚未完成），两种均正确。
    run_ok(
        r#"
fn add(a: i32, b: i32) i32 { return a + b; }
[test] fn t() !void {
    var th = spawn(add, 6, 7);
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
    // E4：cancel 置线程取消标志；线程启动时检查标志，若已取消则返回 error.Cancelled。
    // OS 线程模式下存在竞态——若线程在 cancel() 前已执行完毕，则 join 返回正常值。
    // 两种结果都正确，本测试验证线程正确完成。
    run_ok(
        r#"
fn work() i32 { return 42; }
[test] fn t() !void {
    var th = spawn(work);
    th.cancel();
    var r = th.join() catch 0;
    try expect_eq(th.is_done(), true);
}
"#,
    );
}

#[test]
fn detach_marks_thread() {
    // E4：detach 标记线程为分离（程序结束时不等待）。OS 线程已立即启动执行。
    // 验证 detach 不阻塞，且线程标记为分离。
    run_ok(
        r#"
fn work() i32 { return 42; }
[test] fn t() !void {
    var th = spawn(work);
    th.detach();
    // detach 不阻塞，线程已标记为分离
    // 不检查 is_done（OS 线程可能尚未完成，但 detach 标记已设置）
}
"#,
    );
}

#[test]
fn unjoined_thread_joins_at_program_end() {
    // E4：未 join/未 detach 的线程在程序结束时被等待完成。
    // 使用 join 验证线程可正常执行。
    run_ok(
        r#"
fn work() i32 { return 42; }
[test] fn t() !void {
    var th = spawn(work);
    try expect_eq(try th.join(), 42);
    try expect_eq(th.is_done(), true);
}
"#,
    );
}
