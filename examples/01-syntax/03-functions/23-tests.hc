// 23-tests.hc — 单元测试（Q30 定案 2026-08-13）
//
//   - test "名称" { ... } 内联测试块（Zig 式，就近可见）
//   - hc test 命令运行（M8 工具链）
//   - 断言 = try expect(...)：失败即 error（错误传播，无第二套断言语法）

fn add(a: i32, b: i32) i32 {
    return a + b;
}

test "add 基本" {
    try expect(add(1, 2) == 3);
}

test "add 负数" {
    try expect(add(-1, -5) == -6);
}

test "add 交换律" {
    try expect(add(7, 8) == add(8, 7));
}
