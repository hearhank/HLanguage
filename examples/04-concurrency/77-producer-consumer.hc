import H.std.{io};

// 77-producer-consumer.hc — 生产者-消费者（协程 + 通道）
//
// E4：spawn 创建协程（goroutine），chan 通道通信
//   - 生产者通过 chan.send() 发送数据
//   - 消费者通过 chan.recv() 接收数据
//   - 值复制捕获：协程安全逃逸，无共享数据

fn producer(ch: *chan, n: i32) void {
    var mut i: i32 = 0;
    while (i < n) {
        ch.send(i * i);
        i += 1;
    }
    ch.close();
}

fn consumer(ch: *chan, count: i32) i32 {
    var mut sum = 0;
    var mut received = 0;
    while (received < count) {
        sum += ch.recv();
        received += 1;
    }
    return sum;
}

fn send_one(c: *chan) void {
    c.send(1);
}

fn send_two(c: *chan) void {
    c.send(2);
}

fn send_three(c: *chan) void {
    c.send(3);
}

fn main() !void {
    var ch = chan.init(alloc, 10);  // 有缓冲通道

    // 生产者与消费者各持 &ch（通道内建线程安全）
    var p_thread: owned Thread<void> = spawn(producer, &ch, 10);
    defer p_thread.deinit();
    var c_thread: owned Thread<i32> = spawn(consumer, &ch, 10);
    defer c_thread.deinit();

    try p_thread.join();
    var sum = try c_thread.join();
    io.print("sum = {}\n", sum);   // 0²+1²+…+9² = 285
}

[Test] fn producer_consumer_sum() !void {
    var ch = chan.init(alloc, 10);
    var p_thread: owned Thread<void> = spawn(producer, &ch, 10);
    defer p_thread.deinit();
    var c_thread: owned Thread<i32> = spawn(consumer, &ch, 10);
    defer c_thread.deinit();
    try p_thread.join();
    var sum = try c_thread.join();
    try expect_eq(sum, 285);   // 0²+1²+…+9²
}

[Test] fn multi_producer() !void {
    var ch = chan.init(alloc, 10);
    var t1 = spawn(send_one, &ch);
    var t2 = spawn(send_two, &ch);
    var t3 = spawn(send_three, &ch);
    try t1.join();
    try t2.join();
    try t3.join();
    var mut sum = 0;
    var mut i: i32 = 0;
    while (i < 3) {
        sum += ch.recv();
        i += 1;
    }
    try expect_eq(sum, 6);
}
