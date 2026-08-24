//! hc-tools/tests/harness.rs

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static DIR_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn hc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_hc"))
}

/// 独立临时目录（每个测试独占，避免兄弟文件扫描/残留干扰；测后清理）
fn temp_dir(tag: &str) -> PathBuf {
    let n = DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "hc_harness_{}_{}_{}",
        std::process::id(),
        tag,
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, name: &str, src: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, src).unwrap();
    path
}

fn run_hc(args: &[&Path]) -> Output {
    let mut cmd = Command::new(hc_bin());
    for a in args {
        cmd.arg(a);
    }
    cmd.output().expect("run hc")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

#[test]
fn collects_only_test_fns_skip_and_summary() {
    // 收集：普通 fn 不运行；[test] fn 全跑；SKIP 计入汇总；全过退出 0
    let dir = temp_dir("collect");
    let file = write(
        &dir,
        "collect.hc",
        "fn helper() !void { try expect(false); }\n\
         [test] fn a() !void { try expect(true); }\n\
         [test] fn b() !void { try expect(true); }\n\
         [test] fn c() !void { if (1 > 0) return error.SkipTest; }\n",
    );
    let out = run_hc(&[Path::new("test"), &file]);
    let s = stdout(&out);
    assert!(out.status.success(), "全过应退出 0: {s}");
    assert!(s.contains("[PASS] a"), "应收集运行 [test] a: {s}");
    assert!(s.contains("[PASS] b"), "应收集运行 [test] b: {s}");
    assert!(s.contains("[SKIP] c"), "SKIP 应输出 [SKIP] 行: {s}");
    assert!(!s.contains("helper"), "普通 fn 不应作为测试收集: {s}");
    assert!(
        s.contains("2 passed, 0 failed, 1 skipped"),
        "汇总应计 PASS/SKIP: {s}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn collects_across_files_in_dir() {
    // 收集：目录内多文件各自跑测试，汇总合并
    let dir = temp_dir("dir");
    write(
        &dir,
        "one.hc",
        "[test] fn a() !void { try expect(true); }\n",
    );
    write(
        &dir,
        "two.hc",
        "[test] fn b() !void { try expect(true); }\n\
         [test] fn c() !void { try expect(true); }\n",
    );
    let out = run_hc(&[Path::new("test"), &dir]);
    let s = stdout(&out);
    assert!(out.status.success(), "目录内全过应退出 0: {s}");
    assert!(s.contains("one.hc::[PASS] a"), "应运行 one.hc 的测试: {s}");
    assert!(s.contains("two.hc::[PASS] b"), "应运行 two.hc 的测试: {s}");
    assert!(s.contains("two.hc::[PASS] c"), "应运行 two.hc 的测试: {s}");
    assert!(
        s.contains("3 passed, 0 failed, 0 skipped"),
        "汇总应合并两文件: {s}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn failing_test_exits_nonzero_with_fail_line() {
    // 退出码：测试失败 → [FAIL] 行 + 非零退出 + 失败计入汇总
    let dir = temp_dir("fail");
    let file = write(
        &dir,
        "fail.hc",
        "[test] fn a() !void { try expect_eq(1, 2); }\n",
    );
    let out = run_hc(&[Path::new("test"), &file]);
    let s = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "有 FAIL 应退出码 1: {s}");
    assert!(
        s.contains("[FAIL] a (error.AssertFailed"),
        "应输出 [FAIL] 与错误名: {s}"
    );
    assert!(
        s.contains("0 passed, 1 failed, 0 skipped"),
        "失败应计入汇总: {s}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn injects_io_alloc_into_tests() {
    // 注入（Q-T4）：测试 fn 无需声明参数即可用 io/alloc
    let dir = temp_dir("inject");
    let file = write(
        &dir,
        "inject.hc",
        "[test] fn t() !void {\n\
         \x20   io.print(\"injected\\n\");\n\
         \x20   var p = alloc.alloc(16);\n\
         \x20   defer alloc.free(p);\n\
         }\n",
    );
    let out = run_hc(&[Path::new("test"), &file]);
    let s = stdout(&out);
    assert!(out.status.success(), "注入 io/alloc 应通过: {s}");
    assert!(s.contains("[PASS] t"), "应 [PASS]: {s}");
    assert!(s.contains("injected"), "io.print 应输出注入的 io: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn parse_error_file_reports_fail_nonzero() {
    // 退出码/收集：解析失败文件 → stderr [FAIL] + 非零退出
    let dir = temp_dir("parseerr");
    write(&dir, "bad.hc", "fn main( { this is not valid\n");
    let out = run_hc(&[Path::new("test"), &dir]);
    let e = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "解析失败应退出码 1: {e}");
    assert!(e.contains("[FAIL] bad.hc"), "应报告 [FAIL] bad.hc: {e}");
    let _ = std::fs::remove_dir_all(&dir);
}
