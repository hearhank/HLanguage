//! Comptime 值函数生成：comptime 块中值函数的代码生成
//!
//! 定义：结构体：ComptimeSite

use hc::ast::{Block, Decl, Program};
use hc::diag::{self, Diagnostic};
use hc_rt::{Interp, Value};

/// 装载期求值全部 comptime 块（script 展开后、语义检查前）。无 comptime 块时零开销
/// （站点列表为空，不构造 Interp）。失败 = 已渲染诊断文本（调用方直接打印）。
pub fn eval_comptime_blocks(source: &str, program: &Program) -> Result<(), String> {
    for site in find_comptime_blocks(program) {
        eval_comptime(source, program, &site)?;
    }
    Ok(())
}

/// 待求值的 comptime 块站点
struct ComptimeSite<'a> {
    body: &'a Block,
    /// 所属 `comptime { }` 块 span（`return error.X` 诊断锚点）
    span: hc::token::Span,
}

/// 按源码序收集全部 comptime 块（顶层 + 命名空间体递归；class 内 comptime 块仍待补定）
fn find_comptime_blocks(program: &Program) -> Vec<ComptimeSite<'_>> {
    let mut out = Vec::new();
    for d in &program.decls {
        collect_in_decl(d, &mut out);
    }
    out
}

fn collect_in_decl<'a>(d: &'a Decl, out: &mut Vec<ComptimeSite<'a>>) {
    match d {
        Decl::Comptime { body, span } => out.push(ComptimeSite {
            body,
            span: span.clone(),
        }),
        Decl::Include { .. } => {}
        Decl::Namespace { decls, .. } => {
            for inner in decls {
                collect_in_decl(inner, out);
            }
        }
        _ => {}
    }
}

/// 求值单个 comptime 块体（受限 Interp）。失败 = 编译错误（已渲染诊断文本）。
fn eval_comptime(source: &str, program: &Program, site: &ComptimeSite) -> Result<(), String> {
    let mut interp = Interp::new(source);
    interp.set_script_mode(true);
    interp
        .load(program)
        .map_err(|e| format!("comptime 块装载失败: {}", e.render(source)))?;
    let v = interp
        .exec_fn_body(site.body, &[])
        .map_err(|e| format!("comptime 块求值失败: {}", e.render(source)))?;
    match v {
        // `return error.X` = 块显式失败 → 编译错误（沿 06-09：comptime 块可返回错误）
        Value::Err { name, .. } => {
            let d = Diagnostic::error(
                site.span.clone(),
                format!("comptime 块返回错误 `error.{name}`（编译错误）"),
            );
            Err(diag::render(&[d], source))
        }
        // 其余结果丢弃——comptime 块仅编译期存在
        _ => Ok(()),
    }
}
