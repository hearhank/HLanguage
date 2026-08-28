// 08-map.hc — Map 哈希表
// 覆盖：Map<K,V>.init、put、put 覆盖、get().?、contains、orelse 兜底（C6）
// 注：不做 Map 迭代（顺序不确定，违背确定性语料原则）
// 预期 stdout：
// 5
// true
// false
// 2
// 9
// 0
fn main() !void {
    var m = Map<&[u8], i32>.init(alloc);
    m.put("apple", 5);
    m.put("banana", 7);
    io.print("{}\n", m.get("apple").?);
    io.print("{}\n", m.contains("banana"));
    io.print("{}\n", m.contains("cherry"));
    io.print("{}\n", m.len);
    m.put("apple", 9);
    io.print("{}\n", m.get("apple").?);
    var missing = m.get("cherry") orelse 0;
    io.print("{}\n", missing);
}
