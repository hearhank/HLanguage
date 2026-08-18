//! K2：AST 转储——Rust 参考实现，H 版 parser（stage1/parser.hc）的对照基准。
//!
//! `hc parse <file.hc>` 输出：每 AST 节点一行 `{depth} {Tag} {payload} {start} {end}`，
//! **后序**（子节点先于父节点——H 侧解析时子节点 span 先齐，父节点行后发，两侧同序）。
//!
//! 约定：
//! - `depth` = 树深（声明 = 0，其子节点 +1，逐层递推；Program 不占行）。
//! - 类型节点无 span（`Type` 无 span 字段），行尾无 `{start} {end}`。
//! - 字符串值经 `{:?}` 转义（H 侧 dbg_escape 对齐，K1 已验证）。
//! - 可选字段：无 → `_`；标志位 `{0|1}`。
//!
//! 已知近似：`Decl::Script.close_end` 一并转储（`close=`），验证 H 侧同计算；
//! 其余字段全量转储。

use hc::ast::*;

/// 转储整个 Program；返回行列表（不含尾部换行的字符串）。
pub fn dump_program(program: &Program) -> Vec<String> {
    let mut out = Vec::new();
    for d in &program.decls {
        dump_decl(&mut out, 0, d);
    }
    out
}

fn emit(out: &mut Vec<String>, depth: usize, line: String) {
    out.push(format!("{depth} {line}"));
}

fn sp(s: &hc::token::Span) -> String {
    format!("{} {}", s.start, s.end)
}

fn q(s: &str) -> String {
    format!("{:?}", s)
}

fn cap(c: &Option<(CaptureMode, String)>) -> String {
    match c {
        Some((m, n)) => format!("cap={}:{}", cap_mode(m), n),
        None => String::new(),
    }
}

fn cap_mode(m: &CaptureMode) -> &'static str {
    match m {
        CaptureMode::Read => "Read",
        CaptureMode::Mut => "Mut",
        CaptureMode::Move => "Move",
    }
}

fn opt_str(s: &Option<String>) -> String {
    match s {
        Some(v) => q(v),
        None => "_".to_string(),
    }
}

// ---------- 声明 ----------

fn dump_decl(out: &mut Vec<String>, depth: usize, d: &Decl) {
    match d {
        Decl::Global {
            name,
            ty,
            init,
            pub_,
            span,
        } => {
            if let Some(t) = ty {
                dump_type(out, depth + 1, t);
            }
            if let Some(e) = init {
                dump_expr(out, depth + 1, e);
            }
            emit(
                out,
                depth,
                format!("Global {name} pub={} {}", *pub_ as u8, sp(span)),
            );
        }
        Decl::Const {
            name,
            ty,
            init,
            pub_,
            span,
        } => {
            if let Some(t) = ty {
                dump_type(out, depth + 1, t);
            }
            dump_expr(out, depth + 1, init);
            emit(
                out,
                depth,
                format!("Const {name} pub={} {}", *pub_ as u8, sp(span)),
            );
        }
        Decl::Fn {
            name,
            params,
            ret,
            where_clause,
            body,
            span,
            is_test,
            test_name,
            pub_,
            is_async,
            exported,
        } => {
            for p in params {
                dump_param(out, depth + 1, p);
            }
            if let Some(r) = ret {
                dump_type(out, depth + 1, r);
            }
            for (tn, iface) in where_clause {
                dump_type(out, depth + 1, iface);
                emit(out, depth + 1, format!("Where {tn}"));
            }
            dump_block(out, depth + 1, body);
            emit(
                out,
                depth,
                format!(
                    "Fn {name} pub={} test={} tname={} async={} export={} {}",
                    *pub_ as u8,
                    *is_test as u8,
                    opt_str(test_name),
                    *is_async as u8,
                    *exported as u8,
                    sp(span)
                ),
            );
        }
        Decl::Class {
            name,
            ifaces,
            traits,
            fields,
            methods,
            pub_,
            span,
        } => {
            for i in ifaces {
                dump_type(out, depth + 1, i);
            }
            for t in traits {
                emit(out, depth + 1, format!("Trait {t:?}"));
            }
            for f in fields {
                dump_field(out, depth + 1, f);
            }
            for m in methods {
                dump_method(out, depth + 1, m);
            }
            emit(
                out,
                depth,
                format!("Class {name} pub={} {}", *pub_ as u8, sp(span)),
            );
        }
        Decl::Enum {
            name,
            variants,
            pub_,
            span,
        } => {
            for v in variants {
                dump_enum_variant(out, depth + 1, v);
            }
            emit(
                out,
                depth,
                format!("Enum {name} pub={} {}", *pub_ as u8, sp(span)),
            );
        }
        Decl::Union {
            name,
            fields,
            pub_,
            span,
        } => {
            for f in fields {
                dump_field(out, depth + 1, f);
            }
            emit(
                out,
                depth,
                format!("Union {name} pub={} {}", *pub_ as u8, sp(span)),
            );
        }
        Decl::Interface {
            name,
            supers,
            methods,
            pub_,
            span,
        } => {
            for s in supers {
                dump_type(out, depth + 1, s);
            }
            for m in methods {
                dump_method(out, depth + 1, m);
            }
            emit(
                out,
                depth,
                format!("Interface {name} pub={} {}", *pub_ as u8, sp(span)),
            );
        }
        Decl::Namespace {
            name,
            decls,
            pub_,
            is_module,
            span,
        } => {
            for d2 in decls {
                dump_decl(out, depth + 1, d2);
            }
            emit(
                out,
                depth,
                format!(
                    "Namespace {name} pub={} module={} {}",
                    *pub_ as u8,
                    *is_module as u8,
                    sp(span)
                ),
            );
        }
        Decl::Using { path, alias, span } => {
            let mut p = String::new();
            for (i, seg) in path.iter().enumerate() {
                if i > 0 {
                    p.push('.');
                }
                p.push_str(seg);
            }
            emit(
                out,
                depth,
                format!("Using path={p} alias={} {}", opt_str(alias), sp(span)),
            );
        }
        Decl::Import {
            path,
            alias,
            select,
            span,
        } => {
            let mut p = String::new();
            for (i, seg) in path.iter().enumerate() {
                if i > 0 {
                    p.push('.');
                }
                p.push_str(seg);
            }
            for (name, a) in select.iter().flatten() {
                emit(
                    out,
                    depth + 1,
                    format!("Sel {name} alias={}", opt_str(a)),
                );
            }
            emit(
                out,
                depth,
                format!(
                    "Import path={p} alias={} select={} {}",
                    opt_str(alias),
                    select.is_some() as u8,
                    sp(span)
                ),
            );
        }
        Decl::Script { body, close_end, span } => {
            dump_block(out, depth + 1, body);
            emit(
                out,
                depth,
                format!("Script close={close_end} {}", sp(span)),
            );
        }
        Decl::Comptime { body, span } => {
            dump_block(out, depth + 1, body);
            emit(out, depth, format!("Comptime {}", sp(span)));
        }
    }
}

// ---------- 类型（无 span） ----------

fn dump_type(out: &mut Vec<String>, depth: usize, t: &Type) {
    match t {
        Type::Named(n, args) => {
            for a in args {
                dump_type(out, depth + 1, a);
            }
            emit(out, depth, format!("Named {n} {}", args.len()));
        }
        Type::Ptr(inner, mut_) => {
            dump_type(out, depth + 1, inner);
            emit(out, depth, format!("Ptr mut={}", *mut_ as u8));
        }
        Type::Slice(inner, mut_) => {
            dump_type(out, depth + 1, inner);
            emit(out, depth, format!("Slice mut={}", *mut_ as u8));
        }
        Type::Optional(inner) => {
            dump_type(out, depth + 1, inner);
            emit(out, depth, "Optional".to_string());
        }
        Type::ErrorUnion(err, payload) => {
            match err {
                Some(e) => dump_type(out, depth + 1, e),
                None => {}
            }
            dump_type(out, depth + 1, payload);
            emit(
                out,
                depth,
                format!("ErrorUnion err={}", err.is_some() as u8),
            );
        }
        Type::Tuple(items) => {
            for it in items {
                dump_type(out, depth + 1, it);
            }
            emit(out, depth, format!("Tuple {}", items.len()));
        }
        Type::Array(n, inner) => {
            dump_type(out, depth + 1, inner);
            emit(out, depth, format!("Array {n}"));
        }
        Type::ComptimeInt(n) => emit(out, depth, format!("ComptimeInt {n}")),
        Type::Infer => emit(out, depth, "Infer".to_string()),
        Type::Owned(inner) => {
            dump_type(out, depth + 1, inner);
            emit(out, depth, "Owned".to_string());
        }
    }
}

// ---------- 语句 ----------

fn dump_block(out: &mut Vec<String>, depth: usize, b: &Block) {
    for s in &b.stmts {
        dump_stmt(out, depth + 1, s);
    }
    emit(out, depth, format!("Block {}", sp(&b.span)));
}

fn dump_stmt(out: &mut Vec<String>, depth: usize, s: &Stmt) {
    match s {
        Stmt::VarDecl {
            name,
            mut_,
            ty,
            init,
            span,
        } => {
            if let Some(t) = ty {
                dump_type(out, depth + 1, t);
            }
            if let Some(e) = init {
                dump_expr(out, depth + 1, e);
            }
            emit(
                out,
                depth,
                format!("VarDecl {name} mut={} {}", *mut_ as u8, sp(span)),
            );
        }
        Stmt::ConstDecl { name, init, span } => {
            dump_expr(out, depth + 1, init);
            emit(out, depth, format!("ConstDecl {name} {}", sp(span)));
        }
        Stmt::Expr(e) => {
            dump_expr(out, depth + 1, e);
            emit(out, depth, format!("ExprStmt {}", sp(&e.span())));
        }
        Stmt::If(IfStmt {
            cond,
            capture,
            err_capture,
            then_b,
            else_b,
            span,
        }) => {
            dump_expr(out, depth + 1, cond);
            dump_block(out, depth + 1, then_b);
            if let Some(e) = else_b {
                dump_stmt(out, depth + 1, e);
            }
            let mut line = format!("If {}", sp(span));
            let c = cap(capture);
            if !c.is_empty() {
                line.push(' ');
                line.push_str(&c);
            }
            let e = cap(err_capture);
            if !e.is_empty() {
                line.push_str(" ecap=");
                line.push_str(&e.trim_start_matches("cap="));
            }
            emit(out, depth, line);
        }
        Stmt::While(WhileStmt {
            label,
            cond,
            capture,
            step,
            body,
            span,
        }) => {
            dump_expr(out, depth + 1, cond);
            if let Some(s2) = step {
                dump_expr(out, depth + 1, s2);
            }
            dump_block(out, depth + 1, body);
            let mut line = format!(
                "While label={} {}",
                opt_str(label),
                sp(span)
            );
            let c = cap(capture);
            if !c.is_empty() {
                line.push(' ');
                line.push_str(&c);
            }
            emit(out, depth, line);
        }
        Stmt::For(ForStmt {
            label,
            iter,
            capture,
            capture_name,
            body,
            span,
        }) => {
            dump_expr(out, depth + 1, iter);
            dump_block(out, depth + 1, body);
            emit(
                out,
                depth,
                format!(
                    "For label={} cap={}:{} {}",
                    opt_str(label),
                    cap_mode(capture),
                    capture_name,
                    sp(span)
                ),
            );
        }
        Stmt::Switch(SwitchStmt {
            subject,
            arms,
            has_else,
            span,
        }) => {
            dump_expr(out, depth + 1, subject);
            for a in arms {
                dump_switch_arm(out, depth + 1, a);
            }
            emit(
                out,
                depth,
                format!("Switch else={} {}", *has_else as u8, sp(span)),
            );
        }
        Stmt::Return(e, span) => {
            if let Some(e2) = e {
                dump_expr(out, depth + 1, e2);
            }
            emit(out, depth, format!("Return {}", sp(span)));
        }
        Stmt::Break(label, span) => {
            emit(
                out,
                depth,
                format!("Break label={} {}", opt_str(label), sp(span)),
            );
        }
        Stmt::Continue(label, span) => {
            emit(
                out,
                depth,
                format!("Continue label={} {}", opt_str(label), sp(span)),
            );
        }
        Stmt::Defer(e, span) => {
            dump_expr(out, depth + 1, e);
            emit(out, depth, format!("Defer {}", sp(span)));
        }
        Stmt::Errdefer(e, span) => {
            dump_expr(out, depth + 1, e);
            emit(out, depth, format!("Errdefer {}", sp(span)));
        }
        Stmt::Block(b) => dump_block(out, depth, b),
        Stmt::Empty => emit(out, depth, "Empty".to_string()),
    }
}

fn dump_switch_arm(out: &mut Vec<String>, depth: usize, a: &SwitchArm) {
    for p in &a.patterns {
        dump_switch_pattern(out, depth + 1, p);
    }
    dump_block(out, depth + 1, &a.body);
    let mut line = format!("SwitchArm {}", sp(&a.span));
    let c = cap(&a.capture);
    if !c.is_empty() {
        line.push(' ');
        line.push_str(&c);
    }
    emit(out, depth, line);
}

fn dump_switch_pattern(out: &mut Vec<String>, depth: usize, p: &SwitchPattern) {
    match p {
        SwitchPattern::Error(n) => emit(out, depth, format!("PatError {n}")),
        SwitchPattern::Ident(n) => emit(out, depth, format!("PatIdent {n}")),
        SwitchPattern::Int(s) => emit(out, depth, format!("PatInt {s}")),
        SwitchPattern::Float(s) => emit(out, depth, format!("PatFloat {s}")),
        SwitchPattern::Str(s) => emit(out, depth, format!("PatStr {}", q(s))),
        SwitchPattern::Char(c) => emit(out, depth, format!("PatChar {c}")),
        SwitchPattern::Else => emit(out, depth, "PatElse".to_string()),
    }
}

// ---------- 组件 ----------

fn dump_param(out: &mut Vec<String>, depth: usize, p: &Param) {
    dump_type(out, depth + 1, &p.ty);
    if let Some(d) = &p.default {
        dump_expr(out, depth + 1, d);
    }
    emit(
        out,
        depth,
        format!("Param {} {}", p.name, sp(&p.span)),
    );
}

fn dump_field(out: &mut Vec<String>, depth: usize, f: &FieldDecl) {
    dump_type(out, depth + 1, &f.ty);
    emit(
        out,
        depth,
        format!("FieldDecl {} pub={} {}", f.name, f.pub_ as u8, sp(&f.span)),
    );
}

fn dump_method(out: &mut Vec<String>, depth: usize, m: &Method) {
    for p in &m.params {
        dump_param(out, depth + 1, p);
    }
    if let Some(r) = &m.ret {
        dump_type(out, depth + 1, r);
    }
    for (tn, iface) in &m.where_clause {
        dump_type(out, depth + 1, iface);
        emit(out, depth + 1, format!("Where {tn}"));
    }
    dump_block(out, depth + 1, &m.body);
    emit(out, depth, format!("Method {} {}", m.name, sp(&m.span)));
}

fn dump_enum_variant(out: &mut Vec<String>, depth: usize, v: &EnumVariant) {
    if let Some(p) = &v.payload {
        dump_type(out, depth + 1, p);
    }
    emit(
        out,
        depth,
        format!("EnumVariant {} {}", v.name, sp(&v.span)),
    );
}

// ---------- 表达式 ----------

fn dump_expr(out: &mut Vec<String>, depth: usize, e: &Expr) {
    match e {
        Expr::IntLit { text, span } => {
            emit(out, depth, format!("Int {text} {}", sp(span)));
        }
        Expr::FloatLit { text, span } => {
            emit(out, depth, format!("Float {text} {}", sp(span)));
        }
        Expr::StrLit { value, raw, span } => {
            emit(
                out,
                depth,
                format!("Str raw={} {} {}", *raw as u8, q(value), sp(span)),
            );
        }
        Expr::CharLit(c, span) => {
            emit(out, depth, format!("Char {c} {}", sp(span)));
        }
        Expr::BoolLit(b, span) => {
            emit(out, depth, format!("Bool {} {}", *b as u8, sp(span)));
        }
        Expr::NullLit(span) => emit(out, depth, format!("Null {}", sp(span))),
        Expr::VoidLit(span) => emit(out, depth, format!("Void {}", sp(span))),
        Expr::Ident(name, span) => {
            emit(out, depth, format!("Ident {name} {}", sp(span)));
        }
        Expr::ArrayLit(items, span) => {
            for it in items {
                dump_expr(out, depth + 1, it);
            }
            emit(out, depth, format!("ArrayLit {} {}", items.len(), sp(span)));
        }
        Expr::TupleLit(items, span) => {
            for it in items {
                dump_expr(out, depth + 1, it);
            }
            emit(out, depth, format!("TupleLit {} {}", items.len(), sp(span)));
        }
        Expr::NamedLit {
            ty,
            ty_args,
            fields,
            span,
        } => {
            for a in ty_args {
                dump_type(out, depth + 1, a);
            }
            for (name, value) in fields {
                dump_expr(out, depth + 1, value);
                emit(out, depth + 1, format!("LitField {name}"));
            }
            emit(
                out,
                depth,
                format!("NamedLit {ty} nargs={} {}", ty_args.len(), sp(span)),
            );
        }
        Expr::StructType { fields, span } => {
            for (name, ty) in fields {
                dump_type(out, depth + 1, ty);
                emit(out, depth + 1, format!("STField {name}"));
            }
            emit(
                out,
                depth,
                format!("StructType {} {}", fields.len(), sp(span)),
            );
        }
        Expr::ArrayType { len, elem, span } => {
            dump_expr(out, depth + 1, len);
            dump_expr(out, depth + 1, elem);
            emit(out, depth, format!("ArrayType {}", sp(span)));
        }
        Expr::Dot { base, field, span } => {
            dump_expr(out, depth + 1, base);
            emit(out, depth, format!("Dot {field} {}", sp(span)));
        }
        Expr::Field { base, field, span } => {
            dump_expr(out, depth + 1, base);
            emit(out, depth, format!("Field {field} {}", sp(span)));
        }
        Expr::Index {
            base,
            indices,
            span,
        } => {
            dump_expr(out, depth + 1, base);
            for i in indices {
                dump_expr(out, depth + 1, i);
            }
            emit(out, depth, format!("Index {} {}", indices.len(), sp(span)));
        }
        Expr::Deref(inner, span) => {
            dump_expr(out, depth + 1, inner);
            emit(out, depth, format!("Deref {}", sp(span)));
        }
        Expr::AddrOf(inner, mut_, span) => {
            dump_expr(out, depth + 1, inner);
            emit(out, depth, format!("AddrOf mut={} {}", *mut_ as u8, sp(span)));
        }
        Expr::Unary(op, inner, span) => {
            dump_expr(out, depth + 1, inner);
            emit(
                out,
                depth,
                format!("Unary {} {}", unary_op(op), sp(span)),
            );
        }
        Expr::Binary(op, l, r, span) => {
            dump_expr(out, depth + 1, l);
            dump_expr(out, depth + 1, r);
            emit(
                out,
                depth,
                format!("Binary {} {}", bin_op(op), sp(span)),
            );
        }
        Expr::Orelse(l, r, span) => {
            dump_expr(out, depth + 1, l);
            dump_expr(out, depth + 1, r);
            emit(out, depth, format!("Orelse {}", sp(span)));
        }
        Expr::Unwrap(inner, span) => {
            dump_expr(out, depth + 1, inner);
            emit(out, depth, format!("Unwrap {}", sp(span)));
        }
        Expr::Try(inner, span) => {
            dump_expr(out, depth + 1, inner);
            emit(out, depth, format!("Try {}", sp(span)));
        }
        Expr::Await(inner, span) => {
            dump_expr(out, depth + 1, inner);
            emit(out, depth, format!("Await {}", sp(span)));
        }
        Expr::Catch(e2, kind, span) => {
            dump_expr(out, depth + 1, e2);
            match kind.as_ref() {
                CatchKind::Default(d) => {
                    dump_expr(out, depth + 1, d);
                    emit(out, depth, format!("Catch Default {}", sp(span)));
                }
                CatchKind::Bind { name, body } => {
                    dump_block(out, depth + 1, body);
                    emit(
                        out,
                        depth,
                        format!("Catch Bind name={name} {}", sp(span)),
                    );
                }
            }
        }
        Expr::Call { callee, args, span } => {
            dump_expr(out, depth + 1, callee);
            for a in args {
                dump_expr(out, depth + 1, a);
            }
            emit(out, depth, format!("Call {} {}", args.len(), sp(span)));
        }
        Expr::IfExpr {
            cond,
            capture,
            then_e,
            else_e,
            span,
        } => {
            dump_expr(out, depth + 1, cond);
            dump_expr(out, depth + 1, then_e);
            dump_expr(out, depth + 1, else_e);
            let mut line = format!("IfExpr {}", sp(span));
            let c = cap(capture);
            if !c.is_empty() {
                line.push(' ');
                line.push_str(&c);
            }
            emit(out, depth, line);
        }
        Expr::SwitchExpr { subject, arms, span } => {
            dump_expr(out, depth + 1, subject);
            for a in arms {
                dump_switch_arm(out, depth + 1, a);
            }
            emit(out, depth, format!("SwitchExpr {}", sp(span)));
        }
        Expr::Block(b, span) => {
            dump_block(out, depth + 1, b);
            emit(out, depth, format!("BlockExpr {}", sp(span)));
        }
        Expr::Assign {
            target,
            op,
            value,
            span,
        } => {
            dump_expr(out, depth + 1, target);
            dump_expr(out, depth + 1, value);
            emit(
                out,
                depth,
                format!("Assign {} {}", assign_op(op), sp(span)),
            );
        }
        Expr::ErrorLit(name, span) => {
            emit(out, depth, format!("ErrorLit {name} {}", sp(span)));
        }
        Expr::FnRef(name, span) => {
            emit(out, depth, format!("FnRef {name} {}", sp(span)));
        }
        Expr::TupleDestructure(names, e2, span) => {
            for n in names {
                emit(out, depth + 1, format!("DName {n}"));
            }
            dump_expr(out, depth + 1, e2);
            emit(out, depth, format!("TupleDestructure {}", sp(span)));
        }
        Expr::Move(inner, span) => {
            dump_expr(out, depth + 1, inner);
            emit(out, depth, format!("Move {}", sp(span)));
        }
        Expr::Closure {
            params,
            body,
            is_mut,
            is_move,
            span,
        } => {
            for p in params {
                emit(out, depth + 1, format!("CParam {p}"));
            }
            dump_block(out, depth + 1, body);
            emit(
                out,
                depth,
                format!(
                    "Closure mut={} move={} {}",
                    *is_mut as u8,
                    *is_move as u8,
                    sp(span)
                ),
            );
        }
    }
}

fn unary_op(op: &UnaryOp) -> &'static str {
    match op {
        UnaryOp::Neg => "Neg",
        UnaryOp::Not => "Not",
        UnaryOp::BitNot => "BitNot",
    }
}

fn bin_op(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add => "Add",
        BinOp::Sub => "Sub",
        BinOp::Mul => "Mul",
        BinOp::Div => "Div",
        BinOp::Mod => "Mod",
        BinOp::EucMod => "EucMod",
        BinOp::Eq => "Eq",
        BinOp::Ne => "Ne",
        BinOp::Lt => "Lt",
        BinOp::Le => "Le",
        BinOp::Gt => "Gt",
        BinOp::Ge => "Ge",
        BinOp::And => "And",
        BinOp::Or => "Or",
        BinOp::BitAnd => "BitAnd",
        BinOp::BitOr => "BitOr",
        BinOp::BitXor => "BitXor",
        BinOp::Shl => "Shl",
        BinOp::Shr => "Shr",
        BinOp::Range => "Range",
    }
}

fn assign_op(op: &AssignOp) -> &'static str {
    match op {
        AssignOp::Set => "Set",
        AssignOp::Add => "Add",
        AssignOp::Sub => "Sub",
        AssignOp::Mul => "Mul",
        AssignOp::Div => "Div",
        AssignOp::BitOr => "BitOr",
        AssignOp::BitAnd => "BitAnd",
        AssignOp::BitXor => "BitXor",
    }
}
