// 04-ranges.hc — 区间迭代（Q29 定案 2026-08-13）
//
//   - for (0..10) |i|：区间糖（复用 .. 范围记号，底层仍是 while）
//   - while + 续步保留：自定义步长/条件

fn main(io: Io) !void {
    // for 区间糖
    var sum = 0;
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
