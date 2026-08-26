import H.std.{io};

// 58-copy-semantics.hc — 复制语义（B3/Q16/Q1'）
//
//   - 标量/Continuous：赋值即复制（值语义）
//   - 数组/集合/复杂类型：引用类型——赋值 = 编译错误（Q1'），复制需显式 copy(&x)
//   - String = u8[] 别名（Q3）：复制需显式 copy(&x)（默认深复制，浅复制需显式标注）

struct Point {   // 连续内存值类型（H1 特性标注）
    mut x: f32,
    y: f32,
}

fn main() !void {
    // 标量：赋值即复制
    var a: i32 = 5;
    var mut b = a;
    b = 10;
    io.print("a = {} (不变)\n", a);

    // 纯值 struct：赋值即复制
    var p1 = Point{x = 1.0, y = 2.0};
    var mut p2 = p1;
    p2.x = 99.0;
    io.print("p1.x = {} (不受影响)\n", p1.x);

    // 集合：引用类型——显式 copy（B3）
    var v1 = Vec<i32>.init(alloc);
    v1.append(1);
    var v2 = copy(&v1);              // 显式深拷贝
    v2.append(2);
    io.print("v1 len = {}, v2 len = {}\n", v1.len, v2.len);

    // String = u8[] 别名（Q3/Q1'）：复制走显式 copy；concat 返回新 String
    var s1 = "hi";
    var s2 = copy(&s1);               // 深复制（Q1'）：新建内存、有所有权
    var s3 = s2.concat("!");         // concat 返回新 String
    io.print("{} {}\n", s1, s3);     // s1 未变
}

[Test] fn scalar_and_continuous_copy() !void {
    var a: i32 = 5;
    var mut b = a;
    b = 10;
    try expect_eq(a, 5);   // 原值不变

    var p1 = Point{x = 1.0, y = 2.0};
    var mut p2 = p1;
    p2.x = 99.0;
    try expect_eq(p1.x, 1.0);   // 复制互不影响
}

[Test] fn collection_explicit_copy() !void {
    var v1 = Vec<i32>.init(alloc);
    v1.append(1);
    var v2 = copy(&v1);   // 显式深拷贝（B3）
    v2.append(2);
    try expect_eq(v1.len, 1);
    try expect_eq(v2.len, 2);
}

[Test] fn string_copy_owns() !void {
    var s1 = "hi";
    var s2 = copy(&s1);   // 深复制（Q1'）：新建内存、有所有权
    var s3 = s2.concat("!");   // concat 返回新 String
    try expect_eq_slices(s1.as_slice(), "hi");   // 原变量未变
    try expect_eq_slices(s3.as_slice(), "hi!");
}