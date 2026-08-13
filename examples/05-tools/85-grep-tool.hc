// 85-grep-tool.hc — grep 工具（目录遍历 + 行搜索，综合）
//
//   - 57 目录遍历 + 行读取 + find（47）
//   - 输出 文件名:行 格式

fn search_file(io: Io, path: &[u8], needle: &[u8]) !i32 {
    var data = try io.fs.read_file(path, alloc);
    var text = String.from(data, alloc);
    var lines = text.split('\n');

    var hits = 0;
    for (lines) |line| {
        if (line.find(needle)) |_| {
            io.print("{}: {}\n", path, line);
            hits += 1;
        }
    }
    return hits;
}

fn main(io: Io) !void {
    var needle = "fn ";
    var dir = try io.fs.open_dir(".");
    defer dir.close();

    var entries = try io.fs.list_dir(&dir, alloc);
    var total = 0;
    for (entries) |entry| {
        if (!entry.is_dir and entry.name.ends_with(".hc")) {
            total += try search_file(io, entry.name, needle);
        }
    }
    io.print("{} matches\n", total);
}
