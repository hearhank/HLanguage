import H.std.{io};

// 76-threads-edge.hc — 线程边缘语义（12.21/12.24）
//
//   - Thread 接口：join（消耗所有权）/ cancel / is_done / detach
//   - 四模式类型全演示：OneToOne / OneToMany / ManyToOne / ManyToMany
//   - 线程捕获：值复制 / move / global；作用域绑定可捕获引用（Q18）

fn worker(x: i32) i32 {
    return x * x;
}

async fn async_add(b: *i32, n: i32) i32 {
    return b.* + n;
}

fn main(args: o Vec<String>) !void {
    // Thread 接口：is_done / join（消耗所有权，返回 !T）
    var t: o Thread<i32> = spawn(worker, 9);
    io.print("is_done = {}\n", t.is_done());
    var r = try t.join();
    io.print("result = {}\n", r);

    // detach：显式放弃结果（线程继续，根作用域回收）
    var t2: o Thread<i32> = spawn(worker, 3);
    t2.detach();

    // 四模式类型（写者数量由类型名保证：单写者无锁、多写者互斥）
    var s1 = OneToOne<i32>.init(alloc);    // 单读单写
    var s2 = OneToMany<i32>.init(alloc);   // 单读多写
    var s3 = ManyToOne<i32>.init(alloc);   // 多读单写
    var s4 = ManyToMany<i32>.init(alloc);  // 多读多写（互斥）
    s4.write(1);
    io.print("shared = {}\n", s4.read());

    // 作用域绑定：async 任务可捕获引用（Q18，await 回到当前作用域）
    var base = 5;
    var fut: Future<i32> = async_add(&base, 10);
    var total = await fut;
    io.print("total = {}\n", total);
}

[test] fn thread_interface() !void {
    var t: o Thread<i32> = spawn(worker, 9);
    var r = try t.join();
    try expect_eq(r, 81);
}

[test] fn four_mode_types() !void {
    var s1 = OneToOne<i32>.init(alloc);
    s1.write(2);
    try expect_eq(s1.read(), 2);
    var s4 = ManyToMany<i32>.init(alloc);
    s4.write(1);
    try expect_eq(s4.read(), 1);
}

[test] fn async_scope_binding() !void {
    var base = 5;
    var fut: Future<i32> = async_add(&base, 10);
    var total = await fut;   // 冻结窗口：await 前 base 不可写（Q19）
    try expect_eq(total, 15);
}
