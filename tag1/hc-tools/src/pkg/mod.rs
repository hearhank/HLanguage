//! hc pkg 命令：包管理（add/publish）

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hc::diag;

use crate::cli::{err_color, paint};
use crate::project::fsio::read_program;
use crate::script;

/// IR 降级错误 → 可直接打印文本（`error.{name}: {message}`）。
pub(crate) fn lower_err(e: hc::ir::IrError) -> String {
    format!("error.{}: {}", e.name, e.message)
}

/// C1：包目录入口文件——`src/main.hc` 优先，`main.hc` 次之，否则目录内首个 `.hc`；无 .hc 报错。
/// 2026-08-23：新增 `src/` 子目录支持（新项目结构）。
pub(crate) fn package_entry(dir: &Path) -> Result<PathBuf, String> {
    // 先检查 src/main.hc（新项目结构）
    let src_main = dir.join("src").join("main.hc");
    if src_main.exists() {
        return Ok(src_main);
    }
    // 再检查根目录 main.hc（旧项目结构兼容）
    let root_main = dir.join("main.hc");
    if root_main.exists() {
        return Ok(root_main);
    }
    // 最后检查根目录下任意 .hc 文件
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
    hc_files.into_iter().next().ok_or_else(|| {
        format!(
            "目录 {} 中无 .hc 文件（入口 main.hc 或任意 .hc）",
            dir.display()
        )
    })
}

/// M7.1：解析入口文件 + 同目录全部兄弟 `.hc`（目录 = 包）。
/// 返回（入口展开后源码, 入口程序, 兄弟程序）。入口解析失败渲染诊断；兄弟解析失败报错退出。
/// E1（ADR-0013）：入口与兄弟均做装载期 script 展开。
pub(crate) fn package_programs(
    path: &Path,
) -> Result<(String, hc::Program, Vec<hc::Program>), ExitCode> {
    let source = read_program(path)?;
    let (source, entry) = script::parse_with_scripts(&source).map_err(|msg| {
        eprintln!("{msg}");
        ExitCode::FAILURE
    })?;
    let mut siblings = Vec::new();
    for s in sibling_files(path) {
        let src = std::fs::read_to_string(&s).map_err(|e| {
            eprintln!("error: 读取 {} 失败: {e}", s.display());
            ExitCode::FAILURE
        })?;
        let p = match script::parse_with_scripts(&src) {
            Ok((_, p)) => p,
            Err(msg) => {
                eprintln!(
                    "{} 兄弟文件解析失败 {}:\n{}",
                    paint(err_color(), "31", "[FAIL]"),
                    s.display(),
                    msg
                );
                return Err(ExitCode::FAILURE);
            }
        };
        siblings.push(p);
    }
    Ok((source, entry, siblings))
}

pub(crate) fn sibling_files(path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = path.parent() {
        // 同一目录下的 .hc 文件
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
        // 如果入口在 src/ 子目录，src/ 内同目录兄弟文件
        if dir.ends_with("src") {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p == path {
                        continue;
                    }
                    if p.extension().map_or(false, |e| e == "hc") {
                        if !out.contains(&p) {
                            out.push(p);
                        }
                    }
                }
            }
            // 扫描 src/Modules/ 子目录（模块自动发现，ADR-0026）
            let modules_dir = dir.join("Modules");
            if modules_dir.is_dir() {
                crate::project::fsio::collect_hc_files(&modules_dir, &mut out);
            }
        }
    }
    out.sort();
    out
}

/// 目录顶层 .hc 文件（依赖包文件清单；不递归——目录 = 包）
/// 2026-08-23：新增 `src/` 子目录支持（新项目结构）。
pub(crate) fn dir_hc_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    // 检查根目录 .hc 文件
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().map_or(false, |e| e == "hc") {
                out.push(p);
            }
        }
    }
    // 检查 src/ 子目录 .hc 文件（新项目结构）
    let src_dir = dir.join("src");
    if src_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&src_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().map_or(false, |e| e == "hc") {
                    out.push(p);
                }
            }
        }
        // 检查 src/Modules/ 子目录（模块自动发现，ADR-0026）
        let modules_dir = src_dir.join("Modules");
        if modules_dir.is_dir() {
            crate::project::fsio::collect_hc_files(&modules_dir, &mut out);
        }
    }
    out.sort();
    out
}

/// 检查 src/Modules/ 下每个子目录是否包含 context.hc（ADR-0026 约定）
pub(crate) fn validate_module_contexts(project_root: &Path) {
    let modules_dir = project_root.join("src").join("Modules");
    if !modules_dir.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(&modules_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let ctx_file = path.join("context.hc");
        if !ctx_file.is_file() {
            let module_name = path.file_name().map_or("?", |n| n.to_str().unwrap_or("?"));
            eprintln!(
                "warning: module `{module_name}` in src/Modules/ has no context.hc; \
                 each module should define a context implementing IContext"
            );
        }
    }
}

/// M7.1：合并多文件 IR 模块——入口在前（索引不变），兄弟函数依次追加。
/// 兄弟文件顶层函数扁平名（含 `main`）文件私有不导出；命名空间函数/类型方法
/// 以限定名（含 `.`）导出，索引 `+offset`（对齐运行时文件私有规则）。
/// 闭包表同步追加：兄弟模块的 `MakeClosure.func`（闭包表索引）整体平移 `closure_offset`。
pub(crate) fn merge_modules(
    entry: hc::ir::IrModule,
    siblings: Vec<hc::ir::IrModule>,
) -> hc::ir::IrModule {
    let mut funcs = entry.funcs;
    let mut func_index = entry.func_index;
    let mut closures = entry.closures;
    let mut globals = entry.globals;
    let mut error_codes = entry.error_codes;
    let mut enum_variants = entry.enum_variants;
    let mut continuous = entry.continuous;
    let mut unions = entry.unions;
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
        // K1 union 表：并入兄弟（同名同定义，覆盖等价）
        unions.extend(m.unions);
    }
    hc::ir::IrModule {
        funcs,
        closures,
        func_index,
        globals,
        error_codes,
        enum_variants,
        continuous,
        unions,
    }
}

/// Q-T5：从 IR 模块剔除 test fn——兄弟文件测试函数文件私有，不参与入口文件测试跑器
/// （对齐解释器 `load_siblings` 跳过 test/main）。同步重映射 `func_index` 到剔除后索引。
/// 闭包表不动（test fn 被剔除后其闭包成为孤儿，无害；普通函数引用的闭包索引不变）。
pub(crate) fn strip_test_funcs_in_place(module: &mut hc::ir::IrModule) {
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
pub(crate) fn check_and_merge(
    entry: &hc::Program,
    entry_source: &str,
    siblings: &[&hc::Program],
    strip_sibling_tests: bool,
) -> Result<(hc::ir::IrModule, hc::ErrorCodeTable), String> {
    check_and_merge_deps(entry, entry_source, siblings, strip_sibling_tests, &[])
}

/// M7.1 + C3：联合语义检查（`deps` = 依赖包 (包名, Program)，仅登记 pub 符号供
/// `import pkg.{sym}` 解析）→ 入口与兄弟各自 lower → 合并为单模块与错误码表。
pub(crate) fn check_and_merge_deps(
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

/// Q-T5：入口 + 同包兄弟 → 「测试驱动」LLVM IR 文本（`test fn` 跑器入口）。
pub(crate) fn programs_to_test_ll(
    entry: &hc::Program,
    entry_source: &str,
    siblings: &[&hc::Program],
) -> Result<String, String> {
    let (merged, table) = check_and_merge(entry, entry_source, siblings, true)?;
    Ok(hc::llvm::codegen_tests(&merged, &table))
}
