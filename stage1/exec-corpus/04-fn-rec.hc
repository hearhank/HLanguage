// 04-fn-rec.hc — 函数与递归
// 覆盖：fn 定义/调用、参数传递、return、递归（fib/fact）、函数组合调用（C5）
// 预期 stdout：
// 55
// 120
// 30
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
    io.print("{}\n", fib(10));
    io.print("{}\n", fact(5));
    io.print("{}\n", add3(fib(5), fact(4), 1));
}
