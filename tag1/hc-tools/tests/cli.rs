//! C1 CLI 测试：`hc run <目录>` 包加载形态——入口 `main.hc` 优先/首个 `.hc`、
//! 兄弟文件合并 + build.zon 依赖装载（复用 run_file 路径，无需 zig）。
//!
//! 用 `hc` 二进制（CARGO_BIN_EXE_hc-tools）驱动 CLI，断言输出与退出码。

use std::path::{Path, PathBuf};
use std::process::Command;

fn hc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_hc"))
}

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples")
}

fn zig_cc_available() -> bool {
    Command::new("zig")
        .arg("cc")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 递归复制目录（保留 app → ../jsonlib 的相对依赖结构）。
fn copy_dir_all(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("create dst dir");
    for entry in std::fs::read_dir(src).expect("read src dir") {
        let entry = entry.expect("read entry");
        let to = dst.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_dir_all(&entry.path(), &to);
        } else {
            // 跳过既有构建产物（*.exe/*.a/*.sym），测试从源码态重新构建
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".exe") || name.ends_with(".a") || name.ends_with(".sym") {
                continue;
            }
            std::fs::copy(entry.path(), to).expect("copy file");
        }
    }
}

#[test]
fn run_package_directory_uses_main_entry_and_deps() {
    // 02-packages/app：main.hc 入口 + build.zon 本地依赖 jsonlib（`import jsonlib.{parse}`）
    let out = Command::new(hc_bin())
        .arg("run")
        .arg(examples_dir().join("02-packages/app"))
        .output()
        .expect("run hc");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "目录 run 应成功: {stdout}{stderr}");
    assert!(
        stdout.contains("jsonlib.parse = 42"),
        "应输出依赖包调用结果: {stdout}{stderr}"
    );
}

#[test]
fn run_directory_prefers_main_hc_else_first_hc() {
    // 临时包目录：只有 a.hc（含 main）→ 首个 .hc 作入口
    let dir = std::env::temp_dir().join(format!(
        "hc_cli_dir_entry_{}_{}",
        std::process::id(),
        std::process::id().wrapping_mul(31) % 100000
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("a.hc"),
        "import H.std.{io};\nfn main(args: o Vec(String)) !void { io.print(\"first-hc\\n\"); }\n",
    )
    .unwrap();
    let out = Command::new(hc_bin())
        .arg("run")
        .arg(&dir)
        .output()
        .expect("run hc");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(out.status.success(), "首个 .hc 入口应成功: {stdout}");
    assert!(stdout.contains("first-hc"), "应运行 a.hc: {stdout}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_directory_without_hc_errors() {
    let dir = std::env::temp_dir().join(format!(
        "hc_cli_empty_{}_{}",
        std::process::id(),
        std::process::id().wrapping_mul(17) % 100000
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let out = Command::new(hc_bin())
        .arg("run")
        .arg(&dir)
        .output()
        .expect("run hc");
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(!out.status.success(), "空目录应失败");
    assert!(
        stderr.contains("无 .hc 文件"),
        "应提示无 .hc 文件: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_lib_static_archive_and_link_exe() {
    // C3：`hc build` 库形态——jsonlib（Kind::lib）→ libjsonlib.a + .sym；
    // app（Kind::exe，本地依赖）链接库 → main.exe，运行输出依赖函数结果。
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    let src = examples_dir().join("02-packages");
    let dir = std::env::temp_dir().join(format!(
        "hc_cli_c3_{}_{}",
        std::process::id(),
        std::process::id().wrapping_mul(13) % 100000
    ));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir_all(&src, &dir);

    // 1) 库产出（静态归档 + 符号表）
    let out = Command::new(hc_bin())
        .arg("build")
        .arg(dir.join("jsonlib"))
        .output()
        .expect("hc build lib");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "库构建应成功: {stdout}{stderr}");
    assert!(
        dir.join("jsonlib/libjsonlib.a").exists(),
        "应产出 libjsonlib.a: {stdout}{stderr}"
    );
    assert!(dir.join("jsonlib/jsonlib.sym").exists(), "应产出 .sym 符号表");

    // 2) exe 链接本地库 + 运行
    let out = Command::new(hc_bin())
        .arg("build")
        .arg(dir.join("app"))
        .output()
        .expect("hc build app");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "app 构建应成功: {stdout}{stderr}");
    let exe = dir.join("app/main.exe");
    assert!(exe.exists(), "应产出 main.exe: {stdout}{stderr}");
    let run = Command::new(&exe).output().expect("run exe");
    let out_text = String::from_utf8_lossy(&run.stdout).to_string();
    assert!(
        out_text.contains("jsonlib.parse = 42"),
        "exe 应调用依赖库函数输出 42: {out_text}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
