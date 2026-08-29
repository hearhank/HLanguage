// 12-cast-bits.hc — 类型转换与位运算
// 覆盖：@intCast 收窄/提升/usize 往返、& | ^ >> ~ 位运算、UTF-8 解码式掩码组合（P1/P2）
// 纪律 5：被依赖的自由函数先定义（utf8_len 在 main 之前）
// 预期 stdout：
// 200
// 123456
// 123456
// 300
// 0
// 255
// 15
// 15
// -241
// 1080
// 2
fn utf8_len(lead: u8) usize {
    if (lead < 0x80) { return 1; }
    if (lead < 0xE0) { return 2; }
    if (lead < 0xF0) { return 3; }
    return 4;
}

fn main() !void {
    var mut x: i64 = 300;
    var b: u8 = @intCast(u8, x - 100);
    io.print("{}\n", b);
    var p: usize = 123456;
    var q: i64 = @intCast(i64, p);
    io.print("{}\n", q);
    var r: usize = @intCast(usize, q);
    io.print("{}\n", r);
    var m: i32 = @intCast(i32, x);
    io.print("{}\n", m);
    var bits: i64 = 0xF0;
    io.print("{}\n", bits & 0x0F);
    io.print("{}\n", bits | 0x0F);
    io.print("{}\n", bits ^ 0xFF);
    io.print("{}\n", bits >> 4);
    io.print("{}\n", ~bits);
    // UTF-8 两字节解码（lexer.hc 式掩码）：lead=0xD0 cont=0xB8
    var lead: u8 = 0xD0;
    var cont: u8 = 0xB8;
    var cp: i64 = @intCast(i64, (lead & 0x1F)) << 6 | @intCast(i64, (cont & 0x3F));
    io.print("{}\n", cp);
    io.print("{}\n", utf8_len(lead));
}
