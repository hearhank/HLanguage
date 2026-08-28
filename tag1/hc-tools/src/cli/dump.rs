//! K2：`hc parse <file.hc>`——Rust 参考 parser 输出 AST 树。
//!
//! 每行一个节点，缩进表示嵌套层级，格式 `深度:NodeType|field=val|field=val`。
//! H 版 parser（stage1/parser.hc）输出同一格式，对照测试逐行 diff。

use std::path::Path;
use std::process::ExitCode;

use super::read_source::read_source;
use super::usage::USAGE;

/// K2：`hc parse <file.hc>`——Rust 参考 parser 输出 AST 树。
///
/// 每行一个节点，缩进表示嵌套层级，格式 `深度:NodeType|field=val|field=val`。
/// H 版 parser（stage1/parser.hc）输出同一格式，对照测试逐行 diff。
pub(crate) fn parse_command(args: &[String]) -> ExitCode {
    let Some(path_str) = args.first() else {
        eprintln!("error: `hc parse` requires a file\n\n{USAGE}");
        return ExitCode::from(2);
    };
    let source = match read_source(Path::new(path_str)) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let tokens = hc::lexer::lex(&source);
    let diags: Vec<_> = tokens
        .iter()
        .filter_map(|t| match &t.kind {
            hc::token::TokenKind::Error(msg) => Some(hc::Diagnostic::error(
                t.span.clone(),
                format!("lex error: {msg}"),
            )),
            _ => None,
        })
        .collect();
    if !diags.is_empty() {
        for d in &diags {
            eprintln!("{}:{}: {}", d.span.line, d.span.col, d.message);
        }
        return ExitCode::FAILURE;
    }
    let parser = hc::parser::Parser::new(&source, tokens);
    match parser.parse_program() {
        Ok(program) => {
            dump_ast(&program, 0);
            ExitCode::SUCCESS
        }
        Err(diags) => {
            for d in &diags {
                eprintln!("{}:{}: {}", d.span.line, d.span.col, d.message);
            }
            ExitCode::FAILURE
        }
    }
}

/// K2：递归输出 AST 树（格式：`深度:NodeType|field=val|field=val`）。
fn dump_ast(program: &hc::ast::Program, depth: usize) {
    let indent = " ".repeat(depth * 2);
    println!("{indent}Program");
    for decl in &program.decls {
        dump_decl(decl, depth + 1);
    }
}

fn dump_decl(decl: &hc::ast::Decl, depth: usize) {
    let indent = " ".repeat(depth * 2);
    match decl {
        hc::ast::Decl::Global {
            name,
            ty,
            init,
            pub_,
            ..
        } => {
            print!("{indent}Global|name={name}");
            if let Some(t) = ty {
                print!("|ty={:?}", hc::ast::fmt_type_debug(t));
            }
            if init.is_some() {
                print!("|has_init=true");
            }
            if *pub_ {
                print!("|pub=true");
            }
            println!();
        }
        hc::ast::Decl::Const { name, ty, pub_, .. } => {
            print!("{indent}Const|name={name}");
            if let Some(t) = ty {
                print!("|ty={:?}", hc::ast::fmt_type_debug(t));
            }
            if *pub_ {
                print!("|pub=true");
            }
            println!();
        }
        hc::ast::Decl::Fn {
            name,
            type_params,
            params,
            ret,
            is_test,
            is_async,
            pub_,
            exported,
            is_extern,
            body,
            ..
        } => {
            print!("{indent}Fn|name={name}");
            if !type_params.is_empty() {
                print!("|type_params={:?}", type_params);
            }
            if *pub_ {
                print!("|pub=true");
            }
            if *is_test {
                print!("|test=true");
            }
            if *is_async {
                print!("|async=true");
            }
            if *exported {
                print!("|exported=true");
            }
            if *is_extern {
                print!("|extern=true");
            }
            println!();
            for p in params {
                dump_param(p, depth + 1);
            }
            if let Some(t) = ret {
                println!("{}  ret: {:?}", indent, hc::ast::fmt_type_debug(t));
            }
            dump_block(body, depth + 1);
        }
        hc::ast::Decl::Class {
            name,
            fields,
            methods,
            pub_,
            ..
        } => {
            print!("{indent}Class|name={name}");
            if *pub_ {
                print!("|pub=true");
            }
            println!();
            for f in fields {
                println!(
                    "{}  Field|name={}|ty={:?}",
                    indent,
                    f.name,
                    hc::ast::fmt_type_debug(&f.ty)
                );
            }
            for m in methods {
                dump_method(m, depth + 1);
            }
        }
        hc::ast::Decl::Enum {
            name,
            variants,
            pub_,
            ..
        } => {
            print!("{indent}Enum|name={name}");
            if *pub_ {
                print!("|pub=true");
            }
            println!();
            for v in variants {
                print!("{}  Variant|name={}", indent, v.name);
                if let Some(pty) = &v.payload {
                    print!("|payload={:?}", hc::ast::fmt_type_debug(pty));
                }
                println!();
            }
        }
        hc::ast::Decl::Union {
            name, fields, pub_, ..
        } => {
            print!("{indent}Union|name={name}");
            if *pub_ {
                print!("|pub=true");
            }
            println!();
            for f in fields {
                println!(
                    "{}  Field|name={}|ty={:?}",
                    indent,
                    f.name,
                    hc::ast::fmt_type_debug(&f.ty)
                );
            }
        }
        hc::ast::Decl::Interface {
            name,
            methods,
            pub_,
            ..
        } => {
            print!("{indent}Interface|name={name}");
            if *pub_ {
                print!("|pub=true");
            }
            println!();
            for m in methods {
                dump_method(m, depth + 1);
            }
        }
        hc::ast::Decl::Namespace {
            name,
            decls,
            pub_,
            is_module,
            ..
        } => {
            print!("{indent}Namespace|name={name}");
            if *pub_ {
                print!("|pub=true");
            }
            if *is_module {
                print!("|module=true");
            }
            println!();
            for d in decls {
                dump_decl(d, depth + 1);
            }
        }
        hc::ast::Decl::Import {
            path,
            alias,
            select,
            ..
        } => {
            print!("{indent}Import|path={:?}", path);
            if let Some(a) = alias {
                print!("|alias={a}");
            }
            if let Some(s) = select {
                print!("|select={:?}", s);
            }
            println!();
        }
        hc::ast::Decl::Include { path, alias, .. } => {
            print!("{indent}Include|path={path:?}");
            if let Some(a) = alias {
                print!("|alias={a}");
            }
            println!();
        }
        hc::ast::Decl::Struct {
            name, fields, pub_, ..
        } => {
            print!("{indent}Struct|name={name}");
            if *pub_ {
                print!("|pub=true");
            }
            println!();
            for f in fields {
                println!(
                    "{}  Field|name={}|ty={:?}",
                    indent,
                    f.name,
                    hc::ast::fmt_type_debug(&f.ty)
                );
            }
        }
        hc::ast::Decl::Comptime { .. } => {
            println!("{indent}Comptime");
        }
    }
}

fn dump_param(p: &hc::ast::Param, depth: usize) {
    let indent = " ".repeat(depth * 2);
    print!(
        "{indent}Param|name={}|ty={:?}",
        p.name,
        hc::ast::fmt_type_debug(&p.ty)
    );
    if p.default.is_some() {
        print!("|has_default=true");
    }
    println!();
}

fn dump_method(m: &hc::ast::Method, depth: usize) {
    let indent = " ".repeat(depth * 2);
    println!("{indent}Method|name={}", m.name);
    for p in &m.params {
        dump_param(p, depth + 1);
    }
    if let Some(t) = &m.ret {
        println!("{}  ret: {:?}", indent, hc::ast::fmt_type_debug(t));
    }
    dump_block(&m.body, depth + 1);
}

fn dump_block(b: &hc::ast::Block, depth: usize) {
    let indent = " ".repeat(depth * 2);
    println!("{indent}Block");
    for stmt in &b.stmts {
        dump_stmt(stmt, depth + 1);
    }
}

fn dump_stmt(stmt: &hc::ast::Stmt, depth: usize) {
    let indent = " ".repeat(depth * 2);
    match stmt {
        hc::ast::Stmt::VarDecl {
            name,
            mut_,
            ty,
            init,
            ..
        } => {
            print!("{indent}VarDecl|name={name}");
            if *mut_ {
                print!("|mut=true");
            }
            if let Some(t) = ty {
                print!("|ty={:?}", hc::ast::fmt_type_debug(t));
            }
            if init.is_some() {
                print!("|has_init=true");
            }
            println!();
        }
        hc::ast::Stmt::ConstDecl { name, .. } => {
            println!("{indent}ConstDecl|name={name}");
        }
        hc::ast::Stmt::Expr(e) => {
            print!("{indent}ExprStmt ");
            dump_expr(e, depth);
        }
        hc::ast::Stmt::If(s) => {
            print!("{indent}If");
            if let Some((_, cap)) = &s.capture {
                print!("|capture={cap}");
            }
            if let Some((_, err)) = &s.err_capture {
                print!("|err_capture={err}");
            }
            println!();
            dump_expr(&s.cond, depth + 1);
            dump_block(&s.then_b, depth + 1);
            if let Some(el) = &s.else_b {
                dump_stmt(el, depth + 1);
            }
        }
        hc::ast::Stmt::While(s) => {
            print!("{indent}While");
            if let Some(l) = &s.label {
                print!("|label={l}");
            }
            if let Some((_, cap)) = &s.capture {
                print!("|capture={cap}");
            }
            println!();
            dump_expr(&s.cond, depth + 1);
            dump_block(&s.body, depth + 1);
        }
        hc::ast::Stmt::For(s) => {
            print!("{indent}For");
            if let Some(l) = &s.label {
                print!("|label={l}");
            }
            println!("|capture={} iter={:?}", s.capture_name, s.capture);
            dump_expr(&s.iter, depth + 1);
            dump_block(&s.body, depth + 1);
        }
        hc::ast::Stmt::Switch(s) => {
            println!("{indent}Switch");
            dump_expr(&s.subject, depth + 1);
            for arm in &s.arms {
                print!("{}  SwitchArm", indent);
                if let Some((_, cap)) = &arm.capture {
                    print!("|capture={cap}");
                }
                if arm.guard.is_some() {
                    print!("|has_guard=true");
                }
                println!();
                for pat in &arm.patterns {
                    print!("{}    Pattern", indent);
                    match pat {
                        hc::ast::SwitchPattern::Error(s) => println!("|error={s}"),
                        hc::ast::SwitchPattern::Ident(s) => println!("|ident={s}"),
                        hc::ast::SwitchPattern::Int(s) => println!("|int={s}"),
                        hc::ast::SwitchPattern::Float(s) => println!("|float={s}"),
                        hc::ast::SwitchPattern::Str(s) => println!("|str={s}"),
                        hc::ast::SwitchPattern::Char(c) => println!("|char={c}"),
                        hc::ast::SwitchPattern::Else => println!("|else"),
                    }
                }
                dump_block(&arm.body, depth + 2);
            }
        }
        hc::ast::Stmt::Return(v, _) => {
            print!("{indent}Return");
            if let Some(val) = v {
                println!();
                dump_expr(val, depth + 1);
            } else {
                println!();
            }
        }
        hc::ast::Stmt::Break(l, _) => {
            print!("{indent}Break");
            if let Some(label) = l {
                print!("|label={label}");
            }
            println!();
        }
        hc::ast::Stmt::Continue(l, _) => {
            print!("{indent}Continue");
            if let Some(label) = l {
                print!("|label={label}");
            }
            println!();
        }
        hc::ast::Stmt::Defer(_, _) => println!("{indent}Defer"),
        hc::ast::Stmt::Errdefer(_, _) => println!("{indent}Errdefer"),
        hc::ast::Stmt::Block(b) => dump_block(b, depth),
        hc::ast::Stmt::Empty => println!("{indent}Empty"),
    }
}

fn dump_expr(expr: &hc::ast::Expr, depth: usize) {
    let indent = " ".repeat(depth * 2);
    match expr {
        hc::ast::Expr::IntLit { text, .. } => println!("{indent}IntLit|text={text}"),
        hc::ast::Expr::FloatLit { text, .. } => println!("{indent}FloatLit|text={text}"),
        hc::ast::Expr::StrLit { value, raw, .. } => {
            println!("{indent}StrLit|value={value}|raw={raw}")
        }
        hc::ast::Expr::CharLit(v, _) => println!("{indent}CharLit|value={v}"),
        hc::ast::Expr::BoolLit(v, _) => println!("{indent}BoolLit|value={v}"),
        hc::ast::Expr::NullLit(_) => println!("{indent}NullLit"),
        hc::ast::Expr::VoidLit(_) => println!("{indent}VoidLit"),
        hc::ast::Expr::Ident(name, _) => println!("{indent}Ident|name={name}"),
        hc::ast::Expr::ArrayLit(items, _) => {
            println!("{indent}ArrayLit");
            for e in items {
                dump_expr(e, depth + 1);
            }
        }
        hc::ast::Expr::TupleLit(items, _) => {
            println!("{indent}TupleLit");
            for e in items {
                dump_expr(e, depth + 1);
            }
        }
        hc::ast::Expr::ContainerLit {
            ty, ty_args, items, ..
        } => {
            print!("{indent}ContainerLit|ty={ty}");
            if !ty_args.is_empty() {
                print!("|ty_args=[");
                for (i, ta) in ty_args.iter().enumerate() {
                    if i > 0 {
                        print!(", ");
                    }
                    print!("{}", hc::ast::fmt_type_debug(ta));
                }
                print!("]");
            }
            println!();
            let indent = format!("{indent}  ");
            for item in items {
                dump_expr(item, depth + 1);
            }
        }
        hc::ast::Expr::NamedLit {
            ty,
            ty_args,
            fields,
            ..
        } => {
            print!("{indent}NamedLit|ty={ty}");
            if !ty_args.is_empty() {
                print!(
                    "|ty_args={:?}",
                    ty_args
                        .iter()
                        .map(|t| hc::ast::fmt_type_debug(t))
                        .collect::<Vec<_>>()
                );
            }
            println!();
            for (name, val) in fields {
                println!("{}  field={name}", indent);
                dump_expr(val, depth + 2);
            }
        }
        hc::ast::Expr::StructType { fields, .. } => {
            println!("{indent}StructType");
            for (name, ty) in fields {
                println!(
                    "{}  field={name}|ty={:?}",
                    indent,
                    hc::ast::fmt_type_debug(ty)
                );
            }
        }
        hc::ast::Expr::ArrayType { len, elem, .. } => {
            println!("{indent}ArrayType");
            dump_expr(len, depth + 1);
            dump_expr(elem, depth + 1);
        }
        hc::ast::Expr::Dot { base, field, .. } => {
            println!("{indent}Dot|field={field}");
            dump_expr(base, depth + 1);
        }
        hc::ast::Expr::Field { base, field, .. } => {
            println!("{indent}Field|field={field}");
            dump_expr(base, depth + 1);
        }
        hc::ast::Expr::Index { base, indices, .. } => {
            println!("{indent}Index");
            dump_expr(base, depth + 1);
            for i in indices {
                dump_expr(i, depth + 1);
            }
        }
        hc::ast::Expr::Deref(e, _) => {
            println!("{indent}Deref");
            dump_expr(e, depth + 1);
        }
        hc::ast::Expr::AddrOf(e, mut_, _) => {
            println!("{indent}AddrOf|mut={mut_}");
            dump_expr(e, depth + 1);
        }
        hc::ast::Expr::Unary(op, e, _) => {
            println!("{indent}Unary|op={:?}", op);
            dump_expr(e, depth + 1);
        }
        hc::ast::Expr::Binary(op, l, r, _) => {
            println!("{indent}Binary|op={:?}", op);
            dump_expr(l, depth + 1);
            dump_expr(r, depth + 1);
        }
        hc::ast::Expr::Orelse(l, r, _) => {
            println!("{indent}Orelse");
            dump_expr(l, depth + 1);
            dump_expr(r, depth + 1);
        }
        hc::ast::Expr::Unwrap(e, _) => {
            println!("{indent}Unwrap");
            dump_expr(e, depth + 1);
        }
        hc::ast::Expr::Try(e, _) => {
            println!("{indent}Try");
            dump_expr(e, depth + 1);
        }
        hc::ast::Expr::Await(e, _) => {
            println!("{indent}Await");
            dump_expr(e, depth + 1);
        }
        hc::ast::Expr::Catch(e, kind, _) => {
            println!("{indent}Catch");
            dump_expr(e, depth + 1);
            match kind.as_ref() {
                hc::ast::CatchKind::Default(d) => {
                    println!("{}  Default", indent);
                    dump_expr(d, depth + 2);
                }
                hc::ast::CatchKind::Bind { name, body } => {
                    println!("{}  Bind|name={name}", indent);
                    dump_block(body, depth + 2);
                }
            }
        }
        hc::ast::Expr::Call { callee, args, .. } => {
            println!("{indent}Call");
            dump_expr(callee, depth + 1);
            for a in args {
                dump_expr(a, depth + 1);
            }
        }
        hc::ast::Expr::IfExpr {
            cond,
            capture,
            then_e,
            else_e,
            ..
        } => {
            print!("{indent}IfExpr");
            if let Some((_, cap)) = capture {
                print!("|capture={cap}");
            }
            println!();
            dump_expr(cond, depth + 1);
            dump_expr(then_e, depth + 1);
            dump_expr(else_e, depth + 1);
        }
        hc::ast::Expr::SwitchExpr { subject, arms, .. } => {
            println!("{indent}SwitchExpr");
            dump_expr(subject, depth + 1);
            for arm in arms {
                println!("{}  SwitchArm", indent);
                for pat in &arm.patterns {
                    match pat {
                        hc::ast::SwitchPattern::Ident(s) => {
                            println!("{}    Pattern|ident={s}", indent)
                        }
                        _ => println!("{}    Pattern", indent),
                    }
                }
                dump_block(&arm.body, depth + 2);
            }
        }
        hc::ast::Expr::Block(b, _) => dump_block(b, depth),
        hc::ast::Expr::Assign {
            target, op, value, ..
        } => {
            println!("{indent}Assign|op={:?}", op);
            dump_expr(target, depth + 1);
            dump_expr(value, depth + 1);
        }
        hc::ast::Expr::ErrorLit(name, _) => println!("{indent}ErrorLit|name={name}"),
        hc::ast::Expr::FnRef(name, _) => println!("{indent}FnRef|name={name}"),
        hc::ast::Expr::TupleDestructure(names, e, _) => {
            println!("{indent}TupleDestructure|names={:?}", names);
            dump_expr(e, depth + 1);
        }
        hc::ast::Expr::Move(e, _) => {
            println!("{indent}Move");
            dump_expr(e, depth + 1);
        }
        hc::ast::Expr::Closure {
            params,
            is_mut,
            is_move,
            ..
        } => {
            print!("{indent}Closure|params={:?}", params);
            if *is_mut {
                print!("|mut");
            }
            if *is_move {
                print!("|move");
            }
            println!();
        }
    }
}
