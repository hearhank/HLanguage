//! 错误集推断：!T 返回类型的错误集自动收集
//!
//! 定义：枚举：FnErrorForm
//! 定义：结构体：InferredErrorSets, BodyErrors

use crate::ast::*;
use std::collections::{HashMap, HashSet};

/// !T 推断错误集收集结果（Q-S8）
pub struct InferredErrorSets {
    /// 函数名（含命名空间 / `Type.method` 前缀）→ 推断错误集成员。
    /// 仅覆盖可完整收集的 `!T` 函数；显式 `E!T` 与 `anyerror!T` 不在此列。
    pub sets: HashMap<String, HashSet<String>>,
    /// 无法完整收集（递归自调用）的 `!T` 函数名 → Q-S8 退化为 anyerror（提示显式标注）。
    pub incomplete: Vec<String>,
}

/// 返回类型的错误联合形态（`fn`/方法）
enum FnErrorForm {
    /// E!T：显式命名错误集（const 别名）
    Explicit(String),
    /// !T：推断错误集（从函数体收集）
    Infer,
    /// anyerror!T：接口契约，不静态约束
    Anyerror,
    /// 非错误联合
    None,
}

fn fn_error_form(ret: &Option<Type>) -> FnErrorForm {
    match ret {
        Some(Type::ErrorUnion(Some(err), _)) => match err.strip() {
            Type::Named(n, _) if n == "anyerror" => FnErrorForm::Anyerror,
            Type::Named(n, _) => FnErrorForm::Explicit(n.clone()),
            _ => FnErrorForm::Anyerror,
        },
        Some(Type::ErrorUnion(None, _)) => FnErrorForm::Infer,
        _ => FnErrorForm::None,
    }
}

/// 单个函数体的错误收集：direct = `return error.X`；propagates = `try g()` / `return g()`
struct BodyErrors {
    direct: HashSet<String>,
    propagates: Vec<String>,
}

/// 解析被调名（`g` / `ns.g`）——方法调用（`obj.m()`）无法静态判定类型，跳过
fn callee_name(e: &Expr) -> Option<String> {
    match e {
        Expr::Ident(name, _) => Some(name.clone()),
        Expr::Dot { base, field, .. } => match base.as_ref() {
            Expr::Ident(ns, _) => Some(format!("{ns}.{field}")),
            _ => None,
        },
        _ => None,
    }
}

fn collect_body_errors(b: &Block, out: &mut BodyErrors) {
    for s in &b.stmts {
        collect_stmt_errors(s, out);
    }
}

fn collect_stmt_errors(s: &Stmt, out: &mut BodyErrors) {
    match s {
        Stmt::VarDecl { init, .. } => {
            if let Some(e) = init {
                collect_expr_errors(e, out);
            }
        }
        Stmt::ConstDecl { init, .. } => collect_expr_errors(init, out),
        Stmt::Expr(e) => collect_expr_errors(e, out),
        Stmt::If(ifs) => {
            collect_expr_errors(&ifs.cond, out);
            collect_body_errors(&ifs.then_b, out);
            if let Some(eb) = &ifs.else_b {
                collect_stmt_errors(eb, out);
            }
        }
        Stmt::While(w) => {
            collect_expr_errors(&w.cond, out);
            if let Some(step) = &w.step {
                collect_expr_errors(step, out);
            }
            collect_body_errors(&w.body, out);
        }
        Stmt::For(f) => {
            collect_expr_errors(&f.iter, out);
            collect_body_errors(&f.body, out);
        }
        Stmt::Switch(sw) => {
            collect_expr_errors(&sw.subject, out);
            for arm in &sw.arms {
                collect_body_errors(&arm.body, out);
            }
        }
        Stmt::Return(e, _) => {
            if let Some(e) = e {
                // return g()：g 返回错误联合 → 其错误集随联合直接传递
                if let Expr::Call { callee, .. } = e {
                    if let Some(n) = callee_name(callee) {
                        out.propagates.push(n);
                    }
                }
                collect_expr_errors(e, out);
            }
        }
        Stmt::Defer(e, _) | Stmt::Errdefer(e, _) => collect_expr_errors(e, out),
        Stmt::Block(b) => collect_body_errors(b, out),
        _ => {}
    }
}

fn collect_expr_errors(e: &Expr, out: &mut BodyErrors) {
    match e {
        Expr::ErrorLit(name, _) => {
            out.direct.insert(name.clone());
        }
        Expr::Try(inner, _) => {
            // try g()：g 的错误集传播到当前函数
            if let Expr::Call { callee, .. } = inner.as_ref() {
                if let Some(n) = callee_name(callee) {
                    out.propagates.push(n);
                }
            }
            collect_expr_errors(inner, out);
        }
        Expr::Await(inner, _) => {
            collect_expr_errors(inner, out);
        }
        Expr::ArrayLit(items, _) | Expr::TupleLit(items, _) => {
            for it in items {
                collect_expr_errors(it, out);
            }
        }
        Expr::NamedLit { fields, .. } => {
            for (_, v) in fields {
                collect_expr_errors(v, out);
            }
        }
        Expr::Dot { base, .. } | Expr::Field { base, .. } | Expr::Deref(base, _) => {
            collect_expr_errors(base, out);
        }
        Expr::Index { base, indices, .. } => {
            collect_expr_errors(base, out);
            for i in indices {
                collect_expr_errors(i, out);
            }
        }
        Expr::AddrOf(inner, _, _)
        | Expr::Unary(_, inner, _)
        | Expr::Unwrap(inner, _)
        | Expr::Orelse(inner, _, _)
        | Expr::Move(inner, _) => {
            collect_expr_errors(inner, out);
        }
        Expr::Binary(_, l, r, _) => {
            collect_expr_errors(l, out);
            collect_expr_errors(r, out);
        }
        Expr::Catch(inner, kind, _) => {
            collect_expr_errors(inner, out);
            match kind.as_ref() {
                CatchKind::Default(d) => collect_expr_errors(d, out),
                CatchKind::Bind { body, .. } => collect_body_errors(body, out),
            }
        }
        Expr::Call { callee, args, .. } => {
            collect_expr_errors(callee, out);
            for a in args {
                collect_expr_errors(a, out);
            }
        }
        Expr::IfExpr {
            cond,
            then_e,
            else_e,
            ..
        } => {
            collect_expr_errors(cond, out);
            collect_expr_errors(then_e, out);
            collect_expr_errors(else_e, out);
        }
        Expr::SwitchExpr { subject, arms, .. } => {
            collect_expr_errors(subject, out);
            for arm in arms {
                collect_body_errors(&arm.body, out);
            }
        }
        Expr::Block(b, _) => collect_body_errors(b, out),
        Expr::Assign { target, value, .. } => {
            collect_expr_errors(target, out);
            collect_expr_errors(value, out);
        }
        Expr::TupleDestructure(_, value, _) => collect_expr_errors(value, out),
        Expr::Closure { body, .. } => collect_body_errors(body, out),
        _ => {}
    }
}

/// 登记全部函数/方法（命名空间前缀展平）与显式错误集 const 别名
fn collect_fn_table<'a>(
    decls: &'a [Decl],
    prefix: &str,
    form: &mut HashMap<String, FnErrorForm>,
    bodies: &mut HashMap<String, &'a Block>,
    explicit: &mut HashMap<String, HashSet<String>>,
) {
    for d in decls {
        match d {
            Decl::Fn {
                name, ret, body, ..
            } => {
                let key = format!("{prefix}{name}");
                form.insert(key.clone(), fn_error_form(ret));
                bodies.insert(key, body);
            }
            Decl::Class { name, methods, .. } => {
                for m in methods {
                    let key = format!("{prefix}{name}.{}", m.name);
                    form.insert(key.clone(), fn_error_form(&m.ret));
                    bodies.insert(key, &m.body);
                }
            }
            Decl::Const { name, ty, .. } => {
                if let Some(Type::Named(tn, _)) = ty {
                    if let Some(rest) = tn.strip_prefix("error_set:") {
                        let members: HashSet<String> =
                            rest.split(',').map(|s| s.trim().to_string()).collect();
                        explicit.insert(format!("{prefix}{name}"), members);
                    }
                }
            }
            Decl::Namespace { name, decls, .. } => {
                collect_fn_table(decls, &format!("{prefix}{name}."), form, bodies, explicit);
            }
            _ => {}
        }
    }
}

/// 递归可达检测：start 经 propagate 边可达自身
fn reaches_self(start: &str, edges: &HashMap<String, Vec<String>>) -> bool {
    fn dfs(
        cur: &str,
        start: &str,
        edges: &HashMap<String, Vec<String>>,
        visited: &mut HashSet<String>,
    ) -> bool {
        if let Some(nexts) = edges.get(cur) {
            for next in nexts {
                if next == start {
                    return true;
                }
                if visited.insert(next.clone()) && dfs(next, start, edges, visited) {
                    return true;
                }
            }
        }
        false
    }
    let mut visited = HashSet::new();
    visited.insert(start.to_string());
    dfs(start, start, edges, &mut visited)
}

/// Q-S8：`!T` 推断错误集——从函数体收集 `return error.X` + `try`/`return` 传播的
/// 实际返回集（固定点闭包）。递归自调用无法收集 → 退化为 anyerror（incomplete）。
pub fn infer_error_sets(program: &Program) -> InferredErrorSets {
    let mut form: HashMap<String, FnErrorForm> = HashMap::new();
    let mut bodies: HashMap<String, &Block> = HashMap::new();
    let mut explicit: HashMap<String, HashSet<String>> = HashMap::new();
    collect_fn_table(&program.decls, "", &mut form, &mut bodies, &mut explicit);

    // 每个函数体：直接返回的错误 + 传播的被调名
    let mut direct: HashMap<String, HashSet<String>> = HashMap::new();
    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    for (name, body) in &bodies {
        let mut be = BodyErrors {
            direct: HashSet::new(),
            propagates: Vec::new(),
        };
        collect_body_errors(body, &mut be);
        direct.insert(name.clone(), be.direct);
        edges.insert(name.clone(), be.propagates);
    }

    // 固定点：!T 函数集合 = 直接错误 ∪ 被调已知集（显式 const / 其它 !T）
    let mut sets: HashMap<String, HashSet<String>> = HashMap::new();
    for (name, f) in &form {
        if matches!(f, FnErrorForm::Infer) {
            sets.insert(name.clone(), direct.get(name).cloned().unwrap_or_default());
        }
    }
    for _ in 0..64 {
        let mut changed = false;
        let names: Vec<String> = sets.keys().cloned().collect();
        for f in names {
            let mut new: HashSet<String> = direct.get(&f).cloned().unwrap_or_default();
            if let Some(callees) = edges.get(&f) {
                for g in callees {
                    match form.get(g) {
                        Some(FnErrorForm::Explicit(name)) => {
                            if let Some(m) = explicit.get(name) {
                                new.extend(m.iter().cloned());
                            }
                        }
                        Some(FnErrorForm::Infer) => {
                            if let Some(s) = sets.get(g) {
                                new.extend(s.iter().cloned());
                            }
                        }
                        Some(FnErrorForm::Anyerror) | Some(FnErrorForm::None) | None => {
                            // 接口契约 / 非错误联合 / 内建·外部未知被调：不传播（保守 best-effort）
                        }
                    }
                }
            }
            if new != sets[&f] {
                sets.insert(f, new);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // 递归自调用 → 无法完整收集 → 退化为 anyerror（Q-S8）
    let mut incomplete: Vec<String> = Vec::new();
    for (name, f) in &form {
        if matches!(f, FnErrorForm::Infer) && reaches_self(name, &edges) {
            incomplete.push(name.clone());
            sets.remove(name);
        }
    }
    incomplete.sort();

    InferredErrorSets { sets, incomplete }
}
