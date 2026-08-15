// 35-comptime-branch.hc — comptime 编译期逻辑（12.16/§7）
//
//   - 类型级函数：fn ArrayLen(T: type, n: comptime_int) type（类型即值）
//   - anytype 参数（28 已示基础）
//   - 分工：comptime 管类型级计算；脚本管样板（X1）

fn ArrayLen(T: type, n: comptime_int) type {
    return [n]T;             // 编译期返回定长数组类型
}

fn max_value(a: anytype, b: anytype) anytype {
    return if (a > b) a else b;
}

fn main(io: Io) !void {
    // comptime 类型应用：ArrayLen(i32, 3) = [3]i32
    var arr: ArrayLen(i32, 3) = [1, 2, 3];
    io.print("len = {}\n", arr.len);

    io.print("max = {}\n", max_value(3, 7));
    io.print("max = {}\n", max_value(2.5, 1.5));
}

test fn comptime_type_function() !void {
    var arr: ArrayLen(i32, 3) = [1, 2, 3];
    try expect_eq(arr.len, 3);
}

test fn comptime_branch() !void {
    try expect_eq(max_value(3, 7), 7);
    var m = max_value(2.5, 1.5);
    try expect(m > 2.49 and m < 2.51);
}
