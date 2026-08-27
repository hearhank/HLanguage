import H.std.{io};

// 04-ranges.hc — 区间迭代（Q29 定案 2026-08-13）
//
//   - for (0..10) |i|：区间糖（复用 .. 范围记号，底层仍是 while）
//   - while + 续步保留：自定义步长/条件

fn main() !void {
    // for 区间糖
    var mut sum = 0;
    for (0..10) |i| {
        sum += i;
    }
    io.print("sum = {}\n", sum);

    // while + 续步（自定义步长）
    var mut i = 0;
    while (i < 10) : (i += 2) {
        io.print("step {}\n", i);
    }

    // 区间 + 索引：下标迭代数组
    var arr = [10, 20, 30];
    for (0..arr.len) |idx| {
        io.print("arr[{}] = {}\n", idx, arr[idx]);
    }
}

[Test] fn for_range_sum() !void {
    var mut sum = 0;
    for (0..10) |i| {
        sum += i;
    }
    try expect_eq(sum, 45);
}

[Test] fn index_iteration() !void {
    var arr = [10, 20, 30];
    var mut total = 0;
    for (0..arr.len) |idx| {
        total += arr[idx];
    }
    try expect_eq(total, 60);
}
