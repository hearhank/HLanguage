import H.std.{io};

// 76-threads-edge.hc — 线程边缘语义（E4：协程 + 通道）
//
//   - spawn(f, args...) 创建协程（goroutine），返回 Thread 句柄
//   - Thread 接口：join / cancel / is_done / detach
//   - chan 通道：init(alloc[, cap]) / send / recv / try_send / try_recv / close
//   - 协程捕获：值复制安全逃逸；每协程独立 alloc（Q8）

fn worker(x: i32) i32 {
    return x * x;
}

fn chan_sender(c: *chan) void {
    c.send(42);
}

fn main() !void {
    // Thread 接口：spawn / join（等待协程结束，返回 !T）
    var t: owned Thread<i32> = spawn(worker, 9);
    defer t.deinit();
    var r = try t.join();
    io.print("result = {}\n", r);

    // detach：显式放弃结果（协程继续，程序结束时不等待）
    var t2: owned Thread<i32> = spawn(worker, 3);
    defer t2.deinit();
    t2.detach();

    // 通道（chan）：缓冲通道
    var ch = chan.init(alloc, 1);
    var sender: owned Thread<void> = spawn(chan_sender, &ch);
    defer sender.deinit();
    try sender.join();
    var val = ch.recv();
    io.print("chan recv = {}\n", val);

    // 多值缓冲通道
    var buf_ch = chan.init(alloc, 3);
    buf_ch.send(1);
    buf_ch.send(2);
    buf_ch.send(3);
    io.print("buf_ch recv = {}\n", buf_ch.recv());
    buf_ch.close();

    // is_done 状态：join 后为 true
    var t3: owned Thread<i32> = spawn(worker, 5);
    defer t3.deinit();
    var r3 = try t3.join();
    io.print("after join = {}, is_done = {}\n", r3, t3.is_done());
}

[Test] fn thread_join_value() !void {
    var t: owned Thread<i32> = spawn(worker, 9);
    defer t.deinit();
    var r = try t.join();
    try expect_eq(r, 81);
    try expect_eq(t.is_done(), true);
}

[Test] fn channel_send_recv() !void {
    var ch = chan.init(alloc, 1);
    var sender: owned Thread<void> = spawn(chan_sender, &ch);
    defer sender.deinit();
    try sender.join();
    var val = ch.recv();
    try expect_eq(val, 42);
    ch.close();
}

[Test] fn buffered_channel() !void {
    var ch = chan.init(alloc, 3);
    ch.send(10);
    ch.send(20);
    ch.send(30);
    try expect_eq(ch.recv(), 10);
    try expect_eq(ch.recv(), 20);
    try expect_eq(ch.recv(), 30);
    ch.close();
}

[Test] fn try_send_try_recv() !void {
    var ch = chan.init(alloc, 1);
    try expect_eq(ch.try_send(1), true);
    try expect_eq(ch.try_send(2), false);   // 缓冲区满
    try expect_eq(ch.try_recv().?, 1);
    try expect_eq(ch.try_recv(), null);      // 缓冲区空
}

[Test] fn channel_close() !void {
    var ch = chan.init(alloc, 1);
    ch.close();
}

[Test] fn detach_runs() !void {
    var t: owned Thread<i32> = spawn(worker, 3);
    defer t.deinit();
    t.detach();
}

[Test] fn cancel_then_join() !void {
    var t = spawn(worker, 5);
    t.cancel();
    var r = t.join() catch 0;
    try expect_eq(t.is_done(), true);
}
