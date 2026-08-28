//! 引用收集器（用于判断变量/导入是否被使用）

use hc::ast::*;

/// 收集所有标识符引用（用于判断变量是否被使用）
pub(crate) fn collect_refs(program: &Program) -> Vec<String> {
    let mut refs = Vec::new();
    for d in &program.decls {
        collect_refs_in_decl(d, &mut refs);
    }
    refs
}

fn collect_refs_in_decl(decl: &Decl, refs: &mut Vec<String>) {
    match decl {
        Decl::Fn {
            params, body, ret, ..
        } => {
            for p in params {
                collect_refs_in_type(&p.ty, refs);
            }
            if let Some(t) = ret {
                collect_refs_in_type(t, refs);
            }
            collect_refs_in_block(body, refs);
        }
        Decl::Class {
            fields, methods, ..
        } => {
            for f in fields {
                collect_refs_in_type(&f.ty, refs);
            }
            for m in methods {
                for p in &m.params {
                    collect_refs_in_type(&p.ty, refs);
                }
                if let Some(t) = &m.ret {
                    collect_refs_in_type(t, refs);
                }
                collect_refs_in_block(&m.body, refs);
            }
        }
        Decl::Enum { .. } => {}
        Decl::Struct { fields, .. } => {
            for f in fields {
                collect_refs_in_type(&f.ty, refs);
            }
        }
        Decl::Union { fields, .. } => {
            for f in fields {
                collect_refs_in_type(&f.ty, refs);
            }
        }
        Decl::Interface { methods, .. } => {
            for m in methods {
                for p in &m.params {
                    collect_refs_in_type(&p.ty, refs);
                }
                if let Some(t) = &m.ret {
                    collect_refs_in_type(t, refs);
                }
            }
        }
        Decl::Namespace { decls: inner, .. } => {
            for d in inner {
                collect_refs_in_decl(d, refs);
            }
        }
        Decl::Global { init, ty, .. } => {
            if let Some(t) = ty {
                collect_refs_in_type(t, refs);
            }
            if let Some(e) = init {
                collect_refs_in_expr(e, refs);
            }
        }
        Decl::Const { init, ty, .. } => {
            if let Some(t) = ty {
                collect_refs_in_type(t, refs);
            }
            collect_refs_in_expr(init, refs);
        }
        Decl::Import { .. } => {}
        Decl::Include { .. } => {}
        Decl::Comptime { body, .. } => {
            collect_refs_in_block(body, refs);
        }
    }
}

fn collect_refs_in_block(block: &Block, refs: &mut Vec<String>) {
    for s in &block.stmts {
        collect_refs_in_stmt(s, refs);
    }
}

fn collect_refs_in_stmt(stmt: &Stmt, refs: &mut Vec<String>) {
    match stmt {
        Stmt::VarDecl { init, ty, .. } => {
            if let Some(t) = ty {
                collect_refs_in_type(t, refs);
            }
            if let Some(e) = init {
                collect_refs_in_expr(e, refs);
            }
        }
        Stmt::ConstDecl { init, .. } => collect_refs_in_expr(init, refs),
        Stmt::Expr(e) => collect_refs_in_expr(e, refs),
        Stmt::If(stmt) => {
            collect_refs_in_expr(&stmt.cond, refs);
            collect_refs_in_block(&stmt.then_b, refs);
            if let Some(e) = &stmt.else_b {
                collect_refs_in_stmt(e, refs);
            }
        }
        Stmt::While(stmt) => {
            collect_refs_in_expr(&stmt.cond, refs);
            if let Some(e) = &stmt.step {
                collect_refs_in_expr(e, refs);
            }
            collect_refs_in_block(&stmt.body, refs);
        }
        Stmt::For(stmt) => {
            collect_refs_in_expr(&stmt.iter, refs);
            collect_refs_in_block(&stmt.body, refs);
        }
        Stmt::Switch(stmt) => {
            collect_refs_in_expr(&stmt.subject, refs);
            for arm in &stmt.arms {
                collect_refs_in_block(&arm.body, refs);
            }
        }
        Stmt::Return(e, _) => {
            if let Some(e) = e {
                collect_refs_in_expr(e, refs);
            }
        }
        Stmt::Defer(e, _) | Stmt::Errdefer(e, _) => collect_refs_in_expr(e, refs),
        Stmt::Block(b) => collect_refs_in_block(b, refs),
        Stmt::Break(..) | Stmt::Continue(..) | Stmt::Empty => {}
    }
}

fn collect_refs_in_expr(expr: &Expr, refs: &mut Vec<String>) {
    match expr {
        Expr::Ident(name, _) => refs.push(name.clone()),
        Expr::IntLit { .. }
        | Expr::FloatLit { .. }
        | Expr::StrLit { .. }
        | Expr::CharLit(..)
        | Expr::BoolLit(..)
        | Expr::NullLit(..)
        | Expr::VoidLit(..) => {}
        Expr::ArrayLit(items, _) => {
            for e in items {
                collect_refs_in_expr(e, refs);
            }
        }
        Expr::TupleLit(items, _) => {
            for e in items {
                collect_refs_in_expr(e, refs);
            }
        }
        Expr::ContainerLit { items, ty_args, .. } => {
            for e in items {
                collect_refs_in_expr(e, refs);
            }
            for t in ty_args {
                collect_refs_in_type(t, refs);
            }
        }
        Expr::NamedLit {
            fields, ty_args, ..
        } => {
            for (_, e) in fields {
                collect_refs_in_expr(e, refs);
            }
            for t in ty_args {
                collect_refs_in_type(t, refs);
            }
        }
        Expr::StructType { fields, .. } => {
            for (_, t) in fields {
                collect_refs_in_type(t, refs);
            }
        }
        Expr::ArrayType { len, elem, .. } => {
            collect_refs_in_expr(len, refs);
            collect_refs_in_expr(elem, refs);
        }
        Expr::Dot { base, .. } => collect_refs_in_expr(base, refs),
        Expr::Field { base, .. } => collect_refs_in_expr(base, refs),
        Expr::Index { base, indices, .. } => {
            collect_refs_in_expr(base, refs);
            for i in indices {
                collect_refs_in_expr(i, refs);
            }
        }
        Expr::Deref(e, _)
        | Expr::AddrOf(e, _, _)
        | Expr::Unary(_, e, _)
        | Expr::Unwrap(e, _)
        | Expr::Try(e, _)
        | Expr::Await(e, _)
        | Expr::Move(e, _) => {
            collect_refs_in_expr(e, refs);
        }
        Expr::Binary(_, a, b, _) => {
            collect_refs_in_expr(a, refs);
            collect_refs_in_expr(b, refs);
        }
        Expr::Orelse(a, b, _) => {
            collect_refs_in_expr(a, refs);
            collect_refs_in_expr(b, refs);
        }
        Expr::Catch(e, ck, _) => {
            collect_refs_in_expr(e, refs);
            match ck.as_ref() {
                CatchKind::Default(e) => collect_refs_in_expr(e, refs),
                CatchKind::Bind { name: _, body } => collect_refs_in_block(body, refs),
            }
        }
        Expr::Call { callee, args, .. } => {
            collect_refs_in_expr(callee, refs);
            for a in args {
                collect_refs_in_expr(a, refs);
            }
        }
        Expr::IfExpr {
            cond,
            then_e,
            else_e,
            ..
        } => {
            collect_refs_in_expr(cond, refs);
            collect_refs_in_expr(then_e, refs);
            collect_refs_in_expr(else_e, refs);
        }
        Expr::SwitchExpr { subject, arms, .. } => {
            collect_refs_in_expr(subject, refs);
            for arm in arms {
                collect_refs_in_block(&arm.body, refs);
            }
        }
        Expr::Block(b, _) => collect_refs_in_block(b, refs),
        Expr::Assign { target, value, .. } => {
            collect_refs_in_expr(target, refs);
            collect_refs_in_expr(value, refs);
        }
        Expr::ErrorLit(..) | Expr::FnRef(..) => {}
        Expr::TupleDestructure(_names, e, _) => {
            // names 是声明，不产生引用
            collect_refs_in_expr(e, refs);
        }
        Expr::Closure {
            params: _, body, ..
        } => {
            // params 是声明，不产生引用；body 内引用被收集
            collect_refs_in_block(body, refs);
        }
    }
}

fn collect_refs_in_type(ty: &Type, refs: &mut Vec<String>) {
    match ty {
        Type::Named(name, args) => {
            refs.push(name.clone());
            for a in args {
                collect_refs_in_type(a, refs);
            }
        }
        Type::Ptr(inner, _)
        | Type::Slice(inner, _)
        | Type::Optional(inner)
        | Type::Owned(inner) => {
            collect_refs_in_type(inner, refs);
        }
        Type::ErrorUnion(_, inner) => {
            collect_refs_in_type(inner, refs);
        }
        Type::Tuple(items) => {
            for t in items {
                collect_refs_in_type(t, refs);
            }
        }
        Type::Array(_, inner) => collect_refs_in_type(inner, refs),
        Type::ComptimeInt(_) | Type::Infer => {}
    }
}
