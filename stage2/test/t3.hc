// t3.hc — 对照实验：裸 Ident vs Call callee 的未定义检测
fn main(args: Vec<String>) !void {
    var q = zfoo2;
    zfoo(1);
    io.print("{}\n", q);
}
