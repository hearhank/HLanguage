// 17-ownership.hc — 所有权深潜（move / 借用 / 分配器）
//
// Q23 定案（2026-08-13）：调用点显式 move
//   - 参数签名声明 move + 调用点显式 move：take(move s);
//   - 所有权转移在调用点可见（「没有隐藏控制」）

fn take(io: Io, move y: o String) void {
    io.print("took: {}\n", y);   // y 函数内隐含拥有（12.5），退出自动销毁
}

fn make() o String {
    var s = String.from("made", alloc);   // alloc = 默认分配器（global，Q8）
    return move s;                        // 新建值必须 move 返回（12.5）
}

fn borrow(io: Io, v: *String) void {
    io.print("borrowed: {}\n", v);        // 借用：调用方保留所有权
}

fn main(io: Io) !void {
    // move 进函数（调用点显式；s1 转移后不可再用）
    var s1 = String.from("hello", alloc);
    take(io, move s1);

    // move 返回
    var s2 = make();

    // 借用：调用方保留所有权
    borrow(io, &s2);

    // arena：无所有权，禁止 move（A6）
    var arena = Arena.init(alloc);
    var buf = arena.alloc(64);
    // take(io, move buf);  // 错误！无 o 变量禁止 move（move 须对整个 arena）
}
