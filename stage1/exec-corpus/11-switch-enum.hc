// 11-switch-enum.hc — switch 与纯枚举
// 覆盖：枚举声明与变体访问、== 按序数、switch 枚举变体/int/多模式/默认分支（P4/P5）
// 预期 stdout：
// false
// true
// red
// green
// lucky
enum Color {
    Red,
    Green,
    Blue,
}

fn name_of(c: Color) &[u8] {
    switch (c) {
        Color.Red => { return "red"; }
        Color.Green => { return "green"; }
        else => { return "blue"; }
    }
}

fn main() !void {
    var a = Color.Red;
    var b = Color.Green;
    if (a == b) { io.print("true\n"); } else { io.print("false\n"); }
    if (a == Color.Red) { io.print("true\n"); } else { io.print("false\n"); }
    io.print("{}\n", name_of(a));
    io.print("{}\n", name_of(b));
    var k: i32 = 7;
    switch (k) {
        1, 2, 3 => io.print("small\n"),
        7 => io.print("lucky\n"),
        else => io.print("other\n"),
    }
}
