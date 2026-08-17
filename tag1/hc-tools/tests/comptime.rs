//! 组 D D2：`comptime { }` 块编译期求值端到端测试（ADR-0012，E1.2）。
//!
//! 用 `hc` 二进制（CARGO_BIN_EXE_hc-tools）驱动 CLI，覆盖最小切片：
//!   - 块通过：`types` 全量可见（all / fields），求值后丢弃、正常进入运行
//!   - 块失败：`return error.X` = 编译错误（带块内位置 + 所属块位置）
//!   - 负例：`types.fields` 未知类型 / io 禁用 → 编译错误
//!   - 顺序：script 展开后求值——comptime 块可见 script 生成类型

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
        "hc_comptime_{}_{}_{}",
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

/// stdout + stderr 合并（编译错误诊断可能在任一流）
fn combined(out: &Output) -> String {
    format!("{}\n{}", stdout(out), stderr(out))
}

#[test]
fn comptime_block_passes_and_program_runs() {
    // 块通过：types 全量可见（all / fields），求值结果丢弃，main 正常执行
    let dir = temp_dir("pass");
    let file = write(
        &dir,
        "pass.hc",
        "import H.std.{io};\n\
         class User { name: String }\n\
         comptime {\n\
         \x20   if (types.all.len() < 1) { return error.NoTypes; }\n\
         \x20   if (types.fields(\"User\").len() != 1) { return error.BadSchema; }\n\
         }\n\
         fn main(args: o Vec(String)) !void {\n\
         \x20   io.print(\"comptime ok\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = stdout(&out);
    assert!(out.status.success(), "comptime 块通过应运行成功: {}", combined(&out));
    assert!(s.contains("comptime ok"), "main 应正常执行: {s}");
    // 三后端一致：IR 装载同样通过
    let out_ir = run_hc(&[Path::new("run"), Path::new("--ir"), &file]);
    assert!(out_ir.status.success(), "IR 装载应一致通过: {}", combined(&out_ir));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comptime_return_error_is_compile_error() {
    // 块失败：return error.X = 编译错误（带块内位置 + 错误名）
    let dir = temp_dir("fail");
    let file = write(
        &dir,
        "fail.hc",
        "import H.std.{io};\n\
         comptime {\n\
         \x20   return error.BadSchema;\n\
         }\n\
         fn main(args: o Vec(String)) !void {\n\
         \x20   io.print(\"unreachable\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = combined(&out);
    assert!(!out.status.success(), "comptime 块返回 error 应为编译失败: {s}");
    assert!(s.contains("BadSchema"), "诊断应含错误名: {s}");
    assert!(s.contains("comptime 块返回错误"), "诊断应说明块返回错误: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comptime_unknown_type_fields_is_compile_error() {
    // 负例：types.fields 未知类型 → UnknownType 编译错误
    let dir = temp_dir("unknown");
    let file = write(
        &dir,
        "unknown.hc",
        "import H.std.{io};\n\
         comptime {\n\
         \x20   var f = types.fields(\"NoSuchType\");\n\
         \x20   _ = f;\n\
         }\n\
         fn main(args: o Vec(String)) !void {\n\
         \x20   io.print(\"unreachable\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = combined(&out);
    assert!(!out.status.success(), "未知类型应为编译失败: {s}");
    assert!(s.contains("UnknownType"), "诊断应含 UnknownType: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comptime_io_forbidden() {
    // 负例：comptime 块中 io 不可用（受限 H 核心子集，同 script）
    let dir = temp_dir("io");
    let file = write(
        &dir,
        "io.hc",
        "import H.std.{io};\n\
         comptime {\n\
         \x20   var x = io;\n\
         \x20   _ = x;\n\
         }\n\
         fn main(args: o Vec(String)) !void {\n\
         \x20   io.print(\"unreachable\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = combined(&out);
    assert!(!out.status.success(), "comptime 块中 io 应为编译失败: {s}");
    assert!(s.contains("io 不可用"), "诊断应说明 io 受限: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comptime_sees_script_generated_types() {
    // 顺序：script 展开后求值——comptime 块可见 script 生成的 class
    let dir = temp_dir("script");
    let file = write(
        &dir,
        "script.hc",
        "import H.std.{io};\n\
         class Point { x: i32 }\n\
         script {\n\
         \x20   var fields = types.fields(\"Point\");\n\
         \x20   var out = \"class Gen { \";\n\
         \x20   for (fields) |f| {\n\
         \x20       out = out.concat(\"gen_\").concat(f[0]).concat(\": \")\n\
         \x20           .concat(f[1]).concat(\", \");\n\
         \x20   }\n\
         \x20   out.concat(\"}\");\n\
         }\n\
         comptime {\n\
         \x20   if (types.fields(\"Gen\").len() < 1) { return error.NoGen; }\n\
         }\n\
         fn main(args: o Vec(String)) !void {\n\
         \x20   io.print(\"script+comptime ok\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = stdout(&out);
    assert!(out.status.success(), "comptime 应见 script 生成类型: {}", combined(&out));
    assert!(s.contains("script+comptime ok"), "main 应正常执行: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}
