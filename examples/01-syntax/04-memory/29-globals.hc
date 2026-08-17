import H.std.{io};

// 29-globals.hc — global 变量模式（根作用域上下文，A7/Q3；Q2' 2026-08-14 修订）
//
//   - global：静态生命周期（程序退出时销毁）、不可 move；**有所有权（归根作用域，Q2'）**
//   - 线程内：每线程独立根上下文 + 独立默认分配器（Q8）
//   - 跨线程共享可变数据：用四模式类型（12.21），勿裸用 global 可变

global APP_NAME: &[u8] = "h";       // 只读常量
global MAX_RETRIES: i32 = 3;

fn main(args: o Vec(String)) !void {
    io.print("{} retries = {}\n", APP_NAME, MAX_RETRIES);
}

[test] fn global_constants() !void {
    try expect_eq_slices(APP_NAME, "h");
    try expect_eq(MAX_RETRIES, 3);
}
