// 10-comprehensive.hc — 综合语料
// 覆盖：组合 01–09：类状态 + Vec + Map + String + 控制流 + 递归 + 错误路径（C10 全量验收）
// 预期 stdout：
// 12
// 3
// 4
// 2
// -1
// small
// mid
// 24
class Tally {
    mut total: i32,
    mut count: i32,
    fn add(self: *mut Self, v: i32) void {
        self.total += v;
        self.count += 1;
    }
    fn avg(self: *Self) i32 {
        if (self.count == 0) { return 0; }
        return self.total / self.count;
    }
}
fn lookup(m: Map<&[u8], i32>, k: &[u8]) i32 {
    return m.get(k) orelse -1;
}
fn classify(n: i32) &[u8] {
    if (n >= 10) { return "big"; }
    if (n >= 5) { return "mid"; }
    return "small";
}
fn main() !void {
    var t = alloc.init(Tally{total = 0, count = 0});
    var words = Vec<&[u8]>.init(alloc);
    words.append("alpha");
    words.append("be");
    words.append("gamma");
    var m = Map<&[u8], i32>.init(alloc);
    for (words) |w| {
        t.add(w.len);
        m.put(w, w.len);
    }
    io.print("{}\n", t.total);
    io.print("{}\n", t.count);
    io.print("{}\n", t.avg());
    io.print("{}\n", lookup(m, "be"));
    io.print("{}\n", lookup(m, "zz"));
    io.print("{}\n", classify(t.avg()));
    io.print("{}\n", classify(lookup(m, "alpha")));
    var mut i: i32 = 0;
    var mut f: i32 = 1;
    while (i < 4) {
        i += 1;
        f *= i;
    }
    io.print("{}\n", f);
}
