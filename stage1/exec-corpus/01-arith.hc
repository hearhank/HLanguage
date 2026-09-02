// 01-arith.hc — 算术与比较
// 覆盖：i32 算术与优先级、一元负号、f64、比较运算、&&/|| 逻辑（C3：表达式求值 A）
// 预期 stdout：
// 7
// 13
// 2
// 3
// -3
// 3.5
// true
// false
// true
fn main() !void {
    io.print("{}\n", 1 + 2 * 3);
    io.print("{}\n", (1 + 2) * 4 + 1);
    io.print("{}\n", 7 / 3);
    io.print("{}\n", 7 % 4);
    io.print("{}\n", -5 + 2);
    io.print("{}\n", 1.5 + 2.0);
    io.print("{}\n", 3 > 2);
    io.print("{}\n", 2 == 3);
    io.print("{}\n", (1 < 2) && (3 == 3));
}
