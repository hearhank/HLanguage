// 02-vars.hc — 变量与赋值
// 覆盖：var/var mut、重新赋值、复合赋值 += -= *=（C3：赋值与复合赋值）
// 预期 stdout：
// 24
// 15
// 100
// 7
fn main() !void {
    var mut b: i32 = 3;
    b += 10;
    b -= 1;
    b *= 2;
    io.print("{}\n", b);
    var a: i32 = 5;
    var mut c = a * 3;
    io.print("{}\n", c);
    c = 100;
    io.print("{}\n", c);
    var mut n: i32 = 0;
    n = 7;
    io.print("{}\n", n);
}
