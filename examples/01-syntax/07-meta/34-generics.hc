import H.std.{io};

// 34-generics.hc — comptime 泛型（12.16）
//
//   - 泛型 = 编译期函数：fn List(T: type) type（类型即值）
//   - anytype 参数：调用点推断
//   - 分工：comptime 管类型级计算（泛型）；脚本管样板（X1）

fn max_value(a: anytype, b: anytype) anytype {
    if (a > b) {
        return a;
    }
    return b;
}

fn Pair(T: type) type {
    return struct {
        first: T,
        second: T,
    };
}

fn main(args: o Vec<String>) !void {
    io.print("{}\n", max_value(3, 5));       // anytype：整数
    io.print("{}\n", max_value(3.5, 2.0));   // anytype：浮点

    // comptime 类型应用（Q15 同款）
    var p: Pair<i32> = Pair<i32>{first = 1, second = 2};
    io.print("{}\n", p.first + p.second);
}

[test] fn anytype_generics() !void {
    try expect_eq(max_value(3, 5), 5);
    var m = max_value(3.5, 2.0);
    try expect(m > 3.49 and m < 3.51);
}

[test] fn comptime_type_application() !void {
    var p: Pair<i32> = Pair<i32>{first = 1, second = 2};
    try expect_eq(p.first + p.second, 3);
}
