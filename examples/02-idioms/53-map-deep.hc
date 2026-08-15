// 53-map-deep.hc — Map 操作
//
//   - 键值操作：put / get / contains / remove
//   - 遍历：键值对捕获（|kv|）

fn main(io: Io) !void {
    var m = Map(&[u8], i32).init(alloc);
    m.put("apple", 5);
    m.put("banana", 7);

    io.print("apple = {}\n", m.get("apple").?);          // 5
    io.print("has pear = {}\n", m.contains("pear"));     // false

    // 遍历（键值对捕获）
    var total = 0;
    for (m) |kv| {
        total += kv.value;
    }
    io.print("total = {}\n", total);

    m.remove("apple");
    io.print("size = {}\n", m.len);
}

test fn map_iterate_and_remove() !void {
    var m = Map(&[u8], i32).init(alloc);
    m.put("apple", 5);
    m.put("banana", 7);
    var total = 0;
    for (m) |kv| {
        total += kv.value;
    }
    try expect_eq(total, 12);
    m.remove("apple");
    try expect_eq(m.len, 1);
    try expect(!m.contains("apple"));
}
