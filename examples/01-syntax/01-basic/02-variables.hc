// 02-variables.hc — 变量声明与所有权三形态
//
// 形态 1：非 Arena 分配默认拥有（作用域注册，退出自动销毁；o 冗余标注，仅复杂类型）
// 形态 2：Arena 分配无所有权（归 Arena，禁止 move）
// 形态 3：global（根作用域上下文，静态生命周期）
//
// 分配器模型（Q3/Q8 定案，2026-08-13）：
//   - 默认分配器 = 根作用域的 global（全局可引用，静态方法式调用）
//   - 每个线程有独立的默认分配器（线程创建自己的根上下文）
//   - 显式传递仍推荐：io.alloc / 参数传入（12.18）
//   - arena 显式创建：var arena = Arena.init(alloc);

global APP_NAME: &[u8] = "h";        // 形态 3：global（静态，不可 move）

fn main(io: Io) !void {
    // 形态 1：复杂类型默认拥有（非 Arena，作用域注册，退出自动销毁）——标量无所有权概念（Q15）
    var mut count: i32 = 0;
    count += 1;
    io.print("count = {}\n", count);

    // 形态 2：Arena 分配无所有权（归 Arena；禁止 move）
    var arena = Arena.init(alloc);
    var buf: &[u8] = arena.alloc(256);
    io.print("buf len = {}\n", buf.len);

    io.print("{}\n", APP_NAME);
}

test "变量三形态" {
    // 形态 1：标量（无所有权概念，Q15）
    var mut count: i32 = 0;
    count += 1;
    try expect_eq(count, 1);

    // 形态 2：arena 分配（无所有权，归 Arena）
    var arena = Arena.init(alloc);
    var buf: &[u8] = arena.alloc(256);
    try expect_eq(buf.len, 256);

    // 形态 3：global（静态生命周期）
    try expect_eq_slices(APP_NAME, "h");
}
