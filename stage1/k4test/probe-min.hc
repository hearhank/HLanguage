// probe-min.hc — C5 定位：最小无参调用
fn two() i32 {
    return 2;
}
fn main() !void {
    io.print("{}\n", two());
}
