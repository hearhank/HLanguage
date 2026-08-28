//! B1：`hc lint`——静态诊断命令（6 条规则 L001–L006）

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::lint;
use crate::project::fsio::collect_hc_files;
use crate::script;

use super::usage::USAGE;

/// B1：`hc lint <file.hc|dir> [--json] [--fix]`——静态诊断（6 条规则 L001–L006）
pub(crate) fn lint_command(args: &[String]) -> ExitCode {
    let mut json = false;
    let mut fix = false;
    let mut targets: Vec<&String> = Vec::new();
    for a in args {
        if a == "--json" {
            json = true;
        } else if a == "--fix" {
            fix = true;
        } else {
            targets.push(a);
        }
    }
    if targets.is_empty() {
        eprintln!("error: `hc lint` requires a file or directory\n\n{USAGE}");
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
    let mut all_diags = Vec::new();
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
        match script::parse_with_scripts(&source) {
            Ok((_expanded, program)) => {
                let diags = lint::lint_source(&source, &program, fix);
                if !json {
                    for d in &diags {
                        eprintln!("{}: {}", f.display(), d.render(&source));
                    }
                }
                all_diags.extend(diags);
            }
            Err(msg) => {
                eprintln!("error: {}: {msg}", f.display());
                failed = true;
            }
        }
    }
    if json {
        if !all_diags.is_empty() {
            let file = if files.len() == 1 {
                files[0].to_string_lossy().to_string()
            } else {
                "(multiple)".to_string()
            };
            println!("{}", lint::diags_to_json(&all_diags, &file));
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
