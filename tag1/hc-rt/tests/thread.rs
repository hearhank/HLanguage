//! 组 G1：`spawn(f, args...) o Thread(T)` 协作式延迟执行 + Q8 每线程 alloc。
//!
//! tree-walking interp 侧验证：spawn 立即返回句柄（不并发运行）、join 运行到完成并返回
//! 结果、is_done 状态转移（false → true）、每线程独立 alloc 实例（bump 到自身 arena，
//! 不进全局泄漏跟踪）。G2 在 interp.rs 中扩展 cancel/detach/错误 union 传播矩阵。

use hc_rt::Interp;

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
