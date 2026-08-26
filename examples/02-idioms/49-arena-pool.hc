import H.std.{io};

// 49-arena-pool.hc — Arena 内存池场景（Q3/Q16）
//
//   - arena：统一分配、统一销毁；arena 分配的对象无所有权（禁止 move，A6）
//   - 适用：请求处理——每请求一个 arena，处理完整体回收
//   - arena 实例默认拥有（Q16），作用域退出自动统一回收；提前回收可显式 deinit

fn handle_request<T>(io: *T, arena: *Arena) !void where T: Io {
    // 请求内所有分配走 arena：无 o、禁止 move（A6）
    var buf = arena.alloc(1024);
    io.print("len = {}\n", buf.len);
    // 函数结束：buf 不各自销毁（arena 统一回收）
}

fn main() !void {
    // 每请求独立 arena：结束即整体回收（统一分配、统一销毁）
    var arena = Arena.init(alloc);   // 默认拥有，退出自动统一回收
    try handle_request(&io, &arena);
}

[Test] fn arena_unified_reclaim() !void {
    var arena = Arena.init(alloc);
    var buf = arena.alloc(1024);
    try expect_eq(buf.len, 1024);
    // 函数结束：arena 统一回收（buf 不各自销毁）
}
