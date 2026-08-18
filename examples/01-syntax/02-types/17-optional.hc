import H.std.{io};

// 17-optional.hc — 可选值 ?T 与 switch 表达式
//
// Q27 定案（2026-08-13）：switch 是表达式（穷举保证确定值）
//   - 与 if 表达式、强制 else 同一哲学（显式初始化延伸）

enum Direction {
    north,
    east,
    south,
    west
}

enum Value {
    int: i32,
    float: f64,
    none,
}

fn parse_int(s: &[u8]) ?i32 {
    return null;   // 草图：解析失败返回 null（optional 显式解包）
}

fn main(args: o Vec(String)) !void {
    // optional：orelse 默认值
    var n = parse_int("42") orelse 0;
    io.print("{}\n", n);

    // optional：if (opt) |v| 捕获
    var maybe = parse_int("abc");
    if (maybe) |v| {
        io.print("parsed: {}\n", v);
    } else {
        io.print("not a number\n");
    }

    // switch 作为表达式：穷举 → 确定值
    var dir = Direction.east;
    var dx = switch (dir) {
        Direction.north => 0,
        Direction.east => 1,
        Direction.south => 0,
        Direction.west => -1,
    };
    io.print("dx = {}\n", dx);

    // switch 带负载捕获（12.13）
    var v: Value = Value{float = 3.5};
    var label = switch (v) {
        Value.int => |i| "int",
        Value.float => |f| "float",
        Value.none => "none",
    };
    io.print("{}\n", label);
}

[test] fn orelse_default_value() !void {
    var n = parse_int("42") orelse 0;   // 草图实现返回 null → 默认值
    try expect_eq(n, 0);
}

[test] fn optional_capture() !void {
    var maybe = parse_int("abc");
    var n = if (maybe) |v| v else 0;
    try expect_eq(n, 0);
}

[test] fn switch_exhaustive() !void {
    var dir = Direction.east;
    var dx = switch (dir) {
        Direction.north => 0,
        Direction.east => 1,
        Direction.south => 0,
        Direction.west => -1,
    };
    try expect_eq(dx, 1);
}
