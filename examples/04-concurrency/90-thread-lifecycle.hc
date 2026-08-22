import H.std.{io};

// 90-thread-lifecycle.hc — 线程生命周期（组 G，协作式延迟执行，2026-08-17 落地）
//
// E2.2 提前落地：spawn(f, args...) o Thread<T> / join() !T / cancel() !void /
// is_done() bool / detach() !void。并发模型 = 协作式延迟执行：spawn 立即返回句柄
// 但不并发运行，join / detach / 程序结束时才执行到完成（确定性、单线程）。
// 每线程独立 alloc 实例（Q8）。四模式类型 / async / await / @atomic / mutex
// 明确留第三块。
//
// 捕获规则（组 G3）：值复制 / &global / move 捕获安全、可任意逃逸；&局部 捕获须在
// 声明作用域内 join（Q18 绑定）；spawn→join 之间冻结被捕获引用目标（Q19 违例）。
// 原生后端为子集边界：spawn 需函数引用（FnRef），原生 ABI 未支持（Phase 8），
// 编译模式响亮拒绝（error.NotCallable），不静默误编译。

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

global g: i32 = 0;
fn bump_g() void {
    g = g + 1;
}

fn main() !void {
    // spawn 立即返回句柄；join 运行到完成并返回值
    var th = spawn(add, 6, 7);
    var r = try th.join();
    io.print("add(6, 7) = {}\n", r);   // 13

    // 多实参 + 值复制捕获（G3 安全捕获：值复制任意逃逸）
    var t2 = spawn(triple, 1, 2, 3);
    var r2 = try t2.join();
    io.print("triple(1, 2, 3) = {}\n", r2);   // 6

    // cancel：未运行线程 join → error.Cancelled（catch 默认值）
    var t3 = spawn(add, 1, 2);
    t3.cancel();
    var c = t3.join() catch 0;
    io.print("cancel join result = {}\n", c);   // 0（error.Cancelled 被 catch）

    // detach：立即运行到完成并丢弃结果（副作用发生）
    var t4 = spawn(bump, 41);
    t4.detach();
    io.print("detached is_done = {}\n", t4.is_done());   // true

    // is_done 状态迁移：join 前 false → join 后 true
    var t5 = spawn(add, 1, 1);
    io.print("before join is_done = {}\n", t5.is_done());   // false
    var r5 = try t5.join();
    io.print("after join = {}, is_done = {}\n", r5, t5.is_done());   // 2 true
}

[test] fn spawn_join_value() !void {
    var th = spawn(add, 6, 7);
    try expect_eq(th.is_done(), false);
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
    try expect_error(error.Cancelled, th.join());
    try expect_eq(th.is_done(), true);
}

[test] fn detach_runs_side_effect() !void {
    var th = spawn(bump_g);
    th.detach();
    try expect_eq(g, 1);               // detach 立即运行到完成（副作用发生）
    try expect_eq(th.is_done(), true);
}

[test] fn thread_own_alloc_q8() !void {
    var n0 = alloc.leaks();
    var th = spawn(worker);
    try expect_eq(try th.join(), 8);   // worker 自身 arena bytes
    try expect_eq(alloc.leaks(), n0);  // 不进全局泄漏跟踪
}
