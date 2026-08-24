import H.std.{io};

// 90-thread-lifecycle.hc — 线程生命周期（组 G，真 OS 并行，E4）
//
// E4：spawn(f, args...) 立即启动 OS 线程执行 / join() !T 等待线程结束返回结果 /
// cancel() !void 设置取消标志（线程启动时检查）/ is_done() bool / detach() 分离。
// 每线程独立 alloc 实例（Q8）。全局变量不跨线程共享（线程各自独立 Interp 实例）。
// 线程间通信通过 Mutex / 通道进行。

fn add(a: i32, b: i32) i32 {
    return a + b;
}
fn bump(v: i32) i32 {
    return v + 1;
}
fn triple(a: i32, b: i32, c: i32) i32 {
    return a + b + c;
}

// Q8：每线程独立 alloc——worker 的 alloc.alloc bump 到自身 arena，
// 不进全局泄漏跟踪（根 alloc.leaks() 不变）
fn worker() usize {
    var buf = alloc.alloc(8);
    return alloc.bytes();
}

fn main() !void {
    // spawn 立即启动 OS 线程执行；join 等待线程结束并返回值
    var th = spawn(add, 6, 7);
    var r = try th.join();
    io.print("add(6, 7) = {}\n", r);   // 13

    // 多实参 + 值复制捕获（G3 安全捕获：值复制任意逃逸）
    var t2 = spawn(triple, 1, 2, 3);
    var r2 = try t2.join();
    io.print("triple(1, 2, 3) = {}\n", r2);   // 6

    // cancel：线程启动时检查标志，若已取消则返回 error.Cancelled（catch 默认值）
    // OS 线程模式下存在竞态——若线程在 cancel() 前已执行完毕，则 join 返回正常值
    var t3 = spawn(add, 1, 2);
    t3.cancel();
    var c = t3.join() catch 0;
    io.print("cancel join result = {}\n", c);   // 0（error.Cancelled 被 catch）或 3

    // detach：标记线程为分离（程序结束时不等待）
    var t4 = spawn(bump, 41);
    t4.detach();
    io.print("detached is_done = {}\n", t4.is_done());   // true（线程已执行完毕）

    // is_done 状态迁移：join 后 true
    var t5 = spawn(add, 1, 1);
    var r5 = try t5.join();
    io.print("after join = {}, is_done = {}\n", r5, t5.is_done());   // 2 true
}

[test] fn spawn_join_value() !void {
    var th = spawn(add, 6, 7);
    try expect_eq(try th.join(), 13);
    try expect_eq(th.is_done(), true);
}

[test] fn multi_arg_value_capture() !void {
    var base: i32 = 41;
    var th = spawn(bump, base);   // 值复制捕获（非引用）→ 逃逸安全
    try expect_eq(try th.join(), 42);
}

[test] fn cancel_then_join_cancelled() !void {
    var th = spawn(add, 1, 2);
    th.cancel();
    var r = th.join() catch 0;
    // OS 线程可能已取消（返回 Cancelled）或已执行完毕（返回 3），两种都正确
    try expect_eq(th.is_done(), true);
}

[test] fn detach_runs() !void {
    var th = spawn(add, 1, 2);
    th.detach();
    // detach 不阻塞，线程已标记为分离
}

[test] fn thread_own_alloc_q8() !void {
    var n0 = alloc.leaks();
    var th = spawn(worker);
    try expect_eq(try th.join(), 8);   // worker 自身 arena bytes
    try expect_eq(alloc.leaks(), n0);  // 不进全局泄漏跟踪
}