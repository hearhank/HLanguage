//! hc 工具链 CLI（M7.1：`hc build` / `hc run` / `hc test`——tag1 子集）
//!
//! - `hc run <file.hc>`：脚本模式（tree-walking 解释器，全语言）
//! - `hc run <file.hbc>`：字节码 VM（M3.2，装载 HBC2 字节码复用 IR 语义；标量子集）
//! - `hc run --ir <file.hc>`：IR 参考解释器（标量子集，与字节码 VM 同语义源）
//! - `hc test [file.hc|dir]`：收集并运行 `test fn`，输出 [PASS]/[FAIL]/[SKIP] + 汇总
//! - `hc build <file.hc>`：原生编译（M3.3 LLVM 后端，emit-.ll + `zig cc`）
//! - `hc check <file.hc>`：仅词法/语法/装载检查
//! - `hc init <name>`：创建新项目骨架（build.zon + main.hc，组 H1）

use std::collections::HashSet;
use std::io::IsTerminal;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};

use hc::ast::{Decl, Expr};
use hc::diag;
use hc_rt::Interp;

/// Q-T5 编译模式交叉验证的临时产物目录序号（避免并行冲突）。
static TEST_DIR_COUNTER: AtomicUsize = AtomicUsize::new(0);

mod buildzon;
mod docgen;

/// ANSI 颜色开关：仅当目标流为终端且未设置 NO_COLOR 时启用。
/// 重定向/管道（CI、check-examples.sh 捕获）下自动关闭，保证 grep 解析不受污染。
fn out_color() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}
fn err_color() -> bool {
    std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}
/// 给文本涂 ANSI 颜色（on=false 时原样返回）。
fn paint(on: bool, code: &str, s: &str) -> String {
    if on {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}
/// 单行测试结果标记上色：[PASS] 绿 / [FAIL] 红 / [SKIP] 黄，其余原样。
fn color_test_line(line: &str, on: bool) -> String {
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

const USAGE: &str = "hc <command> [args...]

H 语言工具链（tag1 垂直切片）

USAGE:
    hc run <file.hc>           运行脚本模式（解释执行）
    hc run <file.hbc>          运行字节码 VM（M3.2，装载 HBC2；标量子集）
    hc run --ir <file.hc>      用 IR 参考解释器运行（标量子集）
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
    hc --version
    hc --help
";

/// `hc test` 运行模式：解释器（默认）或原生编译（Q-T5 交叉验证）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum TestMode {
    Interpret,
    Compile,
}

/// 解析 `--mode` 取值，非法值报错退出。
fn parse_test_mode(v: &str) -> Result<TestMode, ExitCode> {
    match v {
        "interpret" => Ok(TestMode::Interpret),
        "compile" => Ok(TestMode::Compile),
        other => {
            eprintln!("error: 未知 --mode `{other}`（可选 interpret|compile）");
            Err(ExitCode::from(2))
        }
    }
}

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

/// IR 降级错误 → 可直接打印文本（`error.{name}: {message}`）。
fn lower_err(e: hc::ir::IrError) -> String {
    format!("error.{}: {}", e.name, e.message)
}

/// 源码 → HBC2 字节码（解析 → 语义检查 → `lower` → `encode`）。
/// 失败返回可直接打印的诊断文本（与 `programs_to_ll` 同前置检查）。
fn source_to_bytecode(source: &str) -> Result<Vec<u8>, String> {
    let program = hc::parse_source(source).map_err(|d| diag::render(&d, source))?;
    let errs = hc::check_semantics(&program);
    if errs.iter().any(|d| d.is_error()) {
        return Err(diag::render(&errs, source));
    }
    let module = hc::ir::lower(&program).map_err(lower_err)?;
    Ok(hc::bytecode::encode(&module))
}

/// 将字节码写入 `<dir>/<stem>.hbc`，返回产物路径。
fn write_bytecode_artifact(dir: &Path, stem: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let hbc_path = dir.join(format!("{stem}.hbc"));
    std::fs::write(&hbc_path, bytes)
        .map_err(|e| format!("写入 {} 失败: {e}", hbc_path.display()))?;
    Ok(hbc_path)
}

/// M7.1：解析入口文件 + 同目录全部兄弟 `.hc`（目录 = 包）。
/// 返回（入口源码, 入口程序, 兄弟程序）。入口解析失败渲染诊断；兄弟解析失败报错退出。
fn package_programs(path: &Path) -> Result<(String, hc::Program, Vec<hc::Program>), ExitCode> {
    let source = read_program(path)?;
    let entry = hc::parse_source(&source).map_err(|d| {
        eprint!("{}", diag::render(&d, &source));
        ExitCode::FAILURE
    })?;
    let mut siblings = Vec::new();
    for s in sibling_files(path) {
        let src = std::fs::read_to_string(&s).map_err(|e| {
            eprintln!("error: 读取 {} 失败: {e}", s.display());
            ExitCode::FAILURE
        })?;
        let p = hc::parse_source(&src).map_err(|d| {
            eprintln!(
                "{} 兄弟文件解析失败 {}:",
                paint(err_color(), "31", "[FAIL]"),
                s.display()
            );
            for dg in &d {
                eprintln!("  {}", dg.message);
            }
            ExitCode::FAILURE
        })?;
        siblings.push(p);
    }
    Ok((source, entry, siblings))
}

/// M7.1：合并多文件 IR 模块——入口在前（索引不变），兄弟函数依次追加。
/// 兄弟文件顶层函数扁平名（含 `main`）文件私有不导出；命名空间函数/类型方法
/// 以限定名（含 `.`）导出，索引 `+offset`（对齐运行时文件私有规则）。
/// 闭包表同步追加：兄弟模块的 `MakeClosure.func`（闭包表索引）整体平移 `closure_offset`。
fn merge_modules(entry: hc::ir::IrModule, siblings: Vec<hc::ir::IrModule>) -> hc::ir::IrModule {
    let mut funcs = entry.funcs;
    let mut func_index = entry.func_index;
    let mut closures = entry.closures;
    let mut globals = entry.globals;
    let mut error_codes = entry.error_codes;
    let mut enum_variants = entry.enum_variants;
    let mut continuous = entry.continuous;
    for m in siblings {
        let offset = funcs.len();
        let coffset = closures.len();
        // 函数表：追加前重映射其体内 MakeClosure.func（指向本模块闭包表）。
        // 兄弟模块的 `@__init__` 一并追加——IrRuntime::init 按 funcs 序执行全部
        // `@__init__`（entry 在前，对齐解释器 `load` 先入口后兄弟的 global 初始化序）。
        for mut f in m.funcs {
            for inst in &mut f.body {
                if let hc::ir::IrInst::MakeClosure { func, .. } = inst {
                    *func += coffset;
                }
            }
            funcs.push(f);
        }
        // 闭包表：追加前重映射其体内嵌套 MakeClosure.func
        for mut c in m.closures {
            for inst in &mut c.body {
                if let hc::ir::IrInst::MakeClosure { func, .. } = inst {
                    *func += coffset;
                }
            }
            closures.push(c);
        }
        // func_index 一名多候选（重载/可选参数）：逐索引平移
        for (name, idxs) in m.func_index {
            if name.contains('.') {
                let e = func_index.entry(name).or_default();
                for i in idxs {
                    e.push(i + offset);
                }
            }
        }
        // 全局表：兄弟全局并入（去重，同名后者胜——对齐解释器后载入覆盖）
        for g in m.globals {
            if !globals.contains(&g) {
                globals.push(g);
            }
        }
        // 错误码表：并入兄弟（名→码全局唯一，重复同名同码，直接覆盖等价）
        error_codes.extend(m.error_codes);
        // 枚举变体表：并入兄弟（同名同定义，覆盖等价）
        enum_variants.extend(m.enum_variants);
        // [continuous] 类名表：并入兄弟（跨文件类同判连续）
        continuous.extend(m.continuous);
    }
    hc::ir::IrModule {
        funcs,
        closures,
        func_index,
        globals,
        error_codes,
        enum_variants,
        continuous,
    }
}

/// Q-T5：从 IR 模块剔除 test fn——兄弟文件测试函数文件私有，不参与入口文件测试跑器
/// （对齐解释器 `load_siblings` 跳过 test/main）。同步重映射 `func_index` 到剔除后索引。
/// 闭包表不动（test fn 被剔除后其闭包成为孤儿，无害；普通函数引用的闭包索引不变）。
fn strip_test_funcs_in_place(module: &mut hc::ir::IrModule) {
    let mut remap = vec![usize::MAX; module.funcs.len()];
    let mut kept = Vec::with_capacity(module.funcs.len());
    for (i, f) in module.funcs.drain(..).enumerate() {
        if !f.is_test {
            remap[i] = kept.len();
            kept.push(f);
        }
    }
    module.funcs = kept;
    let mut new_index = std::collections::HashMap::new();
    for (name, idxs) in module.func_index.drain() {
        let mut remapped = Vec::with_capacity(idxs.len());
        for idx in idxs {
            if remap[idx] != usize::MAX {
                remapped.push(remap[idx]);
            }
        }
        if !remapped.is_empty() {
            new_index.insert(name, remapped);
        }
    }
    module.func_index = new_index;
}

/// M7.1：合并错误码表（包 ID 0 单包；`register` 同名复用码 → 码一致）。
fn merge_error_tables(tables: &[&hc::ErrorCodeTable]) -> hc::ErrorCodeTable {
    let mut merged = hc::ErrorCodeTable::new(0);
    for t in tables {
        for e in t.entries() {
            merged.register(&e.name, &e.span);
        }
    }
    merged
}

/// M7.1：联合语义检查 + 各自 `lower` + 合并为单模块与错误码表
/// （`programs_to_ll` / `programs_to_test_ll` 共用）。失败返回可直接打印的诊断文本（诊断归属入口文件）。
/// `strip_sibling_tests`：测试跑器路径剔除兄弟文件 test fn（文件私有，对齐解释器）。
fn check_and_merge(
    entry: &hc::Program,
    entry_source: &str,
    siblings: &[&hc::Program],
    strip_sibling_tests: bool,
) -> Result<(hc::ir::IrModule, hc::ErrorCodeTable), String> {
    check_and_merge_deps(entry, entry_source, siblings, strip_sibling_tests, &[])
}

/// M7.1 + C3：联合语义检查（`deps` = 依赖包 (包名, Program)，仅登记 pub 符号供
/// `import pkg.{sym}` 解析）→ 入口与兄弟各自 lower → 合并为单模块与错误码表。
fn check_and_merge_deps(
    entry: &hc::Program,
    entry_source: &str,
    siblings: &[&hc::Program],
    strip_sibling_tests: bool,
    deps: &[(&str, &hc::Program)],
) -> Result<(hc::ir::IrModule, hc::ErrorCodeTable), String> {
    let errs = hc::check_semantics_extern_deps(entry, siblings, deps);
    if errs.iter().any(|d| d.is_error()) {
        return Err(diag::render(&errs, entry_source));
    }
    let entry_module = hc::ir::lower(entry).map_err(lower_err)?;
    let mut sibling_modules: Vec<hc::ir::IrModule> = siblings
        .iter()
        .map(|p| hc::ir::lower(p).map_err(lower_err))
        .collect::<Result<Vec<_>, String>>()?;
    if strip_sibling_tests {
        for m in &mut sibling_modules {
            strip_test_funcs_in_place(m);
        }
    }
    let merged = merge_modules(entry_module, sibling_modules);
    let mut tables = vec![hc::error_code_table(entry)];
    for s in siblings {
        tables.push(hc::error_code_table(s));
    }
    let table = merge_error_tables(&tables.iter().collect::<Vec<_>>());
    Ok((merged, table))
}

/// M7.1：入口 + 同包兄弟 → LLVM IR 文本（`main` 入口）。仅测试用（C3 后生产路径
/// 走 [`check_and_merge_deps`] + `codegen_with_links`）。
#[cfg(test)]
fn programs_to_ll(
    entry: &hc::Program,
    entry_source: &str,
    siblings: &[&hc::Program],
) -> Result<String, String> {
    let (merged, table) = check_and_merge(entry, entry_source, siblings, false)?;
    Ok(hc::llvm::codegen(&merged, &table))
}

/// Q-T5：入口 + 同包兄弟 → 「测试驱动」LLVM IR 文本（`test fn` 跑器入口）。
fn programs_to_test_ll(
    entry: &hc::Program,
    entry_source: &str,
    siblings: &[&hc::Program],
) -> Result<String, String> {
    let (merged, table) = check_and_merge(entry, entry_source, siblings, true)?;
    Ok(hc::llvm::codegen_tests(&merged, &table))
}

/// `zig cc <ll> -o <exe>`（M3.3 原生链接）。返回 Ok 或带诊断的 Err。
fn link_exe(ll_path: &Path, exe_path: &Path, extra: &[PathBuf]) -> Result<(), String> {
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

/// C3/C4：`Kind::lib` 构建——codegen_lib（包前缀，剔除 test 函数；静态归档转 runtime
/// helper 为 declare，dll 保持自包含）→ `zig cc -c` + `zig ar rcs lib{name}.a`（静态）
/// 或 `zig cc -shared` → `{name}.dll`（dll，exe 运行时加载）。另写 `{name}.sym`
/// （限定名 → 导出符号，exe 链接引用）。**库无 main 校验**（C4：Kind::lib 含 main → 诊断）。
/// 返回（库文件路径，符号表）。
fn build_lib(dir: &Path, name: &str, dll: bool) -> Result<(PathBuf, Vec<(String, String)>), ExitCode> {
    let entry_path = match package_entry(dir) {
        Ok(e) => e,
        Err(msg) => {
            eprintln!("error: {msg}");
            return Err(ExitCode::FAILURE);
        }
    };
    let (source, entry, siblings) = match package_programs(&entry_path) {
        Ok(t) => t,
        Err(c) => return Err(c),
    };
    let sib_refs: Vec<&hc::Program> = siblings.iter().collect();
    let (merged, table) = match check_and_merge(&entry, &source, &sib_refs, false) {
        Ok(t) => t,
        Err(msg) => {
            eprint!("{msg}");
            return Err(ExitCode::FAILURE);
        }
    };
    // 库不运行测试：剔除 [test] 函数（其断言 helper 在库形态不发射，保留会链接失败）
    let mut merged = merged;
    strip_test_funcs_in_place(&mut merged);
    // C4：库无 main 校验——`Kind::lib` = 不含 main 的包（06-08 定案）
    if merged.func_index.contains_key("main") {
        eprintln!(
            "error: 库包 `{}` 不应含 `main` 入口（Kind::lib = 不含 main 的包；应用请用 Kind.exe）",
            name
        );
        return Err(ExitCode::FAILURE);
    }
    // 符号表：func_index 限定名 → `{pkg}.hc_fn{i}`（pub 边界由语义层 import 检查保证——
    // 非 pub 符号 import 即报错，.sym 全量导出不绕过）
    let mut syms: Vec<(String, String)> = merged
        .func_index
        .iter()
        .map(|(fn_, idxs)| (format!("{name}.{fn_}"), format!("{name}.hc_fn{}", idxs[0])))
        .collect();
    syms.sort();
    let ll = hc::llvm::codegen_lib(&merged, &table, name, dll);
    let ll_path = dir.join(format!("{name}.ll"));
    if let Err(e) = std::fs::write(&ll_path, &ll) {
        eprintln!("error: 写入 {} 失败: {e}", ll_path.display());
        return Err(ExitCode::FAILURE);
    }
    let artifact = if dll {
        // C4：dll 动态库——`zig cc -shared`；自包含 helper（codegen_lib dll_mode）
        let dll_path = dir.join(format!("{name}.dll"));
        let shared = std::process::Command::new("zig")
            .arg("cc")
            .arg("-shared")
            .arg(&ll_path)
            .arg("-o")
            .arg(&dll_path)
            .output();
        match shared {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                eprintln!(
                    "{} zig cc -shared 失败：\n{}",
                    paint(err_color(), "31", "[FAIL]"),
                    String::from_utf8_lossy(&o.stderr)
                );
                return Err(ExitCode::FAILURE);
            }
            Err(e) => {
                eprintln!("调用 zig cc 失败: {e}");
                return Err(ExitCode::FAILURE);
            }
        }
        dll_path
    } else {
        // C3：静态归档——`zig cc -c` → `.o` → `zig ar rcs lib{name}.a`
        let o_path = dir.join(format!("{name}.o"));
        let cc = std::process::Command::new("zig")
            .arg("cc")
            .arg("-c")
            .arg(&ll_path)
            .arg("-o")
            .arg(&o_path)
            .output();
        match cc {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                eprintln!(
                    "{} zig cc -c 失败：\n{}",
                    paint(err_color(), "31", "[FAIL]"),
                    String::from_utf8_lossy(&o.stderr)
                );
                return Err(ExitCode::FAILURE);
            }
            Err(e) => {
                eprintln!("调用 zig cc 失败: {e}");
                return Err(ExitCode::FAILURE);
            }
        }
        let a_path = dir.join(format!("lib{name}.a"));
        let ar = std::process::Command::new("zig")
            .arg("ar")
            .arg("rcs")
            .arg(&a_path)
            .arg(&o_path)
            .output();
        if let Err(e) = ar.and_then(|o| {
            if o.status.success() {
                Ok(())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("zig ar: {}", String::from_utf8_lossy(&o.stderr)),
                ))
            }
        }) {
            eprintln!("error: 归档 {} 失败: {e}", a_path.display());
            return Err(ExitCode::FAILURE);
        }
        let _ = std::fs::remove_file(&o_path);
        a_path
    };
    // 符号表文件（构建产物；exe 链接侧已由返回值直接使用）
    let sym_text = syms
        .iter()
        .map(|(qn, sym)| format!("{qn} {sym}"))
        .collect::<Vec<_>>()
        .join("\n");
    let _ = std::fs::write(dir.join(format!("{name}.sym")), format!("{sym_text}\n"));
    // 清理中间 .ll，保留产物 + .sym
    let _ = std::fs::remove_file(&ll_path);
    Ok((artifact, syms))
}

/// `hc build [--dll] <path>`：M3.3 原生编译（emit-.ll + `zig cc` → 可执行文件）。
/// `zig cc` 缺失时回退字节码镜像 .hbc + 平台启动器（M3.2 前过渡形态）。
/// C3：`Kind::lib` → 静态归档（build_lib）；`Kind::exe` 带本地依赖 → 链接库形态。
/// C4：`--dll` → `Kind::lib` 产 dll（`zig cc -shared`，自包含 helper）；`Kind::exe`
/// 依赖库按 dll 构建并**链接 dll**（OS 运行时加载；dll 复制到 exe 目录供加载器定位）。
fn build_file(path: &Path, dll: bool) -> ExitCode {
    // 目录参数：取目录内 main.hc（否则首个 .hc）作为入口
    let entry_path = if path.is_dir() {
        let files = dir_hc_files(path);
        let main = files
            .iter()
            .find(|f| f.file_stem().map_or(false, |s| s == "main"));
        match main.or_else(|| files.first()) {
            Some(f) => f.clone(),
            None => {
                eprintln!("error: 目录 {} 无 .hc 文件", path.display());
                return ExitCode::FAILURE;
            }
        }
    } else {
        path.to_path_buf()
    };

    let (source, entry, siblings) = match package_programs(&entry_path) {
        Ok(t) => t,
        Err(c) => return c,
    };
    let stem = entry_path.file_stem().unwrap_or_default().to_string_lossy();
    let dir = entry_path.parent().unwrap_or_else(|| Path::new("."));

    // M7.2：build.zon 包清单（C3：kind 分流——lib → 静态归档；exe → 链接依赖库）
    let manifest = match buildzon::load_from_dir(dir) {
        Ok(Some(m)) => {
            println!("包：{} {}（{:?}）", m.name, m.version, m.kind);
            if !m.files.is_empty() {
                println!("  文件：{}", m.files.join(", "));
            }
            if !m.deps.is_empty() {
                let names: Vec<&str> = m.deps.iter().map(|d| d.name.as_str()).collect();
                println!("  依赖：{}", names.join(", "));
            }
            Some(m)
        }
        Ok(None) => None,
        Err(e) => {
            eprintln!("[warn] build.zon 解析失败: {e}");
            None
        }
    };

    // C3/C4：库形态——不产出 exe，编译为静态归档（zig cc -c + zig ar）或 dll（--dll，
    // zig cc -shared）
    if manifest
        .as_ref()
        .is_some_and(|m| m.kind == buildzon::Kind::Lib)
    {
        if !zig_cc_available() {
            eprintln!("error: 库构建需要 zig cc（当前未检测到）");
            return ExitCode::FAILURE;
        }
        return match build_lib(dir, &manifest.as_ref().unwrap().name, dll) {
            Ok((a_path, _)) => {
                println!("库产物: {}", a_path.display());
                ExitCode::SUCCESS
            }
            Err(c) => c,
        };
    }

    // M3.3 原生路径：zig cc 可用 → 生成 .ll → 编译链接为可执行文件
    if zig_cc_available() {
        let sib_refs: Vec<&hc::Program> = siblings.iter().collect();
        // C3：本地依赖先构建为静态库，收集 .a 与外部符号表（限定名 → `{pkg}.hc_fn{i}`）
        let mut libs: Vec<PathBuf> = Vec::new();
        let mut links: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let mut dep_progs: Vec<(String, hc::Program)> = Vec::new();
        if let Some(m) = &manifest {
            for dep in &m.deps {
                let Some(rel) = &dep.path else {
                    continue; // 注册中心依赖归第三块
                };
                let dep_dir = dir.join(rel);
                // H2：缺失依赖诊断——本地依赖 path 须指向存在的完整包
                if !dep_dir.is_dir() {
                    eprintln!(
                        "error: 依赖 {} 路径不存在: {}（本地依赖 path 须指向包目录）",
                        dep.name,
                        dep_dir.display()
                    );
                    return ExitCode::FAILURE;
                }
                if let Ok(Some(dm)) = buildzon::load_from_dir(&dep_dir) {
                    // H2：版本声明检查（本地 path 权威，不符告警）
                    if !dep.version.is_empty()
                        && !dm.version.is_empty()
                        && dep.version != dm.version
                    {
                        eprintln!(
                            "[warn] 依赖 {} 声明版本 {} 与本地 {} 不符",
                            dep.name, dep.version, dm.version
                        );
                    }
                }
                match build_lib(&dep_dir, &dep.name, dll) {
                    Ok((art, syms)) => {
                        // C4：dll 模式把依赖 dll 复制到 exe 目录——Windows 加载器仅在
                        // exe 目录/系统路径/PATH 搜索，子目录不找；静态归档直接链接无需复制
                        let link_path = if dll {
                            let copy = dir.join(art.file_name().unwrap_or_default());
                            let _ = std::fs::copy(&art, &copy);
                            copy
                        } else {
                            art
                        };
                        libs.push(link_path);
                        for (qn, sym) in syms {
                            links.insert(qn, sym);
                        }
                    }
                    Err(c) => return c,
                }
                // 依赖包源码（pub 符号登记用，不合并函数体）
                for f in dir_hc_files(&dep_dir) {
                    if let Ok(src) = std::fs::read_to_string(&f) {
                        if let Ok(p) = hc::parse_source(&src) {
                            dep_progs.push((dep.name.clone(), p));
                        }
                    }
                }
            }
        }
        let dep_refs: Vec<(&str, &hc::Program)> =
            dep_progs.iter().map(|(n, p)| (n.as_str(), p)).collect();
        let ll = match check_and_merge_deps(&entry, &source, &sib_refs, false, &dep_refs)
            .and_then(|(merged, table)| Ok(hc::llvm::codegen_with_links(&merged, &table, &links)))
        {
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
        if let Err(msg) = link_exe(&ll_path, &exe_path, &libs) {
            eprintln!("{} {msg}", paint(err_color(), "31", "[FAIL]"));
            eprintln!("（LLVM IR 已保留：{}）", ll_path.display());
            return ExitCode::FAILURE;
        }
        // 编译成功：清理中间 .ll，保留可执行文件
        let _ = std::fs::remove_file(&ll_path);
        println!("原生产物: {}", exe_path.display());
        return ExitCode::SUCCESS;
    }

    // 回退：真实 HBC2 字节码 + 平台启动器（zig cc 缺失；M3.2 字节码 VM）
    eprintln!("[warn] 未检测到 zig cc——回退字节码 VM（M3.2 标量子集；原生后端需要 zig）");
    if !siblings.is_empty() {
        eprintln!(
            "[warn] 检测到 {} 个同包兄弟文件——字节码回退仅编译入口文件，多文件需 zig 原生后端",
            siblings.len()
        );
    }
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

/// H1：`hc init <name>`——在当前目录生成最小项目骨架（`build.zon` + `main.hc`）。
///
/// 约定见 `docs/SPEC/06-13-project-structure.md`：目录 = 包、源码位于包根、
/// 测试 = `[test]` 标注函数与源码同文件、依赖经 build.zon deps。脚手架即
/// 最小可运行示例——`hc run <name>` / `hc test <name>` 全绿（CLI 测试保证）。
/// 安全：目录已存在且非空 → 拒绝覆盖（不触碰现有文件）。
fn init_project(name: &str) -> ExitCode {
    // 名称校验：合法目录名（字母/数字/`-`/`_`；非空、非 `.`/`..`）
    if name.is_empty()
        || name == "." || name == ".."
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        eprintln!("error: `hc init` 名称 `{name}` 非法（允许字母/数字/`-`/`_`）");
        return ExitCode::from(2);
    }
    let dir = Path::new(name);
    if dir.exists() {
        let non_empty = std::fs::read_dir(dir)
            .map(|mut it| it.next().is_some())
            .unwrap_or(true);
        if non_empty {
            eprintln!("error: 目录 `{name}` 已存在且非空——拒绝覆盖（请换用空目录或新名）");
            return ExitCode::FAILURE;
        }
    }
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("error: 创建目录 `{name}` 失败: {e}");
        return ExitCode::FAILURE;
    }
    let zon = format!(
        "// build.zon — {name} 包清单（hc init 脚手架）\n\
         //\n\
         // 清单即数据：const build = Build{{ ... }}（H 数据字面量，Q26）\n\
         //   - name/version/kind：包标识与形态（exe = 应用，含 main 入口）\n\
         //   - files：包内文件清单（源码位于包根，见 06-13-project-structure.md）\n\
         //   - deps：依赖声明（本地依赖带 path；`hc pkg add <name> --path <dir>` 写入）\n\
         \n\
         const build = Build{{\n\
         \x20   name = \"{name}\",\n\
         \x20   version = \"0.1.0\",\n\
         \x20   kind = Kind.exe,\n\
         \x20   files = [ \"main.hc\", ],\n\
         \x20   deps = [],\n\
         }};\n"
    );
    let main_hc = format!(
        "import H.std.{{io}};\n\
         \n\
         // {name}/main.hc — 项目入口（hc init 脚手架）\n\
         //\n\
         //   - 源码约定：`.hc` 文件位于包根（目录 = 包，M1.4）\n\
         //   - 测试约定：`[test]` 标注函数与源码同文件（Q-T1）\n\
         //   - 运行：`hc run {name}`   测试：`hc test {name}`\n\
         \n\
         fn main(args: o Vec(String)) !void {{\n\
         \x20   io.print(\"hello, {name}!\\n\");\n\
         }}\n\
         \n\
         [test] fn scaffold_smoke() !void {{\n\
         \x20   try expect_eq(1 + 1, 2);\n\
         }}\n"
    );
    if let Err(e) = std::fs::write(dir.join("build.zon"), &zon) {
        eprintln!("error: 写入 {} 失败: {e}", dir.join("build.zon").display());
        return ExitCode::FAILURE;
    }
    if let Err(e) = std::fs::write(dir.join("main.hc"), &main_hc) {
        eprintln!("error: 写入 {} 失败: {e}", dir.join("main.hc").display());
        return ExitCode::FAILURE;
    }
    println!("创建项目 `{name}`：");
    println!("  {name}/build.zon");
    println!("  {name}/main.hc");
    println!("运行：hc run {name}   测试：hc test {name}");
    ExitCode::SUCCESS
}

/// 从 `start` 起的源文本中查找字符 `ch`（build.zon 数据字面量字段为简单标量，
/// 字符串值不含 `{`/`}`/`[`/`]`，粗扫描足够）。
fn find_char(src: &str, start: usize, ch: u8) -> Option<usize> {
    src.as_bytes()
        .get(start..)
        .and_then(|b| b.iter().position(|&c| c == ch))
        .map(|i| start + i)
}

/// 从 `open` 指向的 `{` 起找匹配的 `}`（深度计数，返回 `}` 字节偏移）。
fn find_block_close(src: &str, open: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// H2：定位 build.zon 的 `const build = Build{...}` 中 deps 数组。
/// 返回（deps 数组 `[` 偏移、现有 Pkg 项（name, 干净 `Pkg{...}` span）、Build 字面量 span）。
/// 解析失败返回可读错误文本；无 deps 字段时数组偏移 = None。
///
/// 注：parser 的 ArrayLit/NamedLit span.end 取 `]`/`}` 之后下一个 token（常含尾随
/// 逗号），故本函数按字符扫描重算干净块边界（`find_block_close`）。
fn locate_build_deps(
    src: &str,
) -> Result<(Option<usize>, Vec<(String, (usize, usize))>, (usize, usize)), String> {
    let program = hc::parse_source(src).map_err(|diags| {
        diags
            .iter()
            .map(|d| d.message.clone())
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    let init = program
        .decls
        .iter()
        .find_map(|d| match d {
            Decl::Const { name, init, .. } if name == "build" => Some(init),
            _ => None,
        })
        .ok_or_else(|| "build.zon: 缺少 `const build = Build{ ... }`".to_string())?;
    let Expr::NamedLit {
        ty,
        fields,
        span: build_span,
    } = init
    else {
        return Err("build.zon: `build` 必须是 `Build{ ... }` 数据字面量".into());
    };
    if ty != "Build" {
        return Err("build.zon: `build` 必须是 `Build{ ... }` 数据字面量".into());
    }
    let build_span = (build_span.start, build_span.end);
    for (key, val) in fields {
        if key == "deps" {
            if let Expr::ArrayLit(items, span) = val {
                let array_open = span.start; // `[` 偏移
                let mut pkgs = Vec::new();
                for item in items {
                    if let Expr::NamedLit {
                        ty,
                        fields,
                        span,
                    } = item
                    {
                        if ty == "Pkg" {
                            let mut pkg_name = String::new();
                            for (k, v) in fields {
                                if k == "name" {
                                    if let Expr::StrLit { value, .. } = v {
                                        pkg_name = value.clone();
                                    }
                                }
                            }
                            // 干净 Pkg 块：`Pkg` 起始 .. 匹配 `}` 之后
                            let brace = find_char(src, span.start, b'{').unwrap_or(span.start);
                            let close = find_block_close(src, brace).unwrap_or(span.end);
                            pkgs.push((pkg_name, (span.start, close + 1)));
                        }
                    }
                }
                return Ok((Some(array_open), pkgs, build_span));
            }
        }
    }
    Ok((None, Vec::new(), build_span))
}

/// H2：`hc pkg add <name> [--path <dir>] [--version <ver>]`——在当前目录 build.zon
/// 的 deps 数组中写入/更新本地依赖声明（`Pkg{ name, version, path }`）。
///
/// 缺失 build.zon 报错（提示先 `hc init`）；保留数组外注释与格式；已存在同名依赖
/// → 替换其 Pkg 项（更新 path/version）。deps 数组按「既有 Pkg 原文 + 新项」重建。
fn pkg_add(name: &str, path: &Option<String>, version: &Option<String>) -> ExitCode {
    if name.is_empty()
        || name == "." || name == ".."
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        eprintln!("error: `hc pkg add` 包名 `{name}` 非法（允许字母/数字/`-`/`_`）");
        return ExitCode::from(2);
    }
    let zon_path = Path::new("build.zon");
    let src = match std::fs::read_to_string(zon_path) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: 当前目录无 build.zon（先 `hc init <name>` 创建项目）");
            return ExitCode::FAILURE;
        }
    };
    let (array_open, pkgs, build_span) = match locate_build_deps(&src) {
        Ok(t) => t,
        Err(msg) => {
            eprintln!("error: {msg}");
            return ExitCode::FAILURE;
        }
    };
    let ver = version
        .clone()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "0.1.0".to_string());
    let path_s = path.clone().unwrap_or_default();
    let entry = format!("Pkg{{ name = \"{name}\", version = \"{ver}\", path = \"{path_s}\", }}");
    let mut out = src;
    match array_open {
        Some(open) => {
            // 重建 deps 数组：既有 Pkg 原文（替换同名项）+ 新项
            let close = find_char(&out, open, b']').unwrap_or(out.len().saturating_sub(1));
            let mut inner = String::new();
            let mut replaced = false;
            for (n, (ps, pe)) in &pkgs {
                if n == name {
                    inner.push_str(&format!("    {entry},\n"));
                    replaced = true;
                } else {
                    inner.push_str(&format!("    {},\n", &out[*ps..*pe]));
                }
            }
            if !replaced {
                inner.push_str(&format!("    {entry},\n"));
            }
            out.replace_range(open..=close, &format!("[\n{inner}]"));
        }
        None => {
            // 无 deps 字段：在 Build 字面量 `}` 前插入 deps 字段
            let brace = find_char(&out, build_span.0, b'{').unwrap_or(build_span.0);
            let close = find_block_close(&out, brace).unwrap_or(build_span.1);
            out.insert_str(close, &format!("\n    deps = [ {entry}, ],"));
        }
    }
    match std::fs::write(zon_path, &out) {
        Ok(()) => {
            println!("依赖 `{name}` 已写入 build.zon deps（version {ver}，path `{path_s}`）");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: 写入 build.zon 失败: {e}");
            ExitCode::FAILURE
        }
    }
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
                    println!(
                        "生成 {} 个文件到 {}",
                        paths.len(),
                        out_dir.display()
                    );
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

/// A3（ADR-0010）：程序参数 = [程序名] + 文件后参数（0 号 = 程序名）
fn program_args<'a>(after_file: &'a [String], file: &str) -> Vec<String> {
    std::iter::once(file.to_string())
        .chain(after_file.iter().cloned())
        .collect()
}

fn run_file(path: &Path, prog_args: &[String]) -> ExitCode {
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
    // A3：程序 args（[程序名] + 文件后参数）；io.args() 已取消
    interp.args = prog_args.to_vec();
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
        Ok(()) => {
            // G5/§8.3 Debug 泄漏检测：退出时报告泄漏清单（不改变退出码）
            report_leaks(path.to_string_lossy().as_ref(), &interp.leak_report());
            match interp.exit_code {
                Some(0) => ExitCode::SUCCESS,
                Some(c) => ExitCode::from(c),
                None => ExitCode::SUCCESS,
            }
        }
        Err(e) => {
            // G5/§8.3：出错路径同样报告泄漏
            report_leaks(path.to_string_lossy().as_ref(), &interp.leak_report());
            eprintln!("{}", e.render(&source));
            ExitCode::FAILURE
        }
    }
}

// ---------- `hc run --ir`：IR 参考解释器过渡模式（M3.2 字节码 VM 的过渡形态） ----------

/// G5/§8.3 Debug 泄漏检测：程序退出时报告泄漏清单（打印到 stderr；不改变退出码——
/// 泄漏是资源缺陷，`hc test` 的通过判定仍以断言为准，报告作为 Debug 观测面）。
fn report_leaks(name: &str, leaks: &str) {
    if !leaks.is_empty() {
        eprintln!("{name}::[LEAK]\n{leaks}");
    }
}

/// IR 运行结果归一化（对齐 M2.6 根作用域语义：未处理错误到根 → panic 式失败）
#[derive(Debug, Clone, PartialEq, Eq)]
enum IrRunOutcome {
    /// main 正常返回（非错误值）——退出码 0
    Success,
    /// main 经 io.exit 请求退出（F2：进程退出码 = 请求码；对齐 oracle Interp.exit_code）
    Exited(u8),
    /// main 返回未处理的 error 值（值通道到达入口，panic 式失败，无恢复）
    UnhandledError(String),
}

/// 用 IR 参考解释器运行源码入口 `main`（`hc run --ir` 核心，可测试）。
///
/// 流程：解析 → 语义检查（准确优先）→ `lower` → `execute_ir`；失败返回可直接
/// 打印的文本（诊断渲染 / `error.{name}: {message}` + 切片外特性提示）。
/// 不依赖文件系统与退出码——测试用（生产入口走 [`run_ir_source_with_args`]）。
#[cfg(test)]
fn run_ir_source(source: &str) -> Result<IrRunOutcome, String> {
    run_ir_source_with_args(source, &[])
}

/// 同 [`run_ir_source`]，带程序参数（A3：`main(args)`——0 号 = 程序名）。
fn run_ir_source_with_args(source: &str, prog_args: &[String]) -> Result<IrRunOutcome, String> {
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
    // 3) 降级为线性 IR，交给共享执行器（`hc run --ir` 与字节码 VM 同语义源）；
    //    子集外特性 → 硬错误（不静默丢弃）
    let module = match hc::ir::lower(&program) {
        Ok(m) => m,
        Err(e) => return Err(format!("error.{}: {}", e.name, e.message)),
    };
    execute_ir(&module, prog_args)
}

/// 执行已降级的 IR 模块入口 `main`，结果归一化为 [`IrRunOutcome`]。
///
/// `hc run --ir`（`lower` 后）与字节码 VM（`decode` 后）共用——ADR-0004 唯一语义源。
/// 走 [`IrRuntime`]（共享堆 + 全局 cell + `@__init__` 一次性初始化 + 隐式环境注入），
/// 运行后冲刷 `io.print` 缓冲（`ctx.out`）到 stdout。
fn execute_ir(module: &hc::ir::IrModule, prog_args: &[String]) -> Result<IrRunOutcome, String> {
    // 入口 main 必须存在（NoMain——先查表，避免 call 的 NoFunction 误导为子集外）
    if !module.func_index.contains_key("main") {
        return Err("error.NoMain: 入口函数 `main` 未定义".into());
    }
    let mut rt = hc::ir::IrRuntime::new();
    // A3（ADR-0010）：程序 args（[程序名] + 文件后参数）；io.args() 已取消
    rt.ctx.args = prog_args.iter().map(|a| a.clone().into_bytes()).collect();
    let result = rt.call(module, "main", &[]);
    // G5/§8.3 Debug 泄漏检测：IR 侧程序退出时报告泄漏清单（不改变退出码）
    report_leaks("<ir>", &rt.ctx.leak_report());
    // 冲刷 io.print 缓冲（ctx.out）——成功/退出/错误均先落盘
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(&rt.ctx.out);
    let _ = stdout.flush();
    match result {
        // 未处理错误值到达入口（值通道）：panic 式失败
        Ok(hc::ir::IrValue::Err { name, .. }) => Ok(IrRunOutcome::UnhandledError(name)),
        Ok(_) => Ok(IrRunOutcome::Success),
        Err(e) => {
            // io.exit(code)：正常退出信号（对齐 oracle run_main——退出码 = 请求码，F2）
            if e.name == "ExitRequested" {
                return Ok(IrRunOutcome::Exited(rt.ctx.exit_code.unwrap_or(0)));
            }
            let mut msg = format!("error.{}: {}", e.name, e.message);
            // NoFunction/TypeError 常来自接口类型参/裸指针等 IR 子集外特性：追加提示
            if e.name == "NoFunction" || e.name == "TypeError" {
                msg.push_str(
                    "\n程序使用了 IR 子集外特性（接口类型参/裸指针等）——请用默认 \
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
        Ok(IrRunOutcome::Exited(0)) => ExitCode::SUCCESS,
        Ok(IrRunOutcome::Exited(c)) => ExitCode::from(c),
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
fn run_file_ir(path: &Path, prog_args: &[String]) -> ExitCode {
    let source = match read_program(path) {
        Ok(s) => s,
        Err(c) => return c,
    };
    ir_exit(run_ir_source_with_args(&source, prog_args))
}

/// `hc run file.hbc`：装载 HBC2 字节码 + `execute_ir` + 映射退出码（M3.2 字节码 VM）。
fn run_file_bytecode(path: &Path, prog_args: &[String]) -> ExitCode {
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
    ir_exit(execute_ir(&module, prog_args))
}

/// C1：包目录入口文件——`main.hc` 优先，否则目录内首个 `.hc`（排序后）；无 .hc 报错。
fn package_entry(dir: &Path) -> Result<PathBuf, String> {
    let mut hc_files: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().map_or(false, |e| e == "hc") {
                hc_files.push(p);
            }
        }
    }
    hc_files.sort();
    if let Some(main) = hc_files.iter().find(|p| {
        p.file_name()
            .map_or(false, |n| n.to_string_lossy() == "main.hc")
    }) {
        return Ok(main.clone());
    }
    hc_files.into_iter().next().ok_or_else(|| {
        format!(
            "目录 {} 中无 .hc 文件（入口 main.hc 或任意 .hc）",
            dir.display()
        )
    })
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
        eprintln!(
            "{} 兄弟文件装载: {} {}",
            paint(err_color(), "31", "[FAIL]"),
            e.name,
            e.message
        );
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
        // H2：缺失依赖诊断——本地依赖 path 必须指向存在的完整包（build.zon + .hc）；
        // 缺失/无清单 → 硬错误（不静默跳过），提示修正声明或 `hc pkg add` 重写
        if !dep_dir.is_dir() {
            eprintln!(
                "{} 依赖 {} 路径不存在: {}（本地依赖 path 须指向包目录；修正 build.zon 或移除该声明）",
                paint(err_color(), "31", "[FAIL]"),
                dep.name,
                dep_dir.display()
            );
            return Err(ExitCode::FAILURE);
        }
        if !dep_dir.join("build.zon").exists() {
            eprintln!(
                "{} 依赖 {} 目录 {} 无 build.zon（本地依赖须为完整包：build.zon + .hc）",
                paint(err_color(), "31", "[FAIL]"),
                dep.name,
                dep_dir.display()
            );
            return Err(ExitCode::FAILURE);
        }
        let canon = std::fs::canonicalize(&dep_dir).unwrap_or_else(|_| dep_dir.clone());
        if !visited.insert(canon.clone()) {
            continue; // 已装载（防环）
        }
        let dep_manifest = match buildzon::load_from_dir(&canon) {
            Ok(Some(m)) => m,
            Ok(None) => {
                eprintln!(
                    "{} 依赖 {} 目录 {} 无 build.zon",
                    paint(err_color(), "31", "[FAIL]"),
                    dep.name,
                    canon.display()
                );
                return Err(ExitCode::FAILURE);
            }
            Err(e) => {
                eprintln!("[warn] 依赖 {} 清单解析失败: {e}", dep.name);
                continue;
            }
        };
        // H2：版本声明检查——本地 path 权威，但声明版本与本地清单不符时告警
        if !dep.version.is_empty()
            && !dep_manifest.version.is_empty()
            && dep.version != dep_manifest.version
        {
            eprintln!(
                "[warn] 依赖 {} 声明版本 {} 与本地 {} 不符",
                dep.name, dep.version, dep_manifest.version
            );
        }
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
                eprintln!(
                    "{} 依赖 {} 装载: {} {}",
                    paint(err_color(), "31", "[FAIL]"),
                    dep.name,
                    e.name,
                    e.message
                );
                return Err(ExitCode::FAILURE);
            }
        }
        // 递归装载依赖的依赖
        load_deps_into(interp, &canon, &dep_manifest, visited)?;
    }
    Ok(())
}

type ParsedFile = (PathBuf, String, hc::Program);

/// Q-T5：编译模式交叉验证——原生 runner 退出码 vs 解释器该文件聚合结果。
/// 「解释器该文件有失败」⟺「原生退出码非 0」一致返回 Ok；不一致返回 Err（含诊断）。
/// 中间产物（.ll/.exe/.pdb）写到系统临时目录，运行后清理——不污染源码目录。
fn cross_validate_native(
    source: &str,
    entry: &hc::Program,
    siblings: &[&hc::Program],
    interp_fail: usize,
) -> Result<(), String> {
    let ll = programs_to_test_ll(entry, source, siblings)?;
    let n = TEST_DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
    let work = std::env::temp_dir().join(format!("hc_test_{}_{}", std::process::id(), n));
    std::fs::create_dir_all(&work).map_err(|e| format!("创建临时目录失败: {e}"))?;
    let ll_path = work.join("prog.ll");
    std::fs::write(&ll_path, &ll).map_err(|e| format!("写入 {} 失败: {e}", ll_path.display()))?;
    let exe_name = if cfg!(windows) { "prog.exe" } else { "prog" };
    let exe_path = work.join(exe_name);
    if let Err(e) = link_exe(&ll_path, &exe_path, &[]) {
        let _ = std::fs::remove_dir_all(&work);
        return Err(e);
    }
    let out = match std::process::Command::new(&exe_path).output() {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&work);
            return Err(format!("运行 {} 失败: {e}", exe_path.display()));
        }
    };
    let _ = std::fs::remove_dir_all(&work);

    let native_green = out.status.success();
    let interp_green = interp_fail == 0;
    if interp_green == native_green {
        return Ok(());
    }
    let mut detail = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !stderr.is_empty() {
        if !detail.is_empty() {
            detail.push('\n');
        }
        detail.push_str(&stderr);
    }
    Err(format!(
        "解释器 {} 失败（{}）vs 原生退出 {}（{}）{}",
        interp_fail,
        if interp_green { "绿" } else { "红" },
        out.status
            .code()
            .map_or_else(|| "异常".into(), |c| c.to_string()),
        if native_green { "绿" } else { "红" },
        if detail.is_empty() {
            String::new()
        } else {
            format!("\n{detail}")
        }
    ))
}

fn test_dir(target: &Path, mode: TestMode) -> ExitCode {
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
    // Q-T5：编译模式需 zig cc（原生后端）；缺失不静默降级
    if mode == TestMode::Compile && !zig_cc_available() {
        eprintln!("error: --mode=compile 需 zig cc（原生后端）；未检测到 zig");
        return ExitCode::FAILURE;
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
    let mut total_mismatch = 0usize;
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
                            eprintln!(
                                "{} {name}: {}",
                                paint(err_color(), "31", "[FAIL]"),
                                d.message
                            );
                        }
                    }
                },
                Err(e) => bad.push((f.clone(), format!("io: {e}"))),
            }
        }
        for (f, err) in &bad {
            let name = f.file_name().unwrap_or_default().to_string_lossy();
            eprintln!("{} {name} ({err})", paint(err_color(), "31", "[FAIL]"));
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
                    eprintln!(
                        "{} {name} (sibling load: {} {})",
                        paint(err_color(), "31", "[FAIL]"),
                        e.name,
                        e.message
                    );
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
                eprintln!(
                    "{} {name} (load error: {})",
                    paint(err_color(), "31", "[FAIL]"),
                    e.name
                );
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
            // G5/§8.3 Debug 泄漏检测：每个文件测试结束后报告泄漏（不改变通过判定）
            report_leaks(&name, &interp.leak_report());
            let on = out_color();
            for line in &interp.test_out {
                println!("{name}::{}", color_test_line(line, on));
            }
            // Q-T5：编译模式——原生 runner 退出码 vs 解释器该文件聚合结果交叉验证
            if mode == TestMode::Compile {
                match cross_validate_native(source, program, &siblings, fail) {
                    Ok(()) => println!("{} {name}", paint(out_color(), "32", "[MATCH]")),
                    Err(msg) => {
                        eprintln!("{} {name}: {msg}", paint(err_color(), "31", "[MISMATCH]"));
                        total_mismatch += 1;
                        all_ok = false;
                    }
                }
            }
        }
    }

    let on = out_color();
    println!(
        "{} passed, {} failed, {} skipped",
        paint(on, "32", &total_p.to_string()),
        paint(on, "31", &total_f.to_string()),
        paint(on, "33", &total_s.to_string()),
    );
    if mode == TestMode::Compile && total_mismatch > 0 {
        println!("{}", paint(on, "33", &format!("{total_mismatch} mismatch")));
    }
    if all_ok && total_f == 0 && total_mismatch == 0 {
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

#[cfg(test)]
mod tests {
    use super::{
        color_test_line, merge_modules, paint, programs_to_ll, programs_to_test_ll, run_ir_source,
        source_to_bytecode, strip_test_funcs_in_place, write_bytecode_artifact, IrRunOutcome,
    };

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
    fn ir_io_exit_maps_code() {
        // F2：io.exit 在 IR 侧映射退出码（Exited(code)，对齐 oracle Interp.exit_code）
        let src = "import H.std.{io};\nfn main(args: o Vec(String)) !void { io.exit(ExitType.Error, 3); }\n";
        match run_ir_source(src) {
            Ok(IrRunOutcome::Exited(3)) => {}
            other => panic!("预期 Exited(3)，实际：{other:?}"),
        }
        let ok = "import H.std.{io};\nfn main(args: o Vec(String)) !void { io.exit(ExitType.Exit, 0); }\n";
        match run_ir_source(ok) {
            Ok(IrRunOutcome::Exited(0)) => {}
            other => panic!("预期 Exited(0)，实际：{other:?}"),
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
    fn io_print_through_ir() {
        // Phase 7：io.print 已入 IR 子集——`io` 隐式环境经 LoadGlobal 解析，限定名
        // 调用路由 call_dotted_implicit → call_io_method_ir，成功返回。
        let src = r#"fn main() void { io.print("hi"); }"#;
        match run_ir_source(src) {
            Ok(_) => {}
            other => panic!("预期 Success，实际：{other:?}"),
        }
    }

    #[test]
    fn parse_error_rendered() {
        // 解析失败 → 渲染诊断文本
        assert!(run_ir_source("fn main( {").is_err());
    }

    #[test]
    fn leak_report_does_not_change_exit_code() {
        // G5/§8.3 Debug 泄漏检测：`hc run --ir` 下泄漏程序仍返回 Success——
        // CLI 退出时打印泄漏清单到 stderr，但**不改变退出码**（保持绿，退出码留给 §11）
        expect_success("fn main() void { var buf = alloc.alloc(8); }");
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

    #[test]
    fn merge_modules_exports_qualified_only() {
        // 入口 + 兄弟：兄弟顶层函数扁平名文件私有；命名空间函数限定名导出、索引偏移正确
        let entry =
            hc::ir::lower(&hc::parse_source("fn main() i32 { return Math.square(4); }\n").unwrap())
                .unwrap();
        let sib = hc::ir::lower(
            &hc::parse_source(
                "fn load_config(x: i32) i32 { return x; }\nnamespace Math { fn square(x: i32) i32 { return x * x; } }\n",
            )
            .unwrap(),
        )
        .unwrap();
        let merged = merge_modules(entry, vec![sib]);
        assert!(merged.func_index.contains_key("main"));
        assert!(merged.func_index.contains_key("Math.square"));
        // 兄弟顶层函数扁平名不导出（文件私有）
        assert!(!merged.func_index.contains_key("load_config"));
        // 限定名索引落在追加段（入口在前，偏移后合法）
        assert!(merged.func_index["Math.square"][0] < merged.funcs.len());
        assert!(merged.funcs.len() >= 2);
    }

    #[test]
    fn merge_modules_concats_globals_and_init() {
        // Phase 5 多文件：兄弟 global/const 并入全局表（去重保序），各模块 `@__init__` 保留。
        // IrRuntime::init 预分配全部全局 cell 后按 funcs 序执行各 `@__init__`——同名全局
        // 共享同一 cell，后者覆盖（对齐解释器「后载入覆盖」）。
        let entry = hc::ir::lower(
            &hc::parse_source("global app: i32 = 1;\nfn main() i32 { return app; }\n").unwrap(),
        )
        .unwrap();
        let sib = hc::ir::lower(
            &hc::parse_source("global lib: i32 = 2;\nglobal shared: i32 = 0;\n").unwrap(),
        )
        .unwrap();
        let sib2 = hc::ir::lower(&hc::parse_source("global shared: i32 = 9;\n").unwrap()).unwrap();
        let merged = merge_modules(entry, vec![sib, sib2]);
        // 全局表：声明序 + 去重（同名只保留入口/先序一份）。
        // Phase 7 起隐式环境名（alloc/io/pi/Vec…）也登记全局——入口模块已含，
        // 兄弟并入时去重跳过；用户全局仍保声明序（app → lib → shared）。
        assert_eq!(
            merged.globals,
            vec![
                "app", "alloc", "io", "test_io", "stdout", "stderr", "pi", "Vec", "Deque", "Map",
                "Table", "lib", "shared",
            ]
        );
        // 各模块 `@__init__` 全部保留（funcs 序依次执行）
        let init_count = merged
            .funcs
            .iter()
            .filter(|f| f.name == "@__init__")
            .count();
        assert_eq!(init_count, 3);
    }

    #[test]
    fn programs_to_ll_multi_file_and_private_sibling() {
        // 入口调用兄弟命名空间函数 + 兄弟同名顶层函数（不误报 ambiguous）：联合检查 + 合并 codegen
        let entry = hc::parse_source(
            "fn load_config(x: i32) i32 { return x + 1; }\nfn main() i32 { return load_config(1) + Math.square(4); }\n",
        )
        .unwrap();
        let sib = hc::parse_source(
            "fn load_config(x: i32) i32 { return x * 2; }\nnamespace Math { fn square(x: i32) i32 { return x * x; } }\n",
        )
        .unwrap();
        let ll = programs_to_ll(
            &entry,
            "fn load_config(x: i32) i32 { return x + 1; }\nfn main() i32 { return load_config(1) + Math.square(4); }\n",
            &[&sib],
        )
        .expect("codegen");
        assert!(ll.contains("define"), "应生成函数定义");
        assert!(ll.contains("@main"), "应生成入口 wrapper");
    }

    #[test]
    fn strip_test_funcs_remaps_index() {
        // 剔除 [test] fn 后：扁平/限定名保留且索引重映射到正确函数；[test] fn 名移除
        let mut m = hc::ir::lower(
            &hc::parse_source(
                "[test] fn a() !void {}\nfn helper() i32 { return 1; }\nnamespace N { fn f() i32 { return 2; } }\n",
            )
            .unwrap(),
        )
        .unwrap();
        strip_test_funcs_in_place(&mut m);
        assert!(m.funcs.iter().all(|f| !f.is_test));
        assert!(m.func_index.contains_key("helper"));
        assert!(m.func_index.contains_key("N.f"));
        assert!(!m.func_index.contains_key("a"));
        assert_eq!(m.funcs[m.func_index["helper"][0]].name, "helper");
        assert_eq!(m.funcs[m.func_index["N.f"][0]].name, "f");
    }

    #[test]
    fn test_runner_runs_only_entry_tests() {
        // 兄弟文件 [test] fn 文件私有：测试跑器只调用入口的 test fn
        let entry_src = "[test] fn a() !void {}\nfn main() i32 { return 0; }\n";
        let entry = hc::parse_source(entry_src).unwrap();
        let sib = hc::parse_source("[test] fn b() !void {}\n").unwrap();
        let ll = programs_to_test_ll(&entry, entry_src, &[&sib]).expect("codegen_tests");
        assert!(ll.contains("[RUN] a"), "应含入口测试 a 的运行标记");
        assert!(!ll.contains("[RUN] b"), "不应含兄弟测试 b 的运行标记");
    }

    #[test]
    fn color_helpers_paint_and_test_line() {
        // paint：开=true 产 ANSI 码，关=false 原样返回
        assert_eq!(paint(true, "32", "x"), "\u{1b}[32mx\u{1b}[0m");
        assert_eq!(paint(false, "32", "x"), "x");
        // color_test_line：终端下 [PASS]/[FAIL]/[SKIP] 分别绿/红/黄，其余原样
        assert_eq!(
            color_test_line("[PASS] a", true),
            "\u{1b}[32m[PASS]\u{1b}[0m a"
        );
        assert_eq!(
            color_test_line("[FAIL] b (error.X)", true),
            "\u{1b}[31m[FAIL]\u{1b}[0m b (error.X)"
        );
        assert_eq!(
            color_test_line("[SKIP] c", true),
            "\u{1b}[33m[SKIP]\u{1b}[0m c"
        );
        assert_eq!(color_test_line("[PASS] a", false), "[PASS] a");
        assert_eq!(color_test_line("other line", true), "other line");
    }
}
