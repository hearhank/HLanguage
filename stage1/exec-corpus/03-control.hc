// 03-control.hc — 控制流
// 覆盖：if/else 分支、while、break/continue、for 数组迭代 |x| 载荷（C4：语句求值）
// 预期 stdout：
// 25
// big
// 10
// 1
// 2
// 4
fn main() !void {
    var mut i: i32 = 0;
    var mut sum: i32 = 0;
    while (i < 10) {
        i += 1;
        if (i == 3) { continue; }
        if (i == 8) { break; }
        sum += i;
    }
    io.print("{}\n", sum);
    if (sum > 20) { io.print("big\n"); } else { io.print("small\n"); }
    var arr = [1, 2, 3, 4];
    var mut total: i32 = 0;
    for (arr) |item| {
        total += item;
    }
    io.print("{}\n", total);
    for (arr) |item| {
        if (item == 3) { continue; }
        io.print("{}\n", item);
    }
}
