//! K2：H 版 parser 对照测试——对 `stage1/corpus/*.hc` 逐个文件比较
//! Rust 参考 parser（`hc parse`）与 H 版 parser（`hc run stage1/parser.hc`）的 AST 输出。
//!
//! 对照格式：每行一个节点，缩进表示嵌套层级，格式 `NodeType|key=val|key=val`。
//! 逐行 diff 必须一致。语料覆盖：函数声明、变量声明、if/while、表达式、return 等。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn hc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_hc"))
}

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = <root>/tag1/hc-tools -> 上溯两级为仓库根
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

/// 对照单个文件：Rust 参考（hc parse）与 H 版（hc run parser.hc）输出必须逐行一致。
fn assert_parser_matches(h_parser: &Path, file: &Path) {
    let r = run_hc(&["parse", file.to_str().unwrap()]);
    assert!(
        r.status.success(),
        "hc parse 失败于 {}：{}",
        file.display(),
        stderr(&r)
    );
    let h = run_hc(&["run", h_parser.to_str().unwrap(), file.to_str().unwrap()]);
    assert!(
        h.status.success(),
        "hc run parser.hc 失败于 {}：{}",
        file.display(),
        stderr(&h)
    );
    assert_eq!(
        stdout(&r),
        stdout(&h),
        "K2 对照不一致（Rust 参考 vs H 版 parser）：{}",
        file.display()
    );
}

#[test]
fn corpus_matches_rust_reference() {
    let root = repo_root();
    let corpus = root.join("stage1/corpus");
    assert!(corpus.is_dir(), "语料目录缺失：{}", corpus.display());
    let h_parser = root.join("stage1/parser.hc");
    assert!(
        h_parser.is_file(),
        "H 版 parser 缺失：{}",
        h_parser.display()
    );

    let mut files: Vec<PathBuf> = std::fs::read_dir(&corpus)
        .expect("read corpus")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |x| x == "hc"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "语料为空");
    for f in &files {
        assert_parser_matches(&h_parser, f);
    }
}

#[test]
fn self_source_matches() {
    // H 版 parser 自身源码也要与 Rust 参考逐行一致（自举的第二层自证）
    let root = repo_root();
    let h_parser = root.join("stage1/parser.hc");
    assert_parser_matches(&h_parser, &h_parser);
}
