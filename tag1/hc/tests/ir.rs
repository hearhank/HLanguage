//! M3.1 共享 IR 验收测试：lower（AST→IR）+ run_ir（参考解释器）
//!
//! 语义锚点 = tree-walking 解释器（hc-rt）：标量/短路/if/while/return/
//! try/catch/orelse/断言/限定名调用/作用域遮蔽/复合赋值。

use hc::ir::{lower, run_ir, IrValue};
use hc::parse_source;

/// 解析 + 降级 + 执行（失败时 unwrap 给出诊断）
fn run(src: &str, entry: &str, args: &[IrValue]) -> Result<IrValue, hc::ir::IrError> {
    let program = parse_source(src).unwrap_or_else(|d| panic!("parse failed: {d:?}"));
    let module = lower(&program);
    run_ir(&module, entry, args)
}

#[test]
fn scalar_arith_and_compare() {
    let src = "fn add(a: i32, b: i32) i32 { return a + b; }";
    assert_eq!(
        run(src, "add", &[IrValue::Int(2), IrValue::Int(3)]).unwrap(),
        IrValue::Int(5)
    );
    let src = "fn cmp(a: i32, b: i32) bool { return a < b and b <= 3; }";
    assert_eq!(
        run(src, "cmp", &[IrValue::Int(1), IrValue::Int(3)]).unwrap(),
        IrValue::Bool(true)
    );
}

#[test]
fn short_circuit_and_or() {
    // and 左假 / or 左真 → 不求值右侧（右侧 expect(false) 若被求值则 AssertFailed）
    let src = r#"
fn expect_bad() bool {
    expect(false);
    return true;
}
fn and_sc() bool { return false and expect_bad(); }
fn or_sc() bool { return true or expect_bad(); }
"#;
    assert_eq!(run(src, "and_sc", &[]).unwrap(), IrValue::Bool(false));
    assert_eq!(run(src, "or_sc", &[]).unwrap(), IrValue::Bool(true));
    // 非短路路径：右侧被求值 → AssertFailed
    let src2 =
        "fn eb() bool { expect(false); return true; }\nfn f() bool { return true and eb(); }";
    let e = run(src2, "f", &[]).unwrap_err();
    assert_eq!(e.name, "AssertFailed");
}

#[test]
fn if_stmt_and_else_if() {
    let src = r#"
fn grade(x: i32) i32 {
    if (x >= 90) { return 1; }
    else if (x >= 60) { return 2; }
    else { return 3; }
}
"#;
    assert_eq!(
        run(src, "grade", &[IrValue::Int(95)]).unwrap(),
        IrValue::Int(1)
    );
    assert_eq!(
        run(src, "grade", &[IrValue::Int(70)]).unwrap(),
        IrValue::Int(2)
    );
    assert_eq!(
        run(src, "grade", &[IrValue::Int(30)]).unwrap(),
        IrValue::Int(3)
    );
}

#[test]
fn if_expr_value() {
    let src = r#"
fn label(x: i32) &[u8] { return if (x > 5) "big" else "small"; }
"#;
    assert_eq!(
        run(src, "label", &[IrValue::Int(7)]).unwrap(),
        IrValue::Str(b"big".to_vec())
    );
    assert_eq!(
        run(src, "label", &[IrValue::Int(3)]).unwrap(),
        IrValue::Str(b"small".to_vec())
    );
}

#[test]
fn if_optional_capture() {
    let src = r#"
fn pick(x: ?i32) i32 {
    if (x) |v| {
        return v;
    } else {
        return 0;
    }
}
"#;
    assert_eq!(run(src, "pick", &[IrValue::Null]).unwrap(), IrValue::Int(0));
    assert_eq!(
        run(src, "pick", &[IrValue::Int(7)]).unwrap(),
        IrValue::Int(7)
    );
}

#[test]
fn while_with_step_sum() {
    let src = r#"
fn sum_to(n: i32) i32 {
    var mut i: i32 = 0;
    var mut sum: i32 = 0;
    while (i < n) : (i += 1) {
        sum += i;
    }
    return sum;
}
"#;
    assert_eq!(
        run(src, "sum_to", &[IrValue::Int(5)]).unwrap(),
        IrValue::Int(10)
    );
}

#[test]
fn compound_assign() {
    let src = r#"
fn f() i32 {
    var mut x: i32 = 5;
    x += 3;
    x *= 2;
    return x;
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(16));
}

#[test]
fn block_scope_shadowing_restores() {
    // 内层块遮蔽 x；块退出后恢复外层绑定（对齐解释器作用域）
    let src = r#"
fn f() i32 {
    var x: i32 = 1;
    { var x: i32 = 2; }
    return x;
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(1));
}

#[test]
fn recursion_fib() {
    let src = r#"
fn fib(n: i32) i32 {
    if (n <= 1) { return n; }
    return fib(n - 1) + fib(n - 2);
}
"#;
    assert_eq!(
        run(src, "fib", &[IrValue::Int(10)]).unwrap(),
        IrValue::Int(55)
    );
}

#[test]
fn try_propagates_error_value() {
    // try：错误沿值通道从当前函数返回（对齐 M2.6 传播模型）
    let src = r#"
fn fail() !i32 { return error.NotFound; }
fn g() !i32 { return try fail(); }
"#;
    assert_eq!(run(src, "g", &[]).unwrap(), IrValue::Err("NotFound".into()));
}

#[test]
fn catch_default_bind_and_block_value() {
    let src = r#"
fn fail() !i32 { return error.NotFound; }
fn d() i32 { return fail() catch 42; }
fn b() i32 { return fail() catch |e| { 40 + 2; }; }
fn blk() i32 { return fail() catch |e| { var x: i32 = 40; x + 2; }; }
"#;
    assert_eq!(run(src, "d", &[]).unwrap(), IrValue::Int(42));
    assert_eq!(run(src, "b", &[]).unwrap(), IrValue::Int(42));
    assert_eq!(run(src, "blk", &[]).unwrap(), IrValue::Int(42));
    // catch 块内 return 直接退出函数
    let src2 = r#"
fn fail() !i32 { return error.NotFound; }
fn g() i32 { return fail() catch |e| { return 99; }; }
"#;
    assert_eq!(run(src2, "g", &[]).unwrap(), IrValue::Int(99));
}

#[test]
fn catch_passes_non_error() {
    let src = "fn f() i32 { return 7 catch 42; }";
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(7));
}

#[test]
fn orelse_null_default() {
    let src = "fn d(x: ?i32) i32 { return x orelse 5; }";
    assert_eq!(run(src, "d", &[IrValue::Null]).unwrap(), IrValue::Int(5));
    assert_eq!(run(src, "d", &[IrValue::Int(7)]).unwrap(), IrValue::Int(7));
}

#[test]
fn assert_builtins_ok_and_fail() {
    let src = r#"
test fn t() void {
    expect_eq(1 + 1, 2);
    expect_neq(1, 2);
    expect(true);
    expect_error(error.NotFound, error.NotFound);
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Void);
    let bad = "fn f() void { expect_eq(1, 2); }";
    let e = run(bad, "f", &[]).unwrap_err();
    assert_eq!(e.name, "AssertFailed");
}

#[test]
fn namespace_qualified_call() {
    let src = r#"
namespace Math {
    fn square(x: i32) i32 { return x * x; }
}
fn f(x: i32) i32 { return Math.square(x); }
"#;
    assert_eq!(run(src, "f", &[IrValue::Int(4)]).unwrap(), IrValue::Int(16));
}

#[test]
fn nested_namespace_qualified_call() {
    let src = r#"
namespace io {
    namespace net {
        fn double(x: i32) i32 { return x * 2; }
    }
}
fn f(x: i32) i32 { return io.net.double(x); }
"#;
    assert_eq!(
        run(src, "f", &[IrValue::Int(21)]).unwrap(),
        IrValue::Int(42)
    );
}

#[test]
fn func_index_registers_flat_and_qualified() {
    let src = r#"
namespace io {
    namespace net {
        fn double(x: i32) i32 { return x * 2; }
    }
}
fn top(x: i32) i32 { return x; }
"#;
    let program = parse_source(src).unwrap();
    let module = lower(&program);
    assert!(module.func_index.contains_key("top"));
    assert!(module.func_index.contains_key("double"));
    assert!(module.func_index.contains_key("io.net.double"));
}

#[test]
fn div_zero_lenient() {
    // 除零返回 0（与 tree-walking 宽松语义一致）
    let src = "fn d(a: i32) i32 { return a / 0; }";
    assert_eq!(run(src, "d", &[IrValue::Int(10)]).unwrap(), IrValue::Int(0));
}

#[test]
fn missing_entry_error() {
    let src = "fn f() i32 { return 1; }";
    let program = parse_source(src).unwrap();
    let module = lower(&program);
    let e = run_ir(&module, "nope", &[]).unwrap_err();
    assert_eq!(e.name, "NoFunction");
}

#[test]
fn call_unknown_function() {
    let src = "fn f() i32 { return nope(); }";
    let e = run(src, "f", &[]).unwrap_err();
    assert_eq!(e.name, "NoFunction");
}

#[test]
fn test_fn_is_registered() {
    // test fn 也降级（测试入口经 IR 运行）
    let src = "test fn t() void { expect_eq(2 * 3, 6); }";
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Void);
}
