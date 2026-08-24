// Struct 类型测试
struct Point {
    x: f32,
    y: f32,
}

// 测试 struct 初始化与字段访问
fn test_point() f32 {
    var p: Point = Point{ x = 1.0, y = 2.0 };
    var p2: Point = p;       // 复制（值语义）
    p2.x = 99.0;             // 修改副本
    return p.x;              // 原值应不变 → 1.0
}

// 测试 struct 嵌套
struct Rect {
    min: Point,
    max: Point,
}

// 测试 Align 属性
[Align(4)]
struct Aligned {
    a: i8,
    b: i32,
}

fn main() i32 {
    var r = test_point();
    if (r != 1.0) return 1;
    return 0;
}