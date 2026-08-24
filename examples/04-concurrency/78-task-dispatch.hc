import H.std.{io};

// 78-task-dispatch.hc — 多协程任务分发（通道 + 协程池）
//
// E4：chan 替代原四模式类型
//   - 任务队列：chan（分发者 send + 工作者 recv）
//   - 结果汇合：chan（工作者 send + 分发者 recv）
//   - 使用大缓冲队列避免 send 阻塞

fn worker(tasks: *chan, out: *chan) void {
    while (true) {
        var task = tasks.try_recv();
        if (task) |t| {
            out.send(t * t);
        } else {
            break;
        }
    }
}

fn main() !void {
    var tasks = chan.init(alloc, 20);
    var out = chan.init(alloc, 20);

    // 先发送所有任务到缓冲通道
    var i: i32 = 0;
    while (i < 20) {
        tasks.send(i);
        i += 1;
    }

    // 再启动工作者
    var t1 = spawn(worker, &tasks, &out);
    var t2 = spawn(worker, &tasks, &out);
    var t3 = spawn(worker, &tasks, &out);
    try t1.join();
    try t2.join();
    try t3.join();

    // 汇合结果
    var total = 0;
    while (true) {
        var v = out.try_recv();
        if (v) |val| {
            total += val;
        } else {
            break;
        }
    }
    io.print("total = {}\n", total);   // 0²+1²+…+19² = 2470
}

[test] fn task_dispatch() !void {
    var tasks = chan.init(alloc, 20);
    var out = chan.init(alloc, 20);
    var i: i32 = 0;
    while (i < 20) {
        tasks.send(i);
        i += 1;
    }
    var t1 = spawn(worker, &tasks, &out);
    var t2 = spawn(worker, &tasks, &out);
    var t3 = spawn(worker, &tasks, &out);
    try t1.join();
    try t2.join();
    try t3.join();
    var total = 0;
    while (true) {
        var v = out.try_recv();
        if (v) |val| {
            total += val;
        } else {
            break;
        }
    }
    try expect_eq(total, 2470);   // 0²+1²+…+19²
}