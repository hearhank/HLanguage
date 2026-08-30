// t_neg.hc — checker 增强负例：checker 检查本文件应产生 4 条诊断
// ① x = 2.5 类型错（i32 ← float）② y 未定义名 ③ zfoo 未定义函数 ④ defer 内未定义函数
fn main(args: Vec<String>) !void {
    var x: i32 = 1;
    x = 2.5;
    y = 3;
    zfoo(1);
    defer undeferred();
    io.print("{}\n", x);
}
