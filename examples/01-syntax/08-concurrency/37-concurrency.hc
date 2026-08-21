import H.std.{io};

// 37-concurrency.hc — 并发：线程、四模式类型、async
//
// Q18 定案（2026-08-13）：
//   - spawn(f, args...)：函数 + 显式参数；返回 o Thread<T>
//   - 作用域绑定：async 任务 await 后回到当前作用域 → 可捕获引用
//   - 逃逸线程：引用捕获禁用（编译期检查）
//   - 四模式类型：OneToOne / OneToMany / ManyToOne / ManyToMany

fn compute(x: i32, y: i32) i32 {
    return x * y;
}

async fn async_add(b: *i32, n: i32) i32 {
    return b.* + n;
}

fn main(args: o Vec<String>) !void {
    // 线程 = 数据对象（12.24）：spawn 归当前作用域，join 消耗所有权
    var t: o Thread<i32> = spawn(compute, 6, 7);
    var result = try t.join();
    io.print("result = {}\n", result);

    // 四模式共享容器：多读多写（写者互斥内建）
    var shared = ManyToMany<i32>.init(alloc);
    shared.write(42);
    io.print("shared = {}\n", shared.read());

    // async 任务：await 回到当前作用域 → 可捕获引用（&base 合法）
    var base = 10;
    var fut: Future<i32> = async_add(&base, 5);
    var total = await fut;
    io.print("total = {}\n", total);
}

[test] fn thread_join() !void {
    var t: o Thread<i32> = spawn(compute, 6, 7);
    var result = try t.join();
    try expect_eq(result, 42);
}

[test] fn four_mode_shared_container() !void {
    var shared = ManyToMany<i32>.init(alloc);
    shared.write(42);
    try expect_eq(shared.read(), 42);
}

[test] fn async_scope_binding() !void {
    var base = 10;
    var fut: Future<i32> = async_add(&base, 5);
    var total = await fut;   // 冻结窗口：await 前 base 不可写（Q19）
    try expect_eq(total, 15);
}
