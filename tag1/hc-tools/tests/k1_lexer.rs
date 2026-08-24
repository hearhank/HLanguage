//! K1：H 版 lexer 对照测试——对 `stage1/corpus/*.hc` 逐个文件比较
//! Rust 参考 lexer（`hc lex`）与 H 版 lexer（`hc run stage1/lexer.hc`）的 token 流输出。
//!
//! 对照格式：`{start} {end} {line} {col} {kind:?}`（`kind:?` 为 Rust Debug 形态），
//! 逐行 diff 必须一致。语料覆盖：45 关键字、数字前缀归一化/惰性宽度后缀、
//! 字符串/字符转义全套、错误路径、未知字符双 Error、未闭合注释/字符串、UTF-8 计列。

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

/// 对照单个文件：Rust 参考（hc lex）与 H 版（hc run lexer.hc）输出必须逐行一致。
fn assert_lexer_matches(h_lexer: &Path, file: &Path) {
    let r = run_hc(&["lex", file.to_str().unwrap()]);
    assert!(r.status.success(), "hc lex 失败于 {}：{}", file.display(), stderr(&r));
    let h = run_hc(&["run", h_lexer.to_str().unwrap(), file.to_str().unwrap()]);
    assert!(h.status.success(), "hc run lexer.hc 失败于 {}：{}", file.display(), stderr(&h));
    assert_eq!(
        stdout(&r),
        stdout(&h),
        "K1 对照不一致（Rust 参考 vs H 版 lexer）：{}",
        file.display()
    );
}

#[test]
fn corpus_matches_rust_reference() {
    let root = repo_root();
    let corpus = root.join("stage1/corpus");
    assert!(corpus.is_dir(), "语料目录缺失：{}", corpus.display());
    let h_lexer = root.join("stage1/lexer.hc");
    assert!(h_lexer.is_file(), "H 版 lexer 缺失：{}", h_lexer.display());

    let mut files: Vec<PathBuf> = std::fs::read_dir(&corpus)
        .expect("read corpus")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |x| x == "hc"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "语料为空");
    for f in &files {
        assert_lexer_matches(&h_lexer, f);
    }
}

#[test]
fn self_source_matches() {
    // H 版 lexer 自身源码也要与 Rust 参考逐 token 一致（自举的第一层自证）
    let root = repo_root();
    let h_lexer = root.join("stage1/lexer.hc");
    assert_lexer_matches(&h_lexer, &h_lexer);
}
