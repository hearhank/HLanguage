// 01-hello.hc — 程序入口与 Hello World
//
// 入口：fn main(io: Io) !void
//   - io 显式传入（12.18：IO 是类型，必须显式传递）
//   - !void：入口错误由运行时统一报告（带位置）
//
// io.print：comptime 格式串（Q2 定案，2026-08-13）
//   - 占位符 {} 与参数类型编译期校验，不匹配编译报错
//
// 运行：脚本模式 hc run 01-hello.hc / 编译模式 hc build 后执行二进制

fn main(io: Io) !void {
    io.print("hello, world\n");
    io.print("x = {}, y = {}\n", 42, 3.14);
}

[test] fn hello_entry_runs() !void {
    try main(test_io);   // S2：smoke test（入口 !void 错误自动捕获）
}
