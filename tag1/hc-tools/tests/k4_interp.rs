//! hc-tools/tests/k4_interp.rs
//!
//! K4 对照测试：H 版执行引擎（stage1/interp.hc）与 Rust 参考实现（hc run）对
//! stage1/exec-corpus/ 语料的 stdout 必须一致。
//!
//! 每个语料文件一个测试；C 组任务按能力面逐步摘除 ignore：
//!   C3 → 01/02（表达式求值 A），C4 → 03（语句），C5 → 04（函数/递归），
//!   C6 → 05/08（Vec/Map），C7 → 06（String），C8 → 07（class），
//!   C9 → 09（错误路径），C10 → 10（综合）+ 全部摘除。
//!
//! B2 阶段全部 #[ignore]：interp.hc 尚未落地，骨架入库不影响门禁。

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

fn corpus_dir() -> PathBuf {
    repo_root().join("stage1/exec-corpus")
}

/// 对照单个语料文件：`hc run <f>`（Rust 参考）与 `hc run interp.hc <f>`（H 版）
/// 的 stdout 必须完全一致；双方都必须成功退出。
fn assert_interp_matches(interp: &Path, file: &Path) {
    let r = run_hc(&["run", file.to_str().unwrap()]);
    assert!(
        r.status.success(),
        "Rust 参考 hc run 失败于 {}：{}",
        file.display(),
        stderr(&r)
    );
    let expected = stdout(&r);

    let h = run_hc(&["run", interp.to_str().unwrap(), file.to_str().unwrap()]);
    assert!(
        h.status.success(),
        "H 版 interp 运行失败于 {}：{}",
        file.display(),
        stderr(&h)
    );
    let actual = stdout(&h);

    assert_eq!(
        expected,
        actual,
        "stdout 不一致（{}）\n--- Rust 参考 ---\n{}--- H 版 interp ---\n{}",
        file.display(),
        expected,
        actual
    );
}

fn assert_corpus_pair(name: &str) {
    let interp = repo_root().join("stage1/interp.hc");
    assert!(
        interp.is_file(),
        "H 版解释器缺失：{}（C1 落地后按任务摘除对应 ignore）",
        interp.display()
    );
    let f = corpus_dir().join(name);
    assert!(f.is_file(), "语料文件缺失：{}", f.display());
    assert_interp_matches(&interp, &f);
}

#[test]
#[ignore]
fn c01_arith_matches_rust_reference() {
    assert_corpus_pair("01-arith.hc");
}

#[test]
#[ignore]
fn c02_vars_matches_rust_reference() {
    assert_corpus_pair("02-vars.hc");
}

#[test]
#[ignore]
fn c03_control_matches_rust_reference() {
    assert_corpus_pair("03-control.hc");
}

#[test]
#[ignore]
fn c04_fn_rec_matches_rust_reference() {
    assert_corpus_pair("04-fn-rec.hc");
}

#[test]
#[ignore]
fn c05_vec_matches_rust_reference() {
    assert_corpus_pair("05-vec.hc");
}

#[test]
#[ignore]
fn c06_string_matches_rust_reference() {
    assert_corpus_pair("06-string.hc");
}

#[test]
#[ignore]
fn c07_class_matches_rust_reference() {
    assert_corpus_pair("07-class.hc");
}

#[test]
#[ignore]
fn c08_map_matches_rust_reference() {
    assert_corpus_pair("08-map.hc");
}

#[test]
#[ignore]
fn c09_errors_matches_rust_reference() {
    assert_corpus_pair("09-errors.hc");
}

#[test]
#[ignore]
fn c10_comprehensive_matches_rust_reference() {
    assert_corpus_pair("10-comprehensive.hc");
}
