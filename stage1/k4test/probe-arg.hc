// probe-arg.hc — C5 定位：单参数调用
fn add1(x: i32) i32 {
    return x + 1;
}
fn main() !void {
    io.print("{}\n", add1(41));
}
