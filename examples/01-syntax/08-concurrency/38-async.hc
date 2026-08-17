import H.std.{io};

// 38-async.hc — 异步编程（async/await + Future）
//
// Q19 定案（2026-08-13）：await 任何函数可用
//   - await = 等待 Future 结果（与 join 同源，12.23）
//   - async fn = 返回 Future 的函数；无 Rust 式 async 传染
//   - 线程默认执行；事件循环（Evented）为可选运行时
//   - 并发 await 方向：Go 式协程 + 通道（评审 B2）

async fn fetch_url(io: *T, url: &[u8], alloc: Allocator) !String where T: Io {
    var conn = try io.net.connect(url);
    defer conn.close();
    var body = try io.net.read_all(&conn, alloc);
    return String.from(body, alloc);
}

async fn parse_json(data: &[u8]) !JsonValue {
    return json.parse(data);
}

fn main(args: o Vec(String)) !void {
    // 顺序 await：await 在任何函数可用（阻塞等待线程结果）
    var body = try await fetch_url(&io, "https://example.com", alloc);
    var value = try await parse_json(body);
    io.print("{}\n", value);

    // 并发 await：两个任务并行
    var fut1: Future(!JsonValue) = parse_json(body);
    var fut2: Future(!JsonValue) = parse_json(body);
    var v1 = try await fut1;
    var v2 = try await fut2;
}

[test] fn async_concurrent_await() !void {
    // S3（Q-T6）：不跑网络（fetch_url 依赖外部）；测纯解析路径
    var fut1: Future(!JsonValue) = parse_json("{\"a\":1}");
    var fut2: Future(!JsonValue) = parse_json("[1,2]");
    var v1 = try await fut1;
    var v2 = try await fut2;
    // 成功解析（契约层面验证；内容断言依赖 json.parse 实现）
}
