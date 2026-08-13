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

fn main(io: Io) !void {
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
