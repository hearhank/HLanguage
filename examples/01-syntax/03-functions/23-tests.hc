import H.std.{io};

// 23-tests.hc — 单元测试（Q30/Q8/Q-T1~Q-T6 + Q-R11 定案 2026-08-13）
//
//   - [test] fn 名称() !void { ... }：标记为测试的函数（Q-R11，可被普通代码调用/复用）
//   - hc test 运行：默认脚本模式；--mode=compile 交叉验证（Q-T5）
//   - 断言 API 五件套（Q-T1）：expect / expect_eq / expect_neq / expect_error / expect_eq_slices
//   - 输出：逐项 [PASS]/[FAIL]/[SKIP] + 汇总统计（Q-T2）；失败非零退出码
//   - 独立函数作用域 + 串行执行（Q-T3）；io/alloc 隐式注入（Q-T4）

fn add(a: i32, b: i32) i32 {
    return a + b;
}

fn parse_strict(s: &[u8]) !i32 {
    if (s.len == 0) return error.Empty;
    return 42;
}

[test] fn add_basic() !void {
    try expect(add(1, 2) == 3);
}

[test] fn add_negative() !void {
    try expect(add(-1, -5) == -6);
}

[test] fn add_commutative() !void {
    try expect(add(7, 8) == add(8, 7));
}

[test] fn assertion_api_full() !void {
    try expect(true);                               // 布尔
    try expect_eq(add(2, 3), 5);                    // 相等（失败输出期望 vs 实际）
    try expect_neq(add(2, 3), 6);                   // 不等
    try expect_error(error.Empty, parse_strict("")); // 期望错误 error.Empty
    try expect_eq(parse_strict("ok"), 42);          // 成功路径
    try expect_eq_slices("hello", "hello");         // 切片逐项相等
}

[test] fn skip_example() !void {
    // 跳过（Q-T3）：return error.SkipTest;  → 统计为 SKIP
    // if (某种不支持的条件) return error.SkipTest;
    // 本测试永远通过（演示写法，不实际跳过）
    try expect(true);
}

[test] fn test_functions_are_callable() !void {
    // Q-R11：测试函数可被普通代码调用/复用（原「测试块非函数」限制解除）
    try add_basic();          // 调用另一个测试函数
    try assertion_api_full();
}
