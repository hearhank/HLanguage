//! hc-tools/tests/fmt.rs

use std::path::{Path, PathBuf};
use std::process::Command;

fn hc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_hc"))
}

/// 仓库根 examples/（本测试所在 hc-tools 的上级上级）。
fn repo_examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "hc_fmt_{}_{}_{}",
        std::process::id(),
        tag,
        std::process::id().wrapping_mul(131) % 100000
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_fmt(args: &[&str]) -> std::process::Output {
    Command::new(hc_bin())
        .arg("fmt")
        .args(args)
        .output()
        .expect("run hc fmt")
}

/// 对单个源码字符串：写入临时 .hc → `hc fmt` → 断言成功 + `hc fmt --check` 收敛。
fn assert_snippet_idempotent(tag: &str, src: &str) {
    let dir = temp_dir(&format!("snippet_{tag}"));
    let file = dir.join("t.hc");
    std::fs::write(&file, src).unwrap();
    let out = run_fmt(&[file.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "fmt 应成功: {stderr}");
    let check = run_fmt(&["--check", file.to_str().unwrap()]);
    let msg = format!(
        "片段 `{tag}` 一次格式化后应幂等收敛（--check exit 0）:\n--- 输出 ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(check.status.success(), "{msg}");
}

// ---------- 回归：仅含注释的块不折叠 ----------

#[test]
fn comment_only_block_stays_idempotent() {
    assert_snippet_idempotent(
        "comment_only_block",
        "class Foo {\n    fn bar(self: *Self) String {\n        // 说明\n    }\n}\n",
    );
    // 行尾注释形态同样收敛（`{ // 说明` 保持，不双写空格）
    assert_snippet_idempotent(
        "trailing_comment_block",
        "fn f() void { // 说明\n}\n",
    );
}

#[test]
fn empty_block_stays_inline() {
    assert_snippet_idempotent(
        "empty_block",
        "fn f() !void {\n    user_validate(&u) catch |e| {};\n}\n",
    );
}

// ---------- 代表性排版形态幂等 ----------

#[test]
fn representative_shapes_idempotent() {
    // 多行数组字面量（垂直保留）
    assert_snippet_idempotent(
        "multiline_array",
        "fn f() [3]i32 {\n    return [\n        1,\n        2,\n        3,\n    ];\n}\n",
    );
    // 多行调用（垂直实参式）
    assert_snippet_idempotent(
        "multiline_call",
        "fn f() {\n    io.print(\n        \"{}\\n\",\n        n,\n    );\n}\n",
    );
    // 多行 struct 字面量
    assert_snippet_idempotent(
        "multiline_struct",
        "fn f() {\n    var p = Point{\n        x = 1.0,\n        y = 2.0,\n    };\n}\n",
    );
    // 方法链跨行 + 尾注释对齐
    assert_snippet_idempotent(
        "chain_and_align",
        "fn f() {\n    var out = s.concat(\"a\")\n        .concat(\"b\");   // 注释\n}\n",
    );
    // 闭包捕获 `filter(|v| …)` + 空块保持行内
    assert_snippet_idempotent(
        "closure_capture",
        "fn f() {\n    var evens = arr.filter(|v| v % 2 == 0);\n}\n",
    );
    // 返回类型元组 `fn divmod() (i32, i32)` + 解引用 `b.*` 二元运算
    assert_snippet_idempotent(
        "return_tuple_and_deref",
        "fn divmod(n: i32) (i32, i32) {\n    var b = &n;\n    return b.* + 1, b.* - 1;\n}\n",
    );
    // 方法名关键字 `where`（成员访问形态）
    assert_snippet_idempotent(
        "where_method_name",
        "fn f() {\n    var q = builder.where(name = \"x\");\n}\n",
    );
}

// ---------- 示例断言：整个语料一次格式化收敛 ----------

#[test]
fn examples_corpus_idempotent() {
    let src = repo_examples_dir();
    assert!(src.is_dir(), "examples/ 目录应存在: {}", src.display());
    let tmp = temp_dir("examples");
    copy_dir_all(&src, &tmp);

    let out = run_fmt(&[tmp.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "hc fmt examples/ 应成功: {stderr}");

    let check = run_fmt(&["--check", tmp.to_str().unwrap()]);
    let msg = format!(
        "示例语料一次格式化后应幂等收敛（无 would reformat）:\n{}",
        String::from_utf8_lossy(&check.stdout)
    );
    assert!(check.status.success(), "{msg}");
}

// ---------- 工具 ----------

/// 递归复制目录（跳过构建产物/二进制，fmt 仅需源码态）。
fn copy_dir_all(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("create dst dir");
    for entry in std::fs::read_dir(src).expect("read src dir") {
        let entry = entry.expect("read entry");
        let to = dst.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_dir_all(&entry.path(), &to);
        } else {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".exe")
                || name.ends_with(".dll")
                || name.ends_with(".lib")
                || name.ends_with(".pdb")
                || name.ends_with(".a")
                || name.ends_with(".sym")
            {
                continue;
            }
            std::fs::copy(entry.path(), to).expect("copy file");
        }
    }
}
