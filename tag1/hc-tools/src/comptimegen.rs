//! E1.2（组 D D2）：`comptime { }` 块装载期求值。
//!
//! 流程（ADR-0012 决策 3，最小切片）：script 展开 + 解析后，对每个 `comptime { }` 块用
//! **受限 Interp**（`script_mode`：io/alloc/argv 不可用，`types` 元数据全量可见）求值块体；
//! 结果**丢弃**（不产生运行时代码、不替换源码）；求值失败（运行时错误 / `return error.X`）
//! = 编译错误（带块内位置 + 所属块位置）。类型级副作用回填（注册类型/实例化/常量折叠）
//! 归组 D3/D4——本切片仅「求值 + 错误机制」。
//!
//! 求值在装载期完成（script 展开后、语义检查前），IR/字节码/native 对 comptime 块无感知
//! （各后端跳过 `Decl::Comptime`，镜像 `Decl::Script`）。

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
        Decl::Script { .. } => {}
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
