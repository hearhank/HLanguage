//! 文件系统 I/O：源码/字节码读写、`zig cc` 探测、原生链接、`.hc` 收集。

use std::path::{Path, PathBuf};

use hc::diag;

use crate::package::lower_err;
use crate::scriptgen;

/// 旧字节码镜像魔数（tag1 过渡形态：镜像 = 魔数 + 源码；仅保留读取兼容，
/// 新 `hc build` 回退产出真实 HBC2 字节码）
const HBC_MAGIC: &[u8; 4] = b"HBC1";

/// 读取 .hc 或 .hbc（字节码镜像解包）
pub(crate) fn read_program(path: &Path) -> Result<String, std::process::ExitCode> {
    let bytes = std::fs::read(path).map_err(|e| {
        eprintln!("error: cannot read {}: {e}", path.display());
        std::process::ExitCode::FAILURE
    })?;
    if bytes.len() >= 4 && &bytes[..4] == HBC_MAGIC {
        // 镜像：跳过魔数 + u64 长度前缀，取源码
        if bytes.len() < 12 {
            eprintln!("error: {}: 损坏的字节码镜像", path.display());
            return Err(std::process::ExitCode::FAILURE);
        }
        let len = u64::from_le_bytes(bytes[4..12].try_into().unwrap()) as usize;
        let src = &bytes[12..12 + len.min(bytes.len() - 12)];
        match String::from_utf8(src.to_vec()) {
            Ok(s) => Ok(s),
            Err(_) => {
                eprintln!("error: {}: 镜像源码非 UTF-8", path.display());
                Err(std::process::ExitCode::FAILURE)
            }
        }
    } else {
        String::from_utf8(bytes).map_err(|_| {
            eprintln!("error: {}: 非 UTF-8 源码", path.display());
            std::process::ExitCode::FAILURE
        })
    }
}

/// 判断文件是否为 HBC2 字节码（M3.2 VM 镜像）。
pub(crate) fn is_hbc2(path: &Path) -> bool {
    std::fs::read(path)
        .map(|b| b.len() >= 4 && &b[..4] == &hc::bytecode::MAGIC)
        .unwrap_or(false)
}

/// 读取 HBC2 字节码文件；魔数不符/读取失败返回退出码。
pub(crate) fn read_bytecode(path: &Path) -> Result<Vec<u8>, std::process::ExitCode> {
    let bytes = std::fs::read(path).map_err(|e| {
        eprintln!("error: cannot read {}: {e}", path.display());
        std::process::ExitCode::FAILURE
    })?;
    if bytes.len() < 4 || &bytes[..4] != &hc::bytecode::MAGIC {
        eprintln!("error: {}: 不是 HBC2 字节码", path.display());
        return Err(std::process::ExitCode::FAILURE);
    }
    Ok(bytes)
}

/// `zig cc` 是否可用（M3.3 原生后端驱动；缺失则回退字节码）
pub(crate) fn zig_cc_available() -> bool {
    std::process::Command::new("zig")
        .arg("cc")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 源码 → HBC2 字节码（script 展开 → 解析 → 语义检查 → `lower` → `encode`）。
/// 失败返回可直接打印的诊断文本（与 `programs_to_ll` 同前置检查）。
pub(crate) fn source_to_bytecode(source: &str) -> Result<Vec<u8>, String> {
    // E1（ADR-0013）：装载期 script 展开（无 script 块时零开销快速路径）
    let (expanded, program) = scriptgen::parse_with_scripts(source)?;
    let errs = hc::check_semantics(&program);
    if errs.iter().any(|d| d.is_error()) {
        return Err(diag::render(&errs, &expanded));
    }
    let module = hc::ir::lower(&program).map_err(lower_err)?;
    Ok(hc::bytecode::encode(&module))
}

/// 将字节码写入 `<dir>/<stem>.hbc`，返回产物路径。
pub(crate) fn write_bytecode_artifact(
    dir: &Path,
    stem: &str,
    bytes: &[u8],
) -> Result<PathBuf, String> {
    let hbc_path = dir.join(format!("{stem}.hbc"));
    std::fs::write(&hbc_path, bytes)
        .map_err(|e| format!("写入 {} 失败: {e}", hbc_path.display()))?;
    Ok(hbc_path)
}

/// `zig cc <ll> -o <exe>`（M3.3 原生链接）。返回 Ok 或带诊断的 Err。
pub(crate) fn link_exe(ll_path: &Path, exe_path: &Path, extra: &[PathBuf]) -> Result<(), String> {
    let mut cmd = std::process::Command::new("zig");
    cmd.arg("cc").arg(ll_path);
    for a in extra {
        cmd.arg(a);
    }
    cmd.arg("-o").arg(exe_path);
    let out = cmd.output();
    match out {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(format!(
            "zig cc 编译失败：\n{}",
            String::from_utf8_lossy(&o.stderr)
        )),
        Err(e) => Err(format!("调用 zig cc 失败: {e}")),
    }
}

pub(crate) fn collect_hc_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // 设计草图目录（`examples/study/`）不计入示例回归基线——09 组 A6
            // （示例全量迁移 main(args) + import）时再纳入。
            if path.file_name().map_or(false, |n| n == "study") {
                continue;
            }
            collect_hc_files(&path, out);
        } else if path.extension().map_or(false, |e| e == "hc") {
            out.push(path);
        }
    }
}
