import H.std.{io};

// 27-ownership.hc — 所有权深潜（move / 借用 / 分配器）
//
// Q23 定案（2026-08-13）：调用点显式 move
//   - 参数签名 = 类型标注制（Q22b）：o T 拥有 / *T 只读 / *mut T 可写；move 仅调用点
//   - 所有权转移在调用点可见（「没有隐藏控制」）
// Q-S11 定案（2026-08-13，取代 Q1 谓词）：move 唯一约束 = 拥有所有权（非 Arena）；
//   指针问题（悬垂/别名）由用户负责，不阻塞 move。String 为自有子对象，随实例转移。

fn take<T>(io: *T, y: o String) void where T: Io {
    io.print("took: {}\n", y);   // y 函数内隐含拥有（12.5），退出自动销毁
}

fn make() o String {
    var s = String.from("made", alloc);   // alloc = 默认分配器（global，Q8）
    return move s;                        // 新建值必须 move 返回（12.5）
}

fn borrow<T>(io: *T, v: *String) void where T: Io {
    io.print("borrowed: {}\n", v);        // 借用：调用方保留所有权
}

fn main(args: o Vec<String>) !void {
    // move 进函数（调用点显式；销毁责任转移，原绑定仍可访问——悬垂由用户负责）
    var s1 = String.from("hello", alloc);
    take(&io, move s1);

    // move 返回
    var s2 = make();

    // 借用：调用方保留所有权
    borrow(&io, &s2);

    // arena：无所有权，禁止 move（Q1/Q-S11）
    var arena = Arena.init(alloc);
    var buf = arena.alloc(64);
    // take(io, move buf);  // 错误！Arena 来源无所有权，禁止 move（move 须对整个 arena）
}

[test] fn move_into_function() !void {
    var s1 = String.from("hello", alloc);
    take(&io, move s1);   // 销毁责任转移；原绑定仍可访问（悬垂/冲突由用户负责）
}

[test] fn move_return() !void {
    var s2 = make();
    try expect_eq(s2.len, 4);   // "made"
}

[test] fn borrow_keeps_ownership() !void {
    var s2 = String.from("borrow", alloc);
    borrow(&io, &s2);
    try expect_eq(s2.len, 6);   // 借用后仍可用
}
