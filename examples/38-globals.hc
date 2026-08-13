// 38-globals.hc — global 变量模式（根作用域上下文，A7/Q3）
//
//   - global：静态生命周期、不可 move、无 o（Q3）
//   - 线程内：每线程独立根上下文 + 独立默认分配器（Q8）
//   - 跨线程共享可变数据：用四模式类型（12.21），勿裸用 global 可变

global APP_NAME: &[u8] = "h";       // 只读常量
global MAX_RETRIES: i32 = 3;

fn main(io: Io) !void {
    io.print("{} retries = {}\n", APP_NAME, MAX_RETRIES);
}
