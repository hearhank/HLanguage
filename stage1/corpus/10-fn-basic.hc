// 10-fn-basic.hc — 函数声明基础
fn greet() void {
    var x: i32 = 42;
}
fn add(x: i32, y: i32) i32 {
    return x + y;
}
fn noop() void {}
fn with_else(x: i32) void {
    if (x > 0) {
        var y: i32 = 1;
    }
}