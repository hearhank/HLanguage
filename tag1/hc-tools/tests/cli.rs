//! C1 CLI 测试：`hc run <目录>` 包加载形态——入口 `main.hc` 优先/首个 `.hc`、
//! 兄弟文件合并 + build.zon 依赖装载（复用 run_file 路径，无需 zig）。
//!
//! 用 `hc` 二进制（CARGO_BIN_EXE_hc-tools）驱动 CLI，断言输出与退出码。

use std::path::PathBuf;
use std::process::Command;

fn hc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_hc"))
}

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples")
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
