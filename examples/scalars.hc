// 全标量类型：u8-u128 / i8-i128 / usize / isize / f32 双后端一致性验证
// h run examples/scalars.hc 与 h build examples/scalars.hc --exec 输出必须完全一致

struct Metrics {
    count: u8
    ratio: f32
    big: i64
}

fun main() -> void {
    // 字面量后缀 + 打印
    print("u8:", (5u8).to_str(), "u16:", (300u16).to_str(), "u32:", (70000u32).to_str())
    print("u128:", (123u128).to_str(), "usize:", (42usize).to_str())
    print("i8:", (-5i8).to_str(), "i16:", (-300i16).to_str(), "i32:", (-70000i32).to_str())
    print("i64:", (-9000000000i64).to_str(), "i128:", (-123i128).to_str(), "isize:", (-42isize).to_str())
    print("f32:", (1.5f32).to_str(), "f64:", (2.5).to_str())

    // 整除扩展到所有整数类型（向零截断）
    print("u8整除:", (7u8 / 3u8).to_str(), "i32整除:", (-7i32 / 3i32).to_str())
    print("f32除:", (7.0f32 / 3.0f32).to_str())

    // 提升：混合整数 → 更宽；整数 + 浮点 → 浮点
    print("提升:", (1u8 + 1000u16).to_str())
    print("浮点提升:", (1u8 + 1.5f32).to_str())

    // f32 单精度逐运算截断
    print("单精度:", (0.1f32 + 0.2f32).to_str())

    // struct 字段（全类型）
    m = Metrics{ count: 200u8, ratio: 0.25f32, big: -999i64 }
    print("struct:", m)
    print("字段:", m.count.to_str(), m.ratio.to_str(), m.big.to_str())

    // 数组元素类型
    bytes = [1u8, 2u8, 3u8]
    print("数组:", bytes, "len:", bytes.len.to_str())
    print("元素:", bytes[1].to_str())

    // 字节化往返（类型由字段声明恢复）
    b = m.to_bytes()
    print("字节:", b)
    m2 = Metrics.from_bytes(b)
    print("恢复:", m2.count.to_str(), m2.ratio.to_str(), m2.big.to_str())
}
