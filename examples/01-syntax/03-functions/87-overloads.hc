import H.std.{io};

// 87-overloads.hc — 函数重载与可选参数（2026-08-14 定案）
//
//   - 签名 = 函数名 + 参数类型列表 + 返回类型（共同决定）
//   - 调用点按参数精确匹配；返回类型在目标类型已知时参与选择；歧义报错
//   - 泛型约束编译时验证、接口限制运行时拆除（单态化，零开销）
//   - 可选参数：尾部 + 编译期常量默认值

// 重载 1：参数类型不同
fn describe(v: i32) &[u8] {
    return "int";
}

fn describe(v: &[u8]) &[u8] {
    return "bytes";
}

fn describe(v: f64) &[u8] {
    return "float";
}

// 重载 2：返回类型不同（调用点按目标类型选择）
fn parse(s: &[u8]) i32 {
    return parse_int(s) orelse 0;
}

fn parse(s: &[u8]) f64 {
    return parse_float(s) orelse 0.0;
}

// 重载 3：可选参数（尾部、编译期常量默认值）
fn greet(name: &[u8], punct: &[u8] = "!") &[u8] {
    return name;
}

// 重载 4：泛型重载（编译时约束验证；接口限制运行时拆除）
fn sum<T>(items: &[T]) T where T: INumber {
    var mut total = items[0];
    for (items[1..]) |v| {
        total = total.add(v);
    }
    return total;
}

fn sum(items: &[i32]) i32 {   // 具体重载与泛型并存
    var mut total: i32 = 0;
    for (items) |v| total += v;
    return total;
}

fn main() !void {
    io.print("{}\n", describe(42));         // int（i32 精确匹配）
    io.print("{}\n", describe("hi"));       // bytes
    io.print("{}\n", describe(1.5));        // float（f64 精确匹配）

    // 返回类型参与选择（目标类型已知）
    var i: i32 = parse("42");
    var f: f64 = parse("3.14");
    io.print("{} {}\n", i, f);

    // 可选参数
    io.print("{}\n", greet("hi"));          // hi（默认值）
    io.print("{}\n", greet("hi", "?"));     // hi

    // 泛型 vs 具体重载：i32 数组走具体重载，f64 数组走泛型
    var ints = [1, 2, 3];
    io.print("{}\n", sum(&ints));           // 6
    var floats = [1.5, 2.5];
    io.print("{}\n", sum(&floats));         // 4.0（泛型实例化）
}

[Test] fn overload_resolution() !void {
    try expect_eq_slices(describe(42), "int");
    try expect_eq_slices(describe("hi"), "bytes");
    try expect_eq_slices(describe(1.5), "float");
}

[Test] fn return_type_overload() !void {
    var i: i32 = parse("42");
    var f: f64 = parse("3.14");
    try expect_eq(i, 42);
    try expect(f > 3.13 and f < 3.15);
}

[Test] fn optional_args() !void {
    try expect_eq_slices(greet("hi"), "hi");
    try expect_eq_slices(greet("hi", "?"), "hi");
}

[Test] fn generic_vs_concrete_overload() !void {
    var ints = [1, 2, 3];
    try expect_eq(sum(&ints), 6);
    var floats = [1.5, 2.5];
    try expect_eq(sum(&floats), 4.0);
}
