//! hc 工具链 CLI（M7.1：`hc build` / `hc run` / `hc test`——tag1 子集）
//!
//! - `hc run <file.hc>`：脚本模式（tree-walking 解释器）
//! - `hc run --ir <file.hc>`：IR 参考解释器过渡模式（M3.2 字节码 VM 的过渡形态，标量子集）
//! - `hc test [file.hc|dir]`：收集并运行 `test fn`，输出 [PASS]/[FAIL]/[SKIP] + 汇总
//! - `hc build <file.hc>`：tag1 占位（LLVM 原生后端归 M3.3）
//! - `hc check <file.hc>`：仅词法/语法/装载检查

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hc::diag;
use hc_rt::Interp;

const USAGE: &str = "hc <command> [args...]

H 语言工具链（tag1 垂直切片）

USAGE:
    hc run <file.hc>           运行脚本模式（解释执行）
    hc run --ir <file.hc>      用 IR 参考解释器运行（M3.2 字节码 VM 过渡；标量子集）
    hc test [file.hc|dir]      运行 test fn（默认当前目录全部 .hc）
    hc check <file.hc>         仅检查（词法/语法/装载）
    hc errors <file.hc>        输出错误码表（M2.6：错误名 ↔ 码 + 位置）
    hc build <file.hc>         编译（tag1 占位：LLVM 后端归 M3.3）
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
            // 显式模式标志：`hc run --ir <file>` 走 IR 参考解释器；否则默认 tree-walking
            if path == "--ir" {
                let Some(p) = args.get(3) else {
                    eprintln!("error: `hc run --ir` requires a file path");
                    return ExitCode::from(2);
                };
                run_file_ir(Path::new(p))
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

/// 字节码镜像魔数（tag1：镜像 = 魔数 + 源码；完整字节码 VM 归 M3.2 后续）
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

/// `hc build file.hc`：语法验证 → 生成字节码镜像 .hbc + 平台启动器
/// （tag1 过渡产物：M3.3 LLVM 原生后端前的可分发形态；镜像由解释器加载）
fn build_file(path: &Path) -> ExitCode {
    let source = match read_program(path) {
        Ok(s) => s,
        Err(c) => return c,
    };
    // 1) 编译期检查：词法/语法/装载
    if let Err(diags) = hc::parse_source(&source) {
        eprint!("{}", diag::render(&diags, &source));
        return ExitCode::FAILURE;
    }
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let dir = path.parent().unwrap_or_else(|| Path::new("."));

    // 2) 字节码镜像：HBC1 + u64 源码长度 + 源码
    let src_bytes = source.as_bytes();
    let mut image = Vec::new();
    image.extend_from_slice(HBC_MAGIC);
    image.extend_from_slice(&(src_bytes.len() as u64).to_le_bytes());
    image.extend_from_slice(src_bytes);
    let hbc_path = dir.join(format!("{stem}.hbc"));
    if let Err(e) = std::fs::write(&hbc_path, &image) {
        eprintln!("error: 写入 {} 失败: {e}", hbc_path.display());
        return ExitCode::FAILURE;
    }

    // 3) 平台启动器（直接执行字节码镜像）
    let runner = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("hc-tools"));
    let launcher = if cfg!(windows) {
        let l = dir.join(format!("{stem}.cmd"));
        let content = format!(
            "@echo off\r\nrem H 语言字节码启动器（tag1：由解释器加载 .hbc）\r\n\"{}\" run \"{}\"\r\n",
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
    println!("  字节码镜像: {}", hbc_path.display());
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
/// 流程：解析 → 语义检查（准确优先）→ `lower` → 查 `func_index` 有 `main` →
/// `run_ir(module, "main", [])` → 结果归一化；失败返回可直接打印的文本
/// （诊断渲染 / `error.{name}: {message}` + 切片外特性提示）。
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
    // 3) 降级为线性 IR
    let module = hc::ir::lower(&program);
    // 4) 入口 main 必须存在（NoMain——先查表，避免 run_ir 的 NoFunction 误导为切片外）
    if !module.func_index.contains_key("main") {
        return Err("error.NoMain: 入口函数 `main` 未定义".into());
    }
    // 5) 运行（main(io: Io) 的 io 参数在 IR 下为 Void 占位——用了 io.* 会走 NoFunction
    //    提示，正常；零参 main 可完整运行）
    match hc::ir::run_ir(&module, "main", &[]) {
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

/// `hc run --ir file.hc`：只读文件 + 调用 `run_ir_source` + 映射退出码。
/// 退出码语义：IR 模式只看 run_ir 结果（Ok=0，Err/未处理错误=非零）；
/// main 返回非零 Int 不影响退出码。
fn run_file_ir(path: &Path) -> ExitCode {
    let source = match read_program(path) {
        Ok(s) => s,
        Err(c) => return c,
    };
    match run_ir_source(&source) {
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
    use super::{run_ir_source, IrRunOutcome};

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
}
