//! H4：`hc doc`——Markdown 文档生成命令

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::doc;

/// H4：`hc doc [target] [--out <dir>]`——生成 Markdown 文档（`///` 注释 + 声明签名）。
///
/// - target：文件 / 目录（包）/ `std`（标准库内置目录页）；默认 `.`（当前目录 = 包）。
/// - 输出目录约定：默认 `<target 所在目录>/docs/api/`；`--out <dir>` 覆盖。
/// - `std` 页：H.std 为 Rust 内建（无 .hc 源），输出内置目录化摘要页。
pub(crate) fn doc_command(args: &[String]) -> ExitCode {
    let mut target = ".".to_string();
    let mut out: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--out" {
            i += 1;
            out = args.get(i).cloned();
        } else if let Some(v) = a.strip_prefix("--out=") {
            out = Some(v.to_string());
        } else {
            target = a.clone();
        }
        i += 1;
    }
    if target == "std" {
        let out_dir = out
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("docs/api"));
        match doc::generate_stdlib(&out_dir) {
            Ok(p) => {
                let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                println!("生成 {}（{} 字节）", p.display(), size);
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        }
    } else {
        let target_path = PathBuf::from(&target);
        let out_dir = out.map(PathBuf::from).unwrap_or_else(|| {
            let base = if target_path.is_dir() {
                target_path.clone()
            } else {
                target_path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .to_path_buf()
            };
            base.join("docs/api")
        });
        if target_path.is_dir() {
            match doc::generate_project(&target_path, &out_dir) {
                Ok(paths) => {
                    println!("生成 {} 个文件到 {}", paths.len(), out_dir.display());
                    for p in &paths {
                        println!("  {}", p.display());
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            }
        } else if target_path.is_file() {
            match doc::generate_file(&target_path, &out_dir) {
                Ok(p) => {
                    println!("生成 {}", p.display());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            }
        } else {
            eprintln!("error: `{}` 不是文件/目录或 `std`", target_path.display());
            ExitCode::from(2)
        }
    }
}
