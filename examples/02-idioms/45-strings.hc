// 45-strings.hc — 字符串操作（Q28 定案 2026-08-13）
//
//   - 拼接 = 方法：s.concat(other) ≡ String.concat(a, b)（双语，Q20 精神）
//   - 无 ++ 运算符、无 + 重载（函数 = 唯一处理逻辑，无运算符重载）
//   - String = u8[] 别名（Q3）：引用类型、赋值 = 编译错误（Q1'）；== 比较内容

fn main(io: Io) !void {
    // 拼接：方法形态
    var name = String.from("alice", alloc);
    var greeting = String.from("hello, ", alloc).concat(name);
    io.print("{}\n", greeting);

    // 拼接：模块函数形态（等价）
    var g2 = String.concat(greeting, String.from("!", alloc));
    io.print("{}\n", g2);

    // == 比较内容（两个独立实例）
    var a = String.from("abc", alloc);
    var b = String.from("abc", alloc);
    io.print("equal = {}\n", a == b);
}

[test] fn string_concat() !void {
    var name = String.from("alice", alloc);
    var greeting = String.from("hello, ", alloc).concat(name);
    try expect_eq_slices(greeting.as_slice(), "hello, alice");
    var g2 = String.concat(greeting, String.from("!", alloc));
    try expect_eq_slices(g2.as_slice(), "hello, alice!");
}

[test] fn content_equals() !void {
    var a = String.from("abc", alloc);
    var b = String.from("abc", alloc);
    try expect_eq(a == b, true);
}
