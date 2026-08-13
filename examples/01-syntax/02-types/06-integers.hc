// 06-integers.hc — 整数类型（12.3）
//
// Q39 定案（2026-08-13）：惰性宽度默认 + 显式后缀并存
//   - 字面量默认 comptime_int（使用处定型，超范围编译期报错）
//   - 后缀（42i32 / 255u8）立即定型：anytype/泛型上下文需要
//   - 宽度：i8–i128 / u8–u128 + isize/usize

fn main(io: Io) !void {
    // 惰性宽度：标注类型处定型
    var a: i32 = 42;
    var b: u8 = 255;
    var c: isize = -1;

    // 显式后缀：立即定型
    var d = 42i32;
    var e = 255u8;
    var f = -1isize;

    // 宽度检查：超范围编译期报错
    // var g: u8 = 256;  // 错误：256 超出 u8 范围（惰性宽度定型检查）

    // 算术（溢出按模式检测，Q24）
    var sum = a + d;
    io.print("{} {} {} {} {} {}\n", a, b, c, d, e, sum);
}
