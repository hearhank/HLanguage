//! 名称收集器（用于 unused_var）

use hc::ast::*;
use hc::token::Span;

/// 收集所有变量声明名（包括 fn 参数、for 捕获、switch 捕获等）
pub(crate) fn collect_decls(program: &Program) -> Vec<(String, Span)> {
    let mut decls = Vec::new();
    for d in &program.decls {
        collect_decls_in_decl(d, &mut decls);
    }
    decls
}

fn collect_decls_in_decl(decl: &Decl, decls: &mut Vec<(String, Span)>) {
    match decl {
        Decl::Fn { params, body, .. } => {
            for p in params {
                decls.push((p.name.clone(), p.span.clone()));
            }
            collect_decls_in_block(body, decls);
        }
        Decl::Class { methods, .. } => {
            for m in methods {
                for p in &m.params {
                    decls.push((p.name.clone(), p.span.clone()));
                }
                collect_decls_in_block(&m.body, decls);
            }
        }
        Decl::Namespace { decls: inner, .. } => {
            for d in inner {
                collect_decls_in_decl(d, decls);
            }
        }
        Decl::Global { name, .. } | Decl::Const { name, .. } => {
            decls.push((name.clone(), Span::new(0, 0, 0, 0)));
        }
        _ => {}
    }
}

fn collect_decls_in_block(block: &Block, decls: &mut Vec<(String, Span)>) {
    for s in &block.stmts {
        collect_decls_in_stmt(s, decls);
    }
}

fn collect_decls_in_stmt(stmt: &Stmt, decls: &mut Vec<(String, Span)>) {
    match stmt {
        Stmt::VarDecl { name, span, .. } => {
            decls.push((name.clone(), span.clone()));
        }
        Stmt::ConstDecl { name, span, .. } => {
            decls.push((name.clone(), span.clone()));
        }
        Stmt::If(stmt) => {
            if let Some((_, name)) = &stmt.capture {
                decls.push((name.clone(), stmt.span.clone()));
            }
            if let Some((_, name)) = &stmt.err_capture {
                decls.push((name.clone(), stmt.span.clone()));
            }
            collect_decls_in_block(&stmt.then_b, decls);
            if let Some(else_s) = &stmt.else_b {
                collect_decls_in_stmt(else_s, decls);
            }
        }
        Stmt::While(stmt) => {
            if let Some((_, name)) = &stmt.capture {
                decls.push((name.clone(), stmt.span.clone()));
            }
            collect_decls_in_block(&stmt.body, decls);
        }
        Stmt::For(stmt) => {
            decls.push((stmt.capture_name.clone(), stmt.span.clone()));
            collect_decls_in_block(&stmt.body, decls);
        }
        Stmt::Switch(stmt) => {
            for arm in &stmt.arms {
                if let Some((_, name)) = &arm.capture {
                    decls.push((name.clone(), arm.span.clone()));
                }
                collect_decls_in_block(&arm.body, decls);
            }
        }
        Stmt::Block(b) => collect_decls_in_block(b, decls),
        Stmt::Expr(e) | Stmt::Return(Some(e), _) | Stmt::Defer(e, _) | Stmt::Errdefer(e, _) => {
            collect_decls_in_expr(e, decls);
        }
        _ => {}
    }
}

fn collect_decls_in_expr(expr: &Expr, decls: &mut Vec<(String, Span)>) {
    match expr {
        Expr::Closure { params, body, .. } => {
            for p in params {
                decls.push((p.clone(), Span::new(0, 0, 0, 0)));
            }
            collect_decls_in_block(body, decls);
        }
        Expr::Block(b, _) => collect_decls_in_block(b, decls),
        _ => {}
    }
}
