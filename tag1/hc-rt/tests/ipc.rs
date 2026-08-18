//! G3（E3.2 ipc）验收：进程内 IPC 原语——匿名管道（pipe）+ 命名共享内存（shm）。
//!
//! 形态：`io.ipc.pipe()` → `[reader, writer]`（2 元素数组，同 UDP recv_from 约定）；
//! 写端 `write(data)`/`close()`，读端 `read(alloc)`（排空可读字节，空且写端开 → 空切片，
//! 不阻塞——协作式模型）/`read_all(alloc)`/`is_closed()`/`close()`。
//! `io.ipc.shm(name, size)` → 定长共享字节区（write 覆盖截断到 size，read 取当前内容）。

use hc_rt::Interp;

fn run_ok(src: &str) {
    let program = hc::parse_source(src).unwrap_or_else(|d| panic!("parse: {:?}", d));
    let mut interp = Interp::new(src);
    interp
        .load(&program)
        .unwrap_or_else(|e| panic!("load: {} {}", e.name, e.message));
    let (p, f, _s) = interp.run_tests();
    assert_eq!(f, 0, "failed tests: {:?}", interp.test_out);
    assert!(p >= 1, "no tests ran");
}

#[test]
fn ipc_pipe_write_read() {
    // 管道基本流：写端写 → 读端读（排空）；再读为空切片
    run_ok(
        r#"[test] fn t() !void {
    var pair = try io.ipc.pipe();
    var reader = pair[0];
    var writer = pair[1];
    try writer.write("hello-ipc");
    var data = try reader.read(alloc);
    try expect_eq_slices(data, "hello-ipc");
    var empty = try reader.read(alloc);
    try expect(empty.len == 0);
    try reader.close();
    try writer.close();
}
"#,
    );
}

#[test]
fn ipc_pipe_accumulates_and_drains() {
    // 管道累积：多次写累积缓冲，read_all 排空全部
    run_ok(
        r#"[test] fn t() !void {
    var pair = try io.ipc.pipe();
    var reader = pair[0];
    var writer = pair[1];
    try writer.write("ab");
    try writer.write("cd");
    var data = try reader.read_all(alloc);
    try expect_eq_slices(data, "abcd");
    try reader.close();
    try writer.close();
}
"#,
    );
}

#[test]
fn ipc_pipe_close_is_closed() {
    // 写端关闭语义：is_closed 由 false → true；关闭后读端再读为空
    run_ok(
        r#"[test] fn t() !void {
    var pair = try io.ipc.pipe();
    var reader = pair[0];
    var writer = pair[1];
    try expect_eq(reader.is_closed(), false);
    try writer.write("x");
    try writer.close();
    try expect_eq(reader.is_closed(), true);
    var data = try reader.read(alloc);
    try expect_eq_slices(data, "x");
    var after = try reader.read(alloc);
    try expect(after.len == 0);
    try reader.close();
}
"#,
    );
}

#[test]
fn ipc_pipe_thread_producer() {
    // 管道跨执行上下文：H 线程（协作式 spawn/join）经管道向主流程传数据
    run_ok(
        r#"
fn produce(w: anytype) void {
    w.write("from-thread");
}
[test] fn t() !void {
    var pair = try io.ipc.pipe();
    var reader = pair[0];
    var writer = pair[1];
    var th = spawn(produce, writer);
    try th.join();
    var data = try reader.read(alloc);
    try expect_eq_slices(data, "from-thread");
    try reader.close();
    try writer.close();
}
"#,
    );
}

#[test]
fn ipc_shm_write_read() {
    // 共享内存基本流：shm(name, size) → write 覆盖 → read 取当前内容
    run_ok(
        r#"[test] fn t() !void {
    var s = try io.ipc.shm("g3_shm_1", 8);
    try s.write("hi");
    var data = try s.read(alloc);
    try expect_eq_slices(data, "hi");
    try s.close();
}
"#,
    );
}

#[test]
fn ipc_shm_truncates_to_size() {
    // 共享内存定长语义：write 超过 size 截断
    run_ok(
        r#"[test] fn t() !void {
    var s = try io.ipc.shm("g3_shm_2", 4);
    try s.write("0123456789");
    var data = try s.read(alloc);
    try expect_eq_slices(data, "0123");
    try s.close();
}
"#,
    );
}
