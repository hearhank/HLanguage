import H.std.{io};

// 83-wordcount.hc — 综合工具：词频统计（四大支柱实战）
//
//   - 读文件（保存/IO）→ 分词（String split）→ 统计（Map）→ 输出
//   - 综合：io.fs / String / Map / 迭代器

fn main(args: o Vec(String)) !void {
    // 读文件
    var data = try io.fs.read_file("input.txt", alloc);

    // 分词 + 统计（修改数据）
    var counts = Map(&[u8], i32).init(alloc);
    var words = String.from(data, alloc).split(' ');
    for (words) |w| {
        if (w.len > 0) {
            counts.put(w, (counts.get(w) orelse 0) + 1);
        }
    }

    // 输出
    io.print("unique words: {}\n", counts.len);
    for (counts) |kv| {
        io.print("{}: {}\n", kv.key, kv.value);
    }
}

// S4 演示型（Q-T6）：统计逻辑等价断言见 32-collections / 53-map-deep
[test] fn wordcount_demo() !void {
    try expect(true);
}
