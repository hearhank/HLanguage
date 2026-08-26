import H.std.{io};

// 39-evented.hc — 事件循环（Evented 可选运行时，12.23）
//
// Q35 定案（2026-08-13）：入口 io 显式创建/切换
//   - var ev_io = Io.evented(alloc); —— 显式选择事件循环运行时
//   - 同一套 await 代码在 Threaded（默认）/ Evented 下运行
//   - await = 阻塞等待（Threaded） vs 协作挂起（Evented），语义一致（双模式承诺）

async fn fetch<T>(io: *T, url: &[u8], alloc: Allocator) !&[u8] where T: Io {
    var body = try io.net.get(url);
    return body;
}

fn main() !void {
    // 显式选择 Evented 运行时（协作调度，脚本模式库选项）
    var ev_io = Io.evented(alloc);

    // 同一套 async 代码在两种运行时下都可运行
    var f1 = fetch(&ev_io, "https://a.example.com", alloc);
    var f2 = fetch(&ev_io, "https://b.example.com", alloc);
    var r1 = try await f1;
    var r2 = try await f2;
    io.print("{} {}\n", r1.len, r2.len);
}

[Test] fn evented_runtime_demo() !void {
    // S4 演示型（Q-T6）：fetch 依赖真实网络（io.net.get），不在测试中执行；
    // 事件循环语义验证在 M5 运行时测试套件中覆盖
    try expect(true);
}