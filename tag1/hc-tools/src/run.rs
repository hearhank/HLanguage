//! `hc run`：脚本模式（tree-walking）/ IR 参考解释器 / 字节码 VM + 依赖装载。

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hc::diag;
use hc_rt::Interp;

use crate::buildzon;
use crate::cli::{err_color, paint};
use crate::fsio::{read_bytecode, read_program};
use crate::package::sibling_files;
use crate::project::resolve_registry_dep;
use crate::scriptgen;

/// A3（ADR-0010）：程序参数 = [程序名] + 文件后参数（0 号 = 程序名）
pub(crate) fn program_args<'a>(after_file: &'a [String], file: &str) -> Vec<String> {
    std::iter::once(file.to_string())
        .chain(after_file.iter().cloned())
        .collect()
}

pub(crate) fn run_file(path: &Path, prog_args: &[String]) -> ExitCode {
    let source = match read_program(path) {
        Ok(s) => s,
        Err(c) => return c,
    };
    // E1（ADR-0013）：装载期 script 展开——interp 与程序均基于展开后源码
    let (source, program) = match scriptgen::parse_with_scripts(&source) {
        Ok(t) => t,
        Err(msg) => {
            eprintln!("{msg}");
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

/// G5/§8.3 Debug 泄漏检测：程序退出时报告泄漏清单（打印到 stderr；不改变退出码——
/// 泄漏是资源缺陷，`hc test` 的通过判定仍以断言为准，报告作为 Debug 观测面）。
pub(crate) fn report_leaks(name: &str, leaks: &str) {
    if !leaks.is_empty() {
        eprintln!("{name}::[LEAK]\n{leaks}");
    }
}

/// IR 运行结果归一化（对齐 M2.6 根作用域语义：未处理错误到根 → panic 式失败）
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IrRunOutcome {
    /// main 正常返回（非错误值）——退出码 0
    Success,
    /// main 经 io.exit 请求退出（F2：进程退出码 = 请求码；对齐 oracle Interp.exit_code）
    Exited(u8),
    /// main 返回未处理的 error 值（值通道到达入口，panic 式失败，无恢复）
    UnhandledError(String),
}

/// 同 [`run_ir_source`]，带程序参数（A3：`main(args)`——0 号 = 程序名）。
pub(crate) fn run_ir_source_with_args(
    source: &str,
    prog_args: &[String],
) -> Result<IrRunOutcome, String> {
    // 0) E1（ADR-0013）：装载期 script 展开 + 解析
    let (expanded, program) = scriptgen::parse_with_scripts(source)?;
    // 2) 语义检查（准确优先：能精确判定才报错——与 tree-walking load 内建检查对齐；
    //    有错误则渲染诊断返回失败）
    let errs = hc::check_semantics(&program);
    if errs.iter().any(|d| d.is_error()) {
        return Err(diag::render(&errs, &expanded));
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
pub(crate) fn run_file_ir(path: &Path, prog_args: &[String]) -> ExitCode {
    let source = match read_program(path) {
        Ok(s) => s,
        Err(c) => return c,
    };
    ir_exit(run_ir_source_with_args(&source, prog_args))
}

/// `hc run file.hbc`：装载 HBC2 字节码 + `execute_ir` + 映射退出码（M3.2 字节码 VM）。
pub(crate) fn run_file_bytecode(path: &Path, prog_args: &[String]) -> ExitCode {
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

/// 登记目标文件的同包兄弟声明（跳过其 test/main；解析失败的兄弟仅告警不阻断）
pub(crate) fn load_siblings_into(interp: &mut Interp, path: &Path) -> Result<(), ExitCode> {
    let sibs = sibling_files(path);
    if sibs.is_empty() {
        return Ok(());
    }
    let mut programs = Vec::new();
    for s in &sibs {
        match std::fs::read_to_string(s) {
            Ok(src) => match scriptgen::parse_with_scripts(&src) {
                Ok((_, p)) => programs.push(p),
                Err(msg) => {
                    eprintln!("[warn] 兄弟文件解析失败 {}:\n{}", s.display(), msg);
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

/// 读取目标文件所在目录的 build.zon（如有）并递归装载本地依赖
pub(crate) fn load_manifest_deps_into(interp: &mut Interp, path: &Path) -> Result<(), ExitCode> {
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

/// M7.2：递归装载依赖包（build.zon `deps` 中带 `path` 的本地依赖 + 注册中心依赖）；
/// 依赖文件缺省回退「目录全部 .hc」。
pub(crate) fn load_deps_into(
    interp: &mut Interp,
    dir: &Path,
    manifest: &buildzon::Manifest,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), ExitCode> {
    for dep in &manifest.deps {
        let dep_dir = if let Some(rel) = &dep.path {
            dir.join(rel)
        } else {
            // B3：注册中心依赖——从 ~/.hc/registry/<name>/<version>/ 解析
            match resolve_registry_dep(&dep.name, &dep.version, dep.fingerprint.as_deref()) {
                Ok((reg_dir, _)) => reg_dir,
                Err(msg) => {
                    eprintln!("error: {msg}");
                    return Err(ExitCode::FAILURE);
                }
            }
        };
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
            crate::package::dir_hc_files(&canon)
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
