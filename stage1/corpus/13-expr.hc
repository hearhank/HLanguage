// 13-expr.hc — 表达式基础
fn arith() void {
    var a: i32 = 1 + 2;
    var b: i32 = 3 - 4;
    var c: i32 = 5 * 6;
    var d: i32 = 8 / 2;
    var e: i32 = 10 % 3;
}
fn compare(x: i32, y: i32) bool {
    var eq: bool = x == y;
    var lt: bool = x < y;
    var gt: bool = x > y;
    return eq;
}
fn logic(a: bool, b: bool) bool {
    var r: bool = a && b;
    var s: bool = a || b;
    return r;
}