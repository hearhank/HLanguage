//! M3.3 原生后端集成测试：源码 → LLVM IR → `zig cc` → 运行可执行文件。
//!
//! 依赖外部 `zig cc`（emit-.ll 驱动）；`zig` 缺失时全部测试跳过（打印 SKIP，
//! 不失败——与纯文本发射测试 `hc::llvm` 分离，后者无外部依赖始终运行）。

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn zig_cc_available() -> bool {
    Command::new("zig")
        .arg("cc")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 唯一临时目录（进程 ID + 自增序号，避免并行测试冲突）
fn temp_dir() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "hc_native_test_{}_{}",
        std::process::id(),
        n
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// 源码 → 编译 → 运行 → 退出状态。失败（解析/语义/zig cc）panic。
fn compile_and_run(src: &str) -> std::process::ExitStatus {
    let dir = temp_dir();
    // 1) 前端：解析 → 语义检查 → lower → codegen（与 `hc build` 同路径）
    let program = hc::parse_source(src).expect("parse");
    let errs = hc::check_semantics(&program);
    assert!(
        !errs.iter().any(|d| d.is_error()),
        "语义检查失败: {:?}",
        errs
    );
    let module = hc::ir::lower(&program).expect("lower");
    let table = hc::error_code_table(&program);
    let ll = hc::llvm::codegen(&module, &table);

    // 2) 写 .ll → zig cc → 可执行文件
    let ll_path = dir.join("prog.ll");
    std::fs::write(&ll_path, ll).expect("write .ll");
    let exe_name = if cfg!(windows) { "prog.exe" } else { "prog" };
    let exe_path = dir.join(exe_name);
    let out = Command::new("zig")
        .arg("cc")
        .arg(&ll_path)
        .arg("-o")
        .arg(&exe_path)
        .output()
        .expect("run zig cc");
    assert!(
        out.status.success(),
        "zig cc 失败: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // 3) 运行，返回退出状态
    Command::new(&exe_path)
        .output()
        .expect("run exe")
        .status
}

/// 测试驱动编译运行：源码（含 `test fn`）→ `codegen_tests` → `zig cc` → 运行 → 退出状态。
fn compile_tests_and_run(src: &str) -> std::process::ExitStatus {
    let dir = temp_dir();
    let program = hc::parse_source(src).expect("parse");
    let errs = hc::check_semantics(&program);
    assert!(
        !errs.iter().any(|d| d.is_error()),
        "语义检查失败: {:?}",
        errs
    );
    let module = hc::ir::lower(&program).expect("lower");
    let table = hc::error_code_table(&program);
    let ll = hc::llvm::codegen_tests(&module, &table);

    let ll_path = dir.join("prog.ll");
    std::fs::write(&ll_path, ll).expect("write .ll");
    let exe_name = if cfg!(windows) { "prog.exe" } else { "prog" };
    let exe_path = dir.join(exe_name);
    let out = Command::new("zig")
        .arg("cc")
        .arg(&ll_path)
        .arg("-o")
        .arg(&exe_path)
        .output()
        .expect("run zig cc");
    assert!(
        out.status.success(),
        "zig cc 失败: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Command::new(&exe_path)
        .output()
        .expect("run exe")
        .status
}

#[test]
fn test_runner_green_exits_zero() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    let src = "[test] fn a() !void { try expect_eq(1 + 1, 2); }\n[test] fn b() !void { try expect(true); }\n";
    let st = compile_tests_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn test_runner_red_exits_nonzero() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    let src = "[test] fn a() !void { try expect_eq(1 + 1, 3); }\n";
    let st = compile_tests_and_run(src);
    assert!(!st.success(), "预期非零退出，实际: {st}");
}

#[test]
fn scalar_main_returns_ok() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    let st = compile_and_run("fn main() i32 { return 42; }");
    assert!(st.success(), "exit: {st}");
}

#[test]
fn if_while_try_catch_program() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    let src = r#"
fn sum_to(n: i32) i32 {
    var mut i: i32 = 0;
    var mut sum: i32 = 0;
    while (i < n) : (i += 1) { sum += i; }
    return sum;
}
fn fail() !i32 { return error.NotFound; }
fn ok() !i32 { return 5; }
fn pick(x: i32) i32 {
    if (x > 5) { return x; }
    else if (x > 2) { return x * 2; }
    return 0;
}
fn main() i32 {
    var t = try ok();
    var s = fail() catch 7;
    return sum_to(5) + pick(3) + t + s;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn division_by_zero_is_failure() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    let st = compile_and_run("fn main() i32 { return 10 / 0; }");
    assert!(!st.success(), "预期非零退出，实际: {st}");
}

#[test]
fn unhandled_error_value_is_failure() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    let st = compile_and_run("fn main() !i32 { return error.NotFound; }");
    assert!(!st.success(), "预期非零退出，实际: {st}");
}

#[test]
fn global_const_native_init_and_mutation() {
    // Phase 5 原生：@__init__ 注入（main 前置执行）+ LoadGlobal/StoreGlobal 寻址 @.h_globals
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    let src = r#"
global counter: i32 = 0;
const BASE: i32 = 100;
fn bump() i32 {
    counter = counter + 1;
    return counter + BASE;
}
fn main() i32 {
    var a = bump();
    var b = bump();
    if (a != 101 or b != 102) { return 1; }
    if (counter != 2) { return 1; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn test_runner_global_init_runs_first() {
    // 原生测试跑器：@__init__ 在首个 test fn 前执行（全局已初始化）
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    let src = "global g: i32 = 7;\n[test] fn t() !void { try expect_eq(g, 7); }\n";
    let st = compile_tests_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn global_addr_native_writes_through() {
    // Phase 5 原生：`&mut global` → GlobalAddr（@.h_globals 元素地址入 tag 7）——
    // Deref/StorePtr 写穿回全局，跨调用持久（@__init__ 仅一次）。
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    let src = r#"
global counter: i32 = 0;
fn bump() i32 {
    var p = &mut counter;
    p.* += 1;
    return p.*;
}
fn main() i32 {
    var a = bump();
    var b = bump();
    if (a != 1 or b != 2) { return 1; }
    if (counter != 2) { return 1; }
    var r = &counter;
    if (r.* != 2) { return 1; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn string_literal_and_compare() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // 字符串字面量 + 相等比较 + 短路 and（切片内）
    let src = r#"
fn main() i32 {
    var a = "hi";
    if (a == "hi" and 1 + 1 == 2) { return 0; }
    return 1;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn pointer_write_through_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // Phase 1 指针：`&mut` 取址 + `p.*` 写穿 + 跨函数别名 → 原生可执行一致结果
    let src = r#"
fn bump(p: *mut i32) void {
    p.* += 1;
}
fn main() i32 {
    var mut x: i32 = 41;
    var p = &mut x;
    p.* = 42;
    bump(&mut x);
    var y = p.*;
    if (y == 43) { return 0; }
    return 1;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

// ---------- Phase 2 聚合原生端到端（源码 → LLVM → zig cc → 可执行） ----------

#[test]
fn aggregate_struct_literal_and_field_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // MakeClass/Field/StoreField + 类深比较（hc_eq_agg）
    let src = r#"
class Point {
    x: i32,
    y: i32,
}
fn main() i32 {
    var p = Point{ x = 1, y = 2 };
    if (p.x != 1 or p.y != 2) { return 1; }
    p.y = 5;
    if (p.y != 5) { return 2; }
    if (p != Point{ x = 1, y = 5 }) { return 3; }
    if (p == Point{ x = 2, y = 5 }) { return 4; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn aggregate_array_index_and_store_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // MakeArr/Index/StoreIndex + 数组深比较
    let src = r#"
fn main() i32 {
    var a = [10, 20, 30];
    if (a[0] != 10 or a[2] != 30) { return 1; }
    a[1] = 99;
    if (a[1] != 99) { return 2; }
    if (a == [10, 20, 30]) { return 3; }
    if (a != [10, 99, 30]) { return 4; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn aggregate_len_fields_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // `.len`：Str / Arr / Slice 三形态（hc_field 的 `.len` 分支）
    let src = r#"
fn main() i32 {
    var s = "abc";
    if (s.len != 3) { return 1; }
    var arr = [10, 20, 30, 40];
    if (arr.len != 4) { return 2; }
    var sub = arr[1..3];
    if (sub.len != 2) { return 3; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn aggregate_slice_view_and_alias_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // SliceOf：切片为共享视图——源数组元素写穿（元素 cell 别名）
    let src = r#"
fn main() i32 {
    var arr = [1, 2, 3, 4, 5];
    var sub = arr[1..4];
    if (sub.len != 3 or sub[0] != 2 or sub[2] != 4) { return 1; }
    arr[1] = 99;
    if (sub[0] != 99) { return 2; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn aggregate_slice_store_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // StoreSlice：`arr[lo..hi] = v` 写回源数组元素
    let src = r#"
fn main() i32 {
    var arr = [1, 2, 3, 4, 5];
    arr[1..3] = [20, 30];
    if (arr[1] != 20 or arr[2] != 30 or arr.len != 5) { return 1; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn aggregate_tuple_destructure_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // Destructure：元组（多值返回）解构绑定
    let src = r#"
fn divmod(a: i32, b: i32) (i32, i32) {
    return (a / b, a % b);
}
fn main() i32 {
    var (q, r) = divmod(10, 3);
    if (q != 3 or r != 1) { return 1; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn aggregate_move_expr_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // Move：值语义拷贝（`move x` ≡ 值拷贝，原绑定仍可访问）
    let src = r#"
fn main() i32 {
    var a = [1, 2, 3];
    var b = move a;
    if (b.len != 3 or b[1] != 2 or a.len != 3) { return 1; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn aggregate_unwrap_opt_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // Unwrap：`x.?` 解包 Opt(Some) → 值（native 恒等表示：Opt(Some)=载荷）
    let src = r#"
fn boxed(x: ?i32) ?i32 { return x; }
fn main() i32 {
    var v = boxed(7).?;
    if (v != 7) { return 1; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn aggregate_enum_literal_and_eq_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // MakeEnum：类型名限定枚举常量 + 值比较（name+variant+payload）
    let src = r#"
enum Color { red, green, blue }
fn main() i32 {
    var c = Color.green;
    if (c != Color.green) { return 1; }
    if (c == Color.red) { return 2; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn aggregate_index_oob_is_failure() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // 越界索引：运行时硬错误（hc_abort）→ 非零退出
    let src = "fn main() i32 { var a = [1, 2, 3]; return a[5]; }";
    let st = compile_and_run(src);
    assert!(!st.success(), "预期非零退出，实际: {st}");
}

#[test]
fn aggregate_unwrap_null_is_failure() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // NullUnwrap：运行时硬错误（hc_abort_nullunwrap）→ 非零退出
    let src = r#"
fn boxed(x: ?i32) ?i32 { return x; }
fn main() i32 {
    var v = boxed(null).?;
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(!st.success(), "预期非零退出，实际: {st}");
}

// ---------- Phase 3 switch + range + for 原生端到端（源码 → LLVM → zig cc → 可执行） ----------

#[test]
fn phase3_switch_int_and_else_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // MatchTest 模式描述符 + first-match 线性链 + else 兜底
    let src = r#"
fn pick(x: i32) i32 {
    switch (x) {
        1 => return 10,
        2 => return 20,
        else => return 99,
    }
}
fn main() i32 {
    if (pick(1) != 10) { return 1; }
    if (pick(2) != 20) { return 2; }
    if (pick(9) != 99) { return 3; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn phase3_switch_error_pattern_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // 错误码模式：hc_match_test tag 0 = data(code) 比较
    let src = r#"
fn fail(x: i32) !i32 {
    if (x == 1) { return error.NotFound; }
    return error.Io;
}
fn main() i32 {
    var r = fail(1) catch |err| switch (err) {
        error.NotFound => 10,
        else => 20,
    };
    if (r != 10) { return 1; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn phase3_for_range_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // MakeRange → Arr（[lo, hi)）+ 只读捕获
    let src = r#"
fn main() i32 {
    var mut s: i32 = 0;
    for (0..5) |i| { s += i; }
    if (s != 10) { return 1; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn phase3_for_arr_mut_writeback_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // IterMake（Arr → 源元素指针 is_ref=true）+ IterNext + IterWriteBack 写回
    let src = r#"
fn main() i32 {
    var a = [1, 2, 3];
    for (a) |mut x| { x += 1; }
    if (a[0] != 2 or a[1] != 3 or a[2] != 4) { return 1; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn phase3_for_slice_write_through_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // 切片迭代：源数组元素共享，Mut 写回透传
    let src = r#"
fn main() i32 {
    var arr = [10, 20, 30, 40];
    var sub = arr[1..3];
    for (sub) |mut x| { x = x + 1; }
    if (arr[1] != 21 or arr[2] != 31) { return 1; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn phase3_for_break_continue_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // for 内 break 提前退出 / continue 跳过（JumpIfNot + Jump 到 l_end/l_next）
    let src = r#"
fn main() i32 {
    var mut s: i32 = 0;
    for (0..10) |i| {
        if (i == 3) { continue; }
        if (i == 6) { break; }
        s += i;
    }
    if (s != 12) { return 1; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn phase3_switch_enum_capture_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // 枚举变体（Ident 模式）+ 负载捕获（hc_enum_payload）
    let src = r#"
enum Maybe { some: i32, none }
fn main() i32 {
    var v: Maybe = Maybe{some = 7};
    var label = switch (v) {
        some => |i| i,
        none => -1,
        else => -2,
    };
    if (label != 7) { return 1; }
    var n: Maybe = Maybe.none;
    var label2 = switch (n) {
        some => |i| i,
        none => -1,
        else => -2,
    };
    if (label2 != -1) { return 2; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn phase3_for_str_bytes_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // 字符串迭代：字节 Int（is_ref=false 新单元）
    let src = r#"
fn main() i32 {
    var mut sum: i32 = 0;
    for ("abc") |b| { sum += b; }
    if (sum != 97 + 98 + 99) { return 1; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

// （NotIterable 运行时路径为语义检查后的防御性兜底：`for (n) |x|` 在 check_semantics
//  即被拒绝，无法到达运行时代码——故不设端到端失败用例。）

#[test]
fn phase6_defer_lifo_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // defer LIFO（%defers 计数器数组 + JumpIfNotDefer 守卫）+ 块级作用域退出触发。
    let src = r#"
global log: i32 = 0;
fn rec(v: i32) void { log = log * 10 + v; }
fn main() i32 {
    log = 0;
    {
        defer rec(1);
        defer rec(2);
        defer rec(3);
    }
    if (log != 321) { return 1; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn phase6_defer_runs_on_return_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // `return` 排空函数级 defers：正常值仅非 errdefer。
    let src = r#"
global g: i32 = 0;
fn bump(v: i32) void { g += v; }
fn early() i32 {
    defer bump(5);
    return 1;
}
fn main() i32 {
    g = 0;
    var r = early();
    if (r != 1) { return 1; }
    if (g != 5) { return 2; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn phase6_defer_loop_break_continue_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // break/continue 排空循环体 defers：每轮迭代 defer 都运行（含 continue 路径）。
    let src = r#"
global dlog: i32 = 0;
fn bump() void { dlog += 1; }
fn main() i32 {
    dlog = 0;
    var i: i32 = 0;
    while (true) {
        defer bump();
        i += 1;
        if (i >= 3) { break; }
    }
    if (dlog != 3) { return 1; }
    dlog = 0;
    var clog: i32 = 0;
    i = 0;
    while (i < 5) {
        defer bump();
        i += 1;
        if (i == 3) { continue; }
        clog += 1;
    }
    if (dlog != 5) { return 2; }
    if (clog != 4) { return 3; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn phase6_errdefer_error_path_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
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
fn main() i32 {
    g = 0;
    var r = maybe(false) catch 0;
    if (r != 0) { return 1; }
    if (g != 101) { return 2; }
    g = 0;
    r = maybe(true) catch 0;
    if (r != 5) { return 3; }
    if (g != 1) { return 4; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn phase6_labeled_break_continue_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // 带标签 break/continue：标签跨多层循环定位（JumpIfNotDefer 守卫 + 目标 label）。
    let src = r#"
fn main() i32 {
    var s: i32 = 0;
    :outer while (true) {
        var j: i32 = 0;
        while (j < 10) {
            j += 1;
            if (j == 2) { break :outer; }
            s += j;
        }
    }
    if (s != 1) { return 1; }
    s = 0;
    :outer for (0..3) |i| {
        if (i == 1) { continue :outer; }
        s += i;
    }
    if (s != 2) { return 2; }
    s = 0;
    :outer for (0..3) |i| {
        var j: i32 = 0;
        while (j < 5) {
            j += 1;
            if (i == 1) { continue :outer; }
            s += j;
        }
        s += 100;
    }
    if (s != 230) { return 3; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn phase6_labeled_break_runs_defers_native() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    // 带标签 break 排空目标循环体 defers。
    let src = r#"
global g: i32 = 0;
fn bump() void { g += 1; }
fn main() i32 {
    g = 0;
    :outer while (true) {
        defer bump();
        break :outer;
    }
    if (g != 1) { return 1; }
    return 0;
}
"#;
    let st = compile_and_run(src);
    assert!(st.success(), "exit: {st}");
}
