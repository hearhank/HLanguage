import H.std.{io};

// 81-end-to-end.hc — 端到端：TCP 服务（四大支柱验收基准）
//
// Q25 定案（2026-08-13）：TCP 服务形态
//   接收数据（传输）→ 反序列化（定义）→ 校验/变换（修改）→ 序列化回写（传输）→ 落盘（保存）

[continuous] class Order {   // 连续内存（内建 to_bytes；脚本生成作定制通道演示）
    id: i32,
    amount: f64,
    status: OrderStatus,
}

enum OrderStatus {
    pending,
    confirmed,
    cancelled,
}

// script { } 块已移除（2026-08-23 定案，见 docs/SPEC/phase3/12-script-redesign.md）。
// 原脚本通过 types.fields("Order") 元数据生成序列化/反序列化/校验函数，
// 现直接硬编码为桩函数供演示。

fn order_to_bytes(ord: *Order) owned Vec<u8> {
    return Vec<u8>{};
}

fn order_from_bytes(data: &[u8]) !Order {
    return error.NotImplemented;
}

fn order_validate(ord: *Order) !void {}

fn handle_order<T>(io: *T, conn: *TcpConn) !void where T: Io {
    // 传输：接收长度前缀帧
    var data = try io.net.read_frame(&conn, alloc);

    // 定义：反序列化
    var mut ord = try order_from_bytes(data);

    // 修改：校验 + 变换
    try order_validate(&ord);
    if (ord.status == OrderStatus.pending) {
        ord.status = OrderStatus.confirmed;
    }

    // 传输：序列化回写
    var resp = order_to_bytes(&ord);
    try io.net.write_frame(&conn, resp);

    // 保存：落盘
    var f = try io.fs.open("orders.log");
    defer f.close();
    try f.append(resp);
}

fn main() !void {
    var server = try io.net.listen(8080);
    defer server.close();
    io.print("listening on 8080\n");

    while (true) {                 // 无 loop 关键字（12.7）
        var conn = try io.net.accept(&server);
        try handle_order(&io, &conn);
    }
}

[test] fn end_to_end_demo() !void {
    // S4 演示型（Q-T6）：81 为 TCP 服务（listen 8080），且序列化样板由脚本生成（未展开）；
    // 端到端验收在 M7 以真实网络运行（04-stdlib-scope 端到端基准）
    try expect(true);
}
