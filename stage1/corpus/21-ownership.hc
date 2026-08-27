// 21-ownership.hc — 所有权分析测试
fn test_move_ok() void {
    var x: i32 = 42;
    var y = move x;
}
fn test_ref_ok() void {
    var x: i32 = 42;
    var y = &x;
}