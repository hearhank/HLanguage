//! hc-tools/tests/lint.rs
//!
//! L004 upper_case_abbr 回归测试：
//! 只在 snake_case 分段上完全匹配缩写，子串（ident/width/dbg 等）不误报。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static DIR_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn hc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_hc"))
}

fn temp_dir(tag: &str) -> PathBuf {
    let n = DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("hc_lint_{}_{}_{}", std::process::id(), tag, n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, name: &str, src: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, src).unwrap();
    path
}

fn run_lint(file: &Path) -> Output {
    Command::new(hc_bin())
        .args(["lint"])
        .arg(file)
        .output()
        .expect("run hc lint")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

#[test]
fn l004_ignores_substring_matches() {
    // 子串不构成缩写：ident 中的 "id"、width 中的 "id"、dbg 中的 "db"、恒等函数 id
    let dir = temp_dir("substr");
    let file = write(
        &dir,
        "substr.hc",
        "fn is_ident_start() bool { return true; }\n\
         fn is_ident_cont() bool { return true; }\n\
         fn utf8_width() i32 { return 1; }\n\
         fn dbg_escape() void {}\n\
         fn id(x: i32) i32 { return x; }\n",
    );
    let out = run_lint(&file);
    let e = stderr(&out);
    assert!(
        !e.contains("upper_case_abbr"),
        "子串/通用词不应触发 L004: {e}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn l004_flags_full_segment_and_suggests_upper() {
    // snake_case 分段完全匹配才报，且给出全大写建议
    let dir = temp_dir("fullseg");
    let file = write(
        &dir,
        "fullseg.hc",
        "fn json_response() void {}\n\
         fn parse_html_file() void {}\n",
    );
    let out = run_lint(&file);
    let e = stderr(&out);
    assert!(
        e.contains("缩写 `json` 应全大写（`json_response` → `JSON_response`）"),
        "json 段应报 L004: {e}"
    );
    assert!(
        e.contains("缩写 `html` 应全大写（`parse_html_file` → `parse_HTML_file`）"),
        "html 段应报 L004: {e}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn l004_skips_already_upper_segments() {
    // 已全大写的段不重复报
    let dir = temp_dir("upper");
    let file = write(
        &dir,
        "upper.hc",
        "fn from_JSON() void {}\n\
         fn render_HTML() void {}\n",
    );
    let out = run_lint(&file);
    let e = stderr(&out);
    assert!(!e.contains("upper_case_abbr"), "全大写段不应触发 L004: {e}");
    let _ = std::fs::remove_dir_all(&dir);
}
