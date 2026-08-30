// stage2/test/float_probe.hc — S0 T0a：浮点最小探针
// 字面量全部为单 digit/二进制精确值（lower_parse_float 逐位合成 == Rust 正确舍入）
fn main(args: Vec<String>) !void {
    io.print("{}\n", 1.5);
    io.print("{}\n", 0.1);
    io.print("{}\n", 0.5);
    io.print("{}\n", 2.5);
    io.print("{}\n", 7.5);
    io.print("{}\n", 8.5);
    io.print("{}\n", 10.0);
    io.print("{}\n", 0.0);
    io.print("{}\n", 0.1 + 0.5);
    io.print("{}\n", 7.5 - 2.5);
    io.print("{}\n", 2.5 * 4.0);
    io.print("{}\n", 7.5 / 2.5);
    io.print("{}\n", -2.5);
    io.print("{}\n", 1 + 0.5);
    io.print("{}\n", 0.5 < 1.5);
    io.print("{}\n", 2.5 >= 2.5);
    var mut v: f64 = 1.5;
    v = v * 2.0;
    io.print("{}\n", v);
    v += 0.5;
    io.print("{}\n", v);
}
