// 57-protocol-parse.hc — 二进制协议解析（长度前缀帧）
//
//   - 帧格式：[u64 长度][payload bytes]
//   - 连续类型 → bytes（Q36 零拷贝视图）；端序固定 LE（Q38）
//   - ⚠️ 注意：含切片/指针字段的类型不可标 [continuous]（会序列化指针值）——
//     需递归序列化（脚本定制，Q37 精神）或定长缓冲

[continuous]
class Message {   // 连续内存：可 to_bytes 直映射（8 字节）
    id: i32,
    kind: u8,
}

fn encode(m: *Message) o Vec(u8) {
    var payload = m.to_bytes();
    var frame = Vec(u8).init(alloc);
    frame.append_u64(payload.len);     // 长度前缀（u64 LE，Q38）
    frame.extend(payload);
    return frame;
}

fn decode(data: &[u8]) !Message {
    var len = read_u64_le(data[0..8]);
    return Message.from_bytes(data[8 .. 8 + len]);
}

fn main(io: Io) !void {
    var msg = Message{ id = 7, kind = 1 };
    var frame = encode(&msg);
    io.print("frame len = {}\n", frame.len);   // 8 + 8 = 16

    var decoded = try decode(&frame);
    io.print("id = {}\n", decoded.id);
}

[test] fn encode_decode_roundtrip() !void {
    var msg = Message{ id = 7, kind = 1 };
    var frame = encode(&msg);
    try expect_eq(frame.len, 16);   // 8（u64 前缀）+ 8（POD 字节）
    var decoded = try decode(&frame);
    try expect_eq(decoded.id, 7);
    try expect_eq(decoded.kind, 1);
}
