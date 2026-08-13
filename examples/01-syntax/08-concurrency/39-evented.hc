// 39-evented.hc — 事件循环（Evented 可选运行时，12.23）
//
// Q35 定案（2026-08-13）：入口 io 显式创建/切换
//   - var ev_io = Io.evented(alloc); —— 显式选择事件循环运行时
//   - 同一套 await 代码在 Threaded（默认）/ Evented 下运行
//   - await = 阻塞等待（Threaded） vs 协作挂起（Evented），语义一致（双模式承诺）

async fn fetch(io: Io, url: &[u8], alloc: Allocator) !String {
    var body = try io.net.get(url);
    return String.from(body, alloc);
}

fn main(io: Io) !void {
    // 显式选择 Evented 运行时（协作调度，脚本模式库选项）
    var ev_io = Io.evented(alloc);

    // 同一套 async 代码在两种运行时下都可运行
    var f1 = fetch(ev_io, "https://a.example.com", alloc);
    var f2 = fetch(ev_io, "https://b.example.com", alloc);
    var r1 = try await f1;
    var r2 = try await f2;
    io.print("{} {}\n", r1.len, r2.len);
}
