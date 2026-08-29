// probe-rec1.hc — C5 定位：最小递归（深度 3）
fn down(n: i32) i32 {
    if (n <= 0) { return 0; }
    return down(n - 1);
}
fn main() !void {
    io.print("{}\n", down(3));
}
