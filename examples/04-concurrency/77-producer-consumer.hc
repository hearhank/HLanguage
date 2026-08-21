import H.std.{io};

// 77-producer-consumer.hc — 生产者-消费者（线程 + 四模式类型）
//
// Q32 定案（2026-08-13）：四模式类型 = 内建共享特例
//   - 容器方法取 *Self（只读引用）：read/write 通过 &ch 调用，内部同步
//   - 不占用唯一写者槽；多线程可同时持 &ch
//   - 作用域绑定（join 后回到当前作用域）→ 引用捕获允许（Q18）
//   - 仅四模式类型可模拟（用户类型不可——需唯一写者）

fn producer(ch: *OneToOne<i32>, n: i32) void {
    for (0..n) |i| {
        ch.write(i * i);
    }
}

fn consumer(ch: *OneToOne<i32>, count: i32) i32 {
    var sum = 0;
    for (0..count) |_| {
        sum += ch.read();
    }
    return sum;
}

fn main(args: o Vec<String>) !void {
    var ch: o OneToOne<i32> = OneToOne<i32>.init(alloc);

    // 两个线程共享同一容器：各持 &ch（内建共享特例，Q32）
    var p_thread: o Thread<void> = spawn(producer, &ch, 10);
    var c_thread: o Thread<i32> = spawn(consumer, &ch, 10);

    try p_thread.join();
    var sum = try c_thread.join();
    io.print("sum = {}\n", sum);   // 0²+1²+…+9² = 285
}

[test] fn producer_consumer() !void {
    var ch: o OneToOne<i32> = OneToOne<i32>.init(alloc);
    var p_thread: o Thread<void> = spawn(producer, &ch, 10);
    var c_thread: o Thread<i32> = spawn(consumer, &ch, 10);
    try p_thread.join();
    var sum = try c_thread.join();
    try expect_eq(sum, 285);   // 0²+1²+…+9²
}
