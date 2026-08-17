//! 组 E E2：`await` ≡ `join()`——async fn 调用点返回 `Future(R)`（延迟执行），
//! await 运行体到完成（协作式，复用 G 组 Thread 机制）；协作式取消。
//!
//! tree-walking interp 侧验证：async fn 调用不立即运行体（延迟）、await 运行到完成并
//! 返回结果、错误 union 传播（`Future(!R)`）、cancel 协作标志（await 前取消 →
//! `error.Cancelled`）、is_done 状态转移、内联 `await async_fn()`、await 幂等缓存。
//! 一致性（interp == IR）见 hc-rt/tests/consistency.rs `e2_async_await_consistent`。

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
fn await_returns_value() {
    // 基本 await：async fn 调用返回 Future(i32)，await 运行体到完成返回 42
    run_ok(
        r#"
async fn fetch() i32 { return 42; }
[test] fn t() !void {
    var fut: Future(i32) = fetch();
    try expect_eq(fut.is_done(), false);
    var r = await fut;
    try expect_eq(r, 42);
    try expect_eq(fut.is_done(), true);
}
"#,
    );
}

#[test]
fn inline_await() {
    // 内联 `await async_fn()`：不先存 Future 变量
    run_ok(
        r#"
async fn add(a: i32, b: i32) i32 { return a + b; }
[test] fn t() !void {
    var r = await add(3, 4);
    try expect_eq(r, 7);
}
"#,
    );
}

#[test]
fn async_body_runs_lazily() {
    // 延迟执行：async fn 调用点不运行体（全局副作用在 await 后才发生）
    run_ok(
        r#"
global g: i32 = 0;
async fn bump() i32 { g = g + 1; return g; }
[test] fn t() !void {
    var fut = bump();
    try expect_eq(g, 0);
    var r = await fut;
    try expect_eq(g, 1);
    try expect_eq(r, 1);
}
"#,
    );
}

#[test]
fn await_propagates_error_union() {
    // 错误 union：async fn 返回 `!i32`（Future(!i32)），await 透传 error 值（可 try/catch）
    run_ok(
        r#"
async fn may_fail(ok: bool) !i32 {
    if (!ok) { return error.Boom; }
    return 5;
}
[test] fn t() !void {
    var fut: Future(!i32) = may_fail(false);
    try expect_error(error.Boom, await fut);
    try expect_eq(try await may_fail(true), 5);
}
"#,
    );
}

#[test]
fn cancel_before_await_returns_cancelled() {
    // 协作式取消：cancel 置标志；未运行 Future await 返回 error.Cancelled 并置 done
    run_ok(
        r#"
async fn work() !i32 { return 42; }
[test] fn t() !void {
    var fut: Future(!i32) = work();
    try expect_eq(fut.is_done(), false);
    fut.cancel();
    try expect_error(error.Cancelled, await fut);
    try expect_eq(fut.is_done(), true);
}
"#,
    );
}

#[test]
fn await_after_await_is_cached() {
    // 幂等：await 已完成的 Future 返回缓存 result（体不重跑——全局副作用只发生一次）
    run_ok(
        r#"
global g: i32 = 0;
async fn bump() i32 { g = g + 1; return g; }
[test] fn t() !void {
    var fut = bump();
    var r1 = await fut;
    try expect_eq(r1, 1);
    var r2 = await fut;
    try expect_eq(r2, 1);
    try expect_eq(g, 1);
}
"#,
    );
}

#[test]
fn nested_await_in_async_body() {
    // async 体内可再 await 内层 Future（递归协作式运行到完成）
    run_ok(
        r#"
async fn inner(n: i32) i32 { return n * 2; }
async fn outer(n: i32) i32 { return await inner(n) + 1; }
[test] fn t() !void {
    var r = await outer(10);
    try expect_eq(r, 21);
}
"#,
    );
}
