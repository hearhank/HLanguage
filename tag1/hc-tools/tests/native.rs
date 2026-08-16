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
    let module = hc::ir::lower(&program);
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
    let module = hc::ir::lower(&program);
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
    let src = "test fn a() !void { try expect_eq(1 + 1, 2); }\ntest fn b() !void { try expect(true); }\n";
    let st = compile_tests_and_run(src);
    assert!(st.success(), "exit: {st}");
}

#[test]
fn test_runner_red_exits_nonzero() {
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    let src = "test fn a() !void { try expect_eq(1 + 1, 3); }\n";
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
