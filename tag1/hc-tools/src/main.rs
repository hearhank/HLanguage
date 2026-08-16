//! hc 工具链 CLI（M7.1：`hc build` / `hc run` / `hc test`——tag1 子集）
//!
//! - `hc run <file.hc>`：脚本模式（tree-walking 解释器，全语言）
//! - `hc run <file.hbc>`：字节码 VM（M3.2，装载 HBC2 字节码复用 IR 语义；标量子集）
//! - `hc run --ir <file.hc>`：IR 参考解释器（标量子集，与字节码 VM 同语义源）
//! - `hc test [file.hc|dir]`：收集并运行 `test fn`，输出 [PASS]/[FAIL]/[SKIP] + 汇总
//! - `hc build <file.hc>`：原生编译（M3.3 LLVM 后端，emit-.ll + `zig cc`）
//! - `hc check <file.hc>`：仅词法/语法/装载检查

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hc::diag;
use hc_rt::Interp;

mod buildzon;

const USAGE: &str = "hc <command> [args...]

H 语言工具链（tag1 垂直切片）

USAGE:
    hc run <file.hc>           运行脚本模式（解释执行）
    hc run <file.hbc>          运行字节码 VM（M3.2，装载 HBC2；标量子集）
    hc run --ir <file.hc>      用 IR 参考解释器运行（标量子集）
    hc test [file.hc|dir]      运行 test fn（默认当前目录全部 .hc）
    hc check <file.hc>         仅检查（词法/语法/装载）
    hc errors <file.hc>        输出错误码表（M2.6：错误名 ↔ 码 + 位置）
    hc build <file.hc>         编译为原生可执行（LLVM IR + zig cc）
    hc --version
    hc --help
";

fn main() -> ExitCode {
    // 递归/深层 AST 求值需要更大栈（Windows 主线程默认 1MB；测试线程 8MB+）
    let handle = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(run_cli)
        .expect("spawn worker thread");
    handle.join().unwrap_or(ExitCode::FAILURE)
}

fn run_cli() -> ExitCode {
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
                run_file_ir(Path::new(p))
            } else if is_hbc2(Path::new(path)) {
                run_file_bytecode(Path::new(path))
            } else {
                run_file(Path::new(path))
            }
        }
        "test" => {
            let target = args
                .get(2)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            test_dir(&target)
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
            let Some(path) = args.get(2) else {
                eprintln!("error: `hc build` requires a file path");
                return ExitCode::from(2);
            };
            build_file(Path::new(path))
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

/// 旧字节码镜像魔数（tag1 过渡形态：镜像 = 魔数 + 源码；仅保留读取兼容，
/// 新 `hc build` 回退产出真实 HBC2 字节码）
const HBC_MAGIC: &[u8; 4] = b"HBC1";

/// 读取 .hc 或 .hbc（字节码镜像解包）
fn read_program(path: &Path) -> Result<String, ExitCode> {
    let bytes = std::fs::read(path).map_err(|e| {
        eprintln!("error: cannot read {}: {e}", path.display());
        ExitCode::FAILURE
    })?;
    if bytes.len() >= 4 && &bytes[..4] == HBC_MAGIC {
        // 镜像：跳过魔数 + u64 长度前缀，取源码
        if bytes.len() < 12 {
            eprintln!("error: {}: 损坏的字节码镜像", path.display());
            return Err(ExitCode::FAILURE);
        }
        let len = u64::from_le_bytes(bytes[4..12].try_into().unwrap()) as usize;
        let src = &bytes[12..12 + len.min(bytes.len() - 12)];
        match String::from_utf8(src.to_vec()) {
            Ok(s) => Ok(s),
            Err(_) => {
                eprintln!("error: {}: 镜像源码非 UTF-8", path.display());
                Err(ExitCode::FAILURE)
            }
        }
    } else {
        String::from_utf8(bytes).map_err(|_| {
            eprintln!("error: {}: 非 UTF-8 源码", path.display());
            ExitCode::FAILURE
        })
    }
}

/// 判断文件是否为 HBC2 字节码（M3.2 VM 镜像）。
fn is_hbc2(path: &Path) -> bool {
    std::fs::read(path)
        .map(|b| b.len() >= 4 && &b[..4] == &hc::bytecode::MAGIC)
        .unwrap_or(false)
}

/// 读取 HBC2 字节码文件；魔数不符/读取失败返回退出码。
fn read_bytecode(path: &Path) -> Result<Vec<u8>, ExitCode> {
    let bytes = std::fs::read(path).map_err(|e| {
        eprintln!("error: cannot read {}: {e}", path.display());
        ExitCode::FAILURE
    })?;
    if bytes.len() < 4 || &bytes[..4] != &hc::bytecode::MAGIC {
        eprintln!("error: {}: 不是 HBC2 字节码", path.display());
        return Err(ExitCode::FAILURE);
    }
    Ok(bytes)
}

/// `zig cc` 是否可用（M3.3 原生后端驱动；缺失则回退字节码）
fn zig_cc_available() -> bool {
    std::process::Command::new("zig")
        .arg("cc")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 源码 → LLVM IR 文本（解析 → 语义检查 → `lower` → `codegen`）。
/// 失败返回可直接打印的诊断文本（渲染后的诊断 / 错误）。
fn source_to_ll(source: &str) -> Result<String, String> {
    let program = hc::parse_source(source).map_err(|d| diag::render(&d, source))?;
    let errs = hc::check_semantics(&program);
    if errs.iter().any(|d| d.is_error()) {
        return Err(diag::render(&errs, source));
    }
    let module = hc::ir::lower(&program);
    let table = hc::error_code_table(&program);
    Ok(hc::llvm::codegen(&module, &table))
}

/// 源码 → HBC2 字节码（解析 → 语义检查 → `lower` → `encode`）。
/// 失败返回可直接打印的诊断文本（与 `source_to_ll` 同前置检查）。
fn source_to_bytecode(source: &str) -> Result<Vec<u8>, String> {
    let program = hc::parse_source(source).map_err(|d| diag::render(&d, source))?;
    let errs = hc::check_semantics(&program);
    if errs.iter().any(|d| d.is_error()) {
        return Err(diag::render(&errs, source));
    }
    let module = hc::ir::lower(&program);
    Ok(hc::bytecode::encode(&module))
}

/// 将字节码写入 `<dir>/<stem>.hbc`，返回产物路径。
fn write_bytecode_artifact(dir: &Path, stem: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let hbc_path = dir.join(format!("{stem}.hbc"));
    std::fs::write(&hbc_path, bytes)
        .map_err(|e| format!("写入 {} 失败: {e}", hbc_path.display()))?;
    Ok(hbc_path)
}

/// `hc build file.hc`：M3.3 原生编译（emit-.ll + `zig cc` → 可执行文件）。
/// `zig cc` 缺失时回退字节码镜像 .hbc + 平台启动器（M3.2 前过渡形态）。
fn build_file(path: &Path) -> ExitCode {
    let source = match read_program(path) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let dir = path.parent().unwrap_or_else(|| Path::new("."));

    // M7.2：build.zon 包清单报告（原生后端首轮单文件；跨包链接归后续）
    match buildzon::load_from_dir(dir) {
        Ok(Some(m)) => {
            println!("包：{} {}（{:?}）", m.name, m.version, m.kind);
            if !m.files.is_empty() {
                println!("  文件：{}", m.files.join(", "));
            }
            if !m.deps.is_empty() {
                let names: Vec<&str> = m.deps.iter().map(|d| d.name.as_str()).collect();
                println!("  依赖：{}", names.join(", "));
            }
        }
        Ok(None) => {}
        Err(e) => eprintln!("[warn] build.zon 解析失败: {e}"),
    }

    // M3.3 原生路径：zig cc 可用 → 生成 .ll → 编译链接为可执行文件
    if zig_cc_available() {
        let ll = match source_to_ll(&source) {
            Ok(ll) => ll,
            Err(msg) => {
                eprint!("{msg}");
                return ExitCode::FAILURE;
            }
        };
        let ll_path = dir.join(format!("{stem}.ll"));
        if let Err(e) = std::fs::write(&ll_path, &ll) {
            eprintln!("error: 写入 {} 失败: {e}", ll_path.display());
            return ExitCode::FAILURE;
        }
        let exe_name = if cfg!(windows) {
            format!("{stem}.exe")
        } else {
            stem.to_string()
        };
        let exe_path = dir.join(&exe_name);
        let out = std::process::Command::new("zig")
            .arg("cc")
            .arg(&ll_path)
            .arg("-o")
            .arg(&exe_path)
            .output();
        match out {
            Ok(o) if o.status.success() => {
                // 编译成功：清理中间 .ll，保留可执行文件
                let _ = std::fs::remove_file(&ll_path);
                println!("原生产物: {}", exe_path.display());
                return ExitCode::SUCCESS;
            }
            Ok(o) => {
                eprintln!("[FAIL] zig cc 编译失败：");
                eprint!("{}", String::from_utf8_lossy(&o.stderr));
                eprintln!("（LLVM IR 已保留：{}）", ll_path.display());
                return ExitCode::FAILURE;
            }
            Err(e) => {
                eprintln!("[FAIL] 调用 zig cc 失败: {e}");
                eprintln!("（LLVM IR 已保留：{}）", ll_path.display());
                return ExitCode::FAILURE;
            }
        }
    }

    // 回退：真实 HBC2 字节码 + 平台启动器（zig cc 缺失；M3.2 字节码 VM）
    eprintln!("[warn] 未检测到 zig cc——回退字节码 VM（M3.2 标量子集；原生后端需要 zig）");
    let bytecode = match source_to_bytecode(&source) {
        Ok(b) => b,
        Err(msg) => {
            eprint!("{msg}");
            return ExitCode::FAILURE;
        }
    };
    let hbc_path = match write_bytecode_artifact(dir, &stem, &bytecode) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let runner = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("hc-tools"));
    let launcher = if cfg!(windows) {
        let l = dir.join(format!("{stem}.cmd"));
        let content = format!(
            "@echo off\r\nrem H 语言字节码启动器（tag1：由字节码 VM 加载 .hbc）\r\n\"{}\" run \"{}\"\r\n",
            runner.display(),
            hbc_path.display()
        );
        let _ = std::fs::write(&l, content);
        l
    } else {
        let l = dir.join(format!("{stem}.sh"));
        let content = format!(
            "#!/bin/sh\nexec \"{}\" run \"{}\"\n",
            runner.display(),
            hbc_path.display()
        );
        let _ = std::fs::write(&l, content);
        l
    };

    println!("编译产物：");
    println!("  字节码    : {}", hbc_path.display());
    println!("  启动器    : {}", launcher.display());
    println!("运行方式：{}", launcher.display());
    ExitCode::SUCCESS
}

/// `hc errors file.hc`：输出错误码表（M2.6）——错误名 ↔ 码（包 ID + 包内码）+ 首次出现位置
fn errors_file(path: &Path) -> ExitCode {
    let source = match read_source(path) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let program = match hc::parse_source(&source) {
        Ok(p) => p,
        Err(diags) => {
            eprint!("{}", diag::render(&diags, &source));
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
    match hc::parse_source(&source) {
        Ok(program) => {
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
            Ok(())
        }
        Err(diags) => {
            eprint!("{}", diag::render(&diags, &source));
            Err(ExitCode::FAILURE)
        }
    }
}

fn run_file(path: &Path) -> ExitCode {
    let source = match read_program(path) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let program = match hc::parse_source(&source) {
        Ok(p) => p,
        Err(diags) => {
            eprint!("{}", diag::render(&diags, &source));
            return ExitCode::FAILURE;
        }
    };
    let mut interp = Interp::new(&source);
    // M1.4：同包兄弟文件（同目录 .hc）先登记符号
    if let Err(code) = load_siblings_into(&mut interp, path) {
        return code;
    }
    // M7.2：build.zon 本地依赖（using pkg.xxx 跨包访问）
    if let Err(code) = load_manifest_deps_into(&mut interp, path) {
        return code;
    }
    if let Err(e) = interp.load(&program) {
        eprintln!("{}", e.render(&source));
        return ExitCode::FAILURE;
    }
    match interp.run_main() {
        // io.exit 映射：code 0 → 成功；其它 → 对应退出码
        Ok(()) => match interp.exit_code {
            Some(0) => ExitCode::SUCCESS,
            Some(c) => ExitCode::from(c),
            None => ExitCode::SUCCESS,
        },
        Err(e) => {
            eprintln!("{}", e.render(&source));
            ExitCode::FAILURE
        }
    }
}

// ---------- `hc run --ir`：IR 参考解释器过渡模式（M3.2 字节码 VM 的过渡形态） ----------

/// IR 运行结果归一化（对齐 M2.6 根作用域语义：未处理错误到根 → panic 式失败）
#[derive(Debug, Clone, PartialEq, Eq)]
enum IrRunOutcome {
    /// main 正常返回（非错误值）——退出码 0
    Success,
    /// main 返回未处理的 error 值（值通道到达入口，panic 式失败，无恢复）
    UnhandledError(String),
}

/// 用 IR 参考解释器运行源码入口 `main`（`hc run --ir` 核心，可测试）。
///
/// 流程：解析 → 语义检查（准确优先）→ `lower` → `execute_ir`；失败返回可直接
/// 打印的文本（诊断渲染 / `error.{name}: {message}` + 切片外特性提示）。
/// 不依赖文件系统与退出码——仅 `hc run --ir` 使用，默认路径不受影响。
fn run_ir_source(source: &str) -> Result<IrRunOutcome, String> {
    // 1) 解析（失败渲染诊断）
    let program = match hc::parse_source(source) {
        Ok(p) => p,
        Err(diags) => return Err(diag::render(&diags, source)),
    };
    // 2) 语义检查（准确优先：能精确判定才报错——与 tree-walking load 内建检查对齐；
    //    有错误则渲染诊断返回失败）
    let errs = hc::check_semantics(&program);
    if errs.iter().any(|d| d.is_error()) {
        return Err(diag::render(&errs, source));
    }
    // 3) 降级为线性 IR，交给共享执行器（`hc run --ir` 与字节码 VM 同语义源）
    execute_ir(&hc::ir::lower(&program))
}

/// 执行已降级的 IR 模块入口 `main`，结果归一化为 [`IrRunOutcome`]。
///
/// `hc run --ir`（`lower` 后）与字节码 VM（`decode` 后）共用——ADR-0004 唯一语义源。
fn execute_ir(module: &hc::ir::IrModule) -> Result<IrRunOutcome, String> {
    // 入口 main 必须存在（NoMain——先查表，避免 run_ir 的 NoFunction 误导为切片外）
    if !module.func_index.contains_key("main") {
        return Err("error.NoMain: 入口函数 `main` 未定义".into());
    }
    // 运行（main(io: Io) 的 io 参数在 IR 下为 Void 占位——用了 io.* 会走 NoFunction
    // 提示，正常；零参 main 可完整运行）
    match hc::ir::run_ir(module, "main", &[]) {
        // 未处理错误值到达入口（值通道）：panic 式失败
        Ok(hc::ir::IrValue::Err(name)) => Ok(IrRunOutcome::UnhandledError(name)),
        Ok(_) => Ok(IrRunOutcome::Success),
        Err(e) => {
            let mut msg = format!("error.{}: {}", e.name, e.message);
            // NoFunction/TypeError 常来自 io/集合/指针等 IR 切片外特性：追加提示
            if e.name == "NoFunction" || e.name == "TypeError" {
                msg.push_str(
                    "\n程序使用了 IR 切片外特性（io/集合/指针等）——请用默认 \
                     tree-walking 模式 `hc run <file>`",
                );
            }
            Err(msg)
        }
    }
}

/// IR 运行结果 → 退出码（`hc run --ir` 与字节码 VM 共用）。
/// 退出码语义：只看 run_ir 结果（Ok=0，Err/未处理错误=非零）；main 返回非零 Int 不影响退出码。
fn ir_exit(outcome: Result<IrRunOutcome, String>) -> ExitCode {
    match outcome {
        Ok(IrRunOutcome::Success) => ExitCode::SUCCESS,
        Ok(IrRunOutcome::UnhandledError(name)) => {
            eprintln!("error.{name} 到达入口（未处理）");
            ExitCode::FAILURE
        }
        Err(msg) => {
            eprintln!("{msg}");
            ExitCode::FAILURE
        }
    }
}

/// `hc run --ir file.hc`：只读文件 + 调用 `run_ir_source` + 映射退出码。
fn run_file_ir(path: &Path) -> ExitCode {
    let source = match read_program(path) {
        Ok(s) => s,
        Err(c) => return c,
    };
    ir_exit(run_ir_source(&source))
}

/// `hc run file.hbc`：装载 HBC2 字节码 + `execute_ir` + 映射退出码（M3.2 字节码 VM）。
fn run_file_bytecode(path: &Path) -> ExitCode {
    let bytes = match read_bytecode(path) {
        Ok(b) => b,
        Err(c) => return c,
    };
    let module = match hc::bytecode::decode(&bytes) {
        Ok(m) => m,
        Err(msg) => {
            eprintln!("error: {}: {msg}", path.display());
            return ExitCode::FAILURE;
        }
    };
    ir_exit(execute_ir(&module))
}

/// 同目录兄弟 .hc 文件（M1.4：目录 = 包；build.zon 文件清单解析归 M7.2）
fn sibling_files(path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = path.parent() {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p == path {
                    continue;
                }
                if p.extension().map_or(false, |e| e == "hc") {
                    out.push(p);
                }
            }
        }
    }
    out.sort();
    out
}

/// 登记目标文件的同包兄弟声明（跳过其 test/main；解析失败的兄弟仅告警不阻断）
fn load_siblings_into(interp: &mut Interp, path: &Path) -> Result<(), ExitCode> {
    let sibs = sibling_files(path);
    if sibs.is_empty() {
        return Ok(());
    }
    let mut programs = Vec::new();
    for s in &sibs {
        match std::fs::read_to_string(s) {
            Ok(src) => match hc::parse_source(&src) {
                Ok(p) => programs.push(p),
                Err(diags) => {
                    eprintln!("[warn] 兄弟文件解析失败 {}:", s.display());
                    for d in &diags {
                        eprintln!("  {}", d.message);
                    }
                }
            },
            Err(e) => eprintln!("[warn] 跳过 {}: {e}", s.display()),
        }
    }
    if programs.is_empty() {
        return Ok(());
    }
    let refs: Vec<&hc::Program> = programs.iter().collect();
    interp.load_siblings(&refs).map_err(|e| {
        eprintln!("[FAIL] 兄弟文件装载: {} {}", e.name, e.message);
        ExitCode::FAILURE
    })
}

// ---------- M7.2：build.zon 本地依赖装载 ----------

/// 目录顶层 .hc 文件（依赖包文件清单；不递归——目录 = 包）
fn dir_hc_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().map_or(false, |e| e == "hc") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// 读取目标文件所在目录的 build.zon（如有）并递归装载本地依赖
fn load_manifest_deps_into(interp: &mut Interp, path: &Path) -> Result<(), ExitCode> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let manifest = match buildzon::load_from_dir(dir) {
        Ok(Some(m)) => m,
        Ok(None) => return Ok(()),
        Err(e) => {
            eprintln!("[warn] build.zon 解析失败: {e}");
            return Ok(());
        }
    };
    let mut visited = HashSet::new();
    if let Ok(canon) = std::fs::canonicalize(dir) {
        visited.insert(canon);
    }
    load_deps_into(interp, dir, &manifest, &mut visited)
}

/// M7.2：递归装载本地依赖包（build.zon `deps` 中带 `path` 的项）；
/// 无 path 的注册中心依赖跳过；依赖文件缺省回退「目录全部 .hc」。
fn load_deps_into(
    interp: &mut Interp,
    dir: &Path,
    manifest: &buildzon::Manifest,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), ExitCode> {
    for dep in &manifest.deps {
        let Some(rel) = &dep.path else {
            eprintln!(
                "[warn] 依赖 {} 无本地 path（注册中心归第三块），跳过",
                dep.name
            );
            continue;
        };
        let dep_dir = dir.join(rel);
        let canon = std::fs::canonicalize(&dep_dir).unwrap_or_else(|_| dep_dir.clone());
        if !visited.insert(canon.clone()) {
            continue; // 已装载（防环）
        }
        let dep_manifest = match buildzon::load_from_dir(&canon) {
            Ok(Some(m)) => m,
            Ok(None) => {
                eprintln!("[warn] 依赖 {} 目录 {} 无 build.zon", dep.name, canon.display());
                continue;
            }
            Err(e) => {
                eprintln!("[warn] 依赖 {} 清单解析失败: {e}", dep.name);
                continue;
            }
        };
        // 依赖包文件清单：缺省回退「该目录全部 .hc」
        let mut dep_files = if dep_manifest.files.is_empty() {
            dir_hc_files(&canon)
        } else {
            dep_manifest.files.iter().map(|f| canon.join(f)).collect()
        };
        dep_files.sort();

        let mut programs: Vec<hc::Program> = Vec::new();
        for f in &dep_files {
            match std::fs::read_to_string(f) {
                Ok(src) => match hc::parse_source(&src) {
                    Ok(p) => programs.push(p),
                    Err(diags) => {
                        eprintln!("[warn] 依赖文件解析失败 {}:", f.display());
                        for d in &diags {
                            eprintln!("  {}", d.message);
                        }
                    }
                },
                Err(e) => eprintln!("[warn] 跳过依赖文件 {}: {e}", f.display()),
            }
        }
        if !programs.is_empty() {
            let refs: Vec<&hc::Program> = programs.iter().collect();
            if let Err(e) = interp.load_dep(&dep.name, &refs) {
                eprintln!("[FAIL] 依赖 {} 装载: {} {}", dep.name, e.name, e.message);
                return Err(ExitCode::FAILURE);
            }
        }
        // 递归装载依赖的依赖
        load_deps_into(interp, &canon, &dep_manifest, visited)?;
    }
    Ok(())
}

type ParsedFile = (PathBuf, String, hc::Program);

fn test_dir(target: &Path) -> ExitCode {
    let mut files: Vec<PathBuf> = Vec::new();
    if target.is_dir() {
        collect_hc_files(target, &mut files);
        files.sort();
    } else if target.extension().map_or(false, |e| e == "hc") {
        files.push(target.to_path_buf());
    } else {
        eprintln!("error: `{}` 不是目录或 .hc 文件", target.display());
        return ExitCode::from(2);
    }
    if files.is_empty() {
        eprintln!("error: 未找到 .hc 文件于 {}", target.display());
        return ExitCode::from(2);
    }

    // M1.4：按目录分组（同目录 = 同包；跨目录独立）
    let mut groups: std::collections::BTreeMap<PathBuf, Vec<PathBuf>> =
        std::collections::BTreeMap::new();
    for f in &files {
        let dir = f.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
        groups.entry(dir).or_default().push(f.clone());
    }

    let mut total_p = 0usize;
    let mut total_f = 0usize;
    let mut total_s = 0usize;
    let mut all_ok = true;

    for group in groups.values() {
        // 组内一次性解析（失败的文件单独报告）
        let mut parsed: Vec<ParsedFile> = Vec::new();
        let mut bad: Vec<(PathBuf, String)> = Vec::new();
        for f in group {
            let name = f
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            match std::fs::read_to_string(f) {
                Ok(src) => match hc::parse_source(&src) {
                    Ok(p) => parsed.push((f.clone(), src, p)),
                    Err(diags) => {
                        bad.push((f.clone(), "parse error".into()));
                        for d in &diags {
                            eprintln!("[FAIL] {name}: {}", d.message);
                        }
                    }
                },
                Err(e) => bad.push((f.clone(), format!("io: {e}"))),
            }
        }
        for (f, err) in &bad {
            let name = f.file_name().unwrap_or_default().to_string_lossy();
            eprintln!("[FAIL] {name} ({err})");
            total_f += 1;
            all_ok = false;
        }
        for (idx, (f, source, program)) in parsed.iter().enumerate() {
            let name = f
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let mut interp = Interp::new(source);
            // 同包兄弟符号（跳过其 test/main）
            let siblings: Vec<&hc::Program> = parsed
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != idx)
                .map(|(_, (_, _, p))| p)
                .collect();
            if !siblings.is_empty() {
                if let Err(e) = interp.load_siblings(&siblings) {
                    eprintln!("[FAIL] {name} (sibling load: {} {})", e.name, e.message);
                    total_f += 1;
                    all_ok = false;
                    continue;
                }
            }
            // M7.2：build.zon 本地依赖（using pkg.xxx 跨包访问）
            if load_manifest_deps_into(&mut interp, f).is_err() {
                total_f += 1;
                all_ok = false;
                continue;
            }
            if let Err(e) = interp.load(program) {
                eprintln!("[FAIL] {name} (load error: {})", e.name);
                total_f += 1;
                all_ok = false;
                continue;
            }
            let (p, fail, s) = interp.run_tests();
            total_p += p;
            total_f += fail;
            total_s += s;
            if fail > 0 {
                all_ok = false;
            }
            for line in &interp.test_out {
                println!("{name}::{line}");
            }
        }
    }

    println!(
        "{} passed, {} failed, {} skipped",
        total_p, total_f, total_s
    );
    if all_ok && total_f == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn collect_hc_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_hc_files(&path, out);
        } else if path.extension().map_or(false, |e| e == "hc") {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{run_ir_source, source_to_bytecode, write_bytecode_artifact, IrRunOutcome};

    /// 断言切片内程序运行成功
    fn expect_success(src: &str) {
        match run_ir_source(src) {
            Ok(IrRunOutcome::Success) => {}
            other => panic!("预期运行成功，实际：{other:?}"),
        }
    }

    #[test]
    fn slice_in_simple_return() {
        // 零参 main 完整运行：标量 return
        expect_success("fn main() i32 { return 42; }");
    }

    #[test]
    fn slice_in_if_while_try_catch() {
        // 含 if/else-if/while 续步/try/catch/error 字面量的程序
        let src = r#"
fn sum_to(n: i32) i32 {
    var mut i: i32 = 0;
    var mut sum: i32 = 0;
    while (i < n) : (i += 1) { sum += i; }
    return sum;
}
fn fail() !i32 { return error.NotFound; }
fn ok() !i32 { return 5; }
fn pick(x: i32) i32 {
    if (x > 5) { return x; }
    else if (x > 2) { return x * 2; }
    return 0;
}
fn main() i32 {
    var t = try ok();
    var s = fail() catch 7;
    return sum_to(5) + pick(3) + t + s;
}
"#;
        expect_success(src);
    }

    #[test]
    fn main_io_param_void_placeholder() {
        // main(io: Io) 的 io 参数在 IR 下为 Void 占位；未用 io.* 时正常返回
        expect_success("fn main(io: Io) void {}");
    }

    #[test]
    fn unhandled_error_value() {
        // main 返回未处理 error 值（值通道到入口）→ UnhandledError
        let src = "fn main() !i32 { return error.NotFound; }";
        match run_ir_source(src) {
            Ok(IrRunOutcome::UnhandledError(name)) => assert_eq!(name, "NotFound"),
            other => panic!("预期未处理错误，实际：{other:?}"),
        }
    }

    #[test]
    fn division_by_zero() {
        // 整数除零 → DivisionByZero（对齐 tree-walking arith）
        let src = "fn main() i32 { return 10 / 0; }";
        match run_ir_source(src) {
            Err(msg) => assert!(msg.contains("DivisionByZero"), "消息：{msg}"),
            other => panic!("预期错误，实际：{other:?}"),
        }
    }

    #[test]
    fn no_main_entry() {
        // 无 main 入口 → NoMain（不误导为切片外 NoFunction）
        let src = "fn f() i32 { return 1; }";
        match run_ir_source(src) {
            Err(msg) => assert!(msg.contains("NoMain"), "消息：{msg}"),
            other => panic!("预期 NoMain，实际：{other:?}"),
        }
    }

    #[test]
    fn out_of_slice_io_print_hint() {
        // io.print 属 IR 切片外：io 内建放行语义检查；lower 展平 io.print → 运行 NoFunction
        let src = r#"fn main() void { io.print("hi"); }"#;
        match run_ir_source(src) {
            Err(msg) => {
                assert!(msg.contains("NoFunction"), "消息：{msg}");
                assert!(
                    msg.contains("IR 切片外特性") && msg.contains("hc run"),
                    "缺少切片外提示，消息：{msg}"
                );
            }
            other => panic!("预期 NoFunction，实际：{other:?}"),
        }
    }

    #[test]
    fn parse_error_rendered() {
        // 解析失败 → 渲染诊断文本
        assert!(run_ir_source("fn main( {").is_err());
    }

    #[test]
    fn bytecode_source_round_trips() {
        // source_to_bytecode → decode → 重新 encode 字节级一致（覆盖 HBC2 编码确定性）
        let src = "fn main() i32 { return 42; }";
        let bytes = source_to_bytecode(src).expect("encode");
        assert_eq!(&bytes[..4], &hc::bytecode::MAGIC);
        let module = hc::bytecode::decode(&bytes).expect("decode");
        assert_eq!(hc::bytecode::encode(&module), bytes);
    }

    #[test]
    fn write_bytecode_artifact_decodable() {
        // 产物写入后可重新 decode（回退路径的产物是可装载字节码）
        let dir = std::env::temp_dir().join(format!("hc_bc_artifact_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let bytes = source_to_bytecode("fn main() i32 { return 7; }").expect("encode");
        let p = write_bytecode_artifact(&dir, "prog", &bytes).expect("write");
        let read = std::fs::read(&p).expect("read");
        assert!(hc::bytecode::decode(&read).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
