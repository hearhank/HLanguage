// 58-copy-semantics.hc — 复制语义（B3/Q16）
//
//   - 标量/纯值 struct：赋值即复制（值语义）
//   - 数组/集合/复杂类型：引用类型——复制需显式 copy（B3）
//   - String = u8[] 别名（Q3）：赋值别名共享（Q3c），深拷贝需显式 copy

struct Point {
    mut x: f32,
    y: f32,
}

fn main(io: Io) !void {
    // 标量：赋值即复制
    var a: i32 = 5;
    var b = a;
    b = 10;
    io.print("a = {} (不变)\n", a);

    // 纯值 struct：赋值即复制
    var p1 = Point{ x = 1.0, y = 2.0 };
    var mut p2 = p1;
    p2.x = 99.0;
    io.print("p1.x = {} (不受影响)\n", p1.x);

    // 集合：引用类型——显式 copy（B3）
    var v1 = Vec(i32).init(alloc);
    v1.append(1);
    var v2 = copy(&v1);              // 显式深拷贝
    v2.append(2);
    io.print("v1 len = {}, v2 len = {}\n", v1.len, v2.len);

    // String = u8[] 别名（Q3/Q3c）：赋值别名共享；concat 返回新 String
    var s1 = String.from("hi", alloc);
    var s2 = s1;                     // 别名共享（s1 保持唯一拥有）
    var s3 = s2.concat("!");         // concat 返回新 String
    io.print("{} {}\n", s1, s3);     // s1 未变
}

test "标量与纯值 struct 复制" {
    var a: i32 = 5;
    var b = a;
    b = 10;
    try expect_eq(a, 5);   // 原值不变

    var p1 = Point{ x = 1.0, y = 2.0 };
    var mut p2 = p1;
    p2.x = 99.0;
    try expect_eq(p1.x, 1.0);   // 复制互不影响
}

test "集合显式 copy" {
    var v1 = Vec(i32).init(alloc);
    v1.append(1);
    var v2 = copy(&v1);   // 显式深拷贝（B3）
    v2.append(2);
    try expect_eq(v1.len, 1);
    try expect_eq(v2.len, 2);
}

test "String 别名共享" {
    var s1 = String.from("hi", alloc);
    var s2 = s1;   // 别名共享（Q3c）：不复制
    var s3 = s2.concat("!");   // concat 返回新 String
    try expect_eq_slices(s1.to_bytes(), "hi");   // 原变量未变
    try expect_eq_slices(s3.to_bytes(), "hi!");
}
