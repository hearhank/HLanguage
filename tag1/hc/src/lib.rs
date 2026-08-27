//! H 语言编译器（hc crate 根模块）：入口函数与公共 API 重导出

pub mod codegen;
pub mod diag;
pub mod ir;
pub mod lexer;
pub mod parser;
pub mod runtime;
pub mod semantic;

// Re-export for backward compatibility (internal crate consumers)
pub use codegen::bytecode;
pub use codegen::llvm;
pub use diag::{Diagnostic, Severity};
pub use ir::comptime;
pub use lexer::token;
pub use parser::ast;
pub use runtime::compress;
pub use runtime::errorcodes::{ErrorCodeTable, ErrorEntry};
pub use runtime::regex;
pub use runtime::rng;
pub use semantic::InferredErrorSets;

pub use ast::Program;

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
    runtime::errorcodes::collect(program, 0)
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
