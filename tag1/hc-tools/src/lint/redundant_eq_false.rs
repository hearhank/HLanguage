//! L006: redundant_eq_false——多余的 `== false` 检测

use std::collections::{HashMap, HashSet};

use hc::ast::*;

use super::disable_comments::is_disabled;
use super::models::{LintDiag, LintRule};
use super::rules::find_rule;

pub(crate) fn lint_redundant_eq_false(
    program: &Program,
    source: &str,
    disabled: &HashMap<String, HashSet<usize>>,
    fix: bool,
    diags: &mut Vec<LintDiag>,
) {
    let rule = find_rule("redundant_eq_false").unwrap();
    for d in &program.decls {
        check_redundant_eq_false_in_decl(d, source, disabled, fix, rule, diags);
    }
}

fn check_redundant_eq_false_in_decl(
    decl: &Decl,
    source: &str,
    disabled: &HashMap<String, HashSet<usize>>,
    fix: bool,
    rule: &'static LintRule,
    diags: &mut Vec<LintDiag>,
) {
    match decl {
        Decl::Fn { body, .. } => {
            check_redundant_eq_false_in_block(body, source, disabled, fix, rule, diags)
        }
        Decl::Class { methods, .. } => {
            for m in methods {
                check_redundant_eq_false_in_block(&m.body, source, disabled, fix, rule, diags);
            }
        }
        Decl::Namespace { decls: inner, .. } => {
            for d in inner {
                check_redundant_eq_false_in_decl(d, source, disabled, fix, rule, diags);
            }
        }
        Decl::Comptime { body, .. } => {
            check_redundant_eq_false_in_block(body, source, disabled, fix, rule, diags);
        }
        _ => {}
    }
}

fn check_redundant_eq_false_in_block(
    block: &Block,
    source: &str,
    disabled: &HashMap<String, HashSet<usize>>,
    fix: bool,
    rule: &'static LintRule,
    diags: &mut Vec<LintDiag>,
) {
    for s in &block.stmts {
        check_redundant_eq_false_in_stmt(s, source, disabled, fix, rule, diags);
    }
}

fn check_redundant_eq_false_in_stmt(
    stmt: &Stmt,
    source: &str,
    disabled: &HashMap<String, HashSet<usize>>,
    fix: bool,
    rule: &'static LintRule,
    diags: &mut Vec<LintDiag>,
) {
    match stmt {
        Stmt::Expr(e) => check_redundant_eq_false_in_expr(e, source, disabled, fix, rule, diags),
        Stmt::If(s) => {
            check_redundant_eq_false_in_expr(&s.cond, source, disabled, fix, rule, diags);
            check_redundant_eq_false_in_block(&s.then_b, source, disabled, fix, rule, diags);
            if let Some(e) = &s.else_b {
                check_redundant_eq_false_in_stmt(e, source, disabled, fix, rule, diags);
            }
        }
        Stmt::While(s) => {
            check_redundant_eq_false_in_expr(&s.cond, source, disabled, fix, rule, diags);
            if let Some(e) = &s.step {
                check_redundant_eq_false_in_expr(e, source, disabled, fix, rule, diags);
            }
            check_redundant_eq_false_in_block(&s.body, source, disabled, fix, rule, diags);
        }
        Stmt::For(s) => {
            check_redundant_eq_false_in_expr(&s.iter, source, disabled, fix, rule, diags);
            check_redundant_eq_false_in_block(&s.body, source, disabled, fix, rule, diags);
        }
        Stmt::Switch(s) => {
            check_redundant_eq_false_in_expr(&s.subject, source, disabled, fix, rule, diags);
            for arm in &s.arms {
                check_redundant_eq_false_in_block(&arm.body, source, disabled, fix, rule, diags);
            }
        }
        Stmt::Return(e, _) => {
            if let Some(e) = e {
                check_redundant_eq_false_in_expr(e, source, disabled, fix, rule, diags);
            }
        }
        Stmt::Defer(e, _) | Stmt::Errdefer(e, _) => {
            check_redundant_eq_false_in_expr(e, source, disabled, fix, rule, diags)
        }
        Stmt::VarDecl { init, .. } => {
            if let Some(e) = init {
                check_redundant_eq_false_in_expr(e, source, disabled, fix, rule, diags);
            }
        }
        Stmt::ConstDecl { init, .. } => {
            check_redundant_eq_false_in_expr(init, source, disabled, fix, rule, diags)
        }
        Stmt::Block(b) => check_redundant_eq_false_in_block(b, source, disabled, fix, rule, diags),
        _ => {}
    }
}

fn check_redundant_eq_false_in_expr(
    expr: &Expr,
    source: &str,
    disabled: &HashMap<String, HashSet<usize>>,
    fix: bool,
    rule: &'static LintRule,
    diags: &mut Vec<LintDiag>,
) {
    match expr {
        Expr::Binary(BinOp::Eq, a, b, span) | Expr::Binary(BinOp::Ne, a, b, span) => {
            let op = match expr {
                Expr::Binary(BinOp::Eq, _, _, _) => "==",
                Expr::Binary(BinOp::Ne, _, _, _) => "!=",
                _ => unreachable!(),
            };
            // 检测 `x == false` → `!x`, `x == true` → `x`
            // 检测 `x != false` → `x`, `x != true` → `!x`
            let check_bool_lit = |e: &Expr| -> Option<bool> {
                match e {
                    Expr::BoolLit(v, _) => Some(*v),
                    _ => None,
                }
            };
            let (left_is_bool, right_is_bool) = (check_bool_lit(a), check_bool_lit(b));
            let message = match (op, left_is_bool, right_is_bool) {
                ("==", Some(false), _) | ("==", _, Some(false)) => {
                    Some("`== false` 可简化为 `!`：用 `!x` 替代".to_string())
                }
                ("==", Some(true), _) | ("==", _, Some(true)) => {
                    Some("`== true` 可简化：直接使用表达式".to_string())
                }
                ("!=", Some(false), _) | ("!=", _, Some(false)) => {
                    Some("`!= false` 可简化：直接使用表达式".to_string())
                }
                ("!=", Some(true), _) | ("!=", _, Some(true)) => {
                    Some("`!= true` 可简化为 `!`：用 `!x` 替代".to_string())
                }
                _ => None,
            };
            if let Some(msg) = message {
                if !is_disabled(disabled, "redundant_eq_false", span.line as usize) {
                    diags.push(LintDiag {
                        rule,
                        span: span.clone(),
                        message: msg,
                        fix: if fix {
                            Some("/* simplify */".to_string())
                        } else {
                            None
                        },
                    });
                }
            }
            check_redundant_eq_false_in_expr(a, source, disabled, fix, rule, diags);
            check_redundant_eq_false_in_expr(b, source, disabled, fix, rule, diags);
        }
        Expr::Block(b, _) => {
            check_redundant_eq_false_in_block(b, source, disabled, fix, rule, diags)
        }
        Expr::Call { args, .. } => {
            for a in args {
                check_redundant_eq_false_in_expr(a, source, disabled, fix, rule, diags);
            }
        }
        Expr::IfExpr {
            cond,
            then_e,
            else_e,
            ..
        } => {
            check_redundant_eq_false_in_expr(cond, source, disabled, fix, rule, diags);
            check_redundant_eq_false_in_expr(then_e, source, disabled, fix, rule, diags);
            check_redundant_eq_false_in_expr(else_e, source, disabled, fix, rule, diags);
        }
        Expr::Binary(_, a, b, _) => {
            check_redundant_eq_false_in_expr(a, source, disabled, fix, rule, diags);
            check_redundant_eq_false_in_expr(b, source, disabled, fix, rule, diags);
        }
        Expr::Unary(_, e, _)
        | Expr::Deref(e, _)
        | Expr::AddrOf(e, _, _)
        | Expr::Unwrap(e, _)
        | Expr::Try(e, _)
        | Expr::Await(e, _)
        | Expr::Move(e, _) => {
            check_redundant_eq_false_in_expr(e, source, disabled, fix, rule, diags);
        }
        Expr::Orelse(a, b, _) => {
            check_redundant_eq_false_in_expr(a, source, disabled, fix, rule, diags);
            check_redundant_eq_false_in_expr(b, source, disabled, fix, rule, diags);
        }
        Expr::Catch(e, ck, _) => {
            check_redundant_eq_false_in_expr(e, source, disabled, fix, rule, diags);
            match ck.as_ref() {
                CatchKind::Default(e) => {
                    check_redundant_eq_false_in_expr(e, source, disabled, fix, rule, diags)
                }
                CatchKind::Bind { body, .. } => {
                    check_redundant_eq_false_in_block(body, source, disabled, fix, rule, diags)
                }
            }
        }
        Expr::SwitchExpr { subject, arms, .. } => {
            check_redundant_eq_false_in_expr(subject, source, disabled, fix, rule, diags);
            for arm in arms {
                check_redundant_eq_false_in_block(&arm.body, source, disabled, fix, rule, diags);
            }
        }
        Expr::Assign { target, value, .. } => {
            check_redundant_eq_false_in_expr(target, source, disabled, fix, rule, diags);
            check_redundant_eq_false_in_expr(value, source, disabled, fix, rule, diags);
        }
        Expr::Index { base, indices, .. } => {
            check_redundant_eq_false_in_expr(base, source, disabled, fix, rule, diags);
            for i in indices {
                check_redundant_eq_false_in_expr(i, source, disabled, fix, rule, diags);
            }
        }
        Expr::Field { base, .. } | Expr::Dot { base, .. } => {
            check_redundant_eq_false_in_expr(base, source, disabled, fix, rule, diags);
        }
        Expr::ArrayLit(items, _) | Expr::TupleLit(items, _) => {
            for e in items {
                check_redundant_eq_false_in_expr(e, source, disabled, fix, rule, diags);
            }
        }
        Expr::Closure { body, .. } => {
            check_redundant_eq_false_in_block(body, source, disabled, fix, rule, diags)
        }
        Expr::TupleDestructure(_, e, _) => {
            check_redundant_eq_false_in_expr(e, source, disabled, fix, rule, diags)
        }
        Expr::NamedLit { fields, .. } => {
            for (_, e) in fields {
                check_redundant_eq_false_in_expr(e, source, disabled, fix, rule, diags);
            }
        }
        Expr::ContainerLit { items, .. } => {
            for e in items {
                check_redundant_eq_false_in_expr(e, source, disabled, fix, rule, diags);
            }
        }
        _ => {}
    }
}
