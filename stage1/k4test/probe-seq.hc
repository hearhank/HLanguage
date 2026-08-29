// probe-seq.hc — C5 定位：顺序非嵌套多次调用
fn fib(n: i32) i32 {
    if (n < 2) { return n; }
    return fib(n - 1) + fib(n - 2);
}
fn fact(n: i32) i32 {
    if (n <= 1) { return 1; }
    return n * fact(n - 1);
}
fn add3(a: i32, b: i32, c: i32) i32 {
    return a + b + c;
}
fn main() !void {
    io.print("{}\n", fib(5));
    io.print("{}\n", fact(4));
    io.print("{}\n", add3(1, 2, 3));
    io.print("{}\n", fib(6));
}
