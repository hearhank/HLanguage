//! M3.4 双模式一致性验收测试
//!
//! 同一程序分别经 **tree-walking 解释器**（hc-rt `Interp`，脚本模式）与
//! **IR 参考解释器**（`hc::ir::run_ir`，M3.1 共享 IR 语义源）运行全部
//! `test fn`，PASS/FAIL 结果必须完全一致（ADR-0004 双模式一致性承诺根基）。
//!
//! 结果归一化：
//! - tree-walk：`[PASS]/[FAIL]`（`run_tests` 输出）
//! - IR：`Ok(非错误值)` = PASS；`Ok(错误值)` = FAIL（未处理错误到根 = panic 式失败，
//!   M2.6 传播模型）；`Err` = FAIL
//!
//! 程序约束：必须通过语义检查（`Interp::load` 内置 M2 静态 pass）——例如
//! `catch` 只能用于错误联合值、字面量必须在声明的宽度内。
//! 覆盖范围 = M3.1 IR 切片：标量/短路/if/while/递归/try/catch/orelse/
//! error 字面量/断言/限定名调用（含多级 namespace）/作用域/复合赋值/除零溢出。

use std::collections::HashMap;
use std::thread;

use hc::ir::{lower, run_ir, IrValue};
use hc::parse_source;
use hc_rt::Interp;

/// 双模式一致性：两模式全部 test fn 的 PASS/FAIL 必须一致。
/// 返回 (tree-walk 通过数, IR 通过数) 供调用方断言。
/// 在 32MB 栈线程中运行（tree-walking 递归栈深，镜像 CLI 64MB 做法）。
fn assert_consistent(src: &str) -> (usize, usize) {
    let src = src.to_string();
    thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || check(&src))
        .expect("spawn consistency thread")
        .join()
        .expect("consistency thread panicked")
}

fn check(src: &str) -> (usize, usize) {
    let program = parse_source(src).unwrap_or_else(|d| panic!("parse failed: {d:?}"));

    // 模式 A：tree-walking 解释器
    let mut interp = Interp::new(src);
    interp
        .load(&program)
        .unwrap_or_else(|e| panic!("load failed: {}: {}", e.name, e.message));
    interp.run_tests();
    let mut tw: HashMap<String, bool> = HashMap::new(); // 测试名 → 是否 PASS
    for line in &interp.test_out {
        if let Some(rest) = line.strip_prefix("[PASS] ") {
            tw.insert(rest.to_string(), true);
        } else if let Some(rest) = line.strip_prefix("[FAIL] ") {
            let name = rest.split(' ').next().unwrap_or(rest);
            tw.insert(name.to_string(), false);
        }
    }
    if tw.is_empty() {
        panic!("tree-walk 未运行任何 test fn");
    }

    // 模式 B：IR 参考解释器
    let module = lower(&program);
    let mut tw_pass = 0usize;
    let mut ir_pass = 0usize;
    for (name, passed_tw) in &tw {
        let r = run_ir(&module, name, &[]);
        let passed_ir = match &r {
            Ok(IrValue::Err(_)) => false, // 未处理错误值到根 → FAIL（M2.6）
            Ok(_) => true,
            Err(_) => false,
        };
        if *passed_tw {
            tw_pass += 1;
        }
        if passed_ir {
            ir_pass += 1;
        }
        assert_eq!(
            passed_ir,
            *passed_tw,
            "双模式不一致：test `{name}`\n  tree-walk: {}\n  IR: {:?}",
            if *passed_tw { "PASS" } else { "FAIL" },
            r
        );
    }
    // IR 侧应能找到全部测试名（tree-walk 注册了但 IR 未注册 = 语义源缺漏）
    for (name, _) in &tw {
        assert!(
            module.func_index.contains_key(name),
            "IR 未注册测试 `{name}`（tree-walk 已注册）"
        );
    }
    (tw_pass, ir_pass)
}

/// 断言两模式全部通过
fn assert_all_pass(src: &str) {
    let (tw, ir) = assert_consistent(src);
    assert_eq!(tw, ir, "tree-walk 与 IR 通过数不一致");
    assert!(tw > 0, "一致性程序至少应有一个通过的测试（测试编写问题）");
}

#[test]
fn arith_and_compare() {
    assert_all_pass(
        r#"
test fn arith() void {
    expect_eq(2 + 3 * 4, 14);
    expect_eq(17 %% 5, 2);
    expect_eq(20 / 3, 6);
    expect_eq(20 % 3, 2);
    expect_neq(1 + 1, 3);
    expect(10 > 5 and 5 <= 5);
    expect(!false or 1 < 0);
}
"#,
    );
}

#[test]
fn short_circuit_and_or() {
    // 全通过路径（2 个通过）
    assert_all_pass(
        r#"
fn expect_bad() bool {
    expect(false);
    return true;
}
test fn and_short_circuits() void {
    if (false and expect_bad()) {
        expect(false);
    }
}
test fn or_short_circuits() void {
    if (true or expect_bad()) {
        expect(true);
    }
}
"#,
    );
    // 非短路路径：右侧被求值 → 断言失败（两模式都必须 FAIL）
    let src = r#"
fn expect_bad() bool {
    expect(false);
    return true;
}
test fn and_short_circuits() void {
    if (false and expect_bad()) {
        expect(false);
    }
}
test fn and_eager_fails() void {
    if (true and expect_bad()) {
        expect(false);
    }
}
"#;
    let (tw, ir) = assert_consistent(src);
    assert_eq!((tw, ir), (1, 1));
}

#[test]
fn if_else_chain() {
    assert_all_pass(
        r#"
fn grade(x: i32) i32 {
    if (x >= 90) { return 1; }
    else if (x >= 60) { return 2; }
    else { return 3; }
}
test fn grades() void {
    expect_eq(grade(95), 1);
    expect_eq(grade(70), 2);
    expect_eq(grade(30), 3);
}
"#,
    );
}

#[test]
fn if_expr_and_optional_capture() {
    assert_all_pass(
        r#"
fn label(x: i32) &[u8] { return if (x > 5) "big" else "small"; }
fn pick(x: ?i32) i32 {
    if (x) |v| {
        return v;
    } else {
        return 0;
    }
}
test fn if_expr() void {
    expect_eq_slices(label(7), "big");
    expect_eq_slices(label(3), "small");
    expect_eq(pick(null), 0);
    expect_eq(pick(7), 7);
}
"#,
    );
}

#[test]
fn while_with_step() {
    assert_all_pass(
        r#"
fn sum_to(n: i32) i32 {
    var mut i: i32 = 0;
    var mut sum: i32 = 0;
    while (i < n) : (i += 1) {
        sum += i;
    }
    return sum;
}
test fn sum() void {
    expect_eq(sum_to(5), 10);
}
"#,
    );
}

#[test]
fn recursion() {
    assert_all_pass(
        r#"
fn fib(n: i32) i32 {
    if (n <= 1) { return n; }
    return fib(n - 1) + fib(n - 2);
}
test fn fib10() void {
    expect_eq(fib(10), 55);
}
"#,
    );
}

#[test]
fn try_catch_error_channel() {
    assert_all_pass(
        r#"
fn fail() !i32 { return error.NotFound; }
fn catch_default() i32 { return fail() catch 42; }
fn catch_bind() i32 { return fail() catch |e| { 40 + 2; }; }
fn catch_block_value() i32 { return fail() catch |e| { var x: i32 = 40; x + 2; }; }
test fn catch_variants() void {
    expect_eq(catch_default(), 42);
    expect_eq(catch_bind(), 42);
    expect_eq(catch_block_value(), 42);
}
"#,
    );
}

#[test]
fn try_propagates_to_root() {
    // try 传播未处理错误到测试根：两模式都必须 FAIL
    let src = r#"
fn fail() !i32 { return error.NotFound; }
test fn try_bubbles() void {
    var x = try fail();
    expect_eq(x, 1);
}
"#;
    let (tw, ir) = assert_consistent(src);
    assert_eq!((tw, ir), (0, 0));
}

#[test]
fn orelse() {
    assert_all_pass(
        r#"
fn d(x: ?i32) i32 { return x orelse 5; }
test fn orelse_null() void {
    expect_eq(d(null), 5);
    expect_eq(d(7), 7);
}
"#,
    );
}

#[test]
fn qualified_namespace_calls() {
    // 含多级 namespace（io.net.double）：M3.4 起两模式均支持
    assert_all_pass(
        r#"
namespace Math {
    fn square(x: i32) i32 { return x * x; }
}
namespace io {
    namespace net {
        fn double(x: i32) i32 { return x * 2; }
    }
}
test fn qualified() void {
    expect_eq(Math.square(4), 16);
    expect_eq(io.net.double(21), 42);
}
"#,
    );
}

#[test]
fn block_scope_shadowing() {
    assert_all_pass(
        r#"
fn f() i32 {
    var x: i32 = 1;
    { var x: i32 = 2; }
    return x;
}
test fn shadow_restores() void {
    expect_eq(f(), 1);
}
"#,
    );
}

#[test]
fn expect_error_and_unhandled_root() {
    assert_all_pass(
        r#"
fn fail() !i32 { return error.NotFound; }
test fn err_literal() void {
    expect_error(fail(), error.NotFound);
}
"#,
    );
    // 未处理错误值返回测试根：两模式都 FAIL（M2.6 根作用域 panic 式失败）
    let src = r#"
fn fail() !i32 { return error.NotFound; }
test fn unhandled() !void {
    return error.NotFound;
}
"#;
    let (tw, ir) = assert_consistent(src);
    assert_eq!((tw, ir), (0, 0));
}

#[test]
fn div_zero_and_overflow_both_error() {
    // 除零/整数溢出：两模式都报错 → 测试 FAIL（对齐 tree-walking arith）
    let src = r#"
test fn div_zero() void {
    expect_eq(10 / 0, 0);
}
test fn overflow() void {
    var x = 170141183460469231731687303715884105727;
    expect_eq(x + 1, x);
}
"#;
    let (tw, ir) = assert_consistent(src);
    assert_eq!((tw, ir), (0, 0));
}

#[test]
fn compound_assign() {
    assert_all_pass(
        r#"
fn f() i32 {
    var mut x: i32 = 5;
    x += 3;
    x *= 2;
    return x;
}
test fn comp() void {
    expect_eq(f(), 16);
}
"#,
    );
}
