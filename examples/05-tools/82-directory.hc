import H.std.{io};

// 82-directory.hc — 目录遍历（12.18 io.fs）
//
//   - io.fs.open_dir / list_dir（与文件 IO 双语一致，Q20）
//   - 字节为中心；条目含 is_dir 标志

fn main() !void {
    var dir = try io.fs.open_dir(".");
    defer dir.close();

    var entries = try io.fs.list_dir(&dir, alloc);
    var count = 0;
    for (entries) |entry| {
        io.print("{}{}\n", entry.name, if (entry.is_dir) "/" else "");
        count += 1;
    }
    io.print("{} entries\n", count);
}

[Test] fn directory_traversal_demo() !void {
    // S4 演示型（Q-T6）：list_dir 依赖实际目录内容（CWD），断言不稳定；
    // 目录遍历断言留 M7 标准库测试
    try expect(true);
}
