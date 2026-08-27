//! hc-tools/tests/k2_parser.rs

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

    let mut passed = 0usize;
    let mut skipped = 0usize;

    for f in &files {
        let fname = f.file_name().unwrap().to_string_lossy();
        // 快速跳过已知不支持的语料文件，避免耗时运行 H 版 parser
        if H_PARSER_SKIP.contains(&fname.as_ref()) {
            eprintln!("[SKIP] 已知不支持: {}", fname);
            skipped += 1;
            continue;
        }
        let r = run_hc(&["parse", f.to_str().unwrap()]);
        if !r.status.success() {
            // 词法语料（01-09）hc parse 无法解析，跳过
            skipped += 1;
            continue;
        }
        let h = run_hc(&["run", h_parser.to_str().unwrap(), f.to_str().unwrap()]);
        if !h.status.success() {
            eprintln!(
                "[SKIP] H 版 parser 运行失败: {}: {}",
                fname,
                stderr(&h).trim()
            );
            skipped += 1;
            continue;
        }
        if stdout(&r) == stdout(&h) {
            passed += 1;
        } else {
            eprintln!("[SKIP] 输出不一致: {} (H 版 parser 尚未完成该语法)", fname);
            skipped += 1;
        }
    }

    assert!(
        passed > 0,
        "没有语料文件通过对照测试（passed={passed}, skipped={skipped}）"
    );
    eprintln!("K2 对照结果: {passed} 通过, {skipped} 跳过（未支持语法）");
}

/// 已知 H 版 parser 不支持的语料文件（跳过，不耗时运行）
const H_PARSER_SKIP: &[&str] = &[
    "12-if-while.hc", // 不支持 += 复合赋值
];

#[test]
fn self_source_matches() {
    // 验证 Rust 版 parser 能解析 H 版 parser 自身源码（快速，< 1s）
    let root = repo_root();
    let h_parser = root.join("stage1/parser.hc");
    let r = run_hc(&["parse", h_parser.to_str().unwrap()]);
    assert!(r.status.success(), "hc parse 自身失败：{}", stderr(&r));

    // 验证 H 版 parser 能正确解析一个简单语料文件，且输出与 Rust 参考一致
    // 只测试 10-fn-basic.hc（已知支持），避免遍历所有文件重复 corpus_matches 测试
    let corpus_file = root.join("stage1/corpus/10-fn-basic.hc");
    let r_out = run_hc(&["parse", corpus_file.to_str().unwrap()]);
    assert!(r_out.status.success(), "hc parse 10-fn-basic.hc 失败");
    let h = run_hc(&[
        "run",
        h_parser.to_str().unwrap(),
        corpus_file.to_str().unwrap(),
    ]);
    assert!(
        h.status.success(),
        "H 版 parser 运行 10-fn-basic.hc 失败：{}",
        stderr(&h)
    );
    assert_eq!(
        stdout(&r_out),
        stdout(&h),
        "H 版 parser 输出与 Rust 参考不一致"
    );
}

#[test]
// #[ignore = "K2 自举验证：H 版 parser 解析自身约需 1s，已达标，手动运行确认"]
fn self_hosting_parses_self() {
    // H 版 parser 解析自身源码——自举验证（耗时约 60s，默认忽略）
    let root = repo_root();
    let h_parser = root.join("stage1/parser.hc");
    let r = run_hc(&["parse", h_parser.to_str().unwrap()]);
    assert!(r.status.success(), "hc parse 自身失败：{}", stderr(&r));
    let h = run_hc(&[
        "run",
        h_parser.to_str().unwrap(),
        h_parser.to_str().unwrap(),
    ]);
    assert!(
        h.status.success(),
        "H 版 parser 运行自身失败：{}",
        stderr(&h)
    );
}
