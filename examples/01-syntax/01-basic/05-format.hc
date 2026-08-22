import H.std.{io};

// 05-format.hc — 格式串说明符（Q45 定案 2026-08-13）
//
//   - comptime 校验（Q2）：说明符与参数类型不匹配编译报错
//   - Zig 式说明符：{} 默认 / {d} / {x} / {b} / {e} / {s} / 宽度/精度/对齐

fn main() !void {
    io.print("{}\n", 255);          // 默认（comptime 定型）
    io.print("{d}\n", 255);         // 十进制
    io.print("{x}\n", 255);         // 十六进制小写：ff
    io.print("{X}\n", 255);         // 十六进制大写：FF
    io.print("{b}\n", 5);           // 二进制：101
    io.print("{:.2}\n", 3.14159);   // 精度：3.14
    io.print("{:8}\n", 42);         // 宽度（右对齐）
    io.print("{:<6}\n", "hi");      // 左对齐
    //io.print("{x}\n", "str");    // 错误：说明符与类型不匹配（Q2 comptime 校验）
}

[test("格式化输入运行")] fn format_entry_runs() !void {
    var a: o Vec<String> = [];
    try main(a);   // S2：格式串全部合法，运行不抛错
}
