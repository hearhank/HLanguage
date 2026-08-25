import H.std.{io};

// 08-bool-void.hc — bool 与 void（12.3）
//
// Q41 定案（2026-08-13）：
//   - and/or/! 关键字（12.2）；and/or 短路求值（左边已定则右边不求值）
//   - void 仅作函数返回类型/泛型占位（不可作变量类型、无值对象）

fn is_valid(x: i32, y: i32) bool {
    // 短路：x == 0 时 (100 / x) 不求值（防除零）
    return (x != 0) and (100 / x > 1) and (y > 0);
}

fn main() !void {
    var ok: bool = true;
    var ready = (1 > 0) or (2 > 5);    // or：左边为 true 则右边不求值
    var neg = !ok;

    io.print("{} {} {}\n", is_valid(10, 5), ready, neg);
    io.print("safe = {}\n", is_valid(0, 5));   // 短路保护：不会除零
}

[Test] fn short_circuit_evaluation() !void {
    try expect(is_valid(10, 5));
    try expect(!is_valid(0, 5));     // 短路：x == 0 时不求值 (100 / x)
    try expect(is_valid(0, -1) == false);
    try expect((1 > 0) or (2 > 5));  // or：左边为 true 则右边不求值
    try expect(!(1 > 0) == false);
}
