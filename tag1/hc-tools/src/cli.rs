//! CLI 命令分发：`run` / `test` / `check` / `errors` / `build` / `init` / `pkg` / `doc` / `fmt` / `lex`。

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hc_rt::Interp;

use crate::build::build_file;
use crate::docgen;
use crate::fmtgen;
use crate::fsio::{collect_hc_files, is_hbc2};
use crate::lintgen;
use crate::package::package_entry;
use crate::project::{init_project, pkg_add};
use crate::run::{
    load_manifest_deps_into, load_siblings_into, program_args, run_file, run_file_bytecode,
    run_file_ir,
};
use crate::scriptgen;
use crate::test::test_dir;

/// `hc test` 运行模式：解释器（默认）或原生编译（Q-T5 交叉验证）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TestMode {
    Interpret,
    Compile,
}

/// 解析 `--mode` 取值，非法值报错退出。
pub(crate) fn parse_test_mode(v: &str) -> Result<TestMode, ExitCode> {
    match v {
        "interpret" => Ok(TestMode::Interpret),
        "compile" => Ok(TestMode::Compile),
        other => {
            eprintln!("error: 未知 --mode `{other}`（可选 interpret|compile）");
            Err(ExitCode::from(2))
        }
    }
}

/// ANSI 颜色开关：仅当目标流为终端且未设置 NO_COLOR 时启用。
/// 重定向/管道（CI、check-examples.sh 捕获）下自动关闭，保证 grep 解析不受污染。
pub(crate) fn out_color() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}
pub(crate) fn err_color() -> bool {
    std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}
/// 给文本涂 ANSI 颜色（on=false 时原样返回）。
pub(crate) fn paint(on: bool, code: &str, s: &str) -> String {
    if on {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}
/// 单行测试结果标记上色：[PASS] 绿 / [FAIL] 红 / [SKIP] 黄，其余原样。
pub(crate) fn color_test_line(line: &str, on: bool) -> String {
    if !on {
        return line.to_string();
    }
    for (tag, code) in [("[PASS]", "32"), ("[FAIL]", "31"), ("[SKIP]", "33")] {
        if let Some(rest) = line.strip_prefix(tag) {
            return format!("{}{rest}", paint(true, code, tag));
        }
    }
    line.to_string()
}

pub(crate) const USAGE: &str = "hc <command> [args...]

H 语言工具链（tag1 垂直切片）

USAGE:
    hc run <file.hc>           运行脚本模式（解释执行）
    hc run <file.hbc>          运行字节码 VM（M3.2，装载 HBC2；全语言，同 IR）
    hc run --ir <file.hc>      用 IR 参考解释器运行（全语言，interp == IR）
    hc test [--mode=interpret|compile] [file.hc|dir]
                              运行 test fn（默认当前目录全部 .hc；--mode=compile 原生交叉验证）
    hc check <file.hc>         仅检查（词法/语法/装载）
    hc errors <file.hc>        输出错误码表（M2.6：错误名 ↔ 码 + 位置）
    hc build <file.hc>         编译为原生可执行（LLVM IR + zig cc）
    hc init <name>             创建新项目骨架（build.zon + main.hc，组 H1）
    hc pkg add <name> [--path <dir>] [--version <ver>]
                              写本地依赖声明到 build.zon deps（组 H2）
    hc doc [target] [--out <dir>]
                              生成 Markdown 文档（/// 注释 + 声明签名；target 默认当前目录包，
                              `std` = 标准库内置目录页；输出默认 <target 目录>/docs/api/，组 H4）
    hc lint <file.hc|dir> [--json] [--fix]
                              静态诊断（命名规范补全——缩写全大写、未用变量、可简化构造；
                              6 条规则 L001–L006；--json 输出 JSON，--fix 自动修复 4 规则）
    hc fmt <file.hc|dir> [--check]
                              格式化 .hc 源码（token 级重排，AST 保真；默认原地写回，
                              --check 仅报告将改动的文件，组 I1）
    hc lex <file.hc>          转储 token 流（K1 对照：`{start} {end} {line} {col} {kind:?}`，
                              与 H 版 lexer 输出逐行 diff）
    hc --version
    hc --help
";

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
            let Some(path) = args.get(2) else {
                eprintln!("error: `hc run` requires a file path");
                return ExitCode::from(2);
            };
            // 显式模式标志：`hc run --ir <file>` 走 IR 参考解释器；
            // `.hbc`（HBC2 字节码）走字节码 VM；否则默认 tree-walking
            if path == "--ir" {
                let Some(p) = args.get(3) else {
                    eprintln!("error: `hc run --ir` requires a file path");
                    return ExitCode::from(2);
                };
                // 程序参数：`hc run --ir <file> <args...>` → [程序名] + args（0 号 = 程序名）
                let prog_args = program_args(&args[4..], p);
                run_file_ir(Path::new(p), &prog_args)
            } else if is_hbc2(Path::new(path)) {
                let prog_args = program_args(&args[3..], path);
                run_file_bytecode(Path::new(path), &prog_args)
            } else if Path::new(path).is_dir() {
                // C1：`hc run <目录>`——包加载：入口 `main.hc` 或首个 `.hc`，
                // 兄弟文件 + build.zon 依赖由 run_file 复用装载
                match package_entry(Path::new(path)) {
                    Ok(entry) => {
                        let entry_s = entry.to_string_lossy().into_owned();
                        let prog_args = program_args(&args[3..], &entry_s);
                        run_file(&entry, &prog_args)
                    }
                    Err(msg) => {
                        eprintln!("error: {msg}");
                        ExitCode::FAILURE
                    }
                }
            } else {
                let prog_args = program_args(&args[3..], path);
                run_file(Path::new(path), &prog_args)
            }
        }
        // 调试：打印 script 块展开后的源码（组 C 开发辅助）
        "dump-scripts" => {
            let Some(path) = args.get(2) else {
                eprintln!("error: `hc dump-scripts` requires a file path");
                return ExitCode::from(2);
            };
            let source = match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: 读取 {path} 失败: {e}");
                    return ExitCode::FAILURE;
                }
            };
            match scriptgen::expand_scripts(&source) {
                Ok(expanded) => {
                    println!("{expanded}");
                    ExitCode::SUCCESS
                }
                Err(msg) => {
                    eprintln!("{msg}");
                    ExitCode::FAILURE
                }
            }
        }
        "test" => {
            // 解析可选 --mode=interpret|compile（默认 interpret）与目标路径
            let mut mode = TestMode::Interpret;
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
                } else {
                    target = PathBuf::from(a);
                }
                i += 1;
            }
            test_dir(&target, mode)
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
            let Some(path) = target else {
                eprintln!("error: `hc build [--dll]` requires a file path");
                return ExitCode::from(2);
            };
            build_file(Path::new(path), dll)
        }
        "init" => {
            // H1：`hc init <name>`——创建新项目骨架（build.zon + main.hc）
            let Some(name) = args.get(2) else {
                eprintln!("error: `hc init` requires a project name\n\n{USAGE}");
                return ExitCode::from(2);
            };
            init_project(name)
        }
        "pkg" => {
            // H2：`hc pkg add <name> [--path <dir>] [--version <ver>]`——写本地依赖
            if args.get(2).map(|s| s.as_str()) != Some("add") {
                eprintln!("error: `hc pkg` 子命令暂仅支持 `add`\n\n{USAGE}");
                return ExitCode::from(2);
            }
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
        "lex" => {
            // K1：`hc lex <file.hc>`——转储 token 流（Rust 参考实现，H 版 lexer 对照基准）
            lex_command(&args[2..])
        }
        other => {
            eprintln!("error: unknown command `{other}`\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn read_source(path: &Path) -> Result<String, ExitCode> {
    std::fs::read_to_string(path).map_err(|e| {
        eprintln!("error: cannot read {}: {e}", path.display());
        ExitCode::FAILURE
    })
}

/// K1：`hc lex <file.hc>`——Rust 参考 lexer 输出 token 流。
///
/// 每 token 一行，格式 `{start} {end} {line} {col} {kind:?}`（`kind:?` 为 Rust Debug 形态，
/// 如 `KwFn` / `Ident("main")` / `Str("hi\\n")` / `Char(120)`）。H 版 lexer（stage1/lexer.hc）
/// 输出同一格式，对照测试（hc-tools/tests/k1_lexer.rs）逐行 diff。
fn lex_command(args: &[String]) -> ExitCode {
    let Some(path_str) = args.first() else {
        eprintln!("error: `hc lex` requires a file\n\n{USAGE}");
        return ExitCode::from(2);
    };
    let source = match read_source(Path::new(path_str)) {
        Ok(s) => s,
        Err(code) => return code,
    };
    for tok in hc::lexer::lex(&source) {
        println!(
            "{} {} {} {} {:?}",
            tok.span.start, tok.span.end, tok.span.line, tok.span.col, tok.kind
        );
    }
    ExitCode::SUCCESS
}

/// H4：`hc doc [target] [--out <dir>]`——生成 Markdown 文档（`///` 注释 + 声明签名）。
///
/// - target：文件 / 目录（包）/ `std`（标准库内置目录页）；默认 `.`（当前目录 = 包）。
/// - 输出目录约定：默认 `<target 所在目录>/docs/api/`；`--out <dir>` 覆盖。
/// - `std` 页：H.std 为 Rust 内建（无 .hc 源），输出内置目录化摘要页。
fn doc_command(args: &[String]) -> ExitCode {
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
        match docgen::generate_stdlib(&out_dir) {
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
            match docgen::generate_project(&target_path, &out_dir) {
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
            match docgen::generate_file(&target_path, &out_dir) {
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

/// `hc errors file.hc`：输出错误码表（M2.6）——错误名 ↔ 码（包 ID + 包内码）+ 首次出现位置
fn errors_file(path: &Path) -> ExitCode {
    let source = match read_source(path) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let (_, program) = match scriptgen::parse_with_scripts(&source) {
        Ok((s, p)) => (s, p),
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::FAILURE;
        }
    };
    // 语义检查（错误码表在合法程序上输出）
    let errs: Vec<_> = hc::check_semantics(&program);
    if let Some(d) = errs.iter().find(|d| d.is_error()) {
        eprintln!("{}:{}: {}", d.span.line, d.span.col, d.message);
        return ExitCode::FAILURE;
    }
    let table = hc::error_code_table(&program);
    println!(
        "错误码表（包 ID {}，{} 个错误）：",
        table.package_id(),
        table.len()
    );
    for entry in table.entries() {
        println!(
            "  error.{:<24} 0x{:08X}  (pkg {} + code {}, 首次出现 at {}:{})",
            entry.name,
            entry.code,
            hc::ErrorCodeTable::package_of(entry.code),
            hc::ErrorCodeTable::index_of(entry.code),
            entry.span.line,
            entry.span.col
        );
    }
    ExitCode::SUCCESS
}

fn check_file(path: &Path) -> Result<(), ExitCode> {
    let source = read_source(path)?;
    match scriptgen::parse_with_scripts(&source) {
        Ok((source, program)) => {
            let mut interp = Interp::new(&source);
            // M1.4：同包兄弟文件先登记符号（解析失败仅告警）
            if let Err(code) = load_siblings_into(&mut interp, path) {
                return Err(code);
            }
            // M7.2：build.zon 本地依赖
            if let Err(code) = load_manifest_deps_into(&mut interp, path) {
                return Err(code);
            }
            interp.load(&program).map_err(|e| {
                eprintln!("{}", e.render(&source));
                ExitCode::FAILURE
            })?;
            // B1：lint 诊断（仅警告，不阻塞 check 成功）
            let lint_diags = lintgen::lint_source(&source, &program, false);
            for d in &lint_diags {
                eprintln!("{}: {}", path.display(), d.render(&source));
            }
            Ok(())
        }
        Err(msg) => {
            eprintln!("{msg}");
            Err(ExitCode::FAILURE)
        }
    }
}

/// I1：`hc fmt <file.hc|dir> [--check]`——token 级格式化，AST 保真。
/// 默认原地写回；`--check` 仅报告将改动的文件并以退出码 1 结束（CI 用）。
/// 格式化前用 token 序列自检：产物必须词法干净且 token 序列与源一致（保真保证）。
fn fmt_command(args: &[String]) -> ExitCode {
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
        let sig1 = match fmtgen::token_signature(&source) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: {} 词法失败: {e}", f.display());
                failed = true;
                continue;
            }
        };
        match fmtgen::format_source(&source) {
            Ok(formatted) => {
                let sig2 = match fmtgen::token_signature(&formatted) {
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

/// B1：`hc lint <file.hc|dir> [--json] [--fix]`——静态诊断（6 条规则 L001–L006）
fn lint_command(args: &[String]) -> ExitCode {
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
        match scriptgen::parse_with_scripts(&source) {
            Ok((_expanded, program)) => {
                let diags = lintgen::lint_source(&source, &program, fix);
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
            println!("{}", lintgen::diags_to_json(&all_diags, &file));
        }
    }
    if failed {
        ExitCode::FAILURE
    } else if !all_diags.is_empty() {
        eprintln!("lint: {} 个诊断", all_diags.len());
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
