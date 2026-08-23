//! M3.4 双模式一致性验收测试
//!
//! 同一程序分别经 **tree-walking 解释器**（hc-rt `Interp`，脚本模式）与
//! **IR 参考解释器**（`hc::ir::IrRuntime`，M3.1 共享 IR 语义源）运行全部
//! `test fn`，PASS/FAIL 结果必须完全一致（ADR-0004 双模式一致性承诺根基）。
//! IR 侧共享同一运行时实例：全局/const 只初始化一次，跨 test fn 可变全局可见
//! （对齐 tree-walking 共享 `Interp` 的 `globals: HashMap`）。
//!
//! 结果归一化：
//! - tree-walk：`[PASS]/[FAIL]`（`run_tests` 输出）
//! - IR：`Ok(非错误值)` = PASS；`Ok(错误值)` = FAIL（未处理错误到根 = panic 式失败，
//!   M2.6 传播模型）；`Err` = FAIL
//!
//! 程序约束：必须通过语义检查（`Interp::load` 内置 M2 静态 pass）——例如
//! `catch` 只能用于错误联合值、字面量必须在声明的宽度内。
//! 覆盖范围 = M3.1 IR 切片：标量/短路/if/while/递归/try/catch/orelse/
//! error 字面量/断言/限定名调用（含多级 namespace）/作用域/复合赋值/除零溢出；
//! Phase 5 起含 global/const（`@__init__` 声明序初始化、跨 test fn 可变全局）。

use std::collections::HashMap;
use std::fs;
use std::thread;

use hc::ir::{lower, IrRuntime, IrValue};
use hc::parse_source;
use hc_rt::Interp;

/// 双模式一致性：两模式全部 [test] fn 的 PASS/FAIL 必须一致。
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
    let mut tw_order: Vec<(String, bool)> = Vec::new(); // 声明序（tree-walk 按 span 序输出）
    for line in &interp.test_out {
        if let Some(rest) = line.strip_prefix("[PASS] ") {
            tw.insert(rest.to_string(), true);
            tw_order.push((rest.to_string(), true));
        } else if let Some(rest) = line.strip_prefix("[FAIL] ") {
            let name = rest.split(' ').next().unwrap_or(rest);
            tw.insert(name.to_string(), false);
            tw_order.push((name.to_string(), false));
        }
    }
    if tw.is_empty() {
        panic!("tree-walk 未运行任何 test fn");
    }

    // 模式 B：IR 参考解释器（共享 IrRuntime：全局/const 只初始化一次，
    // 跨 test fn 的可变全局与 tree-walking 的共享 Interp 语义对齐；声明序执行）
    let module = lower(&program).unwrap();
    let mut rt = IrRuntime::new();
    let mut tw_pass = 0usize;
    let mut ir_pass = 0usize;
    for (name, passed_tw) in &tw_order {
        let r = rt.call(&module, name, &[]);
        let passed_ir = match &r {
            Ok(IrValue::Err { .. }) => false, // 未处理错误值到根 → FAIL（M2.6）
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
[test] fn arith() void {
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
[test] fn and_short_circuits() void {
    if (false and expect_bad()) {
        expect(false);
    }
}
[test] fn or_short_circuits() void {
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
[test] fn and_short_circuits() void {
    if (false and expect_bad()) {
        expect(false);
    }
}
[test] fn and_eager_fails() void {
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
[test] fn grades() void {
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
[test] fn if_expr() void {
    expect_eq_slices(label(7), "big");
    expect_eq_slices(label(3), "small");
    expect_eq(pick(null), 0);
    expect_eq(pick(7), 7);
}
"#,
    );
}

#[test]
fn while_optional_capture() {
    // while (maybe) |v|：Some 绑定 v 并循环，None 退出（含 step 前置/后置两种文法）
    assert_all_pass(
        r#"
fn next(x: i32) ?i32 {
    if (x < 5) {
        return x;
    }
    return null;
}
[test] fn while_cap() void {
    var mut n: i32 = 0;
    var mut sum: i32 = 0;
    while (next(n)) |v| {
        sum += v;
        n += 1;
    }
    expect_eq(n, 5);
    expect_eq(sum, 10);
}
[test] fn while_cap_with_step_after() void {
    var mut n: i32 = 0;
    var mut sum: i32 = 0;
    while (next(n)) |v| : (n += 1) {
        sum += v;
    }
    expect_eq(n, 5);
    expect_eq(sum, 10);
}
"#,
    );
}

#[test]
fn if_error_union_capture() {
    // if (e!T) |v| else |err|：成功绑定负载走 then，错误绑定 err 走 else
    assert_all_pass(
        r#"
fn may_fail(x: i32) !i32 {
    if (x > 0) {
        return x;
    }
    return error.Negative;
}
fn try_it(x: i32) !i32 {
    if (may_fail(x)) |v| {
        return v;
    } else |err| {
        return err;
    }
}
[test] fn if_err_cap() void {
    var a = try_it(5) catch |e| { -1; };
    expect_eq(a, 5);
    var b = try_it(-1) catch |e| { 99; };
    expect_eq(b, 99);
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
[test] fn sum() void {
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
[test] fn fib10() void {
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
[test] fn catch_variants() void {
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
[test] fn try_bubbles() void {
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
[test] fn orelse_null() void {
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
namespace Svc {
    namespace Net {
        fn double(x: i32) i32 { return x * 2; }
    }
}
[test] fn qualified() void {
    expect_eq(Math.square(4), 16);
    expect_eq(Svc.Net.double(21), 42);
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
[test] fn shadow_restores() void {
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
[test] fn err_literal() void {
    expect_error(fail(), error.NotFound);
}
"#,
    );
    // 未处理错误值返回测试根：两模式都 FAIL（M2.6 根作用域 panic 式失败）
    let src = r#"
fn fail() !i32 { return error.NotFound; }
[test] fn unhandled() !void {
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
[test] fn div_zero() void {
    expect_eq(10 / 0, 0);
}
[test] fn overflow() void {
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
[test] fn comp() void {
    expect_eq(f(), 16);
}
"#,
    );
}

#[test]
fn pointer_write_through_alias() {
    // Phase 1 指针（M3 语义源 + oracle 双模式）：`&mut` 写穿 + 跨函数别名 + `p.*` 取值
    assert_all_pass(
        r#"
fn bump(p: *mut i32) void {
    p.* += 1;
}
[test] fn pointer_alias() void {
    var mut x: i32 = 41;
    var p = &mut x;
    p.* = 42;
    expect_eq(x, 42);
    bump(&mut x);
    expect_eq(x, 43);
}
[test] fn pointer_deref_value() void {
    var mut x: i32 = 5;
    var p = &mut x;
    p.* += 1;
    expect_eq(p.*, 6);
}
[test] fn pointer_identity_eq() void {
    var mut x: i32 = 5;
    var p = &mut x;
    var q = &mut x;
    expect(p == q);
    expect_eq(p.*, x);
}
"#,
    );
}

// ---------- Phase 2：聚合（字段/索引/切片/字面量/解构/move/unwrap/enum） ----------

#[test]
fn agg_struct_literal_and_field() {
    // MakeClass/Field/StoreField：struct 字面量 + 字段读写 + 类深比较
    assert_all_pass(
        r#"
class Point {
    x: i32,
    y: i32,
}
[test] fn struct_literal_field() void {
    var p = Point{ x = 1, y = 2 };
    expect_eq(p.x, 1);
    expect_eq(p.y, 2);
    p.y = 5;
    expect_eq(p.y, 5);
    expect_eq(p, Point{ x = 1, y = 5 });
    expect_neq(p, Point{ x = 2, y = 5 });
}
"#,
    );
}

// ---------- K1 无标签 union（ADR-0014）：字段字节重解释双模式一致 ----------

#[test]
fn agg_union_int_wider_to_narrower() {
    // 写 i32 → 读 i8/i16/bool（窄字段取低字节，符号扩展/非零判定）
    assert_all_pass(
        r#"
union U {
    a: i32,
    b: i16,
    c: i8,
    d: bool,
}
[test] fn union_int_wider_to_narrower() void {
    var u = U{ a = 0x03040506 }; // 字节 06 05 04 03
    expect_eq(u.c, 6);           // 低 i8 = 6
    expect_eq(u.b, 0x0506);      // 低 i16 = 1286
    expect_eq(u.d, true);        // 低字节 6 ≠ 0
    u.a = 0x00010000;            // 字节 00 00 01 00
    expect_eq(u.b, 0);           // 低 i16 = 0
    expect_eq(u.d, false);       // 低字节 0
}
"#,
    );
}

#[test]
fn agg_union_float_int_reinterpret() {
    // f32 ↔ i32 同位重解释（1.0f32 位型 = 1065353216）
    assert_all_pass(
        r#"
union Num {
    i: i32,
    f: f32,
}
[test] fn union_float_int() void {
    var n = Num{ f = 1.0 };
    expect_eq(n.i, 1065353216);
    n.i = 1065353216;
    expect_eq(n.f, 1.0);
}
"#,
    );
}

#[test]
fn agg_union_bool_int_narrow() {
    // i8 ↔ bool（bool 写 1 字节 / 读低字节非零）
    assert_all_pass(
        r#"
union B {
    i: i8,
    b: bool,
}
[test] fn union_bool_int() void {
    var u = B{ b = true };
    expect_eq(u.i, 1);
    u.i = 0;
    expect_eq(u.b, false);
    u.i = 3;
    expect_eq(u.b, true);
}
"#,
    );
}

#[test]
fn agg_union_write_syncs_others() {
    // 写任意字段 → 其余字段重解释同步（读任意字段 = 最后写入字节）
    assert_all_pass(
        r#"
union U {
    a: i32,
    b: bool,
}
[test] fn union_write_sync() void {
    var u = U{ a = 256 };        // 字节 00 01 00 00
    expect_eq(u.b, false);
    u.a = 1;                      // 字节 01 00 00 00
    expect_eq(u.b, true);
    u.a = 0;
    expect_eq(u.b, false);
}
"#,
    );
}

#[test]
fn agg_union_equality() {
    // 同字节 union 相等；不同字节不等（字段同步后全同态）
    assert_all_pass(
        r#"
union Num {
    i: i32,
    f: f32,
}
[test] fn union_eq() void {
    var a = Num{ i = 1 };
    var b = Num{ i = 1 };
    expect_eq(a, b);
    var c = Num{ i = 2 };
    expect_neq(a, c);
    var d = Num{ i = 1065353216 };
    expect_neq(a, d);   // f = 1.0 ≠ a.f
}
"#,
    );
}

#[test]
fn agg_union_truncated_read_fails() {
    // 窄写入后读宽字段：字节不足（truncated union bytes）→ 两模式测试均 FAIL
    let src = r#"
union T {
    a: i8,
    b: i32,
}
[test] fn union_truncated() void {
    var t = T{ a = 5 };
    expect_eq(t.b, 5);
}
"#;
    let (tw, ir) = assert_consistent(src);
    assert_eq!((tw, ir), (0, 0));
}

#[test]
fn agg_volatile_load_store_roundtrip() {
    // K2：@volatileStore 写穿 + @volatileLoad 读穿——往返一致；写穿对变量可见
    assert_all_pass(
        r#"
[test] fn volatile_roundtrip() void {
    var mut x: i32 = 5;
    var p = &mut x;
    @volatileStore(p, 42);
    var y: i32 = @volatileLoad(p);
    expect_eq(y, 42);
    expect_eq(x, 42);
}
"#,
    );
}

#[test]
fn agg_volatile_load_sees_plain_writes() {
    // K2：volatile load 读到普通赋值 / `p.* = v` 的写入（同一槽，无缓存）
    assert_all_pass(
        r#"
[test] fn volatile_reads_plain_writes() void {
    var mut x: i32 = 7;
    var p = &mut x;
    x = 100;
    var a: i32 = @volatileLoad(p);
    expect_eq(a, 100);
    p.* = 200;
    var b: i32 = @volatileLoad(p);
    expect_eq(b, 200);
}
"#,
    );
}

#[test]
fn agg_volatile_store_non_pointer_fails() {
    // K2：@volatileStore 非指针目标 → BadAssign → 两模式测试均 FAIL
    let src = r#"
[test] fn volatile_bad_store() void {
    @volatileStore(5, 7);
}
"#;
    let (tw, ir) = assert_consistent(src);
    assert_eq!((tw, ir), (0, 0));
}

#[test]
fn agg_ptr_from_int_roundtrip_write_through() {
    // K4：@intFromPtr(p) → usize → @ptrFromInt 重建指针；写穿对原变量可见（round-trip 保真）
    assert_all_pass(
        r#"
[test] fn ptr_roundtrip() void {
    var mut x: i32 = 5;
    var p = &mut x;
    var a: usize = @intFromPtr(p);
    var q = @ptrFromInt(a);
    q.* = 42;
    expect_eq(x, 42);
    var y: i32 = @volatileLoad(q);
    expect_eq(y, 42);
}
"#,
    );
}

#[test]
fn agg_ptr_from_int_unknown_addr_idempotent() {
    // K4：@ptrFromInt(未登记地址) 合成匿名槽——同地址幂等（两次调用同一槽，写读一致）
    assert_all_pass(
        r#"
[test] fn ptr_unknown_addr() void {
    var p1 = @ptrFromInt(0x40000000);
    @volatileStore(p1, 7);
    var p2 = @ptrFromInt(0x40000000);
    var y: i32 = @volatileLoad(p2);
    expect_eq(y, 7);
}
"#,
    );
}

#[test]
fn agg_ptr_from_int_uninit_slot_read_fails() {
    // K4：@ptrFromInt(未登记地址) 合成匿名槽（初值 Void，语义层放行）；读穿未初始化槽
    // → 断言失败（双模式一致 FAIL——运行时错误，非编译期拦截）
    let src = r#"
[test] fn ptr_uninit_read() void {
    var p = @ptrFromInt(0x1000);
    var y: i32 = @volatileLoad(p);
    expect_eq(y, 0);
}
"#;
    let (tw, ir) = assert_consistent(src);
    assert_eq!((tw, ir), (0, 0));
}

#[test]
fn agg_export_fn_transparent_at_runtime() {
    // K5：`export fn` 运行时透明——导出仅影响原生符号层（thunk 生成 + 清单注释），
    // interp/IR 双后端按普通函数调用，参数/返回/嵌套调用结果一致
    assert_all_pass(
        r#"
export fn add(a: i32, b: i32) i32 {
    return a + b;
}
[test] fn export_callable() void {
    expect_eq(add(2, 3), 5);
    expect_eq(add(add(1, 1), add(2, 2)), 6);
}
"#,
    );
}

#[test]
fn agg_array_index_and_store() {
    // MakeArr/Index/StoreIndex：数组字面量 + 单索引读写
    assert_all_pass(
        r#"
[test] fn arr_index() void {
    var a = [10, 20, 30];
    expect_eq(a[0], 10);
    expect_eq(a[2], 30);
    a[1] = 99;
    expect_eq(a[1], 99);
    expect_neq(a, [10, 20, 30]);
    expect_eq(a, [10, 99, 30]);
}
"#,
    );
}

#[test]
fn agg_len_fields() {
    // `.len`：Str / Arr / Slice 三形态字段（Field 指令）
    assert_all_pass(
        r#"
[test] fn len_str_arr_slice() void {
    var s = "abc";
    expect_eq(s.len, 3);
    var arr = [10, 20, 30, 40];
    expect_eq(arr.len, 4);
    var sub = arr[1..3];
    expect_eq(sub.len, 2);
}
"#,
    );
}

#[test]
fn agg_slice_view_and_alias() {
    // SliceOf：切片为共享视图——切片索引与源数组元素 cell 别名（写穿）
    assert_all_pass(
        r#"
[test] fn slice_view() void {
    var arr = [1, 2, 3, 4, 5];
    var sub = arr[1..4];
    expect_eq(sub.len, 3);
    expect_eq(sub[0], 2);
    expect_eq(sub[2], 4);
    arr[1] = 99;
    expect_eq(sub[0], 99);
}
"#,
    );
}

#[test]
fn agg_slice_store_write_through() {
    // StoreSlice：`arr[lo..hi] = v` 写回源数组元素（源须为 Arr）
    assert_all_pass(
        r#"
[test] fn slice_store() void {
    var arr = [1, 2, 3, 4, 5];
    arr[1..3] = [20, 30];
    expect_eq(arr[1], 20);
    expect_eq(arr[2], 30);
    expect_eq(arr.len, 5);
}
"#,
    );
}

#[test]
fn agg_tuple_destructure() {
    // Destructure：元组（多值返回）解构绑定
    assert_all_pass(
        r#"
fn divmod(a: i32, b: i32) (i32, i32) {
    return (a / b, a % b);
}
[test] fn tuple_destructure() void {
    var (q, r) = divmod(10, 3);
    expect_eq(q, 3);
    expect_eq(r, 1);
}
"#,
    );
}

#[test]
fn agg_move_expr() {
    // Move：所有权转移标记（tag1 值语义——`move x` ≡ 值拷贝，原绑定仍可访问）
    assert_all_pass(
        r#"
[test] fn move_is_value_copy() void {
    var a = [1, 2, 3];
    var b = move a;
    expect_eq(b.len, 3);
    expect_eq(b[1], 2);
    expect_eq(a.len, 3);
}
"#,
    );
}

#[test]
fn agg_unwrap_opt() {
    // Unwrap：`x.?` 解包 Opt(Some) → 值；Opt(None) → NullUnwrap 硬错误
    assert_all_pass(
        r#"
fn boxed(x: ?i32) ?i32 { return x; }
[test] fn unwrap_some() void {
    var v = boxed(7).?;
    expect_eq(v, 7);
}
[test] fn unwrap_other_identity() void {
    var v = boxed(7);
    var w = v.?;
    expect_eq(w, 7);
}
"#,
    );
}

#[test]
fn agg_enum_literal_and_eq() {
    // MakeEnum：类型名限定枚举常量 + 值比较（name+variant+payload）
    assert_all_pass(
        r#"
enum Color { red, green, blue }
[test] fn enum_literal() void {
    var c = Color.green;
    expect_eq(c, Color.green);
    expect_neq(c, Color.red);
    var d = Color.blue;
    expect_neq(c, d);
}
"#,
    );
}

#[test]
fn agg_array_deep_eq() {
    // value_eq：数组深比较（元素按值、递归）
    assert_all_pass(
        r#"
[test] fn arr_deep_eq() void {
    var a = [1, 2, 3];
    var b = [1, 2, 3];
    expect_eq(a, b);
    expect_neq(a, [1, 2, 4]);
}
"#,
    );
}

#[test]
fn agg_class_with_array_field() {
    // 嵌套聚合：class 字段为数组，字段索引写穿
    assert_all_pass(
        r#"
class Box {
    items: [3]i32,
    tag: i32,
}
[test] fn class_nested_array() void {
    var b = Box{ items = [1, 2, 3], tag = 7 };
    expect_eq(b.items.len, 3);
    expect_eq(b.items[0], 1);
    b.items[1] = 20;
    expect_eq(b.items[1], 20);
    b.tag = 8;
    expect_eq(b.tag, 8);
}
"#,
    );
}

#[test]
fn agg_index_out_of_bounds_fails() {
    // 越界索引：硬错误（不可 catch）→ 两模式测试均为 FAIL
    let src = r#"
[test] fn oob() void {
    var arr = [1, 2, 3];
    expect_eq(arr[5], 0);
}
"#;
    let (tw, ir) = assert_consistent(src);
    assert_eq!((tw, ir), (0, 0));
}

#[test]
fn agg_unwrap_null_fails() {
    // NullUnwrap：硬错误（不可 catch）→ 两模式测试均为 FAIL
    let src = r#"
fn boxed(x: ?i32) ?i32 { return x; }
[test] fn unwrap_null() void {
    var v = boxed(null).?;
    expect_eq(v, 1);
}
"#;
    let (tw, ir) = assert_consistent(src);
    assert_eq!((tw, ir), (0, 0));
}

// ---------- Phase 3 switch + range + for 双模式一致 ----------

#[test]
fn switch_int_and_else() {
    // 整数模式 first-match 线性链 + else 兜底
    let src = r#"
fn pick(x: i32) i32 {
    switch (x) {
        1 => return 10,
        2 => return 20,
        else => return 99,
    }
}
[test] fn t() void {
    expect_eq(pick(1), 10);
    expect_eq(pick(2), 20);
    expect_eq(pick(9), 99);
}
"#;
    assert_all_pass(src);
}

#[test]
fn switch_no_else_falls_through() {
    // 无匹配 + 无 else → 语句降级为 Void（不崩溃）
    let src = r#"
fn f(x: i32) i32 {
    var mut y: i32 = 0;
    switch (x) { 1 => { y = 5; } }
    return y;
}
[test] fn t() void {
    expect_eq(f(1), 5);
    expect_eq(f(7), 0);
}
"#;
    assert_all_pass(src);
}

#[test]
fn switch_enum_variant_and_capture() {
    // 枚举变体（Ident 模式）+ 负载捕获（|payload|）——表达式形态（对齐 14-enum.hc）
    let src = r#"
enum Maybe { some: i32, none }
[test] fn t() void {
    var v: Maybe = Maybe{some = 7};
    var label = switch (v) {
        some => |i| i,
        none => -1,
        else => -2,
    };
    expect_eq(label, 7);
    var n: Maybe = Maybe.none;
    var label2 = switch (n) {
        some => |i| i,
        none => -1,
        else => -2,
    };
    expect_eq(label2, -1);
}
"#;
    assert_all_pass(src);
}

#[test]
fn switch_error_pattern() {
    // 错误名模式（err_test.hc 的 switch 错误模式子集）：catch |err| switch (err)
    let src = r#"
fn fail(name: i32) !i32 {
    if (name == 1) { return error.NotFound; }
    if (name == 2) { return error.PermissionDenied; }
    return error.Io;
}
[test] fn t() void {
    var r1 = fail(1) catch |err| switch (err) {
        error.NotFound => 10,
        error.PermissionDenied => 20,
        else => 30,
    };
    expect_eq(r1, 10);
    var r2 = fail(2) catch |err| switch (err) {
        error.NotFound => 10,
        error.PermissionDenied => 20,
        else => 30,
    };
    expect_eq(r2, 20);
    var r3 = fail(3) catch |err| switch (err) {
        error.NotFound => 10,
        error.PermissionDenied => 20,
        else => 30,
    };
    expect_eq(r3, 30);
}
"#;
    assert_all_pass(src);
}

#[test]
fn switch_string_pattern() {
    // 字符串字面量模式 + else
    let src = r#"
fn pick(s: String) i32 {
    switch (s) {
        "a" => return 1,
        "b" => return 2,
        else => return 0,
    }
}
[test] fn t() void {
    expect_eq(pick("a"), 1);
    expect_eq(pick("b"), 2);
    expect_eq(pick("z"), 0);
}
"#;
    assert_all_pass(src);
}

#[test]
fn switch_bool_and_null_patterns() {
    // bool true/false 与 null（Ident 模式）
    let src = r#"
fn pick(b: bool) i32 {
    switch (b) {
        true => return 1,
        false => return 0,
    }
}
fn pickn(x: ?i32) i32 {
    switch (x) {
        null => return -1,
        else => return 1,
    }
}
[test] fn t() void {
    expect_eq(pick(true), 1);
    expect_eq(pick(false), 0);
    expect_eq(pickn(null), -1);
    expect_eq(pickn(5), 1);
}
"#;
    assert_all_pass(src);
}

#[test]
fn switch_guard_basic() {
    // C3：switch 守卫——模式匹配后检查守卫条件，守卫失败继续下一分支
    let src = r#"
[test] fn t() void {
    // 守卫为 true → 执行该臂
    var x: i32 = 42;
    var r = switch (x) {
        42 if true => 1,
        42 => 2,
        else => 3,
    };
    expect_eq(r, 1);
    // 守卫为 false → 继续下一分支
    var r2 = switch (x) {
        42 if false => 1,
        42 => 2,
        else => 3,
    };
    expect_eq(r2, 2);
    // 所有守卫都失败 → 进入 else
    var r3 = switch (x) {
        42 if false => 1,
        42 if false => 2,
        else => 3,
    };
    expect_eq(r3, 3);
}
"#;
    assert_all_pass(src);
}

#[test]
fn switch_guard_with_enum() {
    // C3：枚举 switch 守卫
    let src = r#"
enum Value { int: i32, str: String, none }
[test] fn t() void {
    var v: Value = Value{int = 7};
    // 枚举变体匹配 + 守卫检查负载
    var r = switch (v) {
        int if true => |i| i,
        int => 0,
        str => |_| -1,
        none => -2,
    };
    expect_eq(r, 7);
    // 守卫失败 → 下一分支
    var r2 = switch (v) {
        int if false => |i| i,
        int => 99,
        str => |_| -1,
        none => -2,
    };
    expect_eq(r2, 99);
}
"#;
    assert_all_pass(src);
}

#[test]
fn switch_guard_exhaustiveness() {
    // C3：switch 守卫检查——至少一个非守卫臂或 else 臂
    let src = r#"
[test] fn t() void {
    var x: i32 = 42;
    // 有非守卫臂 → 通过穷举检查
    var r = switch (x) {
        1 if x > 0 => 10,
        else => 20,
    };
    expect_eq(r, 20);
    // 只有守卫臂 + else → 通过
    var r2 = switch (x) {
        1 if x > 100 => 10,
        else => 20,
    };
    expect_eq(r2, 20);
}
"#;
    assert_all_pass(src);
}

#[test]
fn switch_guard_in_statement() {
    // C3：语句形态 switch 守卫
    let src = r#"
[test] fn t() void {
    var mut r: i32 = 0;
    var x: i32 = 3;
    switch (x) {
        1 if true => { r = 1; },
        2 if true => { r = 2; },
        3 if false => { r = 3; },
        3 => { r = 33; },
        else => { r = -1; },
    }
    expect_eq(r, 33);
}
"#;
    assert_all_pass(src);
}

#[test]
fn for_range_sugar() {
    // `for (lo..hi)` 区间糖：MakeRange + 只读捕获
    let src = r#"
[test] fn t() void {
    var mut s: i32 = 0;
    for (0..5) |i| { s += i; }
    expect_eq(s, 10);
}
"#;
    assert_all_pass(src);
}

#[test]
fn for_arr_read_only() {
    // 数组只读捕获：元素值拷贝，写回不传播
    let src = r#"
[test] fn t() void {
    var a = [1, 2, 3];
    var mut s: i32 = 0;
    for (a) |x| { s += x; }
    expect_eq(s, 6);
}
"#;
    assert_all_pass(src);
}

#[test]
fn for_arr_mut_writeback() {
    // `for (arr) |mut x|` 写回：循环体末尾把捕获槽写回源数组元素
    let src = r#"
[test] fn t() void {
    var a = [1, 2, 3];
    for (a) |mut x| { x += 1; }
    expect_eq(a[0], 2);
    expect_eq(a[1], 3);
    expect_eq(a[2], 4);
}
"#;
    assert_all_pass(src);
}

#[test]
fn for_slice_view_write_through() {
    // 切片迭代：元素共享源数组 cell（Mut 写回透传到源）
    let src = r#"
[test] fn t() void {
    var arr = [10, 20, 30, 40];
    var sub = arr[1..3];
    for (sub) |mut x| { x = x + 1; }
    expect_eq(arr[1], 21);
    expect_eq(arr[2], 31);
}
"#;
    assert_all_pass(src);
}

#[test]
fn for_str_bytes() {
    // 字符串迭代：字节 Int 值（is_ref=false 新单元）
    let src = r#"
[test] fn t() void {
    var mut sum: i32 = 0;
    for ("abc") |b| { sum += b; }
    expect_eq(sum, 97 + 98 + 99);
}
"#;
    assert_all_pass(src);
}

#[test]
fn for_empty_range_is_empty() {
    // lo ≥ hi → 空数组（区间糖的负向边界）
    let src = r#"
[test] fn t() void {
    var mut s: i32 = 0;
    for (5..2) |i| { s += i; }
    expect_eq(s, 0);
}
"#;
    assert_all_pass(src);
}

#[test]
fn for_break_and_continue() {
    // break 提前退出；continue 跳过（两模式一致）
    let src = r#"
[test] fn t() void {
    var mut s: i32 = 0;
    for (0..10) |i| {
        if (i == 3) { continue; }
        if (i == 6) { break; }
        s += i;
    }
    expect_eq(s, 0 + 1 + 2 + 4 + 5);
}
"#;
    assert_all_pass(src);
}

// ---------- Phase 4：闭包 / 方法 / 重载 双模式一致 ----------

#[test]
fn closure_capture_consistency() {
    // 读捕获共享槽 / move 捕获拷贝 / mut 捕获写穿（对齐 closures.rs oracle）
    assert_all_pass(
        r#"
[test] fn read_cap() void {
    var a = 10;
    var f = |v| v + a;
    a = 100;
    expect_eq(f(5), 105);
}
[test] fn move_cap() void {
    var a = 10;
    var f = move |v| v + a;
    a = 100;
    expect_eq(f(5), 15);
}
[test] fn mut_cap() void {
    var total = 0;
    var acc = mut |v| { total = total + v; return total; };
    expect_eq(acc(3), 3);
    expect_eq(acc(4), 7);
    expect_eq(total, 7);
}
"#,
    );
}

// ---------- Phase 8：闭包捕获精确化 + is_mut 强制 双模式一致 ----------

#[test]
fn closure_precise_capture_consistency() {
    // 捕获精确化：嵌套闭包传递（外层体只在内层体内引用 → 外层仍须捕获 a）、
    // move 捕获闭包值深拷贝其环境副本、mut 捕获写穿 + 嵌套只读闭包共享同一 cell
    // （对齐 closures.rs oracle 与 ir.rs 结构测试）
    assert_all_pass(
        r#"
[test] fn nested_transitive() void {
    var a = 1;
    var f = | | {
        var g = |v| v + a;      // 外层体只在内层闭包体内引用 a → 外层须捕获 a
        return g(10);
    };
    a = 100;
    expect_eq(f(), 110);        // 共享捕获：外部变更对闭包可见
}
[test] fn move_deep_copy() void {
    var x = 1;
    var inner = |v| v + x;
    var outer_move = move | | inner(1);  // move 捕获闭包值 → 深拷贝其 env 副本
    x = 100;
    expect_eq(outer_move(), 2);          // 副本 x 仍为 1 → 1+1=2
}
[test] fn mut_cap_visible_to_nested_read() void {
    var total = 0;
    var acc = mut |v| { total = total + v; return total; };
    var read = |v| total + v;            // 嵌套只读闭包共享同一捕获 cell
    expect_eq(acc(3), 3);
    expect_eq(read(1), 4);
    expect_eq(total, 3);
}
"#,
    );
}

// ---------- P11d：连续类（Continuous）值语义——var 声明即复制 ----------

#[test]
fn continuous_class_value_semantics_consistency() {
    // 对齐 oracle `interp.rs:926-949`：声明的类型为 Named 且连续（`[continuous]`
    // 类），或未标注类型且初始值为标识符（运行时按值实际类名判定）→ 深拷贝。
    // （非连续类为引用类型，语义层禁止按值赋值——`copy(&x)` 显式深拷贝；非本测试范围。）
    assert_all_pass(
        r#"
struct Point {
    x: f32,
    y: f32,
}
[test] fn declared_type_continuous_copy() void {
    var p: Point = Point{x = 1.0, y = 2.0};
    var p2: Point = p;           // 连续类 var 声明即复制（独立副本）
    p2.x = 99.0;
    expect_eq(p.x, 1.0);         // 原值未变
    expect_eq(p2.x, 99.0);
}
[test] fn untyped_ident_continuous_copy() void {
    var p1 = Point{x = 1.0, y = 2.0};
    var mut p2 = p1;             // 未标注类型 + 标识符初始化 → 运行时门按类名复制
    p2.x = 99.0;
    expect_eq(p1.x, 1.0);        // 原值未变
    expect_eq(p2.x, 99.0);
}
"#,
    );
}

#[test]
fn closure_non_mut_cannot_rebind_capture_consistency() {
    // is_mut 只读强制：非 `mut` 闭包内重绑定被捕获变量 → ReadonlyCapture
    // （硬错误，不可 catch）——两模式测试均 FAIL
    let src = r#"
[test] fn rebind() void {
    var total = 0;
    var acc = |v| { total = total + v; return total; };
    acc(3);
}
"#;
    let (tw, ir) = assert_consistent(src);
    assert_eq!((tw, ir), (0, 0));
}

#[test]
fn method_dispatch_consistency() {
    // 实例方法（注入 self 动态分派）+ 静态调用（显式 self）两模式一致
    assert_all_pass(
        r#"
class Rect {
    w: i32,
    h: i32,
    fn area(self: *Self) i32 { return self.w * self.h; }
}
[test] fn instance_method() void {
    var r = Rect{ w = 3, h = 4 };
    expect_eq(r.area(), 12);
}
[test] fn static_call() void {
    var r = Rect{ w = 3, h = 4 };
    expect_eq(Rect.area(&r), 12);
}
"#,
    );
}

#[test]
fn overload_consistency() {
    // func_index 一名多候选：按实参数量精确分派（对齐 pick_fn）
    assert_all_pass(
        r#"
fn sq(x: i32) i32 { return x * x; }
fn sq(x: i32, y: i32) i32 { return x * y; }
[test] fn overloads() void {
    expect_eq(sq(3), 9);
    expect_eq(sq(2, 4), 8);
}
"#,
    );
}

#[test]
fn global_const_init_and_cross_test_mutation() {
    // Phase 5：global/const 声明序初始化（合成 `@__init__`）+ 跨 test fn 可变全局。
    // 两模式均按声明序运行测试且共享运行时（tree-walk 共享 Interp、IR 共享 IrRuntime），
    // 故 a 先于 b 执行、b 可见 a 的写入。
    assert_all_pass(
        r#"
global counter: i32 = 0;
const STEP: i32 = 2;

[test] fn g_a_increments() void {
    counter = counter + STEP;
    expect_eq(counter, 2);
}

[test] fn g_b_sees_prev_mutation() void {
    expect_eq(counter, 2);
    counter += 1;
    expect_eq(counter, 3);
}

[test] fn g_c_const_read() void {
    expect_eq(STEP, 2);
    expect_eq(counter, 3);
}
"#,
    );
}

#[test]
fn global_mutation_between_plain_fns() {
    // 全局在普通函数间共享（非仅 test fn）：bump 读改写全局，main 级联可见。
    assert_all_pass(
        r#"
global g: i32 = 10;
fn bump() i32 {
    g = g * 2;
    return g;
}
[test] fn g_plain_fns() void {
    var a = bump();
    var b = bump();
    expect_eq(a, 20);
    expect_eq(b, 40);
    expect_eq(g, 40);
}
"#,
    );
}

#[test]
fn global_address_of_writes_through() {
    // `&global`/`&mut global` 别名全局 cell：`p.*` 写穿回全局，跨 test fn 可见。
    // 对齐 oracle `AddrOf(Ident)` 对全局名走 `lookup` → 全局 `Rc<RefCell>` 共享。
    assert_all_pass(
        r#"
global counter: i32 = 0;
fn bump() i32 {
    var p = &mut counter;
    p.* += 1;
    return p.*;
}
[test] fn g_addr_first() void {
    expect_eq(bump(), 1);
    expect_eq(counter, 1);
}
[test] fn g_addr_sees_prev() void {
    expect_eq(bump(), 2);
    expect_eq(counter, 2);
}
[test] fn g_addr_read_only() void {
    var r = &counter;
    expect_eq(r.*, 2);
    expect_eq(counter, 2);
}
"#,
    );
}

// ---------- Phase 6：defer / errdefer + 带标签 break/continue 双模式一致 ----------

#[test]
fn defer_runs_lifo_at_scope_exit() {
    // defer LIFO：3, 2, 1 登记序 → 作用域退出按 1, 2, 3 运行（对齐 oracle `run_defers` 逆序）。
    // 经全局可观测（defer 在测试函数体最后语句之后运行）。
    assert_all_pass(
        r#"
global log: i32 = 0;
fn rec(v: i32) void { log = log * 10 + v; }
[test] fn defer_writes_log() void {
    defer rec(1);
    defer rec(2);
    defer rec(3);
}
[test] fn check_log() void {
    expect_eq(log, 321);
}
"#,
    );
}

#[test]
fn defer_same_scope_capture_reads_final_value() {
    // defer 体在作用域退出时重求值：同作用域局部变量读到「退出时最终值」而非登记时值。
    // 依赖 oracle 修复——pop_scope 先跑 defers 再弹作用域。
    assert_all_pass(
        r#"
global sum: i32 = 0;
fn add(v: i32) void { sum += v; }
[test] fn defer_reads_final() void {
    var x: i32 = 1;
    defer add(x);
    x = 100;
}
[test] fn check_sum() void {
    expect_eq(sum, 100);
}
"#,
    );
}

#[test]
fn defer_nested_block_runs_at_block_close() {
    // 内层块 defer 随块结束（弹栈）运行，而非函数结束。
    assert_all_pass(
        r#"
global g: i32 = 0;
fn bump(v: i32) void { g += v; }
[test] fn nested_scope() void {
    g = 0;
    {
        defer bump(10);
        g += 1;
    }
    expect_eq(g, 11);
}
"#,
    );
}

#[test]
fn defer_runs_on_return() {
    // `return` 排空函数级 defers（运行期按返回值判 err_path；正常值仅非 errdefer）。
    assert_all_pass(
        r#"
global g: i32 = 0;
fn bump(v: i32) void { g += v; }
fn early() i32 {
    defer bump(5);
    return 1;
}
[test] fn return_runs_defer() void {
    g = 0;
    expect_eq(early(), 1);
    expect_eq(g, 5);
}
"#,
    );
}

#[test]
fn defer_runs_on_loop_break_and_continue() {
    // break/continue 排空循环体内 defers：每轮迭代 defer 都运行（含 continue 路径）。
    assert_all_pass(
        r#"
global dlog: i32 = 0;
global clog: i32 = 0;
fn bump() void { dlog += 1; }
[test] fn defer_loop_break() void {
    dlog = 0;
    var i: i32 = 0;
    while (true) {
        defer bump();
        i += 1;
        if (i >= 3) { break; }
    }
    expect_eq(dlog, 3);
}
[test] fn defer_loop_continue() void {
    dlog = 0;
    clog = 0;
    var i: i32 = 0;
    while (i < 5) {
        defer bump();
        i += 1;
        if (i == 3) { continue; }
        clog += 1;
    }
    expect_eq(dlog, 5);
    expect_eq(clog, 4);
}
"#,
    );
}

#[test]
fn errdefer_runs_only_on_error_path() {
    // errdefer：错误返回/错误传播路径触发；正常返回与正常作用域结束不触发。
    assert_all_pass(
        r#"
global g: i32 = 0;
fn bump(v: i32) void { g += v; }
fn maybe(ok: bool) !i32 {
    defer bump(1);
    errdefer bump(100);
    if (ok) { return 5; }
    return error.Fail;
}
[test] fn errdefer_error_path() void {
    g = 0;
    var r: i32 = maybe(false) catch 0;
    expect_eq(r, 0);
    expect_eq(g, 101);
}
[test] fn errdefer_ok_path() void {
    g = 0;
    var r: i32 = maybe(true) catch 0;
    expect_eq(r, 5);
    expect_eq(g, 1);
}
[test] fn errdefer_normal_block_close() void {
    g = 0;
    {
        errdefer bump(100);
        bump(1);
    }
    expect_eq(g, 1);
}
"#,
    );
}

#[test]
fn errdefer_runs_on_try_propagation() {
    // `try` 错误传播 = 从当前函数返回错误值：errdefer 须触发（对齐 oracle
    // `is_err_path(Err(signal(Flow::Return(err))))`）。
    assert_all_pass(
        r#"
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
[test] fn try_errdefer() void {
    g = 0;
    var r: i32 = wrapper(false) catch 0;
    expect_eq(r, 0);
    expect_eq(g, 2);
}
[test] fn try_defer_ok() void {
    g = 0;
    var r: i32 = wrapper(true) catch 0;
    expect_eq(r, 5);
    expect_eq(g, 1);
}
"#,
    );
}

#[test]
fn labeled_break_continue() {
    // 带标签 break/continue：跳出/跳到外层标签循环（标签跨多层循环定位）。
    assert_all_pass(
        r#"
[test] fn labeled_break_outer() void {
    var s: i32 = 0;
    :outer while (true) {
        var j: i32 = 0;
        while (j < 10) {
            j += 1;
            if (j == 2) { break :outer; }
            s += j;
        }
    }
    expect_eq(s, 1);
}
[test] fn labeled_continue_self() void {
    var s: i32 = 0;
    :outer for (0..3) |i| {
        if (i == 1) { continue :outer; }
        s += i;
    }
    expect_eq(s, 2);
}
[test] fn labeled_continue_nested() void {
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
    expect_eq(s, 230);
}
"#,
    );
}

#[test]
fn labeled_break_runs_loop_defers() {
    // 带标签 break 排空目标循环体内（含嵌套作用域）的 defers。
    assert_all_pass(
        r#"
global g: i32 = 0;
fn bump() void { g += 1; }
[test] fn labeled_break_defers() void {
    g = 0;
    :outer while (true) {
        defer bump();
        break :outer;
    }
    expect_eq(g, 1);
}
"#,
    );
}

// ---------- Phase 7 全核心标准库双模式一致 ----------

#[test]
fn p7_sort_binary_search_and_scalar() {
    // 自由内建：sort（就地重排）/ binary_search（Opt 结果）/ min/max/sqrt
    assert_all_pass(
        r#"
[test] fn sort_arr() !void {
    var v = [3, 1, 2];
    v.append(0);
    sort(v);
    try expect_eq(v[0], 0);
    try expect_eq(v[1], 1);
    try expect_eq(v[2], 2);
    try expect_eq(v[3], 3);
    var found = binary_search(v, 2).?;
    try expect_eq(found, 2);
    try expect_eq(binary_search(v, 99), null);
}
[test] fn scalar_tools() !void {
    try expect_eq(min(3, 9), 3);
    try expect_eq(max(3, 9), 9);
    try expect_eq(sqrt(9), 3.0);
    try expect_eq(7.add(3), 10);
    try expect_eq(7.div(2), 3);
    try expect_eq(2.pow(8), 256);
}
"#,
    );
}

#[test]
fn p7_math_builtins() {
    // math 命名空间（对齐 oracle call_math interp.rs:4922-4960）：
    // nan/inf/inf_neg 忽略类型名参数；sqrt/abs/pow/floor/ceil/round 取 arg[0]，
    // Int 强制 f64 后计算返回 Float。pow 在 oracle 为 `f.powf(2.0)`（单参平方）。
    assert_all_pass(
        r#"
[test] fn math_special_values() !void {
    var nan = math.nan(f64);
    try expect(nan != nan);   // NaN 不等于自身
    var inf = math.inf(f32);
    try expect(inf > 1.0e30);
    var inf_neg = math.inf_neg(f64);
    try expect(inf_neg < -1.0e30);
}
[test] fn math_numeric() !void {
    try expect_eq(math.sqrt(4.0), 2.0);
    try expect_eq(math.abs(-3.5), 3.5);
    try expect_eq(math.pow(3.0), 9.0);      // 3² = 9
    try expect_eq(math.floor(2.7), 2.0);
    try expect_eq(math.ceil(2.2), 3.0);
    try expect_eq(math.round(2.5), 3.0);
    try expect_eq(math.sqrt(9), 3.0);        // Int 实参强制 f64
}
"#,
    );
}

#[test]
fn p7_map_json_csv_and_string() {
    // Map（from_json/put/get/len/iter）、json.parse、csv.parse、字符串方法族
    assert_all_pass(
        r#"
[test] fn map_ops() !void {
    var m = Map.from_json("{\"a\":1,\"b\":2}");
    m.put("c", 3);
    try expect_eq(m.get("a").?, 1);
    try expect_eq(m.len(), 3);
    var s: i32 = 0;
    for (m.iter()) |kv| { s += @intCast(i32, kv.value); }
    try expect_eq(s, 6);
}
[test] fn json_csv() !void {
    var parsed = json.parse("{\"x\":42}");
    try expect_eq(parsed.get("x").?, 42);
    var rows = csv.parse("a,b\n1,2");
    try expect_eq(rows.len(), 2);
}
[test] fn string_methods() !void {
    var name = "hello,world";
    var parts = name.split(',');
    try expect_eq(parts[0], "hello");
    try expect_eq(name.replace("hello", "hi"), "hi,world");
    try expect_eq(name.substring(0, 5), "hello");
    try expect_eq(name.find(111).?, 4);
    try expect_eq(name.concat("!"), "hello,world!");
}
"#,
    );
}

#[test]
fn p7_serialize_namespace() {
    // D2：serialize 命名空间（parse_int/parse_float/json/csv/parser 辅助组）双后端一致
    assert_all_pass(
        r#"
[test] fn serialize_parse() !void {
    try expect_eq(serialize.parse_int("42") orelse -1, 42);
    try expect_eq(serialize.parse_float("3.5") orelse -1.0, 3.5);
    var obj = serialize.json.parse("{\"a\":1}");
    try expect_eq(obj.get("a").?, 1);
    var rows = serialize.csv.parse("x,y\n1,2");
    try expect_eq(rows.len, 2);
}
[test] fn serialize_helpers() !void {
    var data: &[u8] = "  42,";
    var pos: usize = 0;
    serialize.skip_space(data, &pos);
    try expect_eq(pos, 2);
    var c = serialize.peek(data, &pos) orelse return error.End;
    try expect_eq(c, '4');
    try expect_eq(serialize.is_digit(c), true);
    var n = serialize.parse_number(data, &pos);
    try expect_eq(n, 42);
    serialize.expect(data, &pos, ',') catch return error.Token;
    try expect_eq(pos, 5);
}
"#,
    );
}

#[test]
fn p7_alloc_and_at_builtins() {
    // @ 内建（sizeOf/intCast/typeOf/intFromEnum/enumFromInt 往返）、box 指针
    assert_all_pass(
        r#"
enum Status { ready, busy }
[test] fn at_builtins() !void {
    try expect_eq(@sizeOf(i32), 4);
    try expect_eq(@intCast(i32, 7), 7);
    try expect_eq(@typeOf(42), "i128");
    var k = Status.busy;
    try expect_eq(@intFromEnum(k), 1);
    var k2 = @enumFromInt(Status, 0);
    try expect_eq(@intFromEnum(k2), 0);
}
[test] fn box_read() !void {
    var p = box(42);
    try expect_eq(p.*, 42);
}
[test] fn mut_ptr_write() !void {
    var mut x: i32 = 42;
    var p = &mut x;
    try expect_eq(p.*, 42);
    p.* = 7;
    try expect_eq(p.*, 7);
}
"#,
    );
}

// ---------- B1/B3：io.print 格式说明符（双模式一致） ----------

#[test]
fn b1_print_format_specifiers() {
    // {d}/{X}/{e}/宽度/对齐/精度——双模式执行无错（输出内容由 05-format 示例验证）
    assert_all_pass(
        r#"
[test] fn format_specs() !void {
    io.print("{d}\n", 255);
    io.print("{X}\n", 255);
    io.print("{:.2}\n", 3.14159);
    io.print("{:8}\n", 42);
    io.print("{:<6}\n", "hi");
    io.print("{e}\n", 3.14);
}
"#,
    );
}

#[test]
fn b2_unknown_format_specifier_fails_both() {
    // B2：未知说明符 → FormatError（不再按字面量静默输出），双模式一致失败
    let (tw, ir) = assert_consistent(
        r#"
[test] fn bad_spec() !void {
    io.print("{q}\n", 1);
}
"#,
    );
    assert_eq!(tw, 0, "tree-walking 未知说明符应失败: {tw}");
    assert_eq!(ir, 0, "IR 未知说明符应失败: {ir}");
}

// ---------- E2：arena.init(T) typed 构造（双模式一致） ----------

#[test]
fn e1_arena_init_typed() {
    // arena.init(T) 类型名 / arena.init(T{...}) 字面量：字段值 + bump 记账
    // （堆上 class = 指针宽 8；二次分配对齐到 16 处切 → 8 + 16 = 24）
    assert_all_pass(
        r#"
class Node {
    mut x: i32,
    mut y: i32,
}
[test] fn arena_init_default() !void {
    var arena = Arena.init(alloc);
    var node = arena.init(Node);
    try expect_eq(node.x, 0);
    try expect_eq(node.y, 0);
    try expect_eq(arena.bytes(), 8);
    try expect_eq(arena.blocks(), 1);
}
[test] fn arena_init_literal() !void {
    var arena = Arena.init(alloc);
    var node = arena.init(Node{ x = 1, y = 2 });
    try expect_eq(node.x, 1);
    try expect_eq(node.y, 2);
    try expect_eq(arena.bytes(), 8);
    var node2 = arena.init(Node{ x = 3, y = 4 });
    try expect_eq(node2.x, 3);
    try expect_eq(node2.y, 4);
    try expect_eq(arena.bytes(), 24);
}
"#,
    );
}

#[test]
fn e2_arena_init_after_deinit_fails_both() {
    // deinit 后 arena.init → ArenaDeinitialized，双模式一致失败
    let (tw, ir) = assert_consistent(
        r#"
class Node { mut x: i32 }
[test] fn bad() !void {
    var arena = Arena.init(alloc);
    arena.deinit();
    var node = arena.init(Node);
}
"#,
    );
    assert_eq!(tw, 0, "tree-walking deinit 后 init 应失败: {tw}");
    assert_eq!(ir, 0, "IR deinit 后 init 应失败: {ir}");
}

#[test]
fn g4b_thread_lifecycle_consistent() {
    // 组 G 线程（G4b，定案 A）：interp == IR 双模式一致——spawn/join 返回值、
    // is_done 状态迁移、cancel→Cancelled、detach 立即运行副作用、值复制捕获。
    // 全部为确定性子集（无逃逸引用/无未 join 提升），共享 IrRuntime 跨 test fn 安全。
    // 原生为 out-of-subset（hc-tools/tests/native.rs g4b_thread_spawn_aborts_notcallable）。
    assert_all_pass(
        r#"
fn add(a: i32, b: i32) i32 { return a + b; }
fn bump(v: i32) i32 { return v + 1; }
[test] fn spawn_join_value() void {
    var th = spawn(add, 6, 7);
    expect_eq(th.is_done(), false);
    var r = th.join();
    expect_eq(r, 13);
    expect_eq(th.is_done(), true);
}
[test] fn spawn_value_capture() void {
    var base: i32 = 41;
    var th = spawn(bump, base);
    var r = th.join();
    expect_eq(r, 42);
}
[test] fn cancel_then_join() void {
    var th = spawn(add, 1, 2);
    th.cancel();
    var r = th.join();
    expect_error(error.Cancelled, r);
    expect_eq(th.is_done(), true);
}
global g: i32 = 0;
fn bump_g() void { g = g + 1; }
[test] fn detach_runs_side_effect() void {
    var th = spawn(bump_g);
    th.detach();
    expect_eq(g, 1);
    expect_eq(th.is_done(), true);
}
"#,
    );
}

#[test]
fn e2_async_await_consistent() {
    // 组 E E2：await ≡ join()——interp（lazy Future，await 运行体）== IR（async fn 调用
    // 同步执行 + await 透传）在纯函数下结果一致：await 返回值、内联 await、嵌套 await。
    // 副作用时序（体运行于调用点 vs await 点）与取消为 interp 特有，不进一致性（IR 子集
    // 边界，E4 原生异步落地后对齐；hc-rt/tests/async.rs 覆盖 interp 完整语义）。
    assert_all_pass(
        r#"
async fn add(a: i32, b: i32) i32 { return a + b; }
async fn inner(n: i32) i32 { return n * 2; }
async fn outer(n: i32) i32 { return await inner(n) + 1; }
[test] fn async_await_value() void {
    var fut = add(3, 4);
    expect_eq(await fut, 7);
    expect_eq(await add(1, 2), 3);
    expect_eq(await outer(10), 21);
}
"#,
    );
}

#[test]
fn e4_async_pointer_capture_consistent() {
    // 组 E E4：async fn 指针参数 + await（示例 37/76 `async_scope_binding` 模式）——
    // interp（lazy，&base 捕获 + Future<i32>）== IR（eager 同步执行 + await 透传）在纯
    // 函数下结果一致；IR 侧经 37/76 示例 compile 模式实证可运行。
    assert_all_pass(
        r#"
async fn async_add(b: *i32, n: i32) i32 { return b.* + n; }
[test] fn async_scope_binding() void {
    var base = 10;
    var fut: Future<i32> = async_add(&base, 5);
    expect_eq(await fut, 15);
}
"#,
    );
}

#[test]
fn d35_comptime_array_type_fn_consistent() {
    // 组 D（示例 35）：comptime_int 值参数 + 数组类型函数
    // `fn ArrayLen(T: type, n: comptime_int) type { return [n]T; }`——`ArrayLen<i32, 3>`
    // 即 `[3]i32`。interp == IR 双模式一致：类型应用初始化、`.len`、anytype 运行时函数。
    assert_all_pass(
        r#"
fn ArrayLen(T: type, n: comptime_int) type {
    return [n]T;
}
fn max_value(a: anytype, b: anytype) anytype {
    if (a > b) { return a; }
    return b;
}
[test] fn array_type_fn() void {
    var arr: ArrayLen<i32, 3> = [1, 2, 3];
    expect_eq(arr.len, 3);
    expect_eq(arr[1], 2);
}
[test] fn comptime_int_scaled() void {
    var arr: ArrayLen<f64, 2> = [0.5, 1.5];
    expect(arr.len == 2);
}
[test] fn anytype_runtime_fn() void {
    expect_eq(max_value(3, 7), 7);
    expect(max_value(2.5, 1.5) > 2.49 and max_value(2.5, 1.5) < 2.51);
}
"#,
    );
}

#[test]
fn d1_comptime_type_application_consistent() {
    // 组 D（E1.2）：comptime 类型函数——`fn Pair(T: type) type` 惰性具体化。
    // interp == IR 双模式一致：类型应用构造、字段读写、`return T;` 透传别名、
    // 普通 anytype 运行时函数（非类型函数）。
    assert_all_pass(
        r#"
fn Pair(T: type) type { return struct { first: T, second: T }; }
fn Identity(T: type) type { return T; }
fn max_value(a: anytype, b: anytype) anytype {
    if (a > b) { return a; }
    return b;
}
[test] fn type_application() void {
    var p: Pair<i32> = Pair<i32>{ first = 1, second = 2 };
    expect_eq(p.first, 1);
    expect_eq(p.second, 2);
    p.second = 5;
    expect_eq(p.second, 5);
}
[test] fn passthrough_alias() void {
    var x: Identity<i32> = 42;
    expect_eq(x, 42);
}
[test] fn anytype_runtime_fn() void {
    expect_eq(max_value(3, 5), 5);
    expect_eq(max_value(3.5, 2.5), 3.5);
}
"#,
    );
}

#[test]
fn d4b_anytype_concrete_consistent() {
    // 组 D D4b：anytype 完整语义——调用点按实参具体类型实例化，`anytype` 返回类型
    // 解析为具体类型（f64 / i32 实例）。interp == IR 双模式一致：类型具体化不改变
    // 运行时动态分派结果（值携带类型），跨 f64/i32/异构场景一致。
    assert_all_pass(
        r#"
fn max_value(a: anytype, b: anytype) anytype {
    if (a > b) { return a; }
    return b;
}
fn pick_i(a: anytype, b: anytype) anytype {
    return if (a < b) a else b;
}
[test] fn float_instance() void {
    var m: f64 = max_value(2.5, 1.5);
    expect_eq(m, 2.5);
}
[test] fn int_instance() void {
    var n: i32 = max_value(3, 7);
    expect_eq(n, 7);
}
[test] fn mixed_instances() void {
    expect_eq(max_value(10, 20), 20);
    expect_eq(max_value(1.5, 0.5), 1.5);
    expect_eq(pick_i(4, 9), 4);
}
"#,
    );
}

#[test]
fn d3_nested_instantiation_consistent() {
    // 组 D D3：类型函数嵌套实例化——`PairPair<i32>` 字段类型在内层登记后为
    // 具体化键 `Pair<@i32>`。interp == IR 双模式一致：嵌套 NamedLit 构造、字段
    // 读写、声明式无初值（IR `lower_default_value` 惰性具体化，防 `__none__` 损坏）。
    assert_all_pass(
        r#"
fn Pair(T: type) type { return struct { first: T, second: T }; }
fn PairPair(T: type) type { return struct { a: Pair<T>, b: Pair<T> }; }
[test] fn nested_literal() void {
    var pp: PairPair<i32> = PairPair<i32>{
        a = Pair<i32>{ first = 1, second = 2 },
        b = Pair<i32>{ first = 3, second = 4 },
    };
    expect_eq(pp.a.first, 1);
    expect_eq(pp.a.second, 2);
    expect_eq(pp.b.first, 3);
    expect_eq(pp.b.second, 4);
    expect_eq(pp.a.first + pp.b.second, 5);
    pp.b.second = 10;
    expect_eq(pp.a.first + pp.b.second, 11);
}
[test] fn nested_no_init() void {
    var x: PairPair<i32>;
    expect_eq(x.a.first, 0);
    expect_eq(x.b.second, 0);
}
"#,
    );
}

#[test]
fn d3_recursive_instantiation_consistent() {
    // 组 D D3：递归/自引用类型函数——`LinkedList(T) { value: T, next: ?LinkedList(T) }`。
    // 登记期经 `instantiating` 守卫把字段内自引用替换为自身具体化键（叶）；运行时
    // Optional 字段默认 `None` 终止（`next = null` / 无初值构造不递归）。
    assert_all_pass(
        r#"
fn LinkedList(T: type) type { return struct { value: T, next: ?LinkedList<T> }; }
[test] fn recursive_literal() void {
    var l: LinkedList<i32> = LinkedList<i32>{ value = 1, next = null };
    expect_eq(l.value, 1);
    expect_eq(l.next, null);
}
[test] fn recursive_no_init() void {
    var l: LinkedList<i32>;
    expect_eq(l.value, 0);
    expect_eq(l.next, null);
}
"#,
    );
}

#[test]
fn d5_comptime_value_fn_consistent() {
    // 组 D D5：comptime 值函数（参数含 `T: type`、非返回 `type` 的普通函数）运行时
    // 调用点**三后端一致折叠**——interp `try_comptime_value_call` 与 IR `value_fns`
    // 常量求值产生同一常量，IR 中无类型值/调用残留（类型值仅编译期存在）。
    // 覆盖：纯类型实参、类型 + 值混合实参、体常量表达式（if 分支/比较/混合算术）。
    assert_all_pass(
        r#"
fn array_len(T: type) comptime_int {
    return 4;
}
fn byte_size(T: type, n: comptime_int) comptime_int {
    if (n > 4) { return n + 1; }
    return n * 2;
}
fn pick(T: type, a: comptime_int, b: comptime_int) comptime_int {
    return if (a < b) a else b;
}
[test] fn type_only() void {
    var n: comptime_int = array_len(i32);
    expect_eq(n, 4);
}
[test] fn mixed_params() void {
    var m: comptime_int = byte_size(f64, 7);
    expect_eq(m, 8);
    var l: comptime_int = byte_size(Vec<i32>, 3);
    expect_eq(l, 6);
}
[test] fn if_branch_fold() void {
    expect_eq(pick(i32, 3, 9), 3);
    expect_eq(pick(String, 9, 3), 3);
}
"#,
    );
}

// ---------- 组 G 标准库（G1-G5）interp == IR 一致性 ----------
// 覆盖 Task #50「IR 后端同步 G1-G5 模块」的确定性可复原子集。原则：
//   - 纯函数（io.text / io.archive / to_upper / rng 种子流）直接比对；
//   - 有副作用的（io.net 回环 / io.ipc 管道与共享内存 / io.storage 文件 / io.fs 目录）
//     各自创建并自清理，interp 与 IR 在同一进程内先后执行不互相污染；
//   - 时间（io.time）只用单调不变量（PASS/FAIL 确定，值不比对）。

#[test]
fn g1_udp_loopback_consistent() {
    // G1 net：UDP 双 socket 回环——bind(0) 取临时端口 → send_to 对端地址 → recv_from
    // 取 [addr, data]。interp == IR 双模式一致（fmt_int + String.concat 拼对端地址）。
    assert_all_pass(
        r#"
[test] fn t() !void {
    var s1 = try io.net.udp.bind(0);
    var s2 = try io.net.udp.bind(0);
    defer s1.close();
    defer s2.close();
    var p1 = try s1.local_port();
    try s2.send_to("127.0.0.1:".concat(fmt_int(p1)), "ping-udp");
    var r = try s1.recv_from(alloc);
    try expect_eq_slices(r[1], "ping-udp");
}
"#,
    );
}

#[test]
fn g1_tcp_echo_consistent() {
    // G1 net：TCP 回环 echo——listen(0) → connect → accept → write → shutdown → read_all。
    // 无固定端口（ephemeral），interp 与 IR 各自建监听/连接，互不干扰。
    assert_all_pass(
        r#"
[test] fn t() !void {
    var listener = try io.net.listen("127.0.0.1", 0, alloc);
    var port = try listener.local_port();
    var conn = try io.net.connect("127.0.0.1", port, alloc);
    var accepted = try listener.accept();
    try accepted.write("hello-net");
    accepted.shutdown();
    var reply = try conn.read_all();
    try expect_eq_slices(reply, "hello-net");
    conn.close();
    accepted.close();
    listener.close();
}
"#,
    );
}

#[test]
fn g2_str_upper_lower_consistent() {
    // G2 io 差异项：String.to_upper/to_lower——ASCII 大小写转换（纯函数，非 ASCII 不变）。
    assert_all_pass(
        r#"
[test] fn t() !void {
    try expect_eq_slices("HeLLo 123".to_upper(), "HELLO 123");
    try expect_eq_slices("HeLLo 123".to_lower(), "hello 123");
    try expect_eq_slices("abc\xE9".to_upper(), "ABC\xE9");
}
"#,
    );
}

#[test]
fn g2_fs_list_dir_and_open_dir_consistent() {
    // G2 io 差异项：io.fs.list_dir（路径形态）→ Vec(DirEntry)（name/is_dir）；
    // io.fs.open_dir !Dir → dir.list_dir(alloc) → dir.close()。目录由 Rust 侧预建，
    // interp/IR 双模式只读同一目录，测试后清理。
    fs::create_dir_all("cons_g2_list").unwrap();
    fs::write("cons_g2_list/alpha.txt", b"").unwrap();
    assert_all_pass(
        r#"
[test] fn list_path() void {
    var entries = io.fs.list_dir("cons_g2_list");
    try expect(entries.len == 1);
    try expect_eq_slices(entries[0].name, "alpha.txt");
    try expect(!entries[0].is_dir);
}
[test] fn open_dir_handle() void {
    var dir = try io.fs.open_dir("cons_g2_list");
    var entries = try dir.list_dir(alloc);
    try expect(entries.len == 1);
    try expect_eq_slices(entries[0].name, "alpha.txt");
    try dir.close();
}
"#,
    );
    let _ = fs::remove_file("cons_g2_list/alpha.txt");
    let _ = fs::remove_dir("cons_g2_list");
}

#[test]
fn g3_pipe_write_read_consistent() {
    // G3 ipc：匿名管道——write → read 排空 → 再读空切片（进程内协作式，无持久状态）。
    assert_all_pass(
        r#"
[test] fn t() !void {
    var pair = try io.ipc.pipe();
    var reader = pair[0];
    var writer = pair[1];
    try writer.write("hello-ipc");
    var data = try reader.read(alloc);
    try expect_eq_slices(data, "hello-ipc");
    var empty = try reader.read(alloc);
    try expect(empty.len == 0);
    try reader.close();
    try writer.close();
}
"#,
    );
}

#[test]
fn g3_shm_write_read_consistent() {
    // G3 ipc：命名共享内存——write 覆盖截断到 size → read 取当前内容。Shm 注册表
    // 按后端实例隔离（interp 与 IR 各自持 registry），互不污染。
    assert_all_pass(
        r#"
[test] fn t() !void {
    var s = try io.ipc.shm("cons_g3_shm", 8);
    try s.write("hi");
    var data = try s.read(alloc);
    try expect_eq_slices(data, "hi");
    try s.close();
    var s2 = try io.ipc.shm("cons_g3_shm2", 4);
    try s2.write("0123456789");
    try expect_eq_slices(try s2.read(alloc), "0123");
    try s2.close();
}
"#,
    );
}

#[test]
fn g4_storage_kv_consistent() {
    // G4 storage：文件持久化键值存储——put → get / 缺失 → null / len / contains，
    // 测试自清理（close 落盘后 io.fs.remove 删文件），interp/IR 各建各删不互扰。
    assert_all_pass(
        r#"
[test] fn t() !void {
    var kv = try io.storage.open("cons_g4_kv.dat");
    try kv.put("name", "hank");
    var v = try kv.get("name");
    try expect_eq_slices(v orelse "", "hank");
    var miss = try kv.get("nope");
    try expect_eq_slices(miss orelse "default", "default");
    try expect_eq(kv.len(), 1);
    try expect_eq(kv.contains("name"), true);
    try expect_eq(kv.contains("z"), false);
    try kv.close();
    try io.fs.remove("cons_g4_kv.dat");
}
"#,
    );
}

#[test]
fn g4_archive_roundtrip_consistent() {
    // G4 archive：RLE compress/decompress round-trip + 非法数据 error.InvalidFormat。
    assert_all_pass(
        r#"
[test] fn t() !void {
    var c = try io.archive.compress("aaabbbccccc");
    try expect(c.len < 11);
    try expect_eq_slices(try io.archive.decompress(c), "aaabbbccccc");
    try expect_eq_slices(try io.archive.decompress(try io.archive.compress("\x00\x01\x02\x03")), "\x00\x01\x02\x03");
    try expect_error(error.InvalidFormat, io.archive.decompress("\x00"));
}
"#,
    );
}

#[test]
fn g5_text_ops_consistent() {
    // G5 text：正则子集 matches/find/replace/split + 非法模式 error.InvalidFormat。
    assert_all_pass(
        r#"
[test] fn t() !void {
    try expect_eq(io.text.matches("^hello", "hello world"), true);
    try expect_eq(io.text.matches("^world", "hello world"), false);
    try expect_eq(io.text.find("world", "hello world") orelse -1, 6);
    try expect_eq(io.text.find("xyz", "hello world") orelse -1, -1);
    try expect_eq_slices(io.text.replace("\\s+", "a   b", "-"), "a-b");
    try expect_eq_slices(io.text.replace("\\d+", "item-42-x7", "[n]"), "item-[n]-x[n]");
    var parts = io.text.split(",", "a,b,c");
    try expect_eq(parts.len(), 3);
    try expect_eq_slices(parts[0], "a");
    try expect_eq_slices(parts[2], "c");
    try expect_error(error.InvalidFormat, io.text.matches("(", "x"));
}
"#,
    );
}

#[test]
fn g5_rng_determinism_consistent() {
    // G5 rng：xorshift64* 种子流确定（seed(1) → 固定首值）；seed 可重置复现。
    assert_all_pass(
        r#"
[test] fn t() !void {
    io.rng.seed(1);
    var a1 = io.rng.next();
    try expect_eq(a1, 0xbafacf624f01c45d);
    var a2 = io.rng.next();
    io.rng.seed(1);
    try expect_eq(io.rng.next(), a1);
    try expect_eq(io.rng.next(), a2);
    var i = 0;
    while (i < 50) {
        var v = io.rng.int(10);
        try expect(v >= 0);
        try expect(v < 10);
        i += 1;
    }
}
"#,
    );
}

#[test]
fn g5_time_monotonic_consistent() {
    // G5 time：tick 单调 / elapsed 自 tick 起毫秒 ≥ 0（PASS/FAIL 确定，值不比对）。
    assert_all_pass(
        r#"
[test] fn t() !void {
    var t0 = io.time.tick();
    try expect(t0 > 0);
    var dt = io.time.elapsed(t0);
    try expect(dt >= 0);
    try expect(dt < 100000);
}
"#,
    );
}

#[test]
/// 组 F：@atomic 内建——interp == IR 双模式一致。
/// store/load 写穿读回、Rmw add/sub/exchange 返回旧值。
fn f_atomic_consistent() {
    assert_all_pass(
        r#"
[test] fn atomic_store_load_rmw() !void {
    var x: i64 = 42;
    @atomicStore(i64, &x, 7, .seq_cst);
    try expect_eq(@atomicLoad(i64, &x, .acquire), 7);
    var old = @atomicRmw(i64, &x, .add, 5, .seq_cst);
    try expect_eq(old, 7);
    try expect_eq(@atomicLoad(i64, &x, .seq_cst), 12);
    old = @atomicRmw(i64, &x, .sub, 2, .seq_cst);
    try expect_eq(old, 12);
    old = @atomicRmw(i64, &x, .exchange, 100, .seq_cst);
    try expect_eq(old, 10);
    try expect_eq(@atomicLoad(i64, &x, .seq_cst), 100);
}
"#,
    );
}

#[test]
/// D2-1：Table multi-index 一致性——init / 多参索引 / 多参写入 / len
fn d2_table_multi_index_consistent() {
    assert_all_pass(
        r#"
[test] fn t() !void {
    var t = Table<i32>.init(alloc, 2, 3, 7);
    try expect_eq(t.len(), 2);
    try expect_eq(t[0, 0], 7);
    try expect_eq(t[0, 1], 7);
    try expect_eq(t[1, 2], 7);
    t[1, 1] = 42;
    try expect_eq(t[1, 1], 42);
    t[0, 2] = 99;
    try expect_eq(t[0, 2], 99);
}
"#,
    );
}

#[test]
/// D2-2：Vec 操作一致性——init / append / index / len / index write
fn d2_vec_operations_consistent() {
    assert_all_pass(
        r#"
[test] fn t() !void {
    var v = Vec<i32>.init(alloc);
    try expect_eq(v.len(), 0);
    v.append(10);
    v.append(20);
    v.append(30);
    try expect_eq(v.len(), 3);
    try expect_eq(v[0], 10);
    try expect_eq(v[1], 20);
    try expect_eq(v[2], 30);
    v[1] = 99;
    try expect_eq(v[1], 99);
}
"#,
    );
}

#[test]
/// D2-2：Map 操作一致性——init / put / get / len
fn d2_map_operations_consistent() {
    assert_all_pass(
        r#"
[test] fn t() !void {
    var m = Map<i32, i32>.init(alloc);
    try expect_eq(m.len(), 0);
    m.put(1, 100);
    m.put(2, 200);
    m.put(3, 300);
    try expect_eq(m.len(), 3);
    try expect_eq(m.get(1).?, 100);
    try expect_eq(m.get(2).?, 200);
    try expect_eq(m.get(3).?, 300);
    // 覆盖不存在的键
    try expect(m.get(99) == null);
}
"#,
    );
}

#[test]
/// D2-2：Deque 操作一致性——init / pushFirst / pushLast / popFirst / popLast / len
fn d2_deque_operations_consistent() {
    assert_all_pass(
        r#"
[test] fn t() !void {
    var d = Deque<i32>.init(alloc);
    try expect_eq(d.len, 0);
    d.push_back(10);
    d.push_back(20);
    d.push_front(30);
    try expect_eq(d.len, 3);
    try expect_eq(d.pop_front().?, 30);
    try expect_eq(d.pop_front().?, 10);
    try expect_eq(d.pop_back().?, 20);
    try expect_eq(d.len, 0);
}
"#,
    );
}

#[test]
/// D1-4：线程模式测试——`[test(thread)]` 在独立 OS 线程中执行
fn d1_thread_test_runner() {
    assert_all_pass(
        r#"
[test(thread)] fn t() !void {
    try expect_eq(1 + 1, 2);
}
"#,
    );
}

#[test]
/// D2-2：String 操作一致性——concat / len / find / substring / replace / split
fn d2_string_operations_consistent() {
    assert_all_pass(
        r#"
[test] fn t() !void {
    var s = "hello, world";
    try expect_eq(s.len, 12);
    // concat
    var c = s.concat("!");
    try expect_eq(c.len, 13);
    try expect_eq_slices(c, "hello, world!");
    // find
    var idx = s.find("world");
    try expect_eq(idx.?, 7);
    try expect(s.find("xyz") == null);
    // substring
    var sub = s.substring(0, 5);
    try expect_eq_slices(sub, "hello");
    var sub2 = s.substring(7, 12);
    try expect_eq_slices(sub2, "world");
    // replace
    var r = s.replace("world", "rust");
    try expect_eq_slices(r, "hello, rust");
    // split
    var parts = s.split(", ");
    try expect_eq(parts.len, 2);
    try expect_eq_slices(parts[0], "hello");
    try expect_eq_slices(parts[1], "world");
}
"#,
    );
}

#[test]
/// D1-3：异步测试模式——`[test(async)]` 基本执行
fn d1_async_test_runner() {
    assert_all_pass(
        r#"
[test(async)] fn t() !void {
    try expect_eq(1 + 1, 2);
}
"#,
    );
}

#[test]
/// D1-2：序列测试超时——`[test(timeout=1)]` 基本执行（超时阈值内完成）
fn d1_serial_timeout_test() {
    assert_all_pass(
        r#"
[test(timeout=1)] fn t() !void {
    try expect_eq(3 + 3, 6);
}
"#,
    );
}

#[test]
/// A6 标准库数据结构：Bitmap 位图——interp == IR 双模式一致。
fn a6_bitmap_consistent() {
    assert_all_pass(
        r#"
[test] fn t() !void {
    var bm = try io.bitmap.init(200);
    try expect_eq(bm.len(), 256);
    try expect_eq(bm.get(0), false);
    try expect_eq(bm.count(), 0);
    bm.set(42);
    try expect_eq(bm.get(42), true);
    try expect_eq(bm.get(41), false);
    try expect_eq(bm.count(), 1);
    bm.set(0);
    bm.set(63);
    bm.set(100);
    try expect_eq(bm.count(), 4);
    bm.clear(42);
    try expect_eq(bm.get(42), false);
    try expect_eq(bm.count(), 3);
    bm.set(200);
    try expect_eq(bm.get(200), true);
    try expect_eq(bm.len(), 256);
}
"#,
    );
}

#[test]
/// A6 标准库数据结构：RingBuf 环形缓冲——interp == IR 双模式一致。
fn a6_ringbuf_consistent() {
    assert_all_pass(
        r#"
[test] fn t() !void {
    var rb = try io.ringbuf.init(5);
    try expect_eq(rb.len(), 0);
    try expect_eq(rb.capacity(), 5);
    try expect_eq(rb.is_empty(), true);
    try expect_eq(rb.is_full(), false);
    try expect_eq(rb.push(42), true);
    try expect_eq(rb.push(99), true);
    try expect_eq(rb.len(), 2);
    try expect_eq(rb.pop(), 42);
    try expect_eq(rb.pop(), 99);
    try expect_eq(rb.is_empty(), true);
    try expect_eq(rb.pop(), null);
    try expect_eq(rb.push(1), true);
    try expect_eq(rb.push(2), true);
    try expect_eq(rb.is_full(), false);
    try expect_eq(rb.push(3), true);
    try expect_eq(rb.push(4), true);
    try expect_eq(rb.push(5), true);
    try expect_eq(rb.is_full(), true);
    try expect_eq(rb.push(6), false);
    rb.clear();
    try expect_eq(rb.len(), 0);
}
"#,
    );
}

#[test]
/// A6 标准库数据结构：PageMem 页内存池——interp == IR 双模式一致。
fn a6_pagemem_consistent() {
    assert_all_pass(
        r#"
[test] fn t() !void {
    var pm = try io.pagemem.init(5);
    try expect_eq(pm.total(), 5);
    try expect_eq(pm.available(), 5);
    var a = pm.alloc();
    try expect_eq(a, 0);
    var b = pm.alloc();
    try expect_eq(b, 1);
    try expect_eq(pm.available(), 3);
    pm.free(a);
    try expect_eq(pm.available(), 4);
    var c = pm.alloc();
    try expect_eq(c, a);
    try expect_eq(pm.alloc(), 2);
    try expect_eq(pm.alloc(), 3);
    try expect_eq(pm.alloc(), 4);
    try expect_eq(pm.alloc(), null);
    try expect_eq(pm.available(), 0);
    pm.free(100);
    try expect_eq(pm.available(), 0);
}
"#,
    );
}

#[test]
/// A6 标准库数据结构：IntrList 侵入式链表——interp == IR 双模式一致。
fn a6_intrlist_consistent() {
    assert_all_pass(
        r#"
[test] fn t() !void {
    var list = try io.intrlist.init();
    try expect_eq(list.len(), 0);
    try expect_eq(list.is_empty(), true);
    try expect_eq(list.pop_front(), null);
    try expect_eq(list.pop_back(), null);

    // push_front + pop_front
    var a = list.push_front(10);
    var b = list.push_front(20);
    var c = list.push_front(30);
    try expect_eq(list.len(), 3);
    try expect_eq(list.pop_front(), 30);
    try expect_eq(list.pop_front(), 20);
    try expect_eq(list.pop_front(), 10);
    try expect_eq(list.is_empty(), true);

    // push_back + pop_back
    var d = list.push_back(100);
    var e = list.push_back(200);
    try expect_eq(list.pop_back(), 200);
    try expect_eq(list.pop_back(), 100);
    try expect_eq(list.is_empty(), true);

    // push_front + pop_back (cross)
    list.push_front(1);
    list.push_front(2);
    list.push_front(3);
    try expect_eq(list.pop_back(), 1);
    try expect_eq(list.pop_back(), 2);
    try expect_eq(list.pop_back(), 3);

    // remove middle
    var x = list.push_back(10);
    var y = list.push_back(20);
    var z = list.push_back(30);
    try expect_eq(list.remove(y), 20);
    try expect_eq(list.len(), 2);
    try expect_eq(list.pop_front(), 10);
    try expect_eq(list.pop_front(), 30);

    // clear
    list.push_back(1);
    list.push_back(2);
    list.clear();
    try expect_eq(list.is_empty(), true);
    try expect_eq(list.pop_front(), null);

    // node reuse
    var na = list.push_back(42);
    var nb = list.push_back(99);
    try expect_eq(list.remove(na), 42);
    try expect_eq(list.remove(nb), 99);
    try expect_eq(list.len(), 0);
    var nc = list.push_back(77);
    try expect_eq(list.pop_front(), 77);
}
"#,
    );
}

#[test]
/// A6 标准库数据结构：TreeMap 有序映射——interp == IR 双模式一致。
fn a6_treemap_consistent() {
    assert_all_pass(
        r#"
[test] fn t() !void {
    var map = try io.treemap.init();
    try expect_eq(map.len(), 0);
    try expect_eq(map.is_empty(), true);
    try expect_eq(map.get(42), null);
    try expect_eq(map.contains(42), false);

    // insert + get
    map.insert(10, 100);
    map.insert(20, 200);
    map.insert(30, 300);
    try expect_eq(map.len(), 3);
    try expect_eq(map.get(10), 100);
    try expect_eq(map.get(20), 200);
    try expect_eq(map.get(30), 300);
    try expect_eq(map.get(99), null);

    // update existing key
    map.insert(10, 999);
    try expect_eq(map.get(10), 999);
    try expect_eq(map.len(), 3);

    // contains
    try expect_eq(map.contains(10), true);
    try expect_eq(map.contains(30), true);
    try expect_eq(map.contains(99), false);

    // descending insert
    var map2 = try io.treemap.init();
    map2.insert(30, 300);
    map2.insert(20, 200);
    map2.insert(10, 100);
    try expect_eq(map2.len(), 3);
    try expect_eq(map2.get(30), 300);
    try expect_eq(map2.get(20), 200);
    try expect_eq(map2.get(10), 100);

    // clear
    map2.clear();
    try expect_eq(map2.is_empty(), true);
    try expect_eq(map2.len(), 0);
    try expect_eq(map2.get(10), null);

    // negative keys
    var map3 = try io.treemap.init();
    map3.insert(-5, 10);
    map3.insert(0, 20);
    map3.insert(5, 30);
    try expect_eq(map3.len(), 3);
    try expect_eq(map3.get(-5), 10);
    try expect_eq(map3.get(0), 20);
    try expect_eq(map3.get(5), 30);
}
"#,
    );
}
