// t_call_neg.hc — Call 检查最小负例：应报 undefined function `zfoo`
fn main(args: Vec<String>) !void {
    zfoo(1);
    io.print("ok\n");
}
