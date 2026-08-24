//! CLI 命令分发器：hc run/test/build/check/fmt/lint/doc/pkg 等命令解析
//!
//! 定义：枚举：TestMode, DangleMode

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hc_rt::Interp;

use crate::build::build_file;
use crate::doc;
use crate::fmt;
use crate::lint;
use crate::pkg::package_entry;
use crate::project::fsio::{collect_hc_files, is_hbc2, zig_cc_available};
use crate::project::{init_project, pkg_add, pkg_publish};
use crate::run::{
    load_manifest_deps_into, load_siblings_into, program_args, run_file_bytecode,
    run_file_dangle_bench, run_file_hs, run_file_ir,
};
use crate::script;
use crate::test::{test_dir, test_dir_dangle};

pub(crate) mod versiongen;

/// `hc test` 运行模式：解释器（默认）或原生编译（Q-T5 交叉验证）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TestMode {
    Interpret,
    Compile,
}

/// C2（ADR-0016）：Debug 悬垂标记切换模式（编译单元级，`--dangle=on|off|auto`）。
/// `auto` = Debug 开 / Release 关（默认）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DangleMode {
    On,
    Off,
    Auto,
}

impl DangleMode {
    /// 返回当前模式是否应启用悬垂检查（Auto 按 Debug 模式处理）。
    pub(crate) fn is_on(self) -> bool {
        match self {
            DangleMode::On => true,
            DangleMode::Off => false,
            DangleMode::Auto => true, // tag1 默认 Debug 开
        }
    }
}

/// 解析 `--dangle` 取值，非法值报错退出。
pub(crate) fn parse_dangle_mode(v: &str) -> Result<DangleMode, ExitCode> {
    match v {
        "on" => Ok(DangleMode::On),
        "off" => Ok(DangleMode::Off),
        "auto" => Ok(DangleMode::Auto),
        other => {
            eprintln!("error: 未知 --dangle `{other}`（可选 on|off|auto）");
            Err(ExitCode::from(2))
        }
    }
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
    hc run <file.hc> [--dangle=on|off|auto]
                              运行脚本模式（解释执行；--dangle 控制悬垂指针检查）
    hc run <file.hbc> [--dangle=on|off|auto]
                              运行字节码 VM（M3.2，装载 HBC2；全语言，同 IR）
    hc run --ir <file.hc>      用 IR 参考解释器运行（全语言，interp == IR）
    hc test [--mode=interpret|compile] [--dangle=on|off|auto] [file.hc|dir]
                              运行 test fn（默认当前目录全部 .hc；--mode=compile 原生交叉验证）
    hc check <file.hc>         仅检查（词法/语法/装载）
    hc errors <file.hc>        输出错误码表（M2.6：错误名 ↔ 码 + 位置）
    hc build <file.hc>         编译为原生可执行（LLVM IR + zig cc）
    hc init <name>             创建新项目骨架（build.zon + main.hc，组 H1）
    hc pkg add <name> [--path <dir>] [--version <ver>]
                              写本地依赖声明到 build.zon deps（组 H2）
    hc pkg publish          从当前目录发布包到本地注册中心（~/.hc/registry/，B3）
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
    hc cc <file.c> [--output <file>]
                              编译 C 文件（zig cc 封装，产出原生目标文件或可执行）
    hc lsp                    启动 LSP 语言服务器（stdio 通道，供编辑器集成）
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

/// K2：`hc parse <file.hc>`——Rust 参考 parser 输出 AST 树。
///
/// 每行一个节点，缩进表示嵌套层级，格式 `深度:NodeType|field=val|field=val`。
/// H 版 parser（stage1/parser.hc）输出同一格式，对照测试逐行 diff。
fn parse_command(args: &[String]) -> ExitCode {
    let Some(path_str) = args.first() else {
        eprintln!("error: `hc parse` requires a file\n\n{USAGE}");
        return ExitCode::from(2);
    };
    let source = match read_source(Path::new(path_str)) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let tokens = hc::lexer::lex(&source);
    let diags: Vec<_> = tokens
        .iter()
        .filter_map(|t| match &t.kind {
            hc::token::TokenKind::Error(msg) => Some(hc::Diagnostic::error(
                t.span.clone(),
                format!("lex error: {msg}"),
            )),
            _ => None,
        })
        .collect();
    if !diags.is_empty() {
        for d in &diags {
            eprintln!("{}:{}: {}", d.span.line, d.span.col, d.message);
        }
        return ExitCode::FAILURE;
    }
    let parser = hc::parser::Parser::new(&source, tokens);
    match parser.parse_program() {
        Ok(program) => {
            dump_ast(&program, 0);
            ExitCode::SUCCESS
        }
        Err(diags) => {
            for d in &diags {
                eprintln!("{}:{}: {}", d.span.line, d.span.col, d.message);
            }
            ExitCode::FAILURE
        }
    }
}

/// K2：递归输出 AST 树（格式：`深度:NodeType|field=val|field=val`）。
fn dump_ast(program: &hc::ast::Program, depth: usize) {
    let indent = " ".repeat(depth * 2);
    println!("{indent}Program");
    for decl in &program.decls {
        dump_decl(decl, depth + 1);
    }
}

fn dump_decl(decl: &hc::ast::Decl, depth: usize) {
    let indent = " ".repeat(depth * 2);
    match decl {
        hc::ast::Decl::Global {
            name,
            ty,
            init,
            pub_,
            ..
        } => {
            print!("{indent}Global|name={name}");
            if let Some(t) = ty {
                print!("|ty={:?}", hc::ast::fmt_type_debug(t));
            }
            if init.is_some() {
                print!("|has_init=true");
            }
            if *pub_ {
                print!("|pub=true");
            }
            println!();
        }
        hc::ast::Decl::Const { name, ty, pub_, .. } => {
            print!("{indent}Const|name={name}");
            if let Some(t) = ty {
                print!("|ty={:?}", hc::ast::fmt_type_debug(t));
            }
            if *pub_ {
                print!("|pub=true");
            }
            println!();
        }
        hc::ast::Decl::Fn {
            name,
            type_params,
            params,
            ret,
            is_test,
            is_async,
            pub_,
            exported,
            is_extern,
            body,
            ..
        } => {
            print!("{indent}Fn|name={name}");
            if !type_params.is_empty() {
                print!("|type_params={:?}", type_params);
            }
            if *pub_ {
                print!("|pub=true");
            }
            if *is_test {
                print!("|test=true");
            }
            if *is_async {
                print!("|async=true");
            }
            if *exported {
                print!("|exported=true");
            }
            if *is_extern {
                print!("|extern=true");
            }
            println!();
            for p in params {
                dump_param(p, depth + 1);
            }
            if let Some(t) = ret {
                println!("{}  ret: {:?}", indent, hc::ast::fmt_type_debug(t));
            }
            dump_block(body, depth + 1);
        }
        hc::ast::Decl::Class {
            name,
            fields,
            methods,
            pub_,
            ..
        } => {
            print!("{indent}Class|name={name}");
            if *pub_ {
                print!("|pub=true");
            }
            println!();
            for f in fields {
                println!(
                    "{}  Field|name={}|ty={:?}",
                    indent,
                    f.name,
                    hc::ast::fmt_type_debug(&f.ty)
                );
            }
            for m in methods {
                dump_method(m, depth + 1);
            }
        }
        hc::ast::Decl::Enum {
            name,
            variants,
            pub_,
            ..
        } => {
            print!("{indent}Enum|name={name}");
            if *pub_ {
                print!("|pub=true");
            }
            println!();
            for v in variants {
                print!("{}  Variant|name={}", indent, v.name);
                if let Some(pty) = &v.payload {
                    print!("|payload={:?}", hc::ast::fmt_type_debug(pty));
                }
                println!();
            }
        }
        hc::ast::Decl::Union {
            name, fields, pub_, ..
        } => {
            print!("{indent}Union|name={name}");
            if *pub_ {
                print!("|pub=true");
            }
            println!();
            for f in fields {
                println!(
                    "{}  Field|name={}|ty={:?}",
                    indent,
                    f.name,
                    hc::ast::fmt_type_debug(&f.ty)
                );
            }
        }
        hc::ast::Decl::Interface {
            name,
            methods,
            pub_,
            ..
        } => {
            print!("{indent}Interface|name={name}");
            if *pub_ {
                print!("|pub=true");
            }
            println!();
            for m in methods {
                dump_method(m, depth + 1);
            }
        }
        hc::ast::Decl::Namespace {
            name,
            decls,
            pub_,
            is_module,
            ..
        } => {
            print!("{indent}Namespace|name={name}");
            if *pub_ {
                print!("|pub=true");
            }
            if *is_module {
                print!("|module=true");
            }
            println!();
            for d in decls {
                dump_decl(d, depth + 1);
            }
        }
        hc::ast::Decl::Using { path, alias, .. } => {
            print!("{indent}Using|path={:?}", path);
            if let Some(a) = alias {
                print!("|alias={a}");
            }
            println!();
        }
        hc::ast::Decl::Import {
            path,
            alias,
            select,
            ..
        } => {
            print!("{indent}Import|path={:?}", path);
            if let Some(a) = alias {
                print!("|alias={a}");
            }
            if let Some(s) = select {
                print!("|select={:?}", s);
            }
            println!();
        }
        hc::ast::Decl::Include { path, alias, .. } => {
            print!("{indent}Include|path={path:?}");
            if let Some(a) = alias {
                print!("|alias={a}");
            }
            println!();
        }
        hc::ast::Decl::Struct {
            name, fields, pub_, ..
        } => {
            print!("{indent}Struct|name={name}");
            if *pub_ {
                print!("|pub=true");
            }
            println!();
            for f in fields {
                println!(
                    "{}  Field|name={}|ty={:?}",
                    indent,
                    f.name,
                    hc::ast::fmt_type_debug(&f.ty)
                );
            }
        }
        hc::ast::Decl::Comptime { .. } => {
            println!("{indent}Comptime");
        }
    }
}

fn dump_param(p: &hc::ast::Param, depth: usize) {
    let indent = " ".repeat(depth * 2);
    print!(
        "{indent}Param|name={}|ty={:?}",
        p.name,
        hc::ast::fmt_type_debug(&p.ty)
    );
    if p.default.is_some() {
        print!("|has_default=true");
    }
    println!();
}

fn dump_method(m: &hc::ast::Method, depth: usize) {
    let indent = " ".repeat(depth * 2);
    println!("{indent}Method|name={}", m.name);
    for p in &m.params {
        dump_param(p, depth + 1);
    }
    if let Some(t) = &m.ret {
        println!("{}  ret: {:?}", indent, hc::ast::fmt_type_debug(t));
    }
    dump_block(&m.body, depth + 1);
}

fn dump_block(b: &hc::ast::Block, depth: usize) {
    let indent = " ".repeat(depth * 2);
    println!("{indent}Block");
    for stmt in &b.stmts {
        dump_stmt(stmt, depth + 1);
    }
}

fn dump_stmt(stmt: &hc::ast::Stmt, depth: usize) {
    let indent = " ".repeat(depth * 2);
    match stmt {
        hc::ast::Stmt::VarDecl {
            name,
            mut_,
            ty,
            init,
            ..
        } => {
            print!("{indent}VarDecl|name={name}");
            if *mut_ {
                print!("|mut=true");
            }
            if let Some(t) = ty {
                print!("|ty={:?}", hc::ast::fmt_type_debug(t));
            }
            if init.is_some() {
                print!("|has_init=true");
            }
            println!();
        }
        hc::ast::Stmt::ConstDecl { name, .. } => {
            println!("{indent}ConstDecl|name={name}");
        }
        hc::ast::Stmt::Expr(e) => {
            print!("{indent}ExprStmt ");
            dump_expr(e, depth);
        }
        hc::ast::Stmt::If(s) => {
            print!("{indent}If");
            if let Some((_, cap)) = &s.capture {
                print!("|capture={cap}");
            }
            if let Some((_, err)) = &s.err_capture {
                print!("|err_capture={err}");
            }
            println!();
            dump_expr(&s.cond, depth + 1);
            dump_block(&s.then_b, depth + 1);
            if let Some(el) = &s.else_b {
                dump_stmt(el, depth + 1);
            }
        }
        hc::ast::Stmt::While(s) => {
            print!("{indent}While");
            if let Some(l) = &s.label {
                print!("|label={l}");
            }
            if let Some((_, cap)) = &s.capture {
                print!("|capture={cap}");
            }
            println!();
            dump_expr(&s.cond, depth + 1);
            dump_block(&s.body, depth + 1);
        }
        hc::ast::Stmt::For(s) => {
            print!("{indent}For");
            if let Some(l) = &s.label {
                print!("|label={l}");
            }
            println!("|capture={} iter={:?}", s.capture_name, s.capture);
            dump_expr(&s.iter, depth + 1);
            dump_block(&s.body, depth + 1);
        }
        hc::ast::Stmt::Switch(s) => {
            println!("{indent}Switch");
            dump_expr(&s.subject, depth + 1);
            for arm in &s.arms {
                print!("{}  SwitchArm", indent);
                if let Some((_, cap)) = &arm.capture {
                    print!("|capture={cap}");
                }
                if arm.guard.is_some() {
                    print!("|has_guard=true");
                }
                println!();
                for pat in &arm.patterns {
                    print!("{}    Pattern", indent);
                    match pat {
                        hc::ast::SwitchPattern::Error(s) => println!("|error={s}"),
                        hc::ast::SwitchPattern::Ident(s) => println!("|ident={s}"),
                        hc::ast::SwitchPattern::Int(s) => println!("|int={s}"),
                        hc::ast::SwitchPattern::Float(s) => println!("|float={s}"),
                        hc::ast::SwitchPattern::Str(s) => println!("|str={s}"),
                        hc::ast::SwitchPattern::Char(c) => println!("|char={c}"),
                        hc::ast::SwitchPattern::Else => println!("|else"),
                    }
                }
                dump_block(&arm.body, depth + 2);
            }
        }
        hc::ast::Stmt::Return(v, _) => {
            print!("{indent}Return");
            if let Some(val) = v {
                println!();
                dump_expr(val, depth + 1);
            } else {
                println!();
            }
        }
        hc::ast::Stmt::Break(l, _) => {
            print!("{indent}Break");
            if let Some(label) = l {
                print!("|label={label}");
            }
            println!();
        }
        hc::ast::Stmt::Continue(l, _) => {
            print!("{indent}Continue");
            if let Some(label) = l {
                print!("|label={label}");
            }
            println!();
        }
        hc::ast::Stmt::Defer(_, _) => println!("{indent}Defer"),
        hc::ast::Stmt::Errdefer(_, _) => println!("{indent}Errdefer"),
        hc::ast::Stmt::Block(b) => dump_block(b, depth),
        hc::ast::Stmt::Empty => println!("{indent}Empty"),
    }
}

fn dump_expr(expr: &hc::ast::Expr, depth: usize) {
    let indent = " ".repeat(depth * 2);
    match expr {
        hc::ast::Expr::IntLit { text, .. } => println!("{indent}IntLit|text={text}"),
        hc::ast::Expr::FloatLit { text, .. } => println!("{indent}FloatLit|text={text}"),
        hc::ast::Expr::StrLit { value, raw, .. } => {
            println!("{indent}StrLit|value={value}|raw={raw}")
        }
        hc::ast::Expr::CharLit(v, _) => println!("{indent}CharLit|value={v}"),
        hc::ast::Expr::BoolLit(v, _) => println!("{indent}BoolLit|value={v}"),
        hc::ast::Expr::NullLit(_) => println!("{indent}NullLit"),
        hc::ast::Expr::VoidLit(_) => println!("{indent}VoidLit"),
        hc::ast::Expr::Ident(name, _) => println!("{indent}Ident|name={name}"),
        hc::ast::Expr::ArrayLit(items, _) => {
            println!("{indent}ArrayLit");
            for e in items {
                dump_expr(e, depth + 1);
            }
        }
        hc::ast::Expr::TupleLit(items, _) => {
            println!("{indent}TupleLit");
            for e in items {
                dump_expr(e, depth + 1);
            }
        }
        hc::ast::Expr::NamedLit {
            ty,
            ty_args,
            fields,
            ..
        } => {
            print!("{indent}NamedLit|ty={ty}");
            if !ty_args.is_empty() {
                print!(
                    "|ty_args={:?}",
                    ty_args
                        .iter()
                        .map(|t| hc::ast::fmt_type_debug(t))
                        .collect::<Vec<_>>()
                );
            }
            println!();
            for (name, val) in fields {
                println!("{}  field={name}", indent);
                dump_expr(val, depth + 2);
            }
        }
        hc::ast::Expr::StructType { fields, .. } => {
            println!("{indent}StructType");
            for (name, ty) in fields {
                println!(
                    "{}  field={name}|ty={:?}",
                    indent,
                    hc::ast::fmt_type_debug(ty)
                );
            }
        }
        hc::ast::Expr::ArrayType { len, elem, .. } => {
            println!("{indent}ArrayType");
            dump_expr(len, depth + 1);
            dump_expr(elem, depth + 1);
        }
        hc::ast::Expr::Dot { base, field, .. } => {
            println!("{indent}Dot|field={field}");
            dump_expr(base, depth + 1);
        }
        hc::ast::Expr::Field { base, field, .. } => {
            println!("{indent}Field|field={field}");
            dump_expr(base, depth + 1);
        }
        hc::ast::Expr::Index { base, indices, .. } => {
            println!("{indent}Index");
            dump_expr(base, depth + 1);
            for i in indices {
                dump_expr(i, depth + 1);
            }
        }
        hc::ast::Expr::Deref(e, _) => {
            println!("{indent}Deref");
            dump_expr(e, depth + 1);
        }
        hc::ast::Expr::AddrOf(e, mut_, _) => {
            println!("{indent}AddrOf|mut={mut_}");
            dump_expr(e, depth + 1);
        }
        hc::ast::Expr::Unary(op, e, _) => {
            println!("{indent}Unary|op={:?}", op);
            dump_expr(e, depth + 1);
        }
        hc::ast::Expr::Binary(op, l, r, _) => {
            println!("{indent}Binary|op={:?}", op);
            dump_expr(l, depth + 1);
            dump_expr(r, depth + 1);
        }
        hc::ast::Expr::Orelse(l, r, _) => {
            println!("{indent}Orelse");
            dump_expr(l, depth + 1);
            dump_expr(r, depth + 1);
        }
        hc::ast::Expr::Unwrap(e, _) => {
            println!("{indent}Unwrap");
            dump_expr(e, depth + 1);
        }
        hc::ast::Expr::Try(e, _) => {
            println!("{indent}Try");
            dump_expr(e, depth + 1);
        }
        hc::ast::Expr::Await(e, _) => {
            println!("{indent}Await");
            dump_expr(e, depth + 1);
        }
        hc::ast::Expr::Catch(e, kind, _) => {
            println!("{indent}Catch");
            dump_expr(e, depth + 1);
            match kind.as_ref() {
                hc::ast::CatchKind::Default(d) => {
                    println!("{}  Default", indent);
                    dump_expr(d, depth + 2);
                }
                hc::ast::CatchKind::Bind { name, body } => {
                    println!("{}  Bind|name={name}", indent);
                    dump_block(body, depth + 2);
                }
            }
        }
        hc::ast::Expr::Call { callee, args, .. } => {
            println!("{indent}Call");
            dump_expr(callee, depth + 1);
            for a in args {
                dump_expr(a, depth + 1);
            }
        }
        hc::ast::Expr::IfExpr {
            cond,
            capture,
            then_e,
            else_e,
            ..
        } => {
            print!("{indent}IfExpr");
            if let Some((_, cap)) = capture {
                print!("|capture={cap}");
            }
            println!();
            dump_expr(cond, depth + 1);
            dump_expr(then_e, depth + 1);
            dump_expr(else_e, depth + 1);
        }
        hc::ast::Expr::SwitchExpr { subject, arms, .. } => {
            println!("{indent}SwitchExpr");
            dump_expr(subject, depth + 1);
            for arm in arms {
                println!("{}  SwitchArm", indent);
                for pat in &arm.patterns {
                    match pat {
                        hc::ast::SwitchPattern::Ident(s) => {
                            println!("{}    Pattern|ident={s}", indent)
                        }
                        _ => println!("{}    Pattern", indent),
                    }
                }
                dump_block(&arm.body, depth + 2);
            }
        }
        hc::ast::Expr::Block(b, _) => dump_block(b, depth),
        hc::ast::Expr::Assign {
            target, op, value, ..
        } => {
            println!("{indent}Assign|op={:?}", op);
            dump_expr(target, depth + 1);
            dump_expr(value, depth + 1);
        }
        hc::ast::Expr::ErrorLit(name, _) => println!("{indent}ErrorLit|name={name}"),
        hc::ast::Expr::FnRef(name, _) => println!("{indent}FnRef|name={name}"),
        hc::ast::Expr::TupleDestructure(names, e, _) => {
            println!("{indent}TupleDestructure|names={:?}", names);
            dump_expr(e, depth + 1);
        }
        hc::ast::Expr::Move(e, _) => {
            println!("{indent}Move");
            dump_expr(e, depth + 1);
        }
        hc::ast::Expr::Closure {
            params,
            is_mut,
            is_move,
            ..
        } => {
            print!("{indent}Closure|params={:?}", params);
            if *is_mut {
                print!("|mut");
            }
            if *is_move {
                print!("|move");
            }
            println!();
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

/// `hc errors file.hc`：输出错误码表（M2.6）——错误名 ↔ 码（包 ID + 包内码）+ 首次出现位置
fn errors_file(path: &Path) -> ExitCode {
    let source = match read_source(path) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let (_, program) = match script::parse_with_scripts(&source) {
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
    match script::parse_with_scripts(&source) {
        Ok((source, mut program)) => {
            // M1-1：文件级命名空间自动推断
            let project_root = script::find_project_root(path);
            let ns_name = script::compute_namespace_name(path, project_root.as_deref());
            script::infer_namespace(&mut program, &ns_name);
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
            let lint_diags = lint::lint_source(&source, &program, false);
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

/// A1（ADR-0020）：`hc cc <file.c> [--output <file>]`——zig cc 封装，
/// 编译 C 源文件为目标文件或可执行文件（与 `hc build` 共用同一链接器）。
fn cc_command(args: &[String]) -> ExitCode {
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

/// C2（ADR-0016）：从参数列表提取 `--dangle=on|off|auto` 标志，返回模式与程序参数起始位置。
/// 不匹配则返回 `Auto` 默认，起始位置不变。
fn extract_dangle(args: &[String], start: usize) -> (DangleMode, usize) {
    for i in start..args.len() {
        if let Some(v) = args[i].strip_prefix("--dangle=") {
            match parse_dangle_mode(v) {
                Ok(m) => return (m, i + 1),
                Err(_) => return (DangleMode::Auto, start),
            }
        }
    }
    (DangleMode::Auto, start)
}
