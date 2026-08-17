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
[continuous]
class Point {
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
