//! 文件检查类命令：`hc check`（check_file）/ `hc errors`（errors_file）

use std::path::Path;
use std::process::ExitCode;

use hc_rt::Interp;

use crate::lint;
use crate::run::{load_manifest_deps_into, load_siblings_into};
use crate::script;

use super::read_source::read_source;

/// `hc errors file.hc`：输出错误码表（M2.6）——错误名 ↔ 码（包 ID + 包内码）+ 首次出现位置
pub(crate) fn errors_file(path: &Path) -> ExitCode {
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

pub(crate) fn check_file(path: &Path) -> Result<(), ExitCode> {
    let source = read_source(path)?;
    match script::parse_with_scripts(&source) {
        Ok((source, mut program)) => {
            // M1-1：文件级命名空间自动推断
            let project_root = script::find_project_root(path);
            let ns_name = script::compute_namespace_name(path, project_root.as_deref());
            script::infer_namespace(&mut program, &ns_name, Some(path));
            let mut interp = Interp::new(&source);
            // M1.4：同包兄弟文件先登记符号（解析失败仅告警）
            if let Err(code) = load_siblings_into(&mut interp, path, &ns_name) {
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
