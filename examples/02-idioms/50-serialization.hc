import H.std.{io};

// 50-serialization.hc — 数据序列化分层（2026-08-13 定案；2026-08-14 修订）
//
//   - 连续类型 ↔ byte 数组：内建 to_bytes()/from_bytes()（零拷贝视图，Q36）
//   - 堆上 class ↔ JSON：内建 to_json()/from_json() + 脚本生成可定制（Q37）
//   - Vec/Map/切片 → byte 数组（二进制序列化）

[continuous] class Point {   // 连续内存值类型（H1 特性标注）
    x: f32,
    y: f32,
}

class Order {
    mut id: i32,
    mut amount: f64,
}

fn main(args: o Vec(String)) !void {
    // 连续类型 → bytes：内建方法（零拷贝视图）
    var p = Point{x = 1.0, y = 2.0};
    var bytes: &[u8] = p.to_bytes();
    io.print("size = {}\n", bytes.len);   // 8 字节（两个 f32）

    // bytes → 连续类型：校验对齐/布局（Debug 检测）
    var p2 = try Point.from_bytes(bytes);
    io.print("{} {}\n", p2.x, p2.y);

    // class → JSON：内建默认（零配置可用）
    var mut order: o Order = alloc.init(Order);   // 无参构造（C1'）+ 字段赋值
    order.id = 42;
    var json = order.to_json();
    io.print("{}\n", json);

    // JSON → class：加载
    var order2 = try Order.from_json(json);
    io.print("id = {}\n", order2.id);
}

[test] fn continuous_to_bytes() !void {
    var p = Point{x = 1.0, y = 2.0};
    var bytes: &[u8] = p.to_bytes();
    try expect_eq(bytes.len, 8);   // 两个 f32（直映射）
    var p2 = try Point.from_bytes(bytes);
    try expect_eq(p2.x, 1.0);
    try expect_eq(p2.y, 2.0);
}

[test] fn class_to_json() !void {
    var mut order: o Order = alloc.init(Order);   // 无参构造（C1'）
    order.id = 42;
    var json = order.to_json();
    var order2 = try Order.from_json(json);
    try expect_eq(order2.id, 42);
}
