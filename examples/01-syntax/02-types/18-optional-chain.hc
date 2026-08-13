// 18-optional-chain.hc — 可选值链（12.3/12.2）
//
//   - orelse 默认值 / .? 断言 / if (opt) |v| 捕获（20 已示基础）
//   - 嵌套字段 optional + 默认值链

struct Config {
    host: ?String,
    port: ?i32,
}

fn lookup_port(name: &[u8]) ?i32 {
    return null;   // 草图：查找失败返回 null（optional 显式）
}

fn main(io: Io) !void {
    // 字段类型 ?T 时值自动装箱；null 字面量
    var cfg = Config{ host = null, port = 8080 };

    // 可选值 + 默认值链
    var host = cfg.host orelse String.from("localhost", alloc);
    var port = cfg.port orelse 8080;
    io.print("{}:{}\n", host, port);

    // if (opt) |v| 捕获
    var maybe_port = lookup_port("app");
    if (maybe_port) |p| {
        io.print("port = {}\n", p);
    } else {
        io.print("no port\n");
    }
}

test "字段 optional 默认值" {
    var cfg = Config{ host = null, port = 8080 };
    var port = cfg.port orelse 8080;
    try expect_eq(port, 8080);
    var host = cfg.host orelse String.from("localhost", alloc);
    try expect_eq_slices(host.to_bytes(), "localhost");
}
