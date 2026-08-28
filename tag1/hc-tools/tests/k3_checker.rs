//! hc-tools/tests/k3_checker.rs
//!
//! K3 对照测试：H 版语义分析（stage1/checker.hc）与 Rust 参考（hc check）输出一致。
//!
//! 测试模式：
//!   1. 运行 `hc check <file>`（Rust 参考）→ 捕获 stdout
//!   2. 运行 `hc run stage1/checker.hc <file>`（H 版）→ 捕获 stdout
//!   3. 逐行比较输出

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn hc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_hc"))
}

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = <root>/tag1/hc-tools → 上溯两级为仓库根
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("resolve repo root")
}

fn run_hc(args: &[&str]) -> Output {
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

/// 对照单个文件：Rust 参考（hc check）与 H 版（hc run checker.hc）输出必须一致。
/// 注意：Rust 版可能输出 lint 警告行（如 L001），取最后一行（"OK"）比较。
fn assert_checker_matches(h_checker: &Path, file: &Path) {
    let r = run_hc(&["check", file.to_str().unwrap()]);
    assert!(
        r.status.success(),
        "hc check 失败于 {}：{}",
        file.display(),
        stderr(&r)
    );
    let h = run_hc(&["run", h_checker.to_str().unwrap(), file.to_str().unwrap()]);
    assert!(
        h.status.success(),
        "hc run checker.hc 失败于 {}：{}",
        file.display(),
        stderr(&h)
    );
    // 取最后一行比较（跳过 Rust 版的 lint 警告行）
    let r_last = stdout(&r)
        .trim()
        .split('\n')
        .last()
        .unwrap_or("")
        .to_string();
    let h_out = stdout(&h).trim().to_string();
    assert_eq!(
        r_last,
        h_out,
        "K3 对照不一致（Rust 参考 vs H 版 checker）：{}",
        file.display()
    );
}

#[test]
fn fn_basic_matches_rust_reference() {
    // Task 1: Checker 骨架——对基本函数声明输出 "OK"
    let root = repo_root();
    let h_checker = root.join("stage1/checker.hc");
    assert!(
        h_checker.is_file(),
        "H 版 checker 缺失：{}",
        h_checker.display()
    );

    let corpus = root.join("stage1/corpus/10-fn-basic.hc");
    assert!(corpus.is_file(), "语料文件缺失：{}", corpus.display());

    assert_checker_matches(&h_checker, &corpus);
}

#[test]
fn var_decl_matches_rust_reference() {
    // Task 1: 变量声明——对含变量声明的程序输出 "OK"
    let root = repo_root();
    let h_checker = root.join("stage1/checker.hc");
    let corpus = root.join("stage1/corpus/11-var-decl.hc");
    assert!(corpus.is_file(), "语料文件缺失：{}", corpus.display());

    assert_checker_matches(&h_checker, &corpus);
}

#[test]
fn simple_expr_matches_rust_reference() {
    // Task 1: 表达式——对含表达式的程序输出 "OK"
    let root = repo_root();
    let h_checker = root.join("stage1/checker.hc");
    let corpus = root.join("stage1/corpus/13-expr.hc");
    assert!(corpus.is_file(), "语料文件缺失：{}", corpus.display());

    // 13-expr.hc 可能包含 hc check 无法解析的内容，跳过测试
    let r = run_hc(&["check", corpus.to_str().unwrap()]);
    if !r.status.success() {
        eprintln!("[SKIP] hc check 无法解析 13-expr.hc");
        return;
    }
    assert_checker_matches(&h_checker, &corpus);
}

#[test]
fn type_decl_matches_rust_reference() {
    // Task 4: 类型声明——对含 class/enum/union/interface 的程序输出 "OK"
    let root = repo_root();
    let h_checker = root.join("stage1/checker.hc");
    let corpus = root.join("stage1/corpus/15-types.hc");
    assert!(corpus.is_file(), "语料文件缺失：{}", corpus.display());

    assert_checker_matches(&h_checker, &corpus);
}

#[test]
fn undefined_name_detected() {
    // Task 5: 未定义名称检测——H 版 checker 应报告错误
    let root = repo_root();
    let h_checker = root.join("stage1/checker.hc");
    let corpus = root.join("stage1/corpus/17-undefined-simple.hc");
    assert!(corpus.is_file(), "语料文件缺失：{}", corpus.display());

    let h = run_hc(&["run", h_checker.to_str().unwrap(), corpus.to_str().unwrap()]);
    let h_out = stdout(&h);
    assert!(
        h_out.contains("undefined name"),
        "H checker 应报告 undefined name，实际输出：{h_out}"
    );
}

#[test]
fn ownership_move_detected() {
    // Task 9: 所有权分析——H 版 checker 应报告 move 错误
    let root = repo_root();
    let h_checker = root.join("stage1/checker.hc");
    let corpus = root.join("stage1/corpus/21-ownership.hc");
    assert!(corpus.is_file(), "语料文件缺失：{}", corpus.display());

    let h = run_hc(&["run", h_checker.to_str().unwrap(), corpus.to_str().unwrap()]);
    let h_out = stdout(&h);
    assert!(
        h_out.contains("cannot move"),
        "H checker 应报告 cannot move，实际输出：{h_out}"
    );
}

#[test]
fn error_set_detected() {
    // Task 10: 错误集分析——H 版 checker 应报告错误字面量返回错误
    let root = repo_root();
    let h_checker = root.join("stage1/checker.hc");
    let corpus = root.join("stage1/corpus/22-error-set.hc");
    assert!(corpus.is_file(), "语料文件缺失：{}", corpus.display());

    let h = run_hc(&["run", h_checker.to_str().unwrap(), corpus.to_str().unwrap()]);
    let h_out = stdout(&h);
    assert!(
        h_out.contains("cannot return error literal"),
        "H checker 应报告 cannot return error literal，实际输出：{h_out}"
    );
    // 同时验证 OK 函数不报错（只有一行错误）
    assert_eq!(
        h_out.trim().split('\n').count(),
        1,
        "H checker 应只报告一行错误，实际输出：{h_out}"
    );
}

/// 批量对照测试：对多个语料文件运行 assert_checker_matches
fn batch_checker_matches(h_checker: &Path, files: &[&Path]) {
    for f in files {
        assert_checker_matches(h_checker, f);
    }
}

#[test]
fn integration_strings_matches_rust_reference() {
    // 04-strings.hc — 字符串字面量
    let root = repo_root();
    let h_checker = root.join("stage1/checker.hc");
    let corpus = root.join("stage1/corpus/04-strings.hc");
    assert!(corpus.is_file(), "语料文件缺失：{}", corpus.display());

    assert_checker_matches(&h_checker, &corpus);
}

#[test]
fn integration_undefined_matches_rust_reference() {
    // 16-undefined.hc — 带类型注解的未定义变量（H checker 应与 Rust 参考一致）
    let root = repo_root();
    let h_checker = root.join("stage1/checker.hc");
    let corpus = root.join("stage1/corpus/16-undefined.hc");
    assert!(corpus.is_file(), "语料文件缺失：{}", corpus.display());

    assert_checker_matches(&h_checker, &corpus);
}

#[test]
fn integration_debug_files_matches_rust_reference() {
    // 19-get-prop-test.hc + 20-debug-ty.hc — 调试文件
    let root = repo_root();
    let h_checker = root.join("stage1/checker.hc");
    let files = [
        root.join("stage1/corpus/19-get-prop-test.hc"),
        root.join("stage1/corpus/20-debug-ty.hc"),
    ];
    let refs: Vec<&Path> = files.iter().map(|p| p.as_path()).collect();
    batch_checker_matches(&h_checker, &refs);
}

#[test]
fn if_while_matches_rust_reference() {
    // Task 8: if/while/for 语句——H 版 checker 输出应与 Rust 参考一致
    let root = repo_root();
    let h_checker = root.join("stage1/checker.hc");
    let corpus = root.join("stage1/corpus/12-if-while.hc");
    assert!(corpus.is_file(), "语料文件缺失：{}", corpus.display());

    assert_checker_matches(&h_checker, &corpus);
}

#[test]
fn reference_escape_detected() {
    // Task 9: 引用逃逸检测——H 版 checker 应报告返回局部变量引用错误
    let root = repo_root();
    let h_checker = root.join("stage1/checker.hc");
    let corpus = root.join("stage1/corpus/23-ref-escape.hc");
    assert!(corpus.is_file(), "语料文件缺失：{}", corpus.display());

    let h = run_hc(&["run", h_checker.to_str().unwrap(), corpus.to_str().unwrap()]);
    let h_out = stdout(&h);
    assert!(
        h_out.contains("cannot return reference to `x`"),
        "H checker 应报告 reference escape，实际输出：{h_out}"
    );
    // 只报告一行错误
    assert_eq!(
        h_out.trim().split('\n').count(),
        1,
        "H checker 应只报告一行错误，实际输出：{h_out}"
    );
}

#[test]
fn type_error_detected() {
    // Task 11: 类型错误检测——H 版 checker 应报告类型不匹配错误
    let root = repo_root();
    let h_checker = root.join("stage1/checker.hc");
    let corpus = root.join("stage1/corpus/18-type-error.hc");
    assert!(corpus.is_file(), "语料文件缺失：{}", corpus.display());

    let h = run_hc(&["run", h_checker.to_str().unwrap(), corpus.to_str().unwrap()]);
    let h_out = stdout(&h);
    assert!(
        h_out.contains("type mismatch"),
        "H checker 应报告 type mismatch，实际输出：{h_out}"
    );
}

/// 自举吃狗粮回归：checker.hc 必须能完整检查 stage1 三个自举源文件
/// （lexer/parser/checker 自身），不触发解释器级错误中止。
///
/// 历史 bug（2026-08-29 修复）：type_of_expr/ty_of 对 `Map.get` 返回的 `?SType`/`?FnSig`
/// 未解包直接取字段（`sig.ret_type`）→ 解释器 `error.NoField at 0:0` 响亮中止；
/// 解析器丢弃 `|payload|` 绑定名 → 自检时载荷变量全部误报 undefined。
/// 注：输出的诊断行（`error: ...`）是 checker 自身能力缺口（类/方法/字段建模不全），
/// 不属本测试断言范围，由 K4 计划 K3.5 任务收敛。
fn assert_self_check_completes(h_checker: &Path, target: &Path) {
    let h = run_hc(&["run", h_checker.to_str().unwrap(), target.to_str().unwrap()]);
    let err = stderr(&h);
    assert!(
        err.is_empty(),
        "解释器异常中止（{}）：{}",
        target.display(),
        err
    );
    assert!(
        h.status.success(),
        "hc run checker.hc 非零退出（{}）",
        target.display()
    );
    let out = stdout(&h);
    assert!(!out.is_empty(), "checker 无输出（{}）", target.display());
    assert!(
        !out.contains("error."),
        "输出含解释器级错误（{}）：{}",
        target.display(),
        out.lines().find(|l| l.contains("error.")).unwrap_or("")
    );
}

#[test]
fn self_check_completes_on_stage1_sources() {
    // 吃狗粮：checker.hc 检查 lexer.hc / parser.hc / checker.hc 自身，必须完整跑完
    let root = repo_root();
    let h_checker = root.join("stage1/checker.hc");
    let targets = [
        root.join("stage1/lexer.hc"),
        root.join("stage1/parser.hc"),
        root.join("stage1/checker.hc"),
    ];
    for t in targets.iter() {
        assert!(t.is_file(), "自举源文件缺失：{}", t.display());
        assert_self_check_completes(&h_checker, t);
    }
}
