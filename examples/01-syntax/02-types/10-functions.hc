// 10-functions.hc — 函数作为值（Q43 定案 2026-08-13）
//
//   - 函数自动满足调用接口（Fn1(i32) i32）：直接作值传递，无需 &
//   - 函数与闭包统一走调用接口（Q13/Q43）——「函数 = 唯一处理逻辑」

fn square(x: i32) i32 { return x * x; }
fn cube(x: i32) i32 { return x * x * x; }

fn apply(f: Fn1(i32) i32, x: i32) i32 {
    return f(x);
}

fn main(io: Io) !void {
    // 函数直接作值传递（满足调用接口）
    io.print("{}\n", apply(square, 5));   // 25
    io.print("{}\n", apply(cube, 3));     // 27

    // 函数作值存变量（接口类型）
    var f: Fn1(i32) i32 = square;
    io.print("{}\n", f(4));               // 16
}
