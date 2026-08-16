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
    let module = lower(&program).unwrap();
    let mut tw_pass = 0usize;
    let mut ir_pass = 0usize;
    for (name, passed_tw) in &tw {
        let r = run_ir(&module, name, &[]);
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
namespace io {
    namespace net {
        fn double(x: i32) i32 { return x * 2; }
    }
}
[test] fn qualified() void {
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
