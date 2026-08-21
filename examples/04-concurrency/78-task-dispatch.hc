import H.std.{io};

// 78-task-dispatch.hc — 多线程任务分发（四模式类型）
//
//   - 任务队列：ManyToOne<i32>（单写者分发 + 多读者 worker）
//   - 结果汇合：OneToMany<i32>（多写者 worker + 单读者主线程）
//   - 容器方法取 *Self（内建共享特例，Q32）；作用域绑定可捕获引用（Q18）

fn worker(tasks: *ManyToOne<i32>, out: *OneToMany<i32>) void {
    while (tasks.try_read()) |task| {
        out.write(task * task);
    }
}

fn main(args: o Vec<String>) !void {
    var tasks = ManyToOne<i32>.init(alloc);
    var out = OneToMany<i32>.init(alloc);

    // 分发（单写者）
    for (0..20) |i| {
        tasks.write(i);
    }
    tasks.close();                     // 结束标志

    // 3 个 worker（多读者，各持 &tasks/&out）
    var t1 = spawn(worker, &tasks, &out);
    var t2 = spawn(worker, &tasks, &out);
    var t3 = spawn(worker, &tasks, &out);
    try t1.join();
    try t2.join();
    try t3.join();

    // 汇合（单读者）
    var total = 0;
    while (out.try_read()) |v| {
        total += v;
    }
    io.print("total = {}\n", total);   // 0²+1²+…+19² = 2470
}

[test] fn task_dispatch() !void {
    var tasks = ManyToOne<i32>.init(alloc);
    var out = OneToMany<i32>.init(alloc);
    for (0..20) |i| {
        tasks.write(i);
    }
    tasks.close();
    var t1 = spawn(worker, &tasks, &out);
    var t2 = spawn(worker, &tasks, &out);
    var t3 = spawn(worker, &tasks, &out);
    try t1.join();
    try t2.join();
    try t3.join();
    var total = 0;
    while (out.try_read()) |v| {
        total += v;
    }
    try expect_eq(total, 2470);   // 0²+1²+…+19²
}
