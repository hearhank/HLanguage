// probe-id.hc — C5 定位：参数透传
fn id(x: i32) i32 {
    return x;
}
fn main() !void {
    io.print("{}\n", id(7));
}
