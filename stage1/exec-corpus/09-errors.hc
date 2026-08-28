// 09-errors.hc — 错误路径
// 覆盖：!T 返回、return error.X、try 传播、catch 默认值、orelse（C9）
// 预期 stdout：
// 42
// 0
// 7
// 5
fn parse_pos(n: i32) !i32 {
    if (n < 0) { return error.Negative; }
    return n * 2;
}
fn main() !void {
    var a = try parse_pos(21);
    io.print("{}\n", a);
    var b = parse_pos(-1) catch 0;
    io.print("{}\n", b);
    var m = Map<&[u8], i32>.init(alloc);
    m.put("k", 7);
    var v = m.get("k") orelse 0;
    io.print("{}\n", v);
    var w = m.get("nope") orelse 5;
    io.print("{}\n", w);
}
