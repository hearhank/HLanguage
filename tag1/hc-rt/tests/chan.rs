//! 组 G1：`chan<T>` 通道测试（E4：M:N 协程通信）。
//!
//! 验证 chan.init(alloc[, cap]) 构造、send/recv/try_send/try_recv/close 方法。
//! 通道使用 Mutex+Condvar 实现阻塞式 send/recv，非阻塞操作返回 bool/Opt。
//!
//! 注：spawn 模式下子线程拥有独立 Interp 实例，通道通过 Arc<ChanState> 跨线程共享。

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
fn chan_init_unbuffered() {
    // 无缓冲通道创建
    run_ok(
        r#"
[test] fn t() !void {
    var ch = chan.init(alloc);
    // 无缓冲通道：try_send 因为没有接收者而失败
    try expect_eq(ch.try_send(42), false);
}
"#,
    );
}

#[test]
fn chan_init_buffered() {
    // 有缓冲通道创建
    run_ok(
        r#"
[test] fn t() !void {
    var ch = chan.init(alloc, 3);
    try expect_eq(ch.try_send(10), true);
    try expect_eq(ch.try_send(20), true);
    try expect_eq(ch.try_send(30), true);
    try expect_eq(ch.try_send(40), false);  // 缓冲满
}
"#,
    );
}

#[test]
fn chan_try_recv() {
    // try_recv 非阻塞读取
    run_ok(
        r#"
[test] fn t() !void {
    var ch = chan.init(alloc, 2);
    try expect_eq(ch.try_recv(), null);  // 空通道
    try expect_eq(ch.try_send(99), true);
    var v = ch.try_recv();
    try expect_neq(v, null);
}
"#,
    );
}

#[test]
fn chan_close() {
    // 关闭通道后不能再发送
    run_ok(
        r#"
[test] fn t() !void {
    var ch = chan.init(alloc, 2);
    ch.close();
    try expect_error(error.Closed, ch.send(42));
    try expect_eq(ch.try_send(42), false);
}
"#,
    );
}

#[test]
fn chan_close_wakes_receiver() {
    // 关闭通道后接收者收到 Closed 错误
    run_ok(
        r#"
[test] fn t() !void {
    var ch = chan.init(alloc, 2);
    ch.close();
    try expect_error(error.Closed, ch.recv());
}
"#,
    );
}

#[test]
fn chan_send_recv_buffered() {
    // 有缓冲通道 try_send/try_recv 循环
    run_ok(
        r#"
[test] fn t() !void {
    var ch = chan.init(alloc, 5);
    try expect_eq(ch.try_send(1), true);
    try expect_eq(ch.try_send(2), true);
    try expect_eq(ch.try_send(3), true);
    var v1 = ch.try_recv();
    try expect_neq(v1, null);
    var v2 = ch.try_recv();
    try expect_neq(v2, null);
    var v3 = ch.try_recv();
    try expect_neq(v3, null);
    try expect_eq(ch.try_recv(), null);
}
"#,
    );
}

#[test]
fn chan_spawn_send_recv() {
    // spawn + 通道：子线程发送，主线程接收
    run_ok(
        r#"
fn sender(ch: Chan) void {
    ch.send(42);
}
[test] fn t() !void {
    var ch = chan.init(alloc, 1);
    var th = spawn(sender, ch);
    var v = try ch.recv();
    try expect_eq(v, 42);
    try th.join();
}
"#,
    );
}

#[test]
fn chan_spawn_recv_from_child() {
    // spawn + 通道：主线程发送，子线程接收
    run_ok(
        r#"
fn receiver(ch: Chan) void {
    var v = try ch.recv();
    try expect_eq(v, 99);
}
[test] fn t() !void {
    var ch = chan.init(alloc, 1);
    var th = spawn(receiver, ch);
    ch.send(99);
    try th.join();
}
"#,
    );
}

#[test]
fn chan_spawn_multiple_sends() {
    // spawn + 通道：子线程发送多个值，主线程接收
    run_ok(
        r#"
fn sender(ch: Chan) void {
    ch.send(10);
    ch.send(20);
    ch.send(30);
}
[test] fn t() !void {
    var ch = chan.init(alloc, 3);
    var th = spawn(sender, ch);
    try expect_eq(try ch.recv(), 10);
    try expect_eq(try ch.recv(), 20);
    try expect_eq(try ch.recv(), 30);
    try th.join();
}
"#,
    );
}
