//! H 语言编译器（hc）：lexer / parser / AST / 诊断 / 语义检查 / 共享 IR / 字节码 VM / LLVM 发射
//!
//! 对应实现计划（07-bootstrap-plan.md）第一部分最小功能集（M0–M7）。
//! tag1 垂直切片：核心语法子集 + 完整语义检查（所有权/重载/错误集/推断）
//! + 双后端（字节码 VM / LLVM 原生，共享 `ir::IrModule` 唯一语义源，ADR-0004）。
//! 子集外特性在 IR 降级时以 `error.Unsupported` 硬错误拒绝（见 `ir::lower`），不静默丢弃。

pub mod ast;
pub mod bytecode;
pub mod compress;
pub mod comptime;
pub mod diag;
pub mod ds_bitmap;
pub mod ds_ringbuf;
pub mod errorcodes;
pub mod ir;
pub mod lexer;
pub mod llvm;
pub mod parser;
pub mod regex;
pub mod rle;
pub mod rng;
pub mod semantic;
pub mod token;

pub use ast::Program;
pub use diag::{Diagnostic, Severity};
pub use errorcodes::{ErrorCodeTable, ErrorEntry};
pub use semantic::InferredErrorSets;
pub use token::Span;

/// 语义检查（M2 静态 pass）：返回诊断列表（空 = 通过）
pub fn check_semantics(program: &Program) -> Vec<Diagnostic> {
    semantic::check(program)
}

/// M1.4：跨文件语义检查——外部（兄弟文件/依赖包）符号并入登记
pub fn check_semantics_extern(program: &Program, externs: &[&Program]) -> Vec<Diagnostic> {
    semantic::check_with_extern(program, externs)
}

/// M7.2：主程序 + 依赖包联合语义检查（依赖包以包名前缀登记、仅 pub 可见）
pub fn check_semantics_deps(program: &Program, deps: &[(&str, &Program)]) -> Vec<Diagnostic> {
    semantic::check_with_extern_deps(program, &[], deps)
}

/// M7.2：主程序 + 同包兄弟 + 依赖包的联合语义检查（兄弟全可见；依赖按前缀 + pub）
pub fn check_semantics_extern_deps(
    program: &Program,
    externs: &[&Program],
    deps: &[(&str, &Program)],
) -> Vec<Diagnostic> {
    semantic::check_with_extern_deps(program, externs, deps)
}

/// 错误码表（M2.6）：编译期维护「错误名 ↔ 码」全局唯一映射（tag1 单包 = 包 ID 0）
pub fn error_code_table(program: &Program) -> ErrorCodeTable {
    errorcodes::collect(program, 0)
}

/// M2.6 Q-S8：`!T` 推断错误集收集（函数名 → 推断错误集成员；递归 → incomplete）
pub fn inferred_error_sets(program: &Program) -> InferredErrorSets {
    semantic::infer_error_sets(program)
}

/// 解析源码为程序 AST；失败返回收集到的诊断（首个错误）与部分解析结果。
pub fn parse_source(source: &str) -> Result<ast::Program, Vec<Diagnostic>> {
    let tokens = lexer::lex(source);
    let diags: Vec<Diagnostic> = tokens
        .iter()
        .filter_map(|t| match t.kind {
            token::TokenKind::Error(msg) => Some(Diagnostic::error(
                t.span.clone(),
                format!("lex error: {msg}"),
            )),
            _ => None,
        })
        .collect();
    if !diags.is_empty() {
        return Err(diags);
    }
    parser::Parser::new(source, tokens).parse_program()
}
