//! CLI 命令分发器：hc run/test/build/check/fmt/lint/doc/pkg 等命令解析
//!
//! 类型定义在 models/ 下（一个类型一个文件，ADR-0028）；
//! 各命令实现按功能拆分到同名功能文件。

pub(crate) mod versiongen;

mod models;

mod args;
mod cc;
mod check;
mod colors;
mod doc;
mod dump;
mod fmt;
mod lex;
mod lint;
mod read_source;
mod usage;

pub(crate) use models::*;

pub(crate) use args::{extract_dangle, parse_dangle_mode, parse_test_mode};
pub(crate) use cc::cc_command;
pub(crate) use check::{check_file, errors_file};
pub(crate) use colors::{color_test_line, err_color, out_color, paint};
pub(crate) use doc::doc_command;
pub(crate) use dump::parse_command;
pub(crate) use fmt::fmt_command;
pub(crate) use lex::lex_command;
pub(crate) use lint::lint_command;
pub(crate) use usage::USAGE;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::build::build_file;
use crate::pkg::package_entry;
use crate::project::fsio::is_hbc2;
use crate::project::{init_project, pkg_add, pkg_publish};
use crate::run::{
    program_args, run_file_bytecode, run_file_dangle_bench, run_file_hs, run_file_ir,
};
use crate::test::test_dir_dangle;

pub(crate) fn run_cli() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let cmd = args[1].as_str();
    match cmd {
        "--help" | "-h" => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        "--version" | "-V" => {
            println!("hc {} (tag1)", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        "run" => {
            let bench = args.get(2).map_or(false, |a| a == "--bench");
            let path_offset = if bench { 1 } else { 0 };
            let path = args.get(2 + path_offset).map(|s| s.as_str()).unwrap_or(".");
            // C2（ADR-0016）：从剩余参数提取 --dangle 标志；安全处理 args 不足的情况
            let dangle_start = 3 + path_offset;
            let (dangle_mode, rest_start) = if args.len() > dangle_start {
                extract_dangle(&args, dangle_start)
            } else {
                (DangleMode::Auto, args.len())
            };
            // 显式模式标志：`hc run --ir <file>` 走 IR 参考解释器；
            // `.hbc`（HBC2 字节码）走字节码 VM；否则默认 tree-walking
            if path == "--ir" {
                let Some(p) = args.get(3 + path_offset) else {
                    eprintln!("error: `hc run --ir` requires a file path");
                    return ExitCode::from(2);
                };
                // 程序参数：`hc run --ir <file> <args...>` → [程序名] + args（0 号 = 程序名）
                let prog_args = program_args(&args[rest_start..], p);
                run_file_ir(Path::new(p), &prog_args)
            } else if is_hbc2(Path::new(path)) {
                let prog_args = program_args(&args[rest_start..], path);
                run_file_bytecode(Path::new(path), &prog_args)
            } else if path.ends_with(".hs") {
                // B6-2（E5.6）：`.hs` 脚本文件——直接执行，无 script 展开、无编译模式
                if bench {
                    eprintln!("warning: --bench 对 .hs 文件无效");
                }
                let prog_args = program_args(&args[rest_start..], path);
                run_file_hs(Path::new(path), &prog_args)
            } else if Path::new(path).is_dir() {
                // Q13：`hc run <dir>` 验证——目录必须含 build.zon + main.hc
                let dir = Path::new(path);
                if !dir.join("build.zon").exists() {
                    eprintln!(
                        "error: 目录 {} 缺少 build.zon（项目清单；`hc run <dir>` 需项目目录）",
                        dir.display()
                    );
                    return ExitCode::FAILURE;
                }
                // M4-1：编译时版本号自增（version.hc 存在时更新 build 和 time）
                crate::cli::versiongen::bump_version(dir);
                match package_entry(dir) {
                    Ok(entry) => {
                        let entry_s = entry.to_string_lossy().into_owned();
                        let prog_args = program_args(&args[rest_start..], &entry_s);
                        run_file_dangle_bench(&entry, &prog_args, dangle_mode, bench)
                    }
                    Err(msg) => {
                        eprintln!("error: {msg}");
                        ExitCode::FAILURE
                    }
                }
            } else {
                let prog_args = program_args(&args[rest_start..], path);
                run_file_dangle_bench(Path::new(path), &prog_args, dangle_mode, bench)
            }
        }
        // 调试：打印 script 块展开后的源码（已移除——script 块已迁移到 .hs 文件）
        "dump-scripts" => {
            eprintln!("error: `dump-scripts` 已移除（script 块已从 .hc 中移除，见 docs/SPEC/phase3/12-script-redesign.md）");
            ExitCode::FAILURE
        }
        "test" => {
            // 解析可选 --mode=interpret|compile（默认 interpret）与目标路径
            let mut mode = TestMode::Interpret;
            let mut dangle_mode = DangleMode::Auto;
            let mut target = PathBuf::from(".");
            let mut i = 2;
            while i < args.len() {
                let a = &args[i];
                if let Some(v) = a.strip_prefix("--mode=") {
                    mode = match parse_test_mode(v) {
                        Ok(m) => m,
                        Err(c) => return c,
                    };
                } else if a == "--mode" {
                    i += 1;
                    let Some(v) = args.get(i) else {
                        eprintln!("error: `--mode` 需要取值（interpret|compile）");
                        return ExitCode::from(2);
                    };
                    mode = match parse_test_mode(v) {
                        Ok(m) => m,
                        Err(c) => return c,
                    };
                } else if let Some(v) = a.strip_prefix("--dangle=") {
                    dangle_mode = match parse_dangle_mode(v) {
                        Ok(m) => m,
                        Err(c) => return c,
                    };
                } else {
                    target = PathBuf::from(a);
                }
                i += 1;
            }
            test_dir_dangle(&target, mode, dangle_mode)
        }
        "check" => {
            let Some(path) = args.get(2) else {
                eprintln!("error: `hc check` requires a file path");
                return ExitCode::from(2);
            };
            match check_file(Path::new(path)) {
                Ok(()) => {
                    println!("OK");
                    ExitCode::SUCCESS
                }
                Err(code) => code,
            }
        }
        "errors" => {
            let Some(path) = args.get(2) else {
                eprintln!("error: `hc errors` requires a file path");
                return ExitCode::from(2);
            };
            errors_file(Path::new(path))
        }
        "build" => {
            // C4：`hc build [--dll] <path>`——`--dll` = 库产 dll / exe 依赖按 dll 链接
            let mut dll = false;
            let mut target: Option<&String> = None;
            for a in args.iter().skip(2) {
                if a == "--dll" {
                    dll = true;
                } else {
                    target = Some(a);
                }
            }
            let path = target.map(|s| s.as_str()).unwrap_or(".");
            let build_path = Path::new(path);
            build_file(build_path, dll)
        }
        "init" => {
            // H1：`hc init <name>`——创建新项目骨架（build.zon + main.hc）
            let Some(name) = args.get(2) else {
                eprintln!("error: `hc init` requires a project name\n\n{USAGE}");
                return ExitCode::from(2);
            };
            init_project(name)
        }
        "pkg" => match args.get(2).map(|s| s.as_str()) {
            Some("add") => {
                let Some(name) = args.get(3) else {
                    eprintln!("error: `hc pkg add` requires a package name\n\n{USAGE}");
                    return ExitCode::from(2);
                };
                let mut path: Option<String> = None;
                let mut version: Option<String> = None;
                let mut i = 4;
                while i < args.len() {
                    match args[i].as_str() {
                        "--path" => {
                            i += 1;
                            path = args.get(i).cloned();
                        }
                        "--version" => {
                            i += 1;
                            version = args.get(i).cloned();
                        }
                        other => {
                            eprintln!("error: `hc pkg add` 未知选项 `{other}`");
                            return ExitCode::from(2);
                        }
                    }
                    i += 1;
                }
                pkg_add(name, &path, &version)
            }
            Some("publish") => pkg_publish(),
            _ => {
                eprintln!("error: `hc pkg` 子命令支持 `add` / `publish`\n\n{USAGE}");
                ExitCode::from(2)
            }
        },
        "doc" => {
            // H4：`hc doc [target] [--out <dir>]`——target 默认 `.`，`std` 特殊值
            doc_command(&args[2..])
        }
        "fmt" => {
            // I1：`hc fmt <file.hc|dir> [--check]`——token 级格式化，AST 保真
            fmt_command(&args[2..])
        }
        "lint" => {
            // B1：`hc lint <file.hc|dir> [--json] [--fix]`——静态诊断
            lint_command(&args[2..])
        }
        "parse" => {
            // K2：`hc parse <file.hc>`——转储 AST 树（Rust 参考实现，H 版 parser 对照基准）
            parse_command(&args[2..])
        }
        "lex" => {
            // K1：`hc lex <file.hc>`——转储 token 流（Rust 参考实现，H 版 lexer 对照基准）
            lex_command(&args[2..])
        }
        "cc" => {
            // A1（ADR-0020）：`hc cc <file.c> [--output <file>]`——zig cc 封装
            cc_command(&args[2..])
        }
        "lsp" => {
            // B2：`hc lsp`——启动 LSP 语言服务器
            hc_lsp::run_server();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("error: unknown command `{other}`\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
