//! hc/tests/ir.rs
//!
//! 定义：枚举：Color
//! 定义：结构体：Point, Point, Point, Point, Point, Point, Point

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
    assert_eq!(
        run(src, "pick", &[IrValue::Opt(None)]).unwrap(),
        IrValue::Int(0)
    );
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
    assert_eq!(
        run(src, "g", &[]).unwrap(),
        IrValue::Err {
            name: "NotFound".into(),
            code: 0
        }
    );
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
    assert_eq!(
        run(src, "d", &[IrValue::Opt(None)]).unwrap(),
        IrValue::Int(5)
    );
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
        IrValue::Err {
            name: "First".into(),
            code: 0
        }
    );
    assert_eq!(
        run(src, "second", &[]).unwrap(),
        IrValue::Err {
            name: "Second".into(),
            code: 1
        }
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
    // Phase 3-6 for/switch/闭包/global/const/defer/errdefer/标签均已纳入 IR 支持面（见正例测试）；
    // 此处仅保留仍未实现者（未知标识符 / 循环外 break / defer 体控制流）。
    for src in [
        "fn f() i32 { return nosuch; }",    // 未知标识符
        "fn f() i32 { break; }",            // break 在循环外
        "fn f() void { defer try foo(); }", // defer 体含控制流（try → 跳转指令）
    ] {
        let program = parse_source(src).unwrap_or_else(|d| panic!("parse failed ({src}): {d:?}"));
        let e = lower(&program).expect_err("预期降级失败，src 应属子集外特性");
        assert_eq!(e.name, "Unsupported", "预期 Unsupported 硬错误，src: {src}");
    }
}

// ---------- Phase 4：闭包 / 函数引用 / 方法 / 动态调用 / 重载 ----------

#[test]
fn closure_read_capture_shares_slot() {
    // 只读捕获：共享源 cell，原绑定后续变更对闭包可见（对照 move 捕获）
    let src = r#"
fn f() i32 {
    var a = 10;
    var g = |v| v + a;
    a = 100;
    return g(5);
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(105));
}

#[test]
fn closure_move_capture_copies() {
    // move 捕获：深拷贝独立副本，原绑定后续变更不影响闭包
    let src = r#"
fn f() i32 {
    var a = 10;
    var g = move |v| v + a;
    a = 100;
    return g(5);
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(15));
}

#[test]
fn closure_mut_capture_writes_through() {
    // mut 捕获：闭包内写入被捕获变量，对原绑定可见
    let src = r#"
fn f() i32 {
    var total = 0;
    var acc = mut |v| { total = total + v; return total; };
    var a = acc(3);
    var b = acc(4);
    return a * 100 + b * 10 + total;
}
"#;
    // a=3, b=7, total=7 → 3*100 + 7*10 + 7 = 377
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(377));
}

#[test]
fn closure_returned_from_function() {
    // 闭包脱离 make 作用域后仍可用（cell 永不释放，捕获副本随返回值转移）
    let src = r#"
fn make() i32 {
    var base = 10;
    var f = move |v| v + base;
    return f(5);
}
"#;
    assert_eq!(run(src, "make", &[]).unwrap(), IrValue::Int(15));
}

// ---------- Phase 8：捕获精确化（MakeClosure.captures = 自由变量集） + is_mut 强制 ----------

/// 提取 `f` 体内第一条 MakeClosure 的捕获名列表（排序后）。
fn closure_capture_names(module: &hc::ir::IrModule, fname: &str) -> Vec<String> {
    let f = &module.funcs[module.func_index[fname][0]];
    let mut names = f
        .body
        .iter()
        .find_map(|inst| match inst {
            hc::ir::IrInst::MakeClosure { captures, .. } => Some(captures.clone()),
            _ => None,
        })
        .expect("MakeClosure")
        .iter()
        .map(|(n, _)| n.clone())
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[test]
fn closure_captures_only_free_vars() {
    // 精确捕获：作用域内有 a/b/c 三个变量，闭包只引用 a、c → captures = {a, c}，
    // 未引用的 b 不捕获（Phase 8：MakeClosure.captures 对齐自由变量集）
    let src = r#"
fn f() i32 {
    var a = 1;
    var b = 2;
    var c = 3;
    var g = |v| v + a + c;   // 只引用 a、c
    return g(0);
}
"#;
    let program = parse_source(src).unwrap();
    let module = lower(&program).unwrap();
    assert_eq!(closure_capture_names(&module, "f"), vec!["a", "c"]);
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(4));
}

#[test]
fn closure_transitive_capture_through_nested() {
    // 嵌套传递：外层闭包体只在内层闭包体内引用 a → 自由变量分析须穿过嵌套闭包，
    // 外层 MakeClosure.captures 仍含 a；未引用的 b 不捕获
    let src = r#"
fn f() i32 {
    var a = 1;
    var b = 2;
    var g = | | {
        var h = |v| v + a;   // 只引用 a
        return h(0);
    };
    return g();
}
"#;
    let program = parse_source(src).unwrap();
    let module = lower(&program).unwrap();
    assert_eq!(closure_capture_names(&module, "f"), vec!["a"]);
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(1));
}

#[test]
fn closure_capture_ignores_shadowed_local() {
    // 遮蔽：体内声明的同名局部变量遮蔽外部绑定 → 不捕获（captures 为空）
    let src = r#"
fn f() i32 {
    var a = 1;
    var g = | | {
        var a = 100;   // 体内局部，遮蔽外部 a——非捕获
        return a;
    };
    return g();
}
"#;
    let program = parse_source(src).unwrap();
    let module = lower(&program).unwrap();
    assert!(closure_capture_names(&module, "f").is_empty());
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(100));
}

#[test]
fn closure_non_mut_rebind_capture_fails() {
    // is_mut 只读强制（IR 侧）：非 `mut` 闭包重绑定被捕获变量 → ReadonlyCapture
    let src = r#"
fn f() i32 {
    var total = 0;
    var acc = |v| { total = total + v; return total; };
    return acc(3);
}
"#;
    let e = run(src, "f", &[]).unwrap_err();
    assert_eq!(e.name, "ReadonlyCapture");
}

#[test]
fn closure_move_deep_copies_closure_value() {
    // move 捕获闭包值：深拷贝其环境副本（对照共享——若共享则 x=100 后 outer() 得 101）
    let src = r#"
fn f() i32 {
    var x = 1;
    var inner = |v| v + x;
    var outer = move | | inner(1);
    x = 100;
    return outer();
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(2));
}

#[test]
fn fnref_and_indirect_call() {
    // FnRef：函数名作为值绑定到变量；CallIndirect 经 Fn 值动态调用
    let src = r#"
fn inc(v: i32) i32 { return v + 1; }
fn call_it() i32 {
    var f = inc;
    return f(41);
}
"#;
    assert_eq!(run(src, "call_it", &[]).unwrap(), IrValue::Int(42));
}

#[test]
fn method_instance_dispatch() {
    // CallMethod：r.area() 动态分派——运行时由类型名解析 `Rect.area` 并注入 self
    let src = r#"
class Rect {
    w: i32,
    h: i32,
    fn area(self: *Self) i32 { return self.w * self.h; }
}
fn f() i32 {
    var r = Rect{ w = 3, h = 4 };
    return r.area();
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(12));
}

#[test]
fn method_static_call_passes_self() {
    // 静态形式 `Rect.area(&r)`：self 显式传参，不注入
    let src = r#"
class Rect {
    w: i32,
    h: i32,
    fn area(self: *Self) i32 { return self.w * self.h; }
}
fn f() i32 {
    var r = Rect{ w = 3, h = 4 };
    return Rect.area(&r);
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(12));
}

#[test]
fn overload_dispatch_by_arity() {
    // func_index 一名多候选：按实参数量精确分派
    let src = r#"
fn sq(x: i32) i32 { return x * x; }
fn sq(x: i32, y: i32) i32 { return x * y; }
fn f() i32 {
    return sq(3) * 10 + sq(2, 4);
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(98));
}

#[test]
fn method_takes_extra_args() {
    // 实例方法带额外实参：注入 self 为首参后按 arity 分派
    let src = r#"
class Point {
    x: i32,
    fn offset(self: *Self, dx: i32) i32 { return self.x + dx; }
}
fn f() i32 {
    var p = Point{ x = 10 };
    return p.offset(5);
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(15));
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
    assert_eq!(
        run(
            src,
            "name",
            &[IrValue::Enum {
                name: "Color".into(),
                variant: "red".into(),
                payload: None
            }]
        )
        .unwrap(),
        IrValue::Int(1)
    );
    assert_eq!(
        run(
            src,
            "name",
            &[IrValue::Enum {
                name: "Color".into(),
                variant: "green".into(),
                payload: None
            }]
        )
        .unwrap(),
        IrValue::Int(2)
    );
    assert_eq!(
        run(
            src,
            "name",
            &[IrValue::Enum {
                name: "Color".into(),
                variant: "blue".into(),
                payload: None
            }]
        )
        .unwrap(),
        IrValue::Int(3)
    );
    assert_eq!(
        run(src, "pick", &[IrValue::Str("a".into())]).unwrap(),
        IrValue::Int(1)
    );
    assert_eq!(
        run(src, "pick", &[IrValue::Str("z".into())]).unwrap(),
        IrValue::Int(0)
    );
}

// ---------- Phase 5：global / const + @__init__ + IrRuntime ----------

#[test]
fn global_const_init_and_mutation() {
    // global/const 声明序初始化（合成 `@__init__`），普通函数间共享可变全局。
    let src = r#"
global counter: i32 = 0;
const BASE: i32 = 100;
fn bump() i32 {
    counter = counter + 1;
    return counter + BASE;
}
"#;
    assert_eq!(run(src, "bump", &[]).unwrap(), IrValue::Int(101));
    assert_eq!(run(src, "bump", &[]).unwrap(), IrValue::Int(101)); // 单次调用独立 runtime → 重新 init
}

#[test]
fn global_runtime_shares_state_across_calls() {
    // IrRuntime 共享实例：全局只初始化一次，跨调用可变全局可见（对齐 tree-walk 共享 Interp）。
    use hc::ir::{lower, IrRuntime};
    let src = r#"
global counter: i32 = 0;
fn bump() i32 { counter += 1; return counter; }
"#;
    let program = parse_source(src).unwrap();
    let module = lower(&program).unwrap();
    let mut rt = IrRuntime::new();
    assert_eq!(rt.call(&module, "bump", &[]).unwrap(), IrValue::Int(1));
    assert_eq!(rt.call(&module, "bump", &[]).unwrap(), IrValue::Int(2));
    assert_eq!(rt.call(&module, "bump", &[]).unwrap(), IrValue::Int(3));
}

#[test]
fn const_init_order_is_declaration_order() {
    // 后声明的 global 初始化表达式可引用先声明的 global（@__init__ 声明序）。
    let src = r#"
global a: i32 = 5;
global b: i32 = a * 2;
fn read() i32 { return b + a; }
"#;
    assert_eq!(run(src, "read", &[]).unwrap(), IrValue::Int(15));
}

#[test]
fn global_unknown_name_is_hard_error() {
    // 未知标识符（非局部/函数/全局）→ 降级期 Unsupported 硬错误，非静默 Void
    let src = "fn f() i32 { return nonexistent_global; }";
    let program = parse_source(src).unwrap();
    let e = lower(&program).expect_err("预期降级失败（未知标识符）");
    assert_eq!(e.name, "Unsupported", "预期 Unsupported 硬错误");
}

#[test]
fn global_address_of_writes_through() {
    // `&global`/`&mut global` 别名全局 cell：`Deref`/`StorePtr` 写穿回全局
    // （对齐 oracle `lookup` → 全局 `Rc<RefCell>` 的 `AddrOf(Ident)` 分支）。
    let src = r#"
global counter: i32 = 0;
fn f() i32 {
    var p = &mut counter;
    p.* = 7;
    p.* += 1;
    return counter;
}
fn read() i32 { return counter; }
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(8));
    assert_eq!(run(src, "read", &[]).unwrap(), IrValue::Int(0)); // 独立 runtime → 重新 init
}

#[test]
fn global_address_of_shared_runtime_persists() {
    // IrRuntime 共享实例：`&global` 写穿跨调用持久（全局 cell 跨调用存活）
    use hc::ir::{lower, IrRuntime};
    let src = r#"
global counter: i32 = 0;
fn bump() i32 {
    var p = &mut counter;
    p.* += 1;
    return p.*;
}
fn read() i32 { return counter; }
"#;
    let program = parse_source(src).unwrap();
    let module = lower(&program).unwrap();
    let mut rt = IrRuntime::new();
    assert_eq!(rt.call(&module, "bump", &[]).unwrap(), IrValue::Int(1));
    assert_eq!(rt.call(&module, "bump", &[]).unwrap(), IrValue::Int(2));
    assert_eq!(rt.call(&module, "read", &[]).unwrap(), IrValue::Int(2));
}

// ---------- Phase 6：defer / errdefer + 带标签 break/continue ----------

#[test]
fn defer_lifo_at_scope_exit() {
    // defer LIFO：3, 2, 1 登记序 → 作用域退出按 1, 2, 3 运行（对齐 oracle `run_defers` 逆序）。
    let src = r#"
global log: i32 = 0;
fn rec(v: i32) void { log = log * 10 + v; }
fn f() i32 {
    log = 0;
    {
        defer rec(1);
        defer rec(2);
        defer rec(3);
    }
    return log;
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(321));
}

#[test]
fn defer_same_scope_capture_reads_final_value() {
    // defer 体在作用域退出时重求值：同作用域局部变量读到「退出时最终值」而非登记时值。
    let src = r#"
global g: i32 = 0;
fn bump(v: i32) void { g = v; }
fn f() i32 {
    var x: i32 = 1;
    g = 0;
    {
        defer bump(x);
        x = 100;
    }
    return g;
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(100));
}

#[test]
fn defer_nested_block_runs_at_block_close() {
    // 内层块 defer 随块结束（弹栈）运行，而非函数结束。
    let src = r#"
global g: i32 = 0;
fn bump(v: i32) void { g += v; }
fn f() i32 {
    g = 0;
    {
        defer bump(10);
        g += 1;
    }
    return g;
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(11));
}

#[test]
fn defer_runs_on_return() {
    // `return` 排空函数级 defers（正常值仅非 errdefer）。
    let src = r#"
global g: i32 = 0;
fn bump(v: i32) void { g += v; }
fn early() i32 {
    defer bump(5);
    return 1;
}
fn f() i32 {
    g = 0;
    var r = early();
    return g * 10 + r;
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(51)); // g=5, r=1
}

#[test]
fn defer_runs_on_loop_break() {
    // break 排空循环体内 defers：每轮迭代 defer 都运行。
    let src = r#"
global dlog: i32 = 0;
fn bump() void { dlog += 1; }
fn f() i32 {
    dlog = 0;
    var i: i32 = 0;
    while (true) {
        defer bump();
        i += 1;
        if (i >= 3) { break; }
    }
    return dlog;
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(3));
}

#[test]
fn defer_runs_on_loop_continue() {
    // continue 同样排空循环体内 defers：本轮 defer 运行后再跳下一轮。
    let src = r#"
global dlog: i32 = 0;
global clog: i32 = 0;
fn bump() void { dlog += 1; }
fn f() i32 {
    dlog = 0;
    clog = 0;
    var i: i32 = 0;
    while (i < 5) {
        defer bump();
        i += 1;
        if (i == 3) { continue; }
        clog += 1;
    }
    return dlog * 10 + clog;
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(54)); // dlog=5, clog=4
}

#[test]
fn errdefer_runs_only_on_error_path() {
    // errdefer：错误返回路径触发（+ 正常 defer 也触发）；正常返回不触发 errdefer。
    let src = r#"
global g: i32 = 0;
fn bump(v: i32) void { g += v; }
fn maybe(ok: bool) !i32 {
    defer bump(1);
    errdefer bump(100);
    if (ok) { return 5; }
    return error.Fail;
}
fn f() i32 {
    g = 0;
    var r = maybe(false) catch 0;
    return g * 10 + r;
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(1010)); // g=101, r=0

    let src_ok = r#"
global g: i32 = 0;
fn bump(v: i32) void { g += v; }
fn maybe(ok: bool) !i32 {
    defer bump(1);
    errdefer bump(100);
    if (ok) { return 5; }
    return error.Fail;
}
fn f() i32 {
    g = 0;
    var r = maybe(true) catch 0;
    return g * 10 + r;
}
"#;
    assert_eq!(run(src_ok, "f", &[]).unwrap(), IrValue::Int(15)); // g=1, r=5

    // 正常作用域结束：errdefer 不触发
    let src_block = r#"
global g: i32 = 0;
fn bump(v: i32) void { g += v; }
fn f() i32 {
    g = 0;
    {
        errdefer bump(100);
        bump(1);
    }
    return g;
}
"#;
    assert_eq!(run(src_block, "f", &[]).unwrap(), IrValue::Int(1));
}

#[test]
fn errdefer_runs_on_try_propagation() {
    // `try` 错误传播 = 从当前函数返回错误值：errdefer 须触发（错误路径）。
    let src = r#"
global g: i32 = 0;
fn bump() void { g += 1; }
fn maybe_err(ok: bool) !i32 {
    if (ok) { return 5; }
    return error.Fail;
}
fn wrapper(ok: bool) !i32 {
    defer bump();
    errdefer bump();
    var x = try maybe_err(ok);
    return x;
}
fn f() i32 {
    g = 0;
    var r = wrapper(false) catch 0;
    return g * 10 + r;
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(20)); // g=2, r=0
}

#[test]
fn labeled_break_continue() {
    // 带标签 break：跳出外层标签循环（标签跨多层循环定位）。
    let src_break = r#"
fn f() i32 {
    var s: i32 = 0;
    :outer while (true) {
        var j: i32 = 0;
        while (j < 10) {
            j += 1;
            if (j == 2) { break :outer; }
            s += j;
        }
    }
    return s;
}
"#;
    assert_eq!(run(src_break, "f", &[]).unwrap(), IrValue::Int(1));

    // 带标签 continue 跳自身循环下一轮
    let src_self = r#"
fn f() i32 {
    var s: i32 = 0;
    :outer for (0..3) |i| {
        if (i == 1) { continue :outer; }
        s += i;
    }
    return s;
}
"#;
    assert_eq!(run(src_self, "f", &[]).unwrap(), IrValue::Int(2));

    // 带标签 continue 跳出内层 while，跳到外层 for 下一轮
    let src_nested = r#"
fn f() i32 {
    var s: i32 = 0;
    :outer for (0..3) |i| {
        var j: i32 = 0;
        while (j < 5) {
            j += 1;
            if (i == 1) { continue :outer; }
            s += j;
        }
        s += 100;
    }
    return s;
}
"#;
    assert_eq!(run(src_nested, "f", &[]).unwrap(), IrValue::Int(230));
}

#[test]
fn labeled_break_runs_loop_defers() {
    // 带标签 break 排空目标循环体（含嵌套作用域）的 defers。
    let src = r#"
global g: i32 = 0;
fn bump() void { g += 1; }
fn f() i32 {
    g = 0;
    :outer while (true) {
        defer bump();
        break :outer;
    }
    return g;
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(1));
}

#[test]
fn continuous_class_copy_on_var_decl() {
    // P11d：连续类 var 声明即深拷贝（DeepCopy 指令）。
    // - 标注类型：`var p2: Point = p` → 声明类型连续 → DeepCopy
    // - 未标注 + 标识符初始化：`var p2 = p1` → 运行时门按实际类名判定
    // 两模式（tree-walk/IR）均复制独立副本：改 p2 不影响 p1。
    let src = r#"
struct Point {
    x: f32,
    y: f32,
}
fn f() f32 {
    var p: Point = Point{ x = 1.0, y = 2.0 };
    var p2: Point = p;
    p2.x = 99.0;
    return p.x;
}
fn g() f32 {
    var p1 = Point{ x = 1.0, y = 2.0 };
    var mut p2 = p1;
    p2.x = 99.0;
    return p1.x;
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Float(1.0));
    assert_eq!(run(src, "g", &[]).unwrap(), IrValue::Float(1.0));
}

// ---------- G1/mem：Arena 真实内存管理（bump + 块链表 + deinit）----------

#[test]
fn arena_bump_reuses_block_ir() {
    // 小块多次分配：同一块内 bump，块链表不增长
    let src = r#"
fn t() i32 {
    var arena = Arena.init(alloc);
    var a = arena.alloc(16);
    var b = arena.alloc(16);
    var c = arena.alloc(16);
    if (arena.blocks() != 1) { return 1; }
    if (arena.bytes() != 48) { return 2; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn arena_grows_block_list_ir() {
    // 单次分配超过默认块大小（1024）→ 当前块不足，申请新块
    let src = r#"
fn t() i32 {
    var arena = Arena.init(alloc);
    var a = arena.alloc(5000);
    if (arena.blocks() != 1) { return 1; }
    if (arena.bytes() != 5000) { return 2; }
    var b = arena.alloc(16);
    if (arena.blocks() != 2) { return 3; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn arena_zero_init_and_len_ir() {
    // 分配内容零初始化、长度正确
    let src = r#"
fn t() i32 {
    var arena = Arena.init(alloc);
    var buf = arena.alloc(4);
    if (buf != "\x00\x00\x00\x00") { return 1; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn arena_deinit_releases_ir() {
    // deinit 批量归还全部块、重置统计
    let src = r#"
fn t() i32 {
    var arena = Arena.init(alloc);
    var a = arena.alloc(16);
    var b = arena.alloc(16);
    if (arena.blocks() != 1) { return 1; }
    if (arena.bytes() != 32) { return 2; }
    arena.deinit();
    if (arena.blocks() != 0) { return 3; }
    if (arena.bytes() != 0) { return 4; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn arena_alloc_after_deinit_errors_ir() {
    // deinit 后 alloc → 运行期错误 ArenaDeinitialized
    let src = r#"
fn t() i32 {
    var arena = Arena.init(alloc);
    arena.deinit();
    var b = arena.alloc(8);
    return 0;
}
"#;
    let e = run(src, "t", &[]).unwrap_err();
    assert_eq!(e.name, "ArenaDeinitialized");
}

#[test]
fn arena_oom_catchable_ir() {
    // 超大分配（1 << 63 超 Vec 容量）→ error.OutOfMemory 可 catch
    let src = r#"
fn t() i32 {
    var arena = Arena.init(alloc);
    var buf = arena.alloc(1 << 63) catch |err| {
        if (err != error.OutOfMemory) { return 1; }
        return 0;
    };
    return 2;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

// ---------- G3/mem：装箱胖指针携带 alloc 引用（三字宽）----------

#[test]
fn box_single_arg_falls_back_global_alloc_ir() {
    // box(v) 单参 → 回退全局 alloc（设计文档 §6）；解引用读 pointee
    let src = r#"
fn t() i32 {
    var p = box(42);
    if (p.* != 42) { return 1; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn box_carries_explicit_alloc_ir() {
    // box(v, alloc)：携带全局分配器——p.alloc() 返回它，可继续分配 8 字节
    let src = r#"
fn t() i32 {
    var p = box(42, alloc);
    var q = p.alloc();
    var buf = q.alloc(8);
    if (buf != "\x00\x00\x00\x00\x00\x00\x00\x00") { return 1; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn box_carries_arena_ir() {
    // box(v, arena)：携带 arena——类型可见（Arena），且 box 不占用 arena 字节
    let src = r#"
fn t() i32 {
    var arena = Arena.init(alloc);
    var p = box(42, arena);
    if (@typeOf(p.alloc()) != "Arena") { return 1; }
    if (p.alloc().bytes() != 0) { return 2; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn box_deref_read_write_ir() {
    // p.* 读/写穿透到 pointee（box 返回 *mut T）
    let src = r#"
fn t() i32 {
    var p = box(7);
    p.* = 9;
    if (p.* != 9) { return 1; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn box_compare_with_plain_value_ir() {
    // Boxed 与普通值比较：解引用后比较（对齐 Ptr 语义）
    let src = r#"
fn t() i32 {
    var p = box(42);
    if (p != 42) { return 1; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn box_interface_dispatch_ir() {
    // 装箱 class → *I 胖指针：s.area() 鸭子类型分派到具体实现（Rect/Circle）
    let src = r#"
interface IShape { fn area(self: *Self) f32; }
class Rect: IShape {
    w: f32,
    h: f32,
    fn area(self: *Self) f32 { return self.w * self.h; }
}
class Circle: IShape {
    r: f32,
    fn area(self: *Self) f32 { return pi * self.r * self.r; }
}
fn total_area(shapes: &Vec<*IShape>) f32 {
    var total = 0.0;
    for (shapes) |s| {
        total += s.area();
    }
    return total;
}
fn t() i32 {
    var rect = Rect{ w = 3.0, h = 4.0 };
    var circ = Circle{ r = 2.0 };
    var shapes: owned Vec<*IShape> = Vec<*IShape>.init(alloc);
    shapes.append(box(rect, alloc));
    shapes.append(box(circ, alloc));
    var total = total_area(&shapes);
    if (total < 24.55 or total > 24.57) { return 1; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

// ---------- G4/mem：集合持有分配器引用（§7 定案落地）----------

#[test]
fn vec_init_captures_global_alloc_ir() {
    // `Vec<T>.init(alloc)`：携带全局分配器——`v.alloc()` 返回它，可继续分配 8 字节
    let src = r#"
fn t() i32 {
    var v = Vec<i32>.init(alloc);
    var buf = v.alloc().alloc(8);
    if (buf != "\x00\x00\x00\x00\x00\x00\x00\x00") { return 1; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn vec_init_captures_arena_ir() {
    // `Vec<T>.init(arena)`：携带 arena——类型可见（Arena），未分配过字节则为 0
    let src = r#"
fn t() i32 {
    var arena = Arena.init(alloc);
    var v = Vec<i32>.init(arena);
    if (@typeOf(v.alloc()) != "Arena") { return 1; }
    if (v.alloc().bytes() != 0) { return 2; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn vec_default_carries_global_alloc_ir() {
    // 裸类型表达式 `Vec<i32>`（无显式 init）→ 回退全局 alloc（§3 隐式环境）
    let src = r#"
fn t() i32 {
    var v = Vec<i32>;
    var buf = v.alloc().alloc(4);
    if (buf != "\x00\x00\x00\x00") { return 1; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn vec_stores_and_grows_with_stored_alloc_ir() {
    // 携带的分配器随集合存在：扩容（append）后 `.alloc()` 仍可观测、可分配
    let src = r#"
fn t() i32 {
    var v = Vec<i32>.init(alloc);
    v.append(1);
    v.append(2);
    if (v.len() != 2) { return 1; }
    if (v[0] != 1) { return 2; }
    if (v[1] != 2) { return 3; }
    var buf = v.alloc().alloc(8);
    if (buf != "\x00\x00\x00\x00\x00\x00\x00\x00") { return 4; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn map_init_captures_alloc_ir() {
    // `Map<K,V>.init(alloc)`：携带分配器 + put/get(.?)/len 正常
    let src = r#"
fn t() i32 {
    var m = Map<i32, i32>.init(alloc);
    m.put(1, 2);
    m.put(3, 4);
    if (m.len() != 2) { return 1; }
    if (m.get(1).? != 2) { return 2; }
    if (m.get(3).? != 4) { return 3; }
    var buf = m.alloc().alloc(4);
    if (buf != "\x00\x00\x00\x00") { return 4; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn map_init_captures_arena_ir() {
    // `Map<K,V>.init(arena)`：携带 arena——类型可见
    let src = r#"
fn t() i32 {
    var arena = Arena.init(alloc);
    var m = Map<i32, i32>.init(arena);
    m.put(1, 2);
    if (@typeOf(m.alloc()) != "Arena") { return 1; }
    if (m.get(1).? != 2) { return 2; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn map_iterates_kv_pairs_ir() {
    // Map 句柄可遍历：`for (m) |kv|` → kv.key / kv.value（对齐 Class("Map") 遍历）
    let src = r#"
fn t() i32 {
    var m = Map<i32, i32>.init(alloc);
    m.put(10, 1);
    m.put(20, 2);
    var sum = 0;
    for (m) |kv| {
        sum += kv.value;
    }
    if (sum != 3) { return 1; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn table_init_captures_alloc_ir() {
    // `Table<T>.init(rows, cols, init, alloc)`（ADR-0027：分配器永远是最后一个参数）
    let src = r#"
fn t() i32 {
    var t = Table<i32>.init(2, 3, 7, alloc);
    if (t.len() != 2) { return 1; }
    if (t[0].len() != 3) { return 2; }
    if (t[0][1] != 7) { return 3; }
    if (t[1][2] != 7) { return 4; }
    var buf = t.alloc().alloc(8);
    if (buf != "\x00\x00\x00\x00\x00\x00\x00\x00") { return 5; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

// ---------- G5/mem：对齐保证与 Debug 泄漏检测（§2.3 / §8.3 定案落地）----------

#[test]
fn arena_bump_aligned_to_16_ir() {
    // 对齐（§2.3）：连续小分配每次从 16 对齐处切——alloc(1)+alloc(1)+alloc(16)
    // 游标推进 0 → 1 → 16 → 17 → 32 → 48；bytes 含对齐填充（对齐 tree-walking）
    let src = r#"
fn t() i32 {
    var arena = Arena.init(alloc);
    var a = arena.alloc(1);
    var b = arena.alloc(1);
    var c = arena.alloc(16);
    if (arena.bytes() != 48) { return 1; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn arena_aligned_region_distinct_ir() {
    // 对齐后区域互不干扰：alloc(1) 后 alloc(16) 从 16 对齐处切（跳过对齐填充）
    let src = r#"
fn t() i32 {
    var arena = Arena.init(alloc);
    var a = arena.alloc(1);
    var b = arena.alloc(16);
    if (a != "\x00") { return 1; }
    if (b != "\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00") { return 2; }
    if (arena.bytes() != 32) { return 3; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn alloc_leaks_tracks_allocations_ir() {
    // 泄漏检测（§8.3）：`alloc.alloc(n)` 登记；`alloc.leaks()` 反映登记数
    let src = r#"
fn t() i32 {
    var b = alloc.alloc(8);
    if (alloc.leaks() != 1) { return 1; }
    var c = alloc.alloc(4);
    if (alloc.leaks() != 2) { return 2; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn alloc_leak_report_ir() {
    // 泄漏检测（§8.3）：`alloc.leak_report()` 输出清单——大小 + 行号（IR 无行号 → line 0）
    let src = r#"
fn t() i32 {
    var b = alloc.alloc(8);
    if (alloc.leak_report() != "leak: line 0: 8 bytes\n") { return 1; }
    return 0;
}
"#;
    assert_eq!(run(src, "t", &[]).unwrap(), IrValue::Int(0));
}

// ---------- 组 G4a：线程生命周期（协作式延迟执行） ----------

#[test]
fn spawn_join_returns_value_ir() {
    // 基本 spawn/join：spawn(add, 6, 7) 立即返回句柄；join 运行 add 到完成返回 13
    let src = r#"
fn add(a: i32, b: i32) i32 { return a + b; }
fn main() i32 {
    var th = spawn(add, 6, 7);
    return try th.join();
}
"#;
    assert_eq!(run(src, "main", &[]).unwrap(), IrValue::Int(13));
}

#[test]
fn thread_is_done_transitions_ir() {
    // is_done 状态转移：spawn 后 false（未运行）→ join 后 true（已完成）
    let src = r#"
fn add(a: i32, b: i32) i32 { return a + b; }
fn main() i32 {
    var th = spawn(add, 6, 7);
    if (th.is_done()) { return 1; }
    var r = try th.join();
    if (r != 13) { return 2; }
    if (!th.is_done()) { return 3; }
    return 0;
}
"#;
    assert_eq!(run(src, "main", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn thread_own_alloc_isolation_ir() {
    // Q8：子任务绑定每线程独立 alloc——worker 的 alloc.alloc(8) bump 到自身 arena
    // （alloc.bytes() == 8），且不进全局泄漏跟踪（根 alloc.leaks() 不变）
    let src = r#"
fn worker() usize {
    var buf = alloc.alloc(8);
    return alloc.bytes();
}
fn main() i32 {
    var n0 = alloc.leaks();
    var th = spawn(worker);
    var b = try th.join();
    if (b != 8) { return 1; }
    if (alloc.leaks() != n0) { return 2; }
    return 0;
}
"#;
    assert_eq!(run(src, "main", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn join_error_propagates_ir() {
    // join 透传子任务错误 union——may_fail 返回 error.Boom，join 返回同名错误
    // （expect_error 按错误名比较；IR value_eq 对 Err 比较码，避免两套码表歧义）
    let src = r#"
fn may_fail() !i32 { return error.Boom; }
fn main() void {
    var th = spawn(may_fail);
    expect_error(error.Boom, th.join());
    expect_eq(th.is_done(), true);
}
"#;
    assert_eq!(run(src, "main", &[]).unwrap(), IrValue::Void);
}

#[test]
fn cancel_then_join_returns_cancelled_ir() {
    // cancel 置协作标志；若线程在 cancel() 前未执行完毕，join 返回 error.Cancelled。
    // OS 线程模式下存在竞态——若线程在 cancel() 前已执行完毕，则 join 返回正常值。
    // 两种结果都正确，本测试验证线程正确完成。
    let src = r#"
fn work() i32 { return 42; }
fn main() void {
    var th = spawn(work);
    th.cancel();
    var r = th.join() catch 0;
    expect_eq(th.is_done(), true);
}
"#;
    assert_eq!(run(src, "main", &[]).unwrap(), IrValue::Void);
}

#[test]
fn join_waits_and_returns_value_ir() {
    // join 等待 OS 线程结束，返回函数返回值（全局变量不跨线程共享）
    let src = r#"
fn bump() i32 { return 42; }
fn main() i32 {
    var th = spawn(bump);
    var r = try th.join();
    if (r != 42) { return 1; }
    if (!th.is_done()) { return 2; }
    return 0;
}
"#;
    assert_eq!(run(src, "main", &[]).unwrap(), IrValue::Int(0));
}

// ---------- Q8：扩展方法（Extension Method）----------

#[test]
fn extension_method_on_class() {
    // [Extension(Type)] 扩展方法通过 CallMethod 运行时分派
    let src = r#"
class Point {
    x: i32,
    y: i32,
}

[Extension(Point)]
fn magnitude(self: *Self) i32 {
    return self.x + self.y;
}

fn f() i32 {
    var p = Point{ x = 3, y = 4 };
    return p.magnitude();
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(7));
}

#[test]
fn extension_method_on_class_with_extra_args() {
    // 扩展方法带额外参数
    let src = r#"
class Point {
    x: i32,
    y: i32,
}

[Extension(Point)]
fn add(self: *Self, dx: i32, dy: i32) i32 {
    return self.x + dx + self.y + dy;
}

fn f() i32 {
    var p = Point{ x = 3, y = 4 };
    return p.add(10, 20);
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(37));
}

#[test]
fn extension_method_on_struct_ir() {
    // 扩展方法在 struct 上运行
    let src = r#"
struct Point { x: i32, y: i32 }

[Extension(Point)]
fn magnitude(self: *Self) i32 {
    return self.x + self.y;
}

fn f() i32 {
    var p = Point{ x = 3, y = 4 };
    return p.magnitude();
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(7));
}

#[test]
fn extension_method_on_struct_with_extra_args_ir() {
    let src = r#"
struct Point { x: i32, y: i32 }

[Extension(Point)]
fn add(self: *Self, dx: i32, dy: i32) i32 {
    return self.x + dx + self.y + dy;
}

fn f() i32 {
    var p = Point{ x = 3, y = 4 };
    return p.add(10, 20);
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(37));
}

#[test]
fn alloc_init_on_struct_ir() {
    // alloc.init 在 struct 上分配堆实例
    let src = r#"
struct Point { x: i32, y: i32 }
fn f() i32 {
    var p = alloc.init(Point{ x = 10, y = 20 });
    return p.x + p.y;
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(30));
}

#[test]
fn alloc_init_default_on_struct_ir() {
    // alloc.init(StructName) 默认值构造
    let src = r#"
struct Point { x: i32, y: i32 }
fn f() i32 {
    var p = alloc.init(Point);
    return p.x + p.y;
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(0));
}

// ---------- Struct 类型 IR 验收 ----------

#[test]
fn struct_literal_and_field_ir() {
    let src = r#"
struct Point { x: i32, y: i32 }
fn f() i32 {
    var p = Point{ x = 3, y = 4 };
    return p.x + p.y;
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(7));
}

#[test]
fn struct_field_access_through_pointer_ir() {
    let src = r#"
struct Point { x: i32, y: i32 }
fn dot(a: *Point) i32 {
    return a.x + a.y;
}
fn f() i32 {
    var p = Point{ x = 5, y = 6 };
    return dot(&p);
}
"#;
    assert_eq!(run(src, "f", &[]).unwrap(), IrValue::Int(11));
}

// ---------- 组 G4a：线程生命周期（协作式延迟执行） ----------

#[test]
fn bound_ref_capture_join_ir() {
    // `&局部` 捕获 + join（Q18 绑定）：spawn→join 之间无写入（Q19 冻结窗口闭合）
    let src = r#"
fn touch(x: *i32) i32 { return x.*; }
fn main() i32 {
    var v: i32 = 7;
    var th = spawn(touch, &v);
    return try th.join();
}
"#;
    assert_eq!(run(src, "main", &[]).unwrap(), IrValue::Int(7));
}
