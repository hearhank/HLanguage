//! M3.1 共享 IR 验收测试：lower（AST→IR）+ run_ir（参考解释器）
//!
//! 语义锚点 = tree-walking 解释器（hc-rt）：标量/短路/if/while/return/
//! try/catch/orelse/断言/限定名调用/作用域遮蔽/复合赋值。

use hc::ir::{lower, run_ir, IrValue};
use hc::parse_source;

/// 解析 + 降级 + 执行（失败时 unwrap 给出诊断）
fn run(src: &str, entry: &str, args: &[IrValue]) -> Result<IrValue, hc::ir::IrError> {
    let program = parse_source(src).unwrap_or_else(|d| panic!("parse failed: {d:?}"));
    let module = lower(&program).unwrap();
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
    assert_eq!(run(src, "pick", &[IrValue::Opt(None)]).unwrap(), IrValue::Int(0));
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
    assert_eq!(run(src, "g", &[]).unwrap(), IrValue::Err { name: "NotFound".into(), code: 0 });
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
    assert_eq!(run(src, "d", &[IrValue::Opt(None)]).unwrap(), IrValue::Int(5));
    assert_eq!(run(src, "d", &[IrValue::Int(7)]).unwrap(), IrValue::Int(7));
}

#[test]
fn assert_builtins_ok_and_fail() {
    let src = r#"
[test] fn t() void {
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
    let module = lower(&program).unwrap();
    assert!(module.func_index.contains_key("top"));
    assert!(module.func_index.contains_key("double"));
    assert!(module.func_index.contains_key("io.net.double"));
}

#[test]
fn div_zero_errors() {
    // 整数除/模零 → DivisionByZero（对齐 tree-walking arith）
    let src = "fn d(a: i32) i32 { return a / 0; }";
    let e = run(src, "d", &[IrValue::Int(10)]).unwrap_err();
    assert_eq!(e.name, "DivisionByZero");
    let src2 = "fn m(a: i32) i32 { return a %% 0; }";
    let e = run(src2, "m", &[IrValue::Int(10)]).unwrap_err();
    assert_eq!(e.name, "DivisionByZero");
    // 浮点除零 = IEEE inf（不报错）
    let src3 = "fn f(a: f64) f64 { return a / 0.0; }";
    assert_eq!(
        run(src3, "f", &[IrValue::Float(1.0)]).unwrap(),
        IrValue::Float(f64::INFINITY)
    );
}

#[test]
fn int_overflow_errors() {
    // 整数算术溢出 → Overflow（对齐 tree-walking checked_*）
    let src = "fn f(a: i32, b: i32) i32 { return a * b; }";
    let e = run(src, "f", &[IrValue::Int(i128::MAX), IrValue::Int(2)]).unwrap_err();
    assert_eq!(e.name, "Overflow");
}

#[test]
fn missing_entry_error() {
    let src = "fn f() i32 { return 1; }";
    let program = parse_source(src).unwrap();
    let module = lower(&program).unwrap();
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
    // [test] fn 也降级（测试入口经 IR 运行）
    let src = "[test] fn t() void { expect_eq(2 * 3, 6); }";
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Void);
}

#[test]
fn error_values_carry_ordered_codes() {
    // M4.2：错误值携带编译期码（声明序）；不同错误不同码，比较按码非名
    let src = r#"
fn first() !i32 { return error.First; }
fn second() !i32 { return error.Second; }
"#;
    assert_eq!(
        run(src, "first", &[]).unwrap(),
        IrValue::Err { name: "First".into(), code: 0 }
    );
    assert_eq!(
        run(src, "second", &[]).unwrap(),
        IrValue::Err { name: "Second".into(), code: 1 }
    );
}

#[test]
fn expect_error_distinguishes_error_names() {
    // expect_error 按码比较（M4.2）：同名通过，不同名 → AssertFailed
    let src = r#"
fn ok() void { expect_error(error.NotFound, error.NotFound); }
fn bad() void { expect_error(error.NotFound, error.Permission); }
"#;
    assert_eq!(run(src, "ok", &[]).unwrap(), IrValue::Void);
    let e = run(src, "bad", &[]).unwrap_err();
    assert_eq!(e.name, "AssertFailed");
}

#[test]
fn null_literal_is_opt_none() {
    // null 字面量 = Opt(None)（对齐 tree-walking NullLit）
    let src = "fn f() ?i32 { return null; }";
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Opt(None));
}

#[test]
fn null_equals_null() {
    // Opt(None) == Opt(None) → true（value_eq 递归可选臂）
    let src = "fn f() bool { return null == null; }";
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Bool(true));
}

#[test]
fn pointer_write_through_alias() {
    // Phase 1 指针：`&mut x` 取址 + `p.*` 写穿 → 原变量可见（别名）
    let src = r#"
fn f() i32 {
    var mut x: i32 = 5;
    var p = &mut x;
    p.* = 7;
    return x;
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(7));
}

#[test]
fn pointer_compound_assign_through_alias() {
    let src = r#"
fn f() i32 {
    var mut x: i32 = 5;
    var p = &mut x;
    p.* += 1;
    p.* *= 2;
    return p.*;
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(12));
}

#[test]
fn pointer_cross_function_alias() {
    // 指针跨函数传参：被调函数写穿调用方局部（共享堆 cell）
    let src = r#"
fn bump(p: *mut i32) void {
    p.* += 1;
}
fn f() i32 {
    var mut x: i32 = 41;
    bump(&mut x);
    return x;
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(42));
}

#[test]
fn pointer_addr_value_snapshot() {
    // &(非 lvalue) → AddrValue 快照：写穿不回流到原表达式（对齐 AddrOf 兜底分支）
    let src = "fn f() i32 { var mut x: i32 = 5; var p = &(x + 1); p.* = 10; return x; }";
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(5));
}

#[test]
fn pointer_eq_identity_and_deref() {
    // 同目标指针身份相等；指针与值比较时解引用（对齐 value_eq）
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
    assert_eq!(run(src, "same", &[]).unwrap(), IrValue::Bool(true));
    assert_eq!(run(src, "deref_eq", &[]).unwrap(), IrValue::Bool(true));
}

#[test]
fn out_of_slice_constructs_are_hard_errors() {
    // P0 回归：子集外特性必须返回 Unsupported 硬错误，而非静默丢弃（此前会 void 占位/丢语句）。
    // Phase 3 已将 for/switch 纳入 IR 支持面（见下方正例测试）；此处仅保留仍未实现者。
    let cases: &[&str] = &[
        // 全局常量声明（Phase 5 程序启动初始化）
        "const X: i32 = 1;\nfn f() i32 { return X; }",
        // 闭包（Phase 4）
        "fn f() i32 { var x = |v| v + 1; return 0; }",
    ];
    for src in cases {
        let program = parse_source(src).unwrap_or_else(|d| panic!("parse failed ({src}): {d:?}"));
        let e = lower(&program).expect_err("预期降级失败，src 应属子集外特性");
        assert_eq!(
            e.name, "Unsupported",
            "预期 Unsupported 硬错误，src: {src}"
        );
    }
}

#[test]
fn for_range_loop_lowers_and_runs() {
    // Phase 3：`for (lo..hi)` 区间糖 → MakeRange + IterMake/IterNext；只读捕获。
    let src = r#"
fn f() i32 {
    var mut s: i32 = 0;
    for (0..4) |i| { s += i; }
    return s;
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(6));
}

#[test]
fn for_arr_read_only_loop() {
    // Phase 3：数组迭代只读捕获（元素值拷贝，不写回）。
    let src = r#"
fn f() i32 {
    var a = [10, 20, 30];
    var mut s: i32 = 0;
    for (a) |x| { s += x; }
    return s;
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(60));
}

#[test]
fn for_arr_mut_writeback() {
    // Phase 3：`for (arr) |mut x|` 写回——每项自增后源数组同步（与 oracle 一致）。
    let src = r#"
fn f() i32 {
    var a = [1, 2, 3];
    for (a) |mut x| { x += 1; }
    return a[0] + a[1] + a[2];
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(9));
}

#[test]
fn switch_first_match_semantics() {
    // Phase 3：switch 线性链 first-match；无匹配且无 else → 0。
    let src = r#"
fn f(x: i32) i32 {
    switch (x) {
        1 => return 10,
        2 => return 20,
        else => return 99,
    }
}
"#;
    assert_eq!(run(src, "f", &[IrValue::Int(1)]).unwrap(), IrValue::Int(10));
    assert_eq!(run(src, "f", &[IrValue::Int(2)]).unwrap(), IrValue::Int(20));
    assert_eq!(run(src, "f", &[IrValue::Int(9)]).unwrap(), IrValue::Int(99));
}

#[test]
fn switch_no_else_falls_to_void() {
    // 无匹配 + 无 else → 语句降级为 Void（对齐 oracle Flow::None）。
    let src = r#"
fn f(x: i32) i32 {
    var mut y: i32 = 0;
    switch (x) { 1 => { y = 5; } }
    return y;
}
"#;
    assert_eq!(run(src, "f", &[IrValue::Int(1)]).unwrap(), IrValue::Int(5));
    assert_eq!(run(src, "f", &[IrValue::Int(7)]).unwrap(), IrValue::Int(0));
}

#[test]
fn switch_enum_and_string_patterns() {
    // 枚举变体（Ident 模式）与字符串字面量模式匹配。
    let src = r#"
enum Color { red, green, blue }
fn name(c: Color) i32 {
    switch (c) {
        Color.red => return 1,
        Color.green => return 2,
        else => return 3,
    }
}
fn pick(s: String) i32 {
    switch (s) {
        "a" => return 1,
        "b" => return 2,
        else => return 0,
    }
}
"#;
    assert_eq!(run(src, "name", &[IrValue::Enum { name: "Color".into(), variant: "red".into(), payload: None }]).unwrap(), IrValue::Int(1));
    assert_eq!(run(src, "name", &[IrValue::Enum { name: "Color".into(), variant: "green".into(), payload: None }]).unwrap(), IrValue::Int(2));
    assert_eq!(run(src, "name", &[IrValue::Enum { name: "Color".into(), variant: "blue".into(), payload: None }]).unwrap(), IrValue::Int(3));
    assert_eq!(run(src, "pick", &[IrValue::Str("a".into())]).unwrap(), IrValue::Int(1));
    assert_eq!(run(src, "pick", &[IrValue::Str("z".into())]).unwrap(), IrValue::Int(0));
}
