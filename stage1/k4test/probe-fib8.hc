// probe-fib8.hc — C5 定位：中等递归深度
fn fib(n: i32) i32 {
    if (n < 2) { return n; }
    return fib(n - 1) + fib(n - 2);
}
fn main() !void {
    io.print("{}\n", fib(8));
}
