import H.std.{io};

// 80-batch-async.hc — 异步批处理（Future 并行 + 汇总）
//
//   - 并行发起：Future 列表（Q19 await 任何函数可用）
//   - 全部等待并汇总（Go 协程方向，B2）

async fn fetch<T>(io: *T, url: &[u8], alloc: Allocator) !String where T: Io {
    var body = try io.net.get(url);
    return String.from(body, alloc);
}

fn main() !void {
    var urls = ["https://a.example.com", "https://b.example.com", "https://c.example.com"];

    // 批量发起（并行）
    var futures = Vec<Future<!String>>.init(alloc);
    for (urls) |u| {
        futures.append(fetch(&io, u, alloc));
    }

    // 全部等待并汇总
    var total = 0;
    for (futures) |f| {
        var body = try await f;
        total += body.len;
    }
    io.print("total bytes = {}\n", total);
}

[test] fn batch_async_demo() !void {
    // S4 演示型（Q-T6）：fetch 依赖真实网络，不在测试中执行；
    // Future 并行发起/汇总断言留 M5 运行时测试
    try expect(true);
}
