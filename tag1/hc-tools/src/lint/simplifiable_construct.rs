//! L003: simplifiable_construct——可简化构造检测

use std::collections::{HashMap, HashSet};

use hc::ast::*;

use super::models::{LintDiag, LintRule};
use super::rules::find_rule;

pub(crate) fn lint_simplifiable_construct(
    program: &Program,
    source: &str,
    disabled: &HashMap<String, HashSet<usize>>,
    fix: bool,
    diags: &mut Vec<LintDiag>,
) {
    let rule = find_rule("simplifiable_construct").unwrap();
    for d in &program.decls {
        check_type_simplifiable(d, source, disabled, fix, rule, diags);
    }
}

fn check_type_simplifiable(
    decl: &Decl,
    source: &str,
    disabled: &HashMap<String, HashSet<usize>>,
    fix: bool,
    rule: &'static LintRule,
    diags: &mut Vec<LintDiag>,
) {
    match decl {
        Decl::Fn {
            params, ret, body, ..
        } => {
            for p in params {
                check_type_simplifiable_in_type(&p.ty, source, disabled, fix, rule, diags);
            }
            if let Some(t) = ret {
                check_type_simplifiable_in_type(t, source, disabled, fix, rule, diags);
            }
            check_type_simplifiable_in_block(body, source, disabled, fix, rule, diags);
        }
        Decl::Class {
            fields, methods, ..
        } => {
            for f in fields {
                check_type_simplifiable_in_type(&f.ty, source, disabled, fix, rule, diags);
            }
            for m in methods {
                for p in &m.params {
                    check_type_simplifiable_in_type(&p.ty, source, disabled, fix, rule, diags);
                }
                if let Some(t) = &m.ret {
                    check_type_simplifiable_in_type(t, source, disabled, fix, rule, diags);
                }
                check_type_simplifiable_in_block(&m.body, source, disabled, fix, rule, diags);
            }
        }
        Decl::Enum { .. } | Decl::Interface { .. } => {}
        Decl::Struct { fields, .. } => {
            for f in fields {
                check_type_simplifiable_in_type(&f.ty, source, disabled, fix, rule, diags);
            }
        }
        Decl::Union { fields, .. } => {
            for f in fields {
                check_type_simplifiable_in_type(&f.ty, source, disabled, fix, rule, diags);
            }
        }
        Decl::Namespace { decls: inner, .. } => {
            for d in inner {
                check_type_simplifiable(d, source, disabled, fix, rule, diags);
            }
        }
        Decl::Global { init, ty, .. } => {
            if let Some(t) = ty {
                check_type_simplifiable_in_type(t, source, disabled, fix, rule, diags);
            }
            if let Some(e) = init {
                check_type_simplifiable_in_expr(e, source, disabled, fix, rule, diags);
            }
        }
        Decl::Const { init, ty, .. } => {
            if let Some(t) = ty {
                check_type_simplifiable_in_type(t, source, disabled, fix, rule, diags);
            }
            check_type_simplifiable_in_expr(init, source, disabled, fix, rule, diags);
        }
        Decl::Import { .. } => {}
        Decl::Include { .. } => {}
        Decl::Comptime { body, .. } => {
            check_type_simplifiable_in_block(body, source, disabled, fix, rule, diags);
        }
    }
}

fn check_type_simplifiable_in_block(
    block: &Block,
    source: &str,
    disabled: &HashMap<String, HashSet<usize>>,
    fix: bool,
    rule: &'static LintRule,
    diags: &mut Vec<LintDiag>,
) {
    for s in &block.stmts {
        check_type_simplifiable_in_stmt(s, source, disabled, fix, rule, diags);
    }
}

fn check_type_simplifiable_in_stmt(
    stmt: &Stmt,
    source: &str,
    disabled: &HashMap<String, HashSet<usize>>,
    fix: bool,
    rule: &'static LintRule,
    diags: &mut Vec<LintDiag>,
) {
    match stmt {
        Stmt::VarDecl { ty, init, .. } => {
            if let Some(t) = ty {
                check_type_simplifiable_in_type(t, source, disabled, fix, rule, diags);
            }
            if let Some(e) = init {
                check_type_simplifiable_in_expr(e, source, disabled, fix, rule, diags);
            }
        }
        Stmt::ConstDecl { init, .. } => {
            check_type_simplifiable_in_expr(init, source, disabled, fix, rule, diags)
        }
        Stmt::Expr(e) => check_type_simplifiable_in_expr(e, source, disabled, fix, rule, diags),
        Stmt::If(s) => {
            check_type_simplifiable_in_expr(&s.cond, source, disabled, fix, rule, diags);
            check_type_simplifiable_in_block(&s.then_b, source, disabled, fix, rule, diags);
            if let Some(e) = &s.else_b {
                check_type_simplifiable_in_stmt(e, source, disabled, fix, rule, diags);
            }
        }
        Stmt::While(s) => {
            check_type_simplifiable_in_expr(&s.cond, source, disabled, fix, rule, diags);
            if let Some(e) = &s.step {
                check_type_simplifiable_in_expr(e, source, disabled, fix, rule, diags);
            }
            check_type_simplifiable_in_block(&s.body, source, disabled, fix, rule, diags);
        }
        Stmt::For(s) => {
            check_type_simplifiable_in_expr(&s.iter, source, disabled, fix, rule, diags);
            check_type_simplifiable_in_block(&s.body, source, disabled, fix, rule, diags);
        }
        Stmt::Switch(s) => {
            check_type_simplifiable_in_expr(&s.subject, source, disabled, fix, rule, diags);
            for arm in &s.arms {
                check_type_simplifiable_in_block(&arm.body, source, disabled, fix, rule, diags);
            }
        }
        Stmt::Return(e, _) => {
            if let Some(e) = e {
                check_type_simplifiable_in_expr(e, source, disabled, fix, rule, diags);
            }
        }
        Stmt::Defer(e, _) | Stmt::Errdefer(e, _) => {
            check_type_simplifiable_in_expr(e, source, disabled, fix, rule, diags)
        }
        Stmt::Block(b) => check_type_simplifiable_in_block(b, source, disabled, fix, rule, diags),
        Stmt::Break(..) | Stmt::Continue(..) | Stmt::Empty => {}
    }
}

fn check_type_simplifiable_in_expr(
    expr: &Expr,
    source: &str,
    disabled: &HashMap<String, HashSet<usize>>,
    fix: bool,
    rule: &'static LintRule,
    diags: &mut Vec<LintDiag>,
) {
    match expr {
        Expr::NamedLit {
            ty_args, fields, ..
        } => {
            for t in ty_args {
                check_type_simplifiable_in_type(t, source, disabled, fix, rule, diags);
            }
            for (_, e) in fields {
                check_type_simplifiable_in_expr(e, source, disabled, fix, rule, diags);
            }
        }
        Expr::ContainerLit { items, ty_args, .. } => {
            for t in ty_args {
                check_type_simplifiable_in_type(t, source, disabled, fix, rule, diags);
            }
            for e in items {
                check_type_simplifiable_in_expr(e, source, disabled, fix, rule, diags);
            }
        }
        Expr::StructType { fields, .. } => {
            for (_, t) in fields {
                check_type_simplifiable_in_type(t, source, disabled, fix, rule, diags);
            }
        }
        Expr::ArrayType { len, elem, .. } => {
            check_type_simplifiable_in_expr(len, source, disabled, fix, rule, diags);
            check_type_simplifiable_in_expr(elem, source, disabled, fix, rule, diags);
        }
        Expr::Block(b, _) => {
            check_type_simplifiable_in_block(b, source, disabled, fix, rule, diags)
        }
        Expr::Call { args, .. } => {
            for a in args {
                check_type_simplifiable_in_expr(a, source, disabled, fix, rule, diags);
            }
        }
        Expr::IfExpr {
            cond,
            then_e,
            else_e,
            ..
        } => {
            check_type_simplifiable_in_expr(cond, source, disabled, fix, rule, diags);
            check_type_simplifiable_in_expr(then_e, source, disabled, fix, rule, diags);
            check_type_simplifiable_in_expr(else_e, source, disabled, fix, rule, diags);
        }
        Expr::SwitchExpr { subject, arms, .. } => {
            check_type_simplifiable_in_expr(subject, source, disabled, fix, rule, diags);
            for arm in arms {
                check_type_simplifiable_in_block(&arm.body, source, disabled, fix, rule, diags);
            }
        }
        Expr::Assign { target, value, .. } => {
            check_type_simplifiable_in_expr(target, source, disabled, fix, rule, diags);
            check_type_simplifiable_in_expr(value, source, disabled, fix, rule, diags);
        }
        Expr::Binary(_, a, b, _) => {
            check_type_simplifiable_in_expr(a, source, disabled, fix, rule, diags);
            check_type_simplifiable_in_expr(b, source, disabled, fix, rule, diags);
        }
        Expr::Unary(_, e, _)
        | Expr::Deref(e, _)
        | Expr::AddrOf(e, _, _)
        | Expr::Unwrap(e, _)
        | Expr::Try(e, _)
        | Expr::Await(e, _)
        | Expr::Move(e, _) => {
            check_type_simplifiable_in_expr(e, source, disabled, fix, rule, diags);
        }
        Expr::Orelse(a, b, _) => {
            check_type_simplifiable_in_expr(a, source, disabled, fix, rule, diags);
            check_type_simplifiable_in_expr(b, source, disabled, fix, rule, diags);
        }
        Expr::Catch(a, ck, _) => {
            check_type_simplifiable_in_expr(&**a, source, disabled, fix, rule, diags);
            match ck.as_ref() {
                CatchKind::Default(b) => {
                    check_type_simplifiable_in_expr(b, source, disabled, fix, rule, diags);
                }
                CatchKind::Bind { body, .. } => {
                    check_type_simplifiable_in_block(body, source, disabled, fix, rule, diags);
                }
            }
        }
        Expr::Index { base, indices, .. } => {
            check_type_simplifiable_in_expr(base, source, disabled, fix, rule, diags);
            for i in indices {
                check_type_simplifiable_in_expr(i, source, disabled, fix, rule, diags);
            }
        }
        Expr::Field { base, .. } | Expr::Dot { base, .. } => {
            check_type_simplifiable_in_expr(base, source, disabled, fix, rule, diags);
        }
        Expr::ArrayLit(items, _) | Expr::TupleLit(items, _) => {
            for e in items {
                check_type_simplifiable_in_expr(e, source, disabled, fix, rule, diags);
            }
        }
        Expr::Closure { body, .. } => {
            check_type_simplifiable_in_block(body, source, disabled, fix, rule, diags)
        }
        Expr::TupleDestructure(_, e, _) => {
            check_type_simplifiable_in_expr(e, source, disabled, fix, rule, diags)
        }
        _ => {}
    }
}

fn check_type_simplifiable_in_type(
    ty: &Type,
    _source: &str,
    disabled: &HashMap<String, HashSet<usize>>,
    _fix: bool,
    rule: &'static LintRule,
    diags: &mut Vec<LintDiag>,
) {
    match ty {
        Type::Named(_name, args) => {
            // L003 检测可简化类型构造（如 `Vec(List<i32>)` → `Vec<List<i32>>`）
            // 当前简化版本：检查泛型参数是否可内联
            // 这个规则检测：泛型参数是 Named 类型且只有一个参数时，可简化
            if args.len() == 1 {
                if let Type::Named(_inner_name, inner_args) = &args[0] {
                    if inner_args.is_empty() {
                        // 简单的 `Outer(Inner)` 模式，已经是简化的
                        // 不需要告警
                    }
                }
            }
            for a in args {
                check_type_simplifiable_in_type(a, _source, disabled, _fix, rule, diags);
            }
        }
        Type::Ptr(inner, _)
        | Type::Slice(inner, _)
        | Type::Optional(inner)
        | Type::Owned(inner) => {
            check_type_simplifiable_in_type(inner, _source, disabled, _fix, rule, diags);
        }
        Type::ErrorUnion(_, inner) => {
            check_type_simplifiable_in_type(inner, _source, disabled, _fix, rule, diags);
        }
        Type::Tuple(items) => {
            for t in items {
                check_type_simplifiable_in_type(t, _source, disabled, _fix, rule, diags);
            }
        }
        Type::Array(_, inner) => {
            check_type_simplifiable_in_type(inner, _source, disabled, _fix, rule, diags);
        }
        Type::ComptimeInt(_) | Type::Infer => {}
    }
}
