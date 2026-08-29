// probe-opt.hc — C6.3 验收：get().? 解包 + orelse 兜底
fn main() !void {
    var m = Map<&[u8], i32>.init(alloc);
    m.put("k", 7);
    io.print("{}\n", m.get("k").?);
    var v = m.get("nope") orelse 5;
    io.print("{}\n", v);
    var vec = Vec<i32>.init(alloc);
    vec.append(42);
    io.print("{}\n", vec.get(0).?);
    var w = vec.get(9) orelse -1;
    io.print("{}\n", w);
}
