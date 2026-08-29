// 13-opt-capture.hc — 可选捕获
// 覆盖：if (opt)|v| some/else、while (opt)|v| 迭代终止、Map.get 可选捕获（P3）
// 预期 stdout：
// some=11
// none ok
// it=7
// it=11
// it=13
// map=42
// miss ok
fn main() !void {
    var v = Vec<i32>.init(alloc);
    v.append(7);
    v.append(11);
    v.append(13);
    if (v.get(1)) |x| {
        io.print("some={}\n", x);
    } else {
        io.print("some-BAD\n");
    }
    if (v.get(9)) |y| {
        io.print("none-BAD\n");
    } else {
        io.print("none ok\n");
    }
    var mut idx: usize = 0;
    while (v.get(idx)) |z| {
        io.print("it={}\n", z);
        idx += 1;
    }
    var m = Map<&[u8], i32>.init(alloc);
    m.put("k", 42);
    if (m.get("k")) |kv| {
        io.print("map={}\n", kv);
    } else {
        io.print("map-BAD\n");
    }
    if (m.get("nope")) |mv| {
        io.print("miss-BAD\n");
    } else {
        io.print("miss ok\n");
    }
}
