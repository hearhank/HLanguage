// t2.hc — Map.contains 语义探针（期望：true / false）
fn main(args: Vec<String>) !void {
    var m = Map<&[u8], &[u8]>.init(alloc);
    m.put("a", "1");
    io.print("{}\n", m.contains("a"));
    io.print("{}\n", m.contains("zz"));
}
