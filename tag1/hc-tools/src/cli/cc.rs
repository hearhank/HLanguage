//! A1（ADR-0020）：`hc cc`——zig cc 封装命令

use std::process::ExitCode;

use crate::project::fsio::zig_cc_available;

use super::usage::USAGE;

/// A1（ADR-0020）：`hc cc <file.c> [--output <file>]`——zig cc 封装，
/// 编译 C 源文件为目标文件或可执行文件（与 `hc build` 共用同一链接器）。
pub(crate) fn cc_command(args: &[String]) -> ExitCode {
    let Some(path) = args.first() else {
        eprintln!("error: `hc cc` requires a C source file\n\n{USAGE}");
        return ExitCode::from(2);
    };
    if !zig_cc_available() {
        eprintln!("error: `hc cc` requires zig to be installed (zig cc)");
        return ExitCode::FAILURE;
    }
    let mut cmd = std::process::Command::new("zig");
    cmd.arg("cc");
    cmd.arg(path);
    let mut out = 2;
    while out < args.len() {
        if args[out] == "--output" {
            out += 1;
            if let Some(output) = args.get(out) {
                cmd.arg("-o");
                cmd.arg(output);
            } else {
                eprintln!("error: `--output` requires a file path");
                return ExitCode::from(2);
            }
        } else {
            // 透传其他参数给 zig cc
            cmd.arg(&args[out]);
        }
        out += 1;
    }
    match cmd.status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => {
            eprintln!(
                "error: zig cc 失败 (exit code: {})",
                status.code().unwrap_or(-1)
            );
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("error: 调用 zig cc 失败: {e}");
            ExitCode::FAILURE
        }
    }
}
