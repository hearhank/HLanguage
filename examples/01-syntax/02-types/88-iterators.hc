// 88-iterators.hc — 迭代契约（IIterable 三态，2026-08-14 定案）
//
//   - for (x) |item| 走迭代接口：IIterable(*T) 只读 / IIterable(*mut T) 可写 / IIterable(o T) 拥有
//   - 内建类型（数组/切片/Vec/Map/Table/String）编译器内建实现三态
//   - 用户类型实现迭代接口即可参与 for；arr.iter() 迭代器 = 显式数据对象

// 用户类型：斐波那契数列（一次性迭代器，next 消耗状态）
[continuous]   // 全值字段 → 连续内存值类型（H1 特性标注）
class Fib: IIterable(i32) {
    mut a: i32,
    mut b: i32,
    mut remaining: i32,

    fn next(self: *mut Self) ?i32 {
        if (self.remaining <= 0) {
            return null;   // 迭代结束
        }
        var cur = self.a;
        var nb = self.a + self.b;
        self.a = self.b;
        self.b = nb;
        self.remaining -= 1;
        return cur;
    }
}

fn main(io: Io) !void {
    // 内建类型：只读迭代（IIterable(*T) 默认形态）
    var nums = [1, 2, 3, 4, 5];
    var sum = 0;
    for (nums) |n| {
        sum += n;
    }
    io.print("sum = {}\n", sum);   // 15

    // 内建类型：可写迭代（IIterable(*mut T)，|mut item| 捕获）
    var mut arr = [1, 2, 3];
    for (arr) |mut item| {
        item *= 10;
    }
    io.print("{} {} {}\n", arr[0], arr[1], arr[2]);   // 10 20 30

    // 用户类型迭代（实现 IIterable(i32)）
    var fib = Fib{ a = 0, b = 1, remaining = 6 };
    for (fib) |f| {
        io.print("{} ", f);        // 0 1 1 2 3 5
    }
    io.print("\n");

    // 迭代器 = 显式数据对象（可传递/组合；arr.iter()）
    var it = nums.iter();
    io.print("{}\n", it.next().?);   // 1
}

[test] fn builtin_readonly_iteration() !void {
    var nums = [1, 2, 3, 4, 5];
    var sum = 0;
    for (nums) |n| {
        sum += n;
    }
    try expect_eq(sum, 15);
}

[test] fn builtin_mutable_iteration() !void {
    var mut arr = [1, 2, 3];
    for (arr) |mut item| {
        item *= 10;
    }
    try expect_eq(arr[0], 10);
    try expect_eq(arr[2], 30);
}

[test] fn user_type_iteration() !void {
    var fib = Fib{ a = 0, b = 1, remaining = 6 };
    var got = Vec(i32).init(alloc);
    for (fib) |f| {
        got.append(f);
    }
    try expect_eq(got.len, 6);
    try expect_eq(got[0], 0);
    try expect_eq(got[1], 1);
    try expect_eq(got[5], 5);
}
