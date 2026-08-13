// 50-serialization.hc — 数据序列化分层（2026-08-13 定案）
//
//   - struct ↔ byte 数组：内建 to_bytes()/from_bytes()（零拷贝视图，Q36）
//   - class ↔ JSON：内建 to_json()/from_json() + 脚本生成可定制（Q37）
//   - Vec/Map/切片 → byte 数组（二进制序列化）

struct Point {
    x: f32,
    y: f32,
}

class Order {
    mut id: i32,
    mut amount: f64,
}

fn main(io: Io) !void {
    // struct → bytes：内建方法（零拷贝视图）
    var p = Point{ x = 1.0, y = 2.0 };
    var bytes: &[u8] = p.to_bytes();
    io.print("size = {}\n", bytes.len);   // 8 字节（两个 f32）

    // bytes → struct：校验对齐/布局（Debug 检测）
    var p2 = try Point.from_bytes(bytes);
    io.print("{} {}\n", p2.x, p2.y);

    // class → JSON：内建默认（零配置可用）
    var mut order: o Order = Order.new(alloc);
    order.id = 42;
    var json = order.to_json();
    io.print("{}\n", json);

    // JSON → class：加载
    var order2 = try Order.from_json(json);
    io.print("id = {}\n", order2.id);
}
