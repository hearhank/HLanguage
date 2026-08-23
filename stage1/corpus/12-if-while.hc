// 12-if-while.hc — if/while 语句
fn test_if(x: i32) void {
    if (x > 0) {
        var y: i32 = 1;
    }
}
fn test_while() void {
    var mut i: i32 = 0;
    while (i < 10) {
        var x: i32 = i;
        i+=1;
    }
}
fn test_if_else(x: i32) void {
    if (x > 0) {
        var y: i32 = 1;
    } else {
        var z: i32 = 2;
    }
}
