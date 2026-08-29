// probe-fib6.hc — C5 定位：浅递归是否稳定
fn fib(n: i32) i32 {
    if (n < 2) { return n; }
    return fib(n - 1) + fib(n - 2);
}
fn main() !void {
    io.print("{}\n", fib(6));
}
