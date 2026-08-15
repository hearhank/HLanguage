// 51-collection-bytes.hc — 集合二进制序列化（Q38 定案 2026-08-13）
//
//   - 内建泛型：v.to_bytes() —— 长度前缀（u64 LE）+ 元素字节序列
//   - 字节序固定小端（LE，跨平台一致）；元素递归用内建转换
//   - 定制格式（压缩/加密/版本化）走脚本生成覆盖（Q38 C）

fn main(io: Io) !void {
    var v = Vec(i32).init(alloc);
    v.append(1);
    v.append(2);
    v.append(3);

    // 内建序列化：长度前缀 + 元素字节
    var bytes = v.to_bytes();
    io.print("bytes len = {}\n", bytes.len);   // 8（u64 前缀）+ 12（3 × i32）

    // 反序列化
    var v2 = try Vec(i32).from_bytes(bytes);
    io.print("count = {}\n", v2.len);

    // String → bytes：[len][utf8]
    var s = String.from("hello", alloc);
    var s_bytes = s.to_bytes();
    io.print("s bytes = {}\n", s_bytes.len);
}

test fn collection_to_bytes() !void {
    var v = Vec(i32).init(alloc);
    v.append(1);
    v.append(2);
    v.append(3);
    var bytes = v.to_bytes();
    try expect_eq(bytes.len, 8 + 12);   // u64 前缀 + 3 × i32
    var v2 = try Vec(i32).from_bytes(bytes);
    try expect_eq(v2.len, 3);
}

test fn string_to_bytes() !void {
    var s = String.from("hello", alloc);
    try expect_eq(s.as_slice().len, 5);   // 内容视图无前缀
    try expect_eq(s.to_bytes().len, 13);  // 序列化格式：8（u64 前缀）+ 5
}
