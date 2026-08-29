// 21-ownership.hc — 所有权分析（ADR-0030：指针形态转移）
class Box { v: i32, }
fn take(b: owned *mut Box) void {}
fn peek(p: *i32) void {}
fn test_move_ok() void {
    var mut b: owned Box = alloc.init(Box{v = 1});
    take(move &mut b);
}
fn test_scalar_rejected() void {
    var n: i32 = 42;
    take(move n);
}
fn test_ref_ok() void {
    var b: Box = alloc.init(Box{v = 2});
    var r = &b;
    peek(&r.v);
}
