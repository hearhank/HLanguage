import H.std.{io};

// 85-grep-tool.hc — grep 工具（目录遍历 + 行搜索，综合）
//
//   - 57 目录遍历 + 行读取 + find（47）
//   - 输出 文件名:行 格式

fn search_file<T>(io: *T, path: &[u8], needle: &[u8]) !i32 where T: Io {
    var data = try io.fs.read_file(path, alloc);
    var text = String.from(data, alloc);
    var lines = text.split('\n');

    var mut hits = 0;
    for (lines) |line| {
        if (line.find(needle)) |_| {
            io.print("{}: {}\n", path, line);
            hits += 1;
        }
    }
    return hits;
}

fn main() !void {
    var needle = "fn ";
    var dir = try io.fs.open_dir(".");
    defer dir.close();

    var entries = try io.fs.list_dir(&dir, alloc);
    var mut total = 0;
    for (entries) |entry| {
        if (!entry.is_dir and entry.name.ends_with(".hc")) {
            total += try search_file(&io, entry.name, needle);
        }
    }
    io.print("{} matches\n", total);
}

[Test] fn grep_tool_demo() !void {
    // S4 演示型（Q-T6）：遍历当前目录 .hc 文件（外部文件系统依赖），不在测试中执行；
    // 行搜索逻辑等价断言见 52-string-deep / 56-csv-parse
    try expect(true);
}
