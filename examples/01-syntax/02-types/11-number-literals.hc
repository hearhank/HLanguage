import H.std.{io};

// 11-number-literals.hc — 数字进制字面量（Q46 定案 2026-08-13）
//
//   - 0x 十六进制 / 0b 二进制 / 0o 八进制
//   - _ 数字分隔符；e 科学计数
//   - 惰性宽度 + 后缀并存（Q39/Q40）

fn main(args: o Vec(String)) !void {
    var hex: u32 = 0xFF;          // 255
    var bin: u8 = 0b1010;         // 10
    var oct: i32 = 0o17;          // 15
    var big = 1_000_000;          // 分隔符（可读性）
    var sci = 1.5e9;              // 科学计数
    var speed = 0b1100_0010;      // 分隔符用于二进制

    io.print("{x} {} {} {} {}\n", hex, bin, oct, big, sci);
    io.print("{b}\n", speed);
}

[test] fn base_literals() !void {
    var hex: u32 = 0xFF;
    var bin: u8 = 0b1010;
    var oct: i32 = 0o17;
    var big = 1_000_000;
    var sci = 1.5e9;
    try expect_eq(hex, 255);
    try expect_eq(bin, 10);
    try expect_eq(oct, 15);
    try expect_eq(big, 1000000);
    try expect(sci > 1.49e9 and sci < 1.51e9);
}
