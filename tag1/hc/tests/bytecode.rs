//! M3.2 字节码 VM 验收测试：`encode`/`decode` 往返 + 字节码 VM == IR 参考解释器。
//!
//! 核心断言 = 一致性：`run_bytecode(encode(lower(p)), entry, args)` 的结果与
//! `run_ir(lower(p), entry, args)`（唯一语义源）逐值相等（含错误路径）。
//! 覆盖 M3.1 切片：标量/短路/if/while/return/try/catch/orelse/断言/限定名调用/字符串。

use hc::bytecode::{encode, run_bytecode};
use hc::ir::{lower, run_ir, IrError, IrValue};
use hc::parse_source;

/// 解析 + lower + encode → 字节码 VM 执行（失败时 unwrap 给出诊断）
fn run_bc(src: &str, entry: &str, args: &[IrValue]) -> Result<IrValue, IrError> {
    let program = parse_source(src).unwrap_or_else(|d| panic!("parse failed: {d:?}"));
    let module = lower(&program).unwrap();
    run_bytecode(&encode(&module), entry, args)
}

/// 断言字节码 VM 与参考解释器（同语义源）结果一致：值 / 错误名逐项相等。
fn assert_consistent(src: &str, entry: &str, args: &[IrValue]) {
    let program = parse_source(src).unwrap_or_else(|d| panic!("parse failed: {d:?}"));
    let module = lower(&program).unwrap();
    let reference = run_ir(&module, entry, args);
    let via_bc = run_bytecode(&encode(&module), entry, args);
    match (reference, via_bc) {
        (Ok(a), Ok(b)) => assert_eq!(a, b, "字节码 VM 返回值与参考解释器不一致"),
        (Err(a), Err(b)) => assert_eq!(a.name, b.name, "字节码 VM 错误名与参考解释器不一致"),
        (a, b) => panic!("结果形态不一致：参考 {a:?} vs 字节码 {b:?}"),
    }
}

#[test]
fn scalar_arith_and_compare() {
    let src = "fn add(a: i32, b: i32) i32 { return a + b; }";
    assert_consistent(src, "add", &[IrValue::Int(2), IrValue::Int(3)]);
    assert_eq!(
        run_bc(src, "add", &[IrValue::Int(2), IrValue::Int(3)]).unwrap(),
        IrValue::Int(5)
    );
    let src = "fn cmp(a: i32, b: i32) bool { return a < b and b <= 3; }";
    assert_consistent(src, "cmp", &[IrValue::Int(1), IrValue::Int(3)]);
}

#[test]
fn short_circuit_and_or() {
    let src = r#"
fn expect_bad() bool {
    expect(false);
    return true;
}
fn and_sc() bool { return false and expect_bad(); }
fn or_sc() bool { return true or expect_bad(); }
"#;
    assert_consistent(src, "and_sc", &[]);
    assert_consistent(src, "or_sc", &[]);
    assert_eq!(run_bc(src, "and_sc", &[]).unwrap(), IrValue::Bool(false));
    assert_eq!(run_bc(src, "or_sc", &[]).unwrap(), IrValue::Bool(true));
}

#[test]
fn if_stmt_else_if_and_expr() {
    let src = r#"
fn grade(x: i32) i32 {
    if (x >= 90) { return 1; }
    else if (x >= 60) { return 2; }
    else { return 3; }
}
fn label(x: i32) &[u8] { return if (x > 5) "big" else "small"; }
"#;
    assert_consistent(src, "grade", &[IrValue::Int(70)]);
    assert_consistent(src, "label", &[IrValue::Int(3)]);
    assert_eq!(
        run_bc(src, "label", &[IrValue::Int(7)]).unwrap(),
        IrValue::Str(b"big".to_vec())
    );
}

#[test]
fn while_loop_with_step() {
    let src = r#"
fn sum_to(n: i32) i32 {
    var mut i: i32 = 0;
    var mut sum: i32 = 0;
    while (i < n) : (i += 1) { sum += i; }
    return sum;
}
"#;
    assert_consistent(src, "sum_to", &[IrValue::Int(5)]);
    assert_eq!(
        run_bc(src, "sum_to", &[IrValue::Int(5)]).unwrap(),
        IrValue::Int(10)
    );
}

#[test]
fn try_catch_and_orelse() {
    let src = r#"
fn fail() !i32 { return error.NotFound; }
fn ok() !i32 { return 5; }
fn f() i32 {
    var t = try ok();
    var s = fail() catch 7;
    return t + s;
}
fn d(x: ?i32) i32 { return x orelse 5; }
"#;
    assert_consistent(src, "f", &[]);
    assert_eq!(run_bc(src, "f", &[]).unwrap(), IrValue::Int(12));
    assert_consistent(src, "d", &[IrValue::Opt(None)]);
    assert_consistent(src, "d", &[IrValue::Int(7)]);
}

#[test]
fn error_value_and_assert_builtins() {
    let src = r#"
fn fail() !i32 { return error.NotFound; }
[test] fn t() void {
    expect_eq(1 + 1, 2);
    expect_neq(1, 2);
    expect(true);
    expect_error(error.NotFound, error.NotFound);
}
"#;
    assert_consistent(src, "fail", &[]);
    assert_eq!(run_bc(src, "fail", &[]).unwrap(), IrValue::Err { name: "NotFound".into(), code: 0 });
    assert_consistent(src, "t", &[]);
    // 断言失败路径（AssertFailed 错误名一致）
    let bad = "fn f() void { expect_eq(1, 2); }";
    assert_consistent(bad, "f", &[]);
    assert_eq!(run_bc(bad, "f", &[]).unwrap_err().name, "AssertFailed");
}

#[test]
fn namespace_qualified_call() {
    let src = r#"
namespace Math {
    fn square(x: i32) i32 { return x * x; }
}
fn f(x: i32) i32 { return Math.square(x); }
"#;
    assert_consistent(src, "f", &[IrValue::Int(4)]);
    assert_eq!(run_bc(src, "f", &[IrValue::Int(4)]).unwrap(), IrValue::Int(16));
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
    assert_consistent(src, "f", &[IrValue::Int(21)]);
}

#[test]
fn div_zero_and_float_ieee() {
    let src = "fn d(a: i32) i32 { return a / 0; }";
    assert_consistent(src, "d", &[IrValue::Int(10)]);
    assert_eq!(run_bc(src, "d", &[IrValue::Int(10)]).unwrap_err().name, "DivisionByZero");
    let src3 = "fn f(a: f64) f64 { return a / 0.0; }";
    assert_consistent(src3, "f", &[IrValue::Float(1.0)]);
    assert_eq!(
        run_bc(src3, "f", &[IrValue::Float(1.0)]).unwrap(),
        IrValue::Float(f64::INFINITY)
    );
}

#[test]
fn int_overflow_error() {
    let src = "fn f(a: i32, b: i32) i32 { return a * b; }";
    assert_consistent(src, "f", &[IrValue::Int(i128::MAX), IrValue::Int(2)]);
    assert_eq!(
        run_bc(src, "f", &[IrValue::Int(i128::MAX), IrValue::Int(2)]).unwrap_err().name,
        "Overflow"
    );
}

#[test]
fn missing_entry_and_unknown_call() {
    let src = "fn f() i32 { return 1; }";
    let program = parse_source(src).unwrap();
    let module = lower(&program).unwrap();
    let reference = run_ir(&module, "nope", &[]).unwrap_err();
    let via_bc = run_bytecode(&encode(&module), "nope", &[]).unwrap_err();
    assert_eq!(reference.name, via_bc.name);
    assert_eq!(via_bc.name, "NoFunction");

    let src2 = "fn f() i32 { return nope(); }";
    assert_consistent(src2, "f", &[]);
    assert_eq!(run_bc(src2, "f", &[]).unwrap_err().name, "NoFunction");
}

#[test]
fn pointer_write_through_bytecode() {
    // Phase 1 指针：取址/解引用/写穿经 encode/decode 后与参考解释器一致
    let src = r#"
fn f() i32 {
    var mut x: i32 = 5;
    var p = &mut x;
    p.* = 7;
    p.* += 1;
    return x;
}
fn bump(p: *mut i32) void {
    p.* *= 2;
}
fn g() i32 {
    var mut x: i32 = 21;
    bump(&mut x);
    return x;
}
"#;
    assert_consistent(src, "f", &[]);
    assert_eq!(run_bc(src, "f", &[]).unwrap(), IrValue::Int(8));
    assert_consistent(src, "g", &[]);
    assert_eq!(run_bc(src, "g", &[]).unwrap(), IrValue::Int(42));
}

#[test]
fn pointer_eq_and_deref_bytecode() {
    let src = r#"
fn same() bool {
    var mut x: i32 = 5;
    var p = &mut x;
    var q = &mut x;
    return p == q;
}
fn deref_eq() bool {
    var mut x: i32 = 5;
    var p = &mut x;
    return p.* == 5;
}
"#;
    assert_consistent(src, "same", &[]);
    assert_consistent(src, "deref_eq", &[]);
    assert_eq!(run_bc(src, "same", &[]).unwrap(), IrValue::Bool(true));
    assert_eq!(run_bc(src, "deref_eq", &[]).unwrap(), IrValue::Bool(true));
}

#[test]
fn func_index_round_trips_flat_and_qualified() {
    // namespace 内函数扁平名 + 限定名双注册，经 encode/decode 后仍完整
    let src = r#"
namespace io {
    namespace net {
        fn double(x: i32) i32 { return x * 2; }
    }
}
fn top(x: i32) i32 { return x; }
"#;
    let program = parse_source(src).unwrap();
    let module = lower(&program).unwrap();
    let bytes = encode(&module);
    let decoded = hc::bytecode::decode(&bytes).expect("decode");
    assert_eq!(decoded.func_index, module.func_index);
    assert!(decoded.func_index.contains_key("top"));
    assert!(decoded.func_index.contains_key("double"));
    assert!(decoded.func_index.contains_key("io.net.double"));
}
