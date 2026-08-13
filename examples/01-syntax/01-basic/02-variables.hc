// 02-variables.hc — 变量声明与所有权三形态
//
// 形态 1：o 拥有（所有权注册当前作用域，退出自动销毁）
// 形态 2：无 o，所有权在分配器（如 arena，禁止 move）
// 形态 3：global（根作用域上下文，静态生命周期）
//
// 分配器模型（Q3/Q8 定案，2026-08-13）：
//   - 默认分配器 = 根作用域的 global（全局可引用，静态方法式调用）
//   - 每个线程有独立的默认分配器（线程创建自己的根上下文）
//   - 显式传递仍推荐：io.alloc / 参数传入（12.18）
//   - arena 显式创建：var arena = Arena.init(alloc);

global APP_NAME: &[u8] = "h";        // 形态 3：global（静态，不可 move）

fn main(io: Io) !void {
    // 形态 1：o 拥有（作用域注册，退出自动销毁）
    var mut count: o i32 = 0;
    count += 1;
    io.print("count = {}\n", count);

    // 形态 2：无 o，所有权在 arena（禁止对 buf 使用 move）
    var arena = Arena.init(alloc);
    var buf: &[u8] = arena.alloc(256);
    io.print("buf len = {}\n", buf.len);

    io.print("{}\n", APP_NAME);
}
