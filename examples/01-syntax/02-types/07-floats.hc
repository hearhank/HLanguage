import H.std.{io};

// 07-floats.hc — 浮点类型（12.3）
//
// Q40 定案（2026-08-13）：与整数对称
//   - 惰性宽度默认（comptime_float）+ 后缀（3.14f32）并存
//   - 特殊值：标准库常量 math.nan / math.inf / math.inf_neg（Zig 式 std.math）

fn main() !void {
    // 惰性宽度：标注类型处定型
    var pi: f64 = 3.14159;
    var half: f32 = 0.5;

    // 显式后缀：立即定型
    var pi32 = 3.14f32;
    var big = 1.0f128;

    // 特殊值：标准库常量（类型参数 comptime 式）
    var nan = math.nan(f64);
    var inf = math.inf(f32);
    var inf_neg = math.inf_neg(f64);

    // 算术
    var area = pi * half * half;
    io.print("{} {} {}\n", pi32, big, area);
    io.print("inf = {}\n", inf);
}

[Test] fn float_arithmetic() !void {
    var pi: f64 = 3.14159;
    var area = pi * 0.5 * 0.5;
    try expect(area > 0.78 and area < 0.79);
}

[Test] fn special_values() !void {
    var nan = math.nan(f64);
    try expect(nan != nan);          // NaN 不等于自身
    var inf = math.inf(f32);
    try expect(inf > 1.0e30);
}
