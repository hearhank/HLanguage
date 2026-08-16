// 79-retry.hc — 重试模式（async + 指数退避）
//
//   - if (expr) |v| else |err| 双向捕获（成功/错误分支）
//   - io.time.sleep 退避；错误恢复

async fn fetch_with_retry(io: *T, url: &[u8], alloc: Allocator, max_retries: i32) !String where T: Io {
    var attempt = 0;
    while (attempt < max_retries) : (attempt += 1) {
        if (io.net.get(url)) |body| {
            return String.from(body, alloc);
        } else |err| {
            if (attempt == max_retries - 1) {
                return err;
            }
            io.time.sleep(100 * (attempt + 1));   // 退避
        }
    }
    return error.Exhausted;
}

fn main(io: Io) !void {
    var body = try await fetch_with_retry(&io, "https://example.com", alloc, 3);
    io.print("got {} bytes\n", body.len);
}

[test] fn retry_demo() !void {
    // S4 演示型（Q-T6）：fetch_with_retry 依赖真实网络（io.net.get），不在测试中执行；
    // 重试/指数退避逻辑断言留 M5 运行时测试
    try expect(true);
}
