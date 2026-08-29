// probe-call.hc — 最小用户函数调用
fn double(x: i32) i32 {
    return x * 2;
}
fn main() !void {
    io.print("{}\n", double(21));
}
