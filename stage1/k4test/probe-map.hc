// probe-map.hc — C6.2 验收：Map init/put（覆盖）/contains/len
fn main() !void {
    var m = Map<&[u8], i32>.init(alloc);
    m.put("apple", 5);
    m.put("banana", 7);
    io.print("{}\n", m.len);
    io.print("{}\n", m.contains("banana"));
    io.print("{}\n", m.contains("cherry"));
    m.put("apple", 9);
    io.print("{}\n", m.len);
}
