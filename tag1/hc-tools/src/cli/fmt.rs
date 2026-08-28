//! I1：`hc fmt`——token 级格式化命令

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::fmt;
use crate::project::fsio::collect_hc_files;

use super::usage::USAGE;

/// I1：`hc fmt <file.hc|dir> [--check]`——token 级格式化，AST 保真。
/// 默认原地写回；`--check` 仅报告将改动的文件并以退出码 1 结束（CI 用）。
/// 格式化前用 token 序列自检：产物必须词法干净且 token 序列与源一致（保真保证）。
pub(crate) fn fmt_command(args: &[String]) -> ExitCode {
    let mut check = false;
    let mut targets: Vec<&String> = Vec::new();
    for a in args {
        if a == "--check" {
            check = true;
        } else {
            targets.push(a);
        }
    }
    if targets.is_empty() {
        eprintln!("error: `hc fmt` requires a file or directory\n\n{USAGE}");
        return ExitCode::from(2);
    }
    let mut files: Vec<PathBuf> = Vec::new();
    for t in targets {
        let p = Path::new(t);
        if p.is_dir() {
            collect_hc_files(p, &mut files);
        } else if p.is_file() {
            files.push(p.to_path_buf());
        } else {
            eprintln!("error: 找不到 {t}");
            return ExitCode::FAILURE;
        }
    }
    files.sort();
    files.dedup();
    let mut would_change = false;
    let mut failed = false;
    for f in &files {
        let source = match std::fs::read_to_string(f) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: 读取 {} 失败: {e}", f.display());
                failed = true;
                continue;
            }
        };
        let sig1 = match fmt::token_signature(&source) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: {} 词法失败: {e}", f.display());
                failed = true;
                continue;
            }
        };
        match fmt::format_source(&source) {
            Ok(formatted) => {
                let sig2 = match fmt::token_signature(&formatted) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("error: {} 格式化产物词法失败: {e}", f.display());
                        failed = true;
                        continue;
                    }
                };
                if sig1 != sig2 {
                    eprintln!("error: {} 格式化后 token 序列变化（内部错误）", f.display());
                    failed = true;
                    continue;
                }
                if formatted != source {
                    if check {
                        would_change = true;
                        println!("would reformat {}", f.display());
                    } else {
                        if let Err(e) = std::fs::write(f, &formatted) {
                            eprintln!("error: 写回 {} 失败: {e}", f.display());
                            failed = true;
                            continue;
                        }
                        println!("formatted {}", f.display());
                    }
                }
            }
            Err(e) => {
                eprintln!("error: {}: {e}", f.display());
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else if check && would_change {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
