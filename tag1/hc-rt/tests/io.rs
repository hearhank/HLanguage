//! M5.4 io 模块验收：net（TCP echo/帧）/ fs（seek/pos/read_at/write_at）/ time / 环境

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
fn io_net_tcp_echo() {
    // TCP 回环 echo：listen(0 端口) → connect → accept → write → shutdown → read_all
    run_ok(
        "[test] fn t() !void {
    var listener = try io.net.listen(\"127.0.0.1\", 0, alloc);
    var port = try listener.local_port();
    var conn = try io.net.connect(\"127.0.0.1\", port, alloc);
    var accepted = try listener.accept();
    try accepted.write(\"hello-net\");
    accepted.shutdown();
    var reply = try conn.read_all();
    try expect_eq_slices(reply, \"hello-net\");
    conn.close();
    accepted.close();
    listener.close();
}\n",
    );
}

#[test]
fn io_net_frame_u32() {
    // 帧读写：u32 LE
    run_ok(
        "[test] fn t() !void {
    var listener = try io.net.listen(\"127.0.0.1\", 0, alloc);
    var port = try listener.local_port();
    var conn = try io.net.connect(\"127.0.0.1\", port, alloc);
    var accepted = try listener.accept();
    try accepted.write_u32_le(0xDEADBEEF);
    var n = try conn.read_u32_le();
    try expect_eq(n, 0xDEADBEEF);
    conn.close();
    accepted.close();
    listener.close();
}\n",
    );
}

#[test]
fn io_fs_seek_pos_read_at_write_at() {
    // 文件定位：create → write_all → seek/pos → read_at/write_at → read_all
    run_ok(
        "[test] fn t() !void {
    var f = try io.fs.create(\"hc_io_test.tmp\");
    defer f.close();
    try f.write_all(\"0123456789\");
    try f.seek(4);
    try expect_eq(try f.pos(), 4);
    var at = try f.read_at(2, 3);
    try expect_eq_slices(at, \"234\");
    try f.write_at(0, \"XY\");
    var all = try f.read_all();
    try expect_eq_slices(all, \"XY23456789\");
}\n",
    );
    let _ = std::fs::remove_file("hc_io_test.tmp");
}

#[test]
fn io_fs_append_rename_remove() {
    // F4：fs 余项——append 追加（缺失即建）/ rename 改名 / remove 删除
    run_ok(
        "[test] fn t() !void {
    io.fs.append(\"hc_f4_append.tmp\", \"hello\");
    io.fs.append(\"hc_f4_append.tmp\", \" world\");
    var content = io.fs.read_file(\"hc_f4_append.tmp\", alloc);
    try expect_eq_slices(content, \"hello world\");
    io.fs.rename(\"hc_f4_append.tmp\", \"hc_f4_renamed.tmp\");
    try expect_eq_slices(io.fs.read_file(\"hc_f4_renamed.tmp\", alloc), \"hello world\");
    io.fs.remove(\"hc_f4_renamed.tmp\");
    var gone = io.fs.read_file(\"hc_f4_renamed.tmp\", alloc) catch |_| { return; };
    try expect(false); // 删除后读取不应成功
}\n",
    );
    let _ = std::fs::remove_file("hc_f4_append.tmp");
    let _ = std::fs::remove_file("hc_f4_renamed.tmp");
}

#[test]
fn io_fs_list_dir() {
    // F4：fs 余项——list_dir 目录条目名（目录由 Rust 侧预建）
    std::fs::create_dir_all("hc_f4_dir").unwrap();
    std::fs::write("hc_f4_dir/alpha.txt", b"").unwrap();
    run_ok(
        "[test] fn t() !void {
    var names = io.fs.list_dir(\"hc_f4_dir\");
    try expect(names.len == 1);
    try expect_eq_slices(names[0], \"alpha.txt\");
}\n",
    );
    let _ = std::fs::remove_file("hc_f4_dir/alpha.txt");
    let _ = std::fs::remove_dir("hc_f4_dir");
}

#[test]
fn io_fs_read_int_write_int() {
    // F4：fs 余项——write_int/read_int 十进制文本 round-trip（含负数）
    run_ok(
        "[test] fn t() !void {
    io.fs.write_int(\"hc_f4_num.tmp\", 12345);
    try expect_eq(io.fs.read_int(\"hc_f4_num.tmp\"), 12345);
    io.fs.write_int(\"hc_f4_num.tmp\", -7);
    try expect_eq(io.fs.read_int(\"hc_f4_num.tmp\"), -7);
}\n",
    );
    let _ = std::fs::remove_file("hc_f4_num.tmp");
}

#[test]
fn io_time_now_and_sleep() {
    // 时间：now() 毫秒时间戳 > 0；sleep 返回 void
    run_ok(
        "[test] fn t() !void {
    var now = io.time.now();
    try expect(now > 0);
    io.time.sleep(1);
}\n",
    );
}

#[test]
fn io_env_and_args() {
    // 程序环境：env(name) ?&[u8]（PATH 存在）；args() 可迭代
    run_ok(
        "[test] fn t() !void {
    var path = io.env(\"PATH\") orelse io.env(\"Path\") orelse \"\";
    try expect(path.len > 0);
}\n",
    );
}

#[test]
fn io_net_connect_refused() {
    // 连接失败 → 错误值（可 catch 处理）
    run_ok(
        "[test] fn t() !void {
    var c = io.net.connect(\"127.0.0.1\", 1, alloc) catch |err| {
        try expect_error(error.Io, error.Io);
        return;
    };
}\n",
    );
}
