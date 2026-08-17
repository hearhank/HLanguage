import H.std.{io};

// 14-enum.hc — 枚举与 switch（定义数据）
//
// Q9 定案（2026-08-13）：枚举实例化
//   - 带负载：Value{int = 42}（与 struct 字面量同形态：Type{field = value}）
//   - 无负载：Value.none（枚举常量）
//
// switch：穷举（漏分支编译期报错）+ 负载捕获 |x|

enum Value {
    int: i32,
    float: f64,
    none,
}

fn main(args: o Vec(String)) !void {
    var v: Value = Value{int = 42};

    switch (v) {
        int => |i| io.print("int: {}\n", i),
        float => |f| io.print("float: {}\n", f),
        none => io.print("none\n"),
    }

    // 无负载常量
    var n: Value = Value.none;
    switch (n) {
        int => |i| io.print("int: {}\n", i),
        float => |f| io.print("float: {}\n", f),
        none => io.print("none\n"),
    }
}

[test] fn enum_instantiation() !void {
    var v: Value = Value{int = 42};
    var label = switch (v) {
        int => |i| i,
        float => |f| 0,
        none => 0,
    };
    try expect_eq(label, 42);
}

[test] fn payloadless_constant() !void {
    var n: Value = Value.none;
    try expect(switch (n) {
        none => true,
        else => false,
    });
}
