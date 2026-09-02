//! hc/tests/bytecode.rs
//!
//! 定义：枚举：Maybe

use hc::bytecode::{encode, run_bytecode};
use hc::ir::{lower, run_ir, IrError, IrValue, StringDataIr};
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
    let src = "fn cmp(a: i32, b: i32) bool { return a < b && b <= 3; }";
    assert_consistent(src, "cmp", &[IrValue::Int(1), IrValue::Int(3)]);
}

#[test]
fn short_circuit_and_or() {
    let src = r#"
fn expect_bad() bool {
    expect(false);
    return true;
}
fn and_sc() bool { return false && expect_bad(); }
fn or_sc() bool { return true || expect_bad(); }
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
        IrValue::String(StringDataIr::from_bytes(b"big".to_vec()))
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
    assert_eq!(
        run_bc(src, "fail", &[]).unwrap(),
        IrValue::Err {
            name: "NotFound".into(),
            code: 0
        }
    );
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
    assert_eq!(
        run_bc(src, "f", &[IrValue::Int(4)]).unwrap(),
        IrValue::Int(16)
    );
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
    assert_eq!(
        run_bc(src, "d", &[IrValue::Int(10)]).unwrap_err().name,
        "DivisionByZero"
    );
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
        run_bc(src, "f", &[IrValue::Int(i128::MAX), IrValue::Int(2)])
            .unwrap_err()
            .name,
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
fn phase3_switch_round_trip() {
    // MatchTest（opcode 31）+ 模式描述符 encode/decode：int/else
    let src = r#"
fn pick(x: i32) i32 {
    switch (x) {
        1 => return 10,
        2 => return 20,
        else => return 99,
    }
}
"#;
    assert_consistent(src, "pick", &[IrValue::Int(1)]);
    assert_consistent(src, "pick", &[IrValue::Int(2)]);
    assert_consistent(src, "pick", &[IrValue::Int(9)]);
    assert_eq!(
        run_bc(src, "pick", &[IrValue::Int(2)]).unwrap(),
        IrValue::Int(20)
    );
}

#[test]
fn phase3_switch_pattern_kinds_round_trip() {
    // 错误名 / 字符串 / bool / null 模式经 encode/decode 后一致
    let src = r#"
fn fail(x: i32) !i32 {
    if (x == 1) { return error.NotFound; }
    return error.Io;
}
fn pick(s: String) i32 {
    switch (s) {
        "a" => return 1,
        else => return 0,
    }
}
fn pb(b: bool) i32 {
    switch (b) { true => return 1, false => return 0, }
}
"#;
    assert_consistent(src, "fail", &[IrValue::Int(1)]);
    assert_consistent(
        src,
        "pick",
        &[IrValue::String(StringDataIr::from_bytes(b"a".to_vec()))],
    );
    assert_consistent(
        src,
        "pick",
        &[IrValue::String(StringDataIr::from_bytes(b"z".to_vec()))],
    );
    assert_consistent(src, "pb", &[IrValue::Bool(true)]);
    assert_consistent(src, "pb", &[IrValue::Bool(false)]);
}

#[test]
fn phase3_make_range_round_trip() {
    // MakeRange（opcode 32）：区间糖 → Arr
    let src = r#"
fn f() i32 {
    var mut s: i32 = 0;
    for (0..5) |i| { s += i; }
    return s;
}
"#;
    assert_consistent(src, "f", &[]);
    assert_eq!(run_bc(src, "f", &[]).unwrap(), IrValue::Int(10));
}

#[test]
fn phase3_enum_payload_round_trip() {
    // EnumPayload（opcode 33）：switch 捕获枚举负载
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
}
"#;
    assert_consistent(src, "t", &[]);
}

#[test]
fn phase3_iter_make_next_writeback_round_trip() {
    // IterMake/IterNext/IterWriteBack（opcode 34-36）：数组 mut 写回
    let src = r#"
fn f() i32 {
    var a = [1, 2, 3];
    for (a) |mut x| { x += 1; }
    return a[0] + a[1] + a[2];
}
"#;
    assert_consistent(src, "f", &[]);
    assert_eq!(run_bc(src, "f", &[]).unwrap(), IrValue::Int(9));
}

#[test]
fn phase3_iter_str_bytes_round_trip() {
    // 字符串迭代：字节 Int（is_ref=false 新单元，无写回）
    let src = r#"
fn f() i32 {
    var mut sum: i32 = 0;
    for ("abc") |b| { sum += b; }
    return sum;
}
"#;
    assert_consistent(src, "f", &[]);
    assert_eq!(run_bc(src, "f", &[]).unwrap(), IrValue::Int(294));
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

// ---------- Phase 4：闭包 / 函数引用 / 方法 / 动态调用 / 重载 ----------

#[test]
fn phase4_closure_round_trip() {
    // MakeClosure（opcode 37）+ 闭包表：读捕获共享 + move 捕获拷贝，经 encode/decode 一致
    let src = r#"
fn read_cap() i32 {
    var a = 10;
    var g = |v| v + a;
    a = 100;
    return g(5);
}
fn move_cap() i32 {
    var a = 10;
    var g = move |v| v + a;
    a = 100;
    return g(5);
}
"#;
    assert_consistent(src, "read_cap", &[]);
    assert_consistent(src, "move_cap", &[]);
    assert_eq!(run_bc(src, "read_cap", &[]).unwrap(), IrValue::Int(105));
    assert_eq!(run_bc(src, "move_cap", &[]).unwrap(), IrValue::Int(15));
}

#[test]
fn phase4_fnref_and_indirect_round_trip() {
    // FnRef（opcode 38）+ CallIndirect（opcode 39）
    let src = r#"
fn inc(v: i32) i32 { return v + 1; }
fn call_it() i32 {
    var f = inc;
    return f(41);
}
"#;
    assert_consistent(src, "call_it", &[]);
    assert_eq!(run_bc(src, "call_it", &[]).unwrap(), IrValue::Int(42));
}

#[test]
fn phase4_method_round_trip() {
    // CallMethod（opcode 40）：实例方法动态分派
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
    assert_consistent(src, "f", &[]);
    assert_eq!(run_bc(src, "f", &[]).unwrap(), IrValue::Int(12));
}

#[test]
fn phase4_overload_round_trip() {
    // func_index 一名多候选（重载）经 encode/decode 后按 arity 分派一致
    let src = r#"
fn sq(x: i32) i32 { return x * x; }
fn sq(x: i32, y: i32) i32 { return x * y; }
fn f() i32 {
    return sq(3) * 10 + sq(2, 4);
}
"#;
    assert_consistent(src, "f", &[]);
    assert_eq!(run_bc(src, "f", &[]).unwrap(), IrValue::Int(98));
}

#[test]
fn phase5_global_const_round_trip() {
    // LoadGlobal（opcode 41）/ StoreGlobal（opcode 42）+ 全局表经 encode/decode 往返，
    // 字节码 VM 与参考解释器一致（每次调用独立 runtime → 全局重新初始化）。
    let src = r#"
global counter: i32 = 0;
const BASE: i32 = 100;
fn bump() i32 {
    counter = counter + 1;
    return counter + BASE;
}
fn read() i32 { return counter; }
"#;
    assert_consistent(src, "bump", &[]);
    assert_consistent(src, "read", &[]);
    assert_eq!(run_bc(src, "bump", &[]).unwrap(), IrValue::Int(101));
    assert_eq!(run_bc(src, "read", &[]).unwrap(), IrValue::Int(0)); // 新 runtime 重新 init
}

#[test]
fn phase5_global_const_decl_order_round_trip() {
    // @__init__ 声明序：后声明 global 初始化引用先声明 global
    let src = r#"
global a: i32 = 5;
global b: i32 = a * 2;
fn read() i32 { return b + a; }
"#;
    assert_consistent(src, "read", &[]);
    assert_eq!(run_bc(src, "read", &[]).unwrap(), IrValue::Int(15));
}

#[test]
fn phase5_global_addr_round_trip() {
    // GlobalAddr（opcode 43）：`&mut global` 别名写穿经 encode/decode 后一致
    let src = r#"
global counter: i32 = 0;
fn bump() i32 {
    var p = &mut counter;
    p.* += 1;
    return p.*;
}
fn read() i32 { return counter; }
"#;
    assert_consistent(src, "bump", &[]);
    assert_consistent(src, "read", &[]);
    assert_eq!(run_bc(src, "bump", &[]).unwrap(), IrValue::Int(1));
    assert_eq!(run_bc(src, "read", &[]).unwrap(), IrValue::Int(0)); // 新 runtime 重新 init
}

// ---------- 组 G4a：线程生命周期字节码往返（零 opcode 改动——Thread 复用 Class） ----------

#[test]
fn spawn_join_round_trip() {
    // spawn/join：字节码 VM 与参考解释器一致（Class("Thread") 复用既有 class 语义）
    let src = r#"
fn add(a: i32, b: i32) i32 { return a + b; }
fn main() i32 {
    var th = spawn(add, 6, 7);
    return try th.join();
}
"#;
    assert_consistent(src, "main", &[]);
    assert_eq!(run_bc(src, "main", &[]).unwrap(), IrValue::Int(13));
}

#[test]
fn thread_is_done_round_trip() {
    // is_done 状态转移经字节码往返一致
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
    assert_consistent(src, "main", &[]);
    assert_eq!(run_bc(src, "main", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn thread_own_alloc_round_trip() {
    // Q8 每线程 alloc：worker 的 alloc.bytes() == 8，且不进全局泄漏跟踪
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
    assert_consistent(src, "main", &[]);
    assert_eq!(run_bc(src, "main", &[]).unwrap(), IrValue::Int(0));
}

#[test]
fn cancel_join_round_trip() {
    // cancel → join 可能返回 error.Cancelled 或正常值（OS 线程下存在竞态——
    // 线程可能在 cancel() 前已执行完毕）。两种结果均正确，本测试验证字节码
    // 往返一致（不崩溃、不静默丢弃）
    let src = r#"
fn work() i32 { return 42; }
fn main() void {
    var th = spawn(work);
    th.cancel();
    var r = th.join() catch 0;
    expect_eq(th.is_done(), true);
}
"#;
    assert_consistent(src, "main", &[]);
    assert_eq!(run_bc(src, "main", &[]).unwrap(), IrValue::Void);
}

#[test]
fn union_round_trip() {
    // K1 union（ADR-0014）：union 表 + UnionSync（opcode 48）经字节码往返一致
    let src = r#"
union Num {
    i: i32,
    f: f32,
    b: bool,
}
fn read_b(x: i32) bool {
    var n = Num{ i = x };
    return n.b;
}
fn main() i32 {
    if (read_b(1) != true) { return 1; }
    if (read_b(256) != false) { return 2; }
    return 0;
}
"#;
    assert_consistent(src, "main", &[]);
    assert_eq!(run_bc(src, "main", &[]).unwrap(), IrValue::Int(0));
    assert_consistent(src, "read_b", &[IrValue::Int(1)]);
    assert_eq!(
        run_bc(src, "read_b", &[IrValue::Int(1)]).unwrap(),
        IrValue::Bool(true)
    );
}
