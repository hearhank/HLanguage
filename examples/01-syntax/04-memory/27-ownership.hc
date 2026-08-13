// 27-ownership.hc — 所有权深潜（move / 借用 / 分配器）
//
// Q23 定案（2026-08-13）：调用点显式 move
//   - 参数签名 = 类型标注制（Q22b）：o T 拥有 / *T 只读 / *mut T 可写；move 仅调用点
//   - 所有权转移在调用点可见（「没有隐藏控制」）
// Q1–Q3 定案（2026-08-13）：可移动性谓词——实例 move = 整个内存树所有权一起移动；
//   所有字段及递归子项无引用值（*T/*mut T/&[T]/&mut [T]）才可 move。
//   String 无引用值字段（自有子对象），满足谓词，可 move。

fn take(io: *T, y: o String) void where T: Io {
    io.print("took: {}\n", y);   // y 函数内隐含拥有（12.5），退出自动销毁
}

fn make() o String {
    var s = String.from("made", alloc);   // alloc = 默认分配器（global，Q8）
    return move s;                        // 新建值必须 move 返回（12.5）
}

fn borrow(io: *T, v: *String) void where T: Io {
    io.print("borrowed: {}\n", v);        // 借用：调用方保留所有权
}

fn main(io: Io) !void {
    // move 进函数（调用点显式；s1 转移后不可再用）
    var s1 = String.from("hello", alloc);
    take(&io, move s1);

    // move 返回
    var s2 = make();

    // 借用：调用方保留所有权
    borrow(&io, &s2);

    // arena：无所有权，禁止 move（A6）
    var arena = Arena.init(alloc);
    var buf = arena.alloc(64);
    // take(io, move buf);  // 错误！无 o 变量禁止 move（move 须对整个 arena）
}

test "move 进函数" {
    var s1 = String.from("hello", alloc);
    take(&test_io, move s1);   // 转移后 s1 不可再用
}

test "move 返回" {
    var s2 = make();
    try expect_eq(s2.len, 4);   // "made"
}

test "借用不转移所有权" {
    var s2 = String.from("borrow", alloc);
    borrow(&test_io, &s2);
    try expect_eq(s2.len, 6);   // 借用后仍可用
}
