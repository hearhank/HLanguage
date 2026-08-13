// 81-end-to-end.hc — 端到端：TCP 服务（四大支柱验收基准）
//
// Q25 定案（2026-08-13）：TCP 服务形态
//   接收数据（传输）→ 反序列化（定义）→ 校验/变换（修改）→ 序列化回写（传输）→ 落盘（保存）

struct Order {
    id: i32,
    amount: f64,
    status: OrderStatus,
}

enum OrderStatus {
    pending,
    confirmed,
    cancelled,
}

// 脚本生成：序列化/反序列化/校验样板（就地替换本块，A4 机制待细化）
script {
    // 生成（本位置）：
    //   fn order_to_bytes(o: *Order) o Vec(u8) { ... }
    //   fn order_from_bytes(data: &[u8]) !Order { ... }
    //   fn order_validate(o: *Order) !void { ... }
}

fn handle_order(io: Io, conn: *TcpConn) !void {
    // 传输：接收长度前缀帧
    var data = try io.net.read_frame(&conn, alloc);

    // 定义：反序列化
    var mut order = try order_from_bytes(data);

    // 修改：校验 + 变换
    try order_validate(&order);
    if (order.status == OrderStatus.pending) {
        order.status = OrderStatus.confirmed;
    }

    // 传输：序列化回写
    var resp = order_to_bytes(&order);
    try io.net.write_frame(&conn, resp);

    // 保存：落盘
    var f = try io.fs.open("orders.log");
    defer f.close();
    try f.append(resp);
}

fn main(io: Io) !void {
    var server = try io.net.listen(8080);
    defer server.close();
    io.print("listening on 8080\n");

    while (true) {                 // 无 loop 关键字（12.7）
        var conn = try io.net.accept(&server);
        try handle_order(io, &conn);
    }
}
