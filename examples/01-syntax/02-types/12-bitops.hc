// 12-bitops.hc — 位运算实战（12.2）
//
//   - & | ^ ~ << >>（符号式位运算）
//   - 场景：标志位、位掩码、移位

const FLAG_READ = 0b001;
const FLAG_WRITE = 0b010;
const FLAG_EXEC = 0b100;

fn main(io: Io) !void {
    // 标志位组合
    var flags = FLAG_READ | FLAG_WRITE;     // 0b011
    io.print("readable = {}\n", (flags & FLAG_READ) != 0);

    // 置位 / 清除
    flags |= FLAG_EXEC;
    flags &= ~FLAG_WRITE;
    io.print("{b}\n", flags);

    // 移位
    var x: u8 = 1;
    var shifted = x << 4;                   // 16
    io.print("{}\n", shifted);

    // 异或
    var parity = 0b1010 ^ 0b0101;           // 全 1
    io.print("{b}\n", parity);
}
