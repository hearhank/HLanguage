// 40-io.hc — 文件与 IO（保存数据支柱，12.18）
//
// Q20 定案（2026-08-13）：文件 IO 双语
//   - 模块函数：io.fs.open / io.fs.read_all / io.fs.write_all（显式传 alloc）
//   - 方法形态：f.read_all(alloc) / f.write_all(data)（与模块函数等价，Q5 精神）
//   - 关闭：defer f.close()（F1，作用域退出保证）
//   - io 显式传递（12.18）；错误一律 error union；字节为中心 + UTF-8 函数

fn main(io: Io) !void {
    // 写：方法形态
    var fw = try io.fs.open("out.txt");
    defer fw.close();
    try fw.write_all("hello, file\n");

    // 读：模块函数形态（等价）
    var fr = try io.fs.open("out.txt");
    defer fr.close();
    var data = try io.fs.read_all(&fr, alloc);
    io.print("read {} bytes\n", data.len);

    // 文本编码：字节视图 + UTF-8 解码（12.18）
    var text = try utf8.decode(data);
    io.print("{}\n", text);
}

[test] fn file_io_demo() !void {
    // S4 演示型（Q-T6）：main 读写 out.txt（真实文件副作用），不在测试中执行；
    // 文件 IO 行为断言留 M7 标准库测试（输出捕获 1.x）
    try expect(true);
}
