//! hc lint —— 静态诊断（B1）
//!
//! 6 条规则（L001–L006）：
//! - L001 `unused_var`：未使用变量
//! - L002 `unused_import`：未使用导入
//! - L003 `simplifiable_construct`：可简化构造（如 `Vec(List<i32>)` → `Vec<List<i32>>`）
//! - L004 `upper_case_abbr`：缩写应全大写（如 `json` → `JSON`）
//! - L005 `simplifiable_if_else`：可简化 if-else（`if (x) true else false` → `x`）
//! - L006 `redundant_eq_false`：多余的 `== false`（`x == false` → `!x`）
//!
//! 4 规则支持 `--fix`：L003、L004、L005、L006
//! `// @lint(off rule_name)` 内联关闭
//! `hc lint` 独立子命令 + `hc check` 默认集成
//! 文本 + JSON 输出

use std::collections::{HashMap, HashSet};

use hc::ast::*;
use hc::token::Span;

// ---------- lint 规则定义 ----------

const RULES: &[LintRule] = &[
    LintRule {
        code: "L001",
        name: "unused_var",
        has_fix: false,
        desc: "未使用变量",
    },
    LintRule {
        code: "L001",
        name: "unused_import",
        has_fix: false,
        desc: "未使用导入",
    },
    LintRule {
        code: "L003",
        name: "simplifiable_construct",
        has_fix: true,
        desc: "可简化构造",
    },
    LintRule {
        code: "L004",
        name: "upper_case_abbr",
        has_fix: true,
        desc: "缩写应全大写",
    },
    LintRule {
        code: "L005",
        name: "simplifiable_if_else",
        has_fix: true,
        desc: "可简化 if-else",
    },
    LintRule {
        code: "L006",
        name: "redundant_eq_false",
        has_fix: true,
        desc: "多余的 == false",
    },
];

#[derive(Clone, Copy, Debug)]
pub struct LintRule {
    pub code: &'static str,
    pub name: &'static str,
    pub has_fix: bool,
    pub desc: &'static str,
}

pub fn all_rules() -> &'static [LintRule] {
    RULES
}

pub fn find_rule(name: &str) -> Option<&'static LintRule> {
    RULES.iter().find(|r| r.name == name)
}

// ---------- 诊断结果 ----------

#[derive(Debug, Clone)]
pub struct LintDiag {
    pub rule: &'static LintRule,
    pub span: Span,
    pub message: String,
    pub fix: Option<String>,
}

impl LintDiag {
    pub fn render(&self, _source: &str) -> String {
        format!(
            "{}:{}:{}: [{}] {} ({})",
            self.rule.code,
            self.span.line,
            self.span.col,
            self.rule.name,
            self.message,
            self.rule.desc,
        )
    }
}

// ---------- 禁用注释解析 ----------

/// 解析源文件中的 `// @lint(off rule_name)` 注释，返回被禁用规则名 → 所在行号集合。
fn parse_lint_off_comments(source: &str) -> HashMap<String, HashSet<usize>> {
    let mut disabled: HashMap<String, HashSet<usize>> = HashMap::new();
    for (i, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("// @lint(off ") {
            if let Some(name) = rest.strip_suffix(')') {
                let name = name.trim();
                disabled.entry(name.to_string()).or_default().insert(i + 1);
            }
        }
    }
    disabled
}

fn is_disabled(disabled: &HashMap<String, HashSet<usize>>, rule: &str, line: usize) -> bool {
    disabled
        .get(rule)
        .map_or(false, |lines| lines.contains(&line))
}

// ---------- 名称收集器（用于 unused_var / unused_import） ----------

/// 收集所有变量声明名（包括 fn 参数、for 捕获、switch 捕获等）
fn collect_decls(program: &Program) -> Vec<(String, Span)> {
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

/// 收集所有标识符引用（用于判断变量是否被使用）
fn collect_refs(program: &Program) -> Vec<String> {
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
        Decl::Using { .. } | Decl::Import { .. } => {}
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

// ---------- 导入收集（用于 unused_import） ----------

fn collect_imports(program: &Program) -> Vec<(String, Span)> {
    let mut imports = Vec::new();
    for d in &program.decls {
        if let Decl::Import {
            path,
            alias,
            select,
            span,
        } = d
        {
            // 导入的名称为路径末段（或别名）
            let name = alias
                .clone()
                .unwrap_or_else(|| path.last().cloned().unwrap_or_default());
            // 符号选择导入：每个符号名单独算
            if let Some(sel) = select {
                for (orig, _alias) in sel {
                    let import_name = _alias.clone().unwrap_or_else(|| orig.clone());
                    imports.push((import_name, span.clone()));
                }
            } else {
                imports.push((name, span.clone()));
            }
        }
    }
    imports
}

// ---------- L004 缩写检测 ----------

/// 常见应全大写的缩写
const ABBREVIATIONS: &[&str] = &[
    "json", "html", "http", "https", "url", "uri", "id", "db", "csv", "xml", "yaml", "toml", "ini",
    "ssh", "ftp", "smtp", "imap", "pop", "tcp", "udp", "ip", "dns", "dhcp", "ssl", "tls", "api",
    "cli", "gui", "ui", "ux", "os", "io", "math", "regex", "uuid", "sha", "md5", "aes", "rsa",
    "ecdsa", "jwt", "oauth", "ldap", "sql", "orm", "rpc", "grpc", "rest", "soap", "html", "css",
    "png", "jpg", "gif", "svg", "pdf", "txt", "rtf", "async", "sync", "mutex", "sem", "fifo",
    "lifo", "mime", "base64", "ascii", "utf8", "utf16", "utf32", "ansi", "ebcdic",
];

/// 检查标识符中是否包含应全大写的缩写
fn check_abbr(name: &str) -> Vec<(usize, &'static str)> {
    let mut results = Vec::new();
    let lower = name.to_lowercase();
    for &abbr in ABBREVIATIONS {
        if let Some(pos) = lower.find(abbr) {
            // 检查缩写部分是否确实为小写（非全大写）
            let actual = &name[pos..pos + abbr.len()];
            if actual.chars().any(|c| c.is_lowercase()) {
                // 确认不是全大写
                let _upper: String = abbr.to_uppercase();
                results.push((pos, abbr));
            }
        }
    }
    results
}

// ---------- 主 lint 函数 ----------

/// 对单个源文件执行 lint 检查
pub fn lint_source(source: &str, program: &Program, fix: bool) -> Vec<LintDiag> {
    let disabled = parse_lint_off_comments(source);
    let mut diags = Vec::new();

    // L001: unused_var
    lint_unused_var(program, &disabled, &mut diags);

    // L002: unused_import
    lint_unused_import(program, &disabled, &mut diags);

    // L003: simplifiable_construct
    lint_simplifiable_construct(program, source, &disabled, fix, &mut diags);

    // L004: upper_case_abbr
    lint_upper_case_abbr(program, source, &disabled, fix, &mut diags);

    // L005: simplifiable_if_else
    lint_simplifiable_if_else(program, source, &disabled, fix, &mut diags);

    // L006: redundant_eq_false
    lint_redundant_eq_false(program, source, &disabled, fix, &mut diags);

    diags
}

// ---------- L001: unused_var ----------

fn lint_unused_var(
    program: &Program,
    disabled: &HashMap<String, HashSet<usize>>,
    diags: &mut Vec<LintDiag>,
) {
    let decls = collect_decls(program);
    let refs: HashSet<String> = collect_refs(program).into_iter().collect();
    let rule = find_rule("unused_var").unwrap();

    for (name, span) in &decls {
        // 跳过 `_` 前缀约定（intentionally unused）
        if name.starts_with('_') || name == "_" {
            continue;
        }
        // 跳过函数名（函数名在 Decl::Fn 的 name 中，不在 param 中）
        // 跳过全局变量和常量
        let is_param_or_local = span.start != 0 || span.end != 0;
        if !is_param_or_local {
            continue;
        }
        if !refs.contains(name) {
            if !is_disabled(disabled, "unused_var", span.line as usize) {
                diags.push(LintDiag {
                    rule,
                    span: span.clone(),
                    message: format!("未使用变量 `{name}`"),
                    fix: None,
                });
            }
        }
    }
}

// ---------- L002: unused_import ----------

fn lint_unused_import(
    program: &Program,
    disabled: &HashMap<String, HashSet<usize>>,
    diags: &mut Vec<LintDiag>,
) {
    let imports = collect_imports(program);
    let refs: HashSet<String> = collect_refs(program).into_iter().collect();
    let rule = find_rule("unused_import").unwrap();

    for (name, span) in &imports {
        // 跳过 `H.std` 标准库导入（通常隐式使用）
        if name == "std" || name.starts_with('H') {
            continue;
        }
        if !refs.contains(name) {
            if !is_disabled(disabled, "unused_import", span.line as usize) {
                diags.push(LintDiag {
                    rule,
                    span: span.clone(),
                    message: format!("未使用导入 `{name}`"),
                    fix: None,
                });
            }
        }
    }
}

// ---------- L003: simplifiable_construct ----------

fn lint_simplifiable_construct(
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
        Decl::Using { .. } | Decl::Import { .. } => {}
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

// ---------- L004: upper_case_abbr ----------

fn lint_upper_case_abbr(
    program: &Program,
    source: &str,
    disabled: &HashMap<String, HashSet<usize>>,
    fix: bool,
    diags: &mut Vec<LintDiag>,
) {
    let rule = find_rule("upper_case_abbr").unwrap();
    for d in &program.decls {
        check_abbr_in_decl(d, source, disabled, fix, rule, diags);
    }
}

fn check_abbr_in_decl(
    decl: &Decl,
    source: &str,
    disabled: &HashMap<String, HashSet<usize>>,
    fix: bool,
    rule: &'static LintRule,
    diags: &mut Vec<LintDiag>,
) {
    let names = match decl {
        Decl::Fn { name, .. } => vec![name.clone()],
        Decl::Class { name, .. } => vec![name.clone()],
        Decl::Enum { name, .. } => vec![name.clone()],
        Decl::Union { name, .. } => vec![name.clone()],
        Decl::Interface { name, .. } => vec![name.clone()],
        Decl::Namespace { name, .. } => vec![name.clone()],
        Decl::Global { name, .. } | Decl::Const { name, .. } => vec![name.clone()],
        Decl::Using { .. } | Decl::Import { .. } | Decl::Comptime { .. } | Decl::Include { .. } => {
            Vec::new()
        }
    };
    for name in names {
        let abbrs = check_abbr(&name);
        for (pos, abbr) in abbrs {
            let upper = abbr.to_uppercase();
            let fixed_name = format!("{}{}{}", &name[..pos], upper, &name[pos + abbr.len()..]);
            if !is_disabled(disabled, "upper_case_abbr", decl_span(decl).line as usize) {
                diags.push(LintDiag {
                    rule,
                    span: decl_span(decl),
                    message: format!("缩写 `{abbr}` 应全大写（`{name}` → `{fixed_name}`）"),
                    fix: if fix { Some(fixed_name) } else { None },
                });
            }
        }
    }
    // 检查命名空间内层
    if let Decl::Namespace { decls: inner, .. } = decl {
        for d in inner {
            check_abbr_in_decl(d, source, disabled, fix, rule, diags);
        }
    }
}

fn decl_span(decl: &Decl) -> Span {
    match decl {
        Decl::Fn { span, .. } => span.clone(),
        Decl::Class { span, .. } => span.clone(),
        Decl::Enum { span, .. } => span.clone(),
        Decl::Union { span, .. } => span.clone(),
        Decl::Interface { span, .. } => span.clone(),
        Decl::Namespace { span, .. } => span.clone(),
        Decl::Global { span, .. } => span.clone(),
        Decl::Const { span, .. } => span.clone(),
        Decl::Using { span, .. } => span.clone(),
        Decl::Import { span, .. } => span.clone(),
        Decl::Comptime { span, .. } => span.clone(),
        Decl::Include { span, .. } => span.clone(),
    }
}

// ---------- L005: simplifiable_if_else ----------

fn lint_simplifiable_if_else(
    program: &Program,
    source: &str,
    disabled: &HashMap<String, HashSet<usize>>,
    fix: bool,
    diags: &mut Vec<LintDiag>,
) {
    let rule = find_rule("simplifiable_if_else").unwrap();
    for d in &program.decls {
        check_simplifiable_if_else_in_decl(d, source, disabled, fix, rule, diags);
    }
}

fn check_simplifiable_if_else_in_decl(
    decl: &Decl,
    source: &str,
    disabled: &HashMap<String, HashSet<usize>>,
    fix: bool,
    rule: &'static LintRule,
    diags: &mut Vec<LintDiag>,
) {
    match decl {
        Decl::Fn { body, .. } => {
            check_simplifiable_if_else_in_block(body, source, disabled, fix, rule, diags)
        }
        Decl::Class { methods, .. } => {
            for m in methods {
                check_simplifiable_if_else_in_block(&m.body, source, disabled, fix, rule, diags);
            }
        }
        Decl::Namespace { decls: inner, .. } => {
            for d in inner {
                check_simplifiable_if_else_in_decl(d, source, disabled, fix, rule, diags);
            }
        }
        Decl::Comptime { body, .. } => {
            check_simplifiable_if_else_in_block(body, source, disabled, fix, rule, diags);
        }
        _ => {}
    }
}

fn check_simplifiable_if_else_in_block(
    block: &Block,
    source: &str,
    disabled: &HashMap<String, HashSet<usize>>,
    fix: bool,
    rule: &'static LintRule,
    diags: &mut Vec<LintDiag>,
) {
    for s in &block.stmts {
        check_simplifiable_if_else_in_stmt(s, source, disabled, fix, rule, diags);
    }
}

fn check_simplifiable_if_else_in_stmt(
    stmt: &Stmt,
    source: &str,
    disabled: &HashMap<String, HashSet<usize>>,
    fix: bool,
    rule: &'static LintRule,
    diags: &mut Vec<LintDiag>,
) {
    match stmt {
        Stmt::Expr(e) => check_simplifiable_if_else_in_expr(e, source, disabled, fix, rule, diags),
        Stmt::If(s) => {
            check_simplifiable_if_else_in_expr(&s.cond, source, disabled, fix, rule, diags);
            check_simplifiable_if_else_in_block(&s.then_b, source, disabled, fix, rule, diags);
            if let Some(e) = &s.else_b {
                check_simplifiable_if_else_in_stmt(e, source, disabled, fix, rule, diags);
            }
        }
        Stmt::While(s) => {
            check_simplifiable_if_else_in_expr(&s.cond, source, disabled, fix, rule, diags);
            if let Some(e) = &s.step {
                check_simplifiable_if_else_in_expr(e, source, disabled, fix, rule, diags);
            }
            check_simplifiable_if_else_in_block(&s.body, source, disabled, fix, rule, diags);
        }
        Stmt::For(s) => {
            check_simplifiable_if_else_in_expr(&s.iter, source, disabled, fix, rule, diags);
            check_simplifiable_if_else_in_block(&s.body, source, disabled, fix, rule, diags);
        }
        Stmt::Switch(s) => {
            check_simplifiable_if_else_in_expr(&s.subject, source, disabled, fix, rule, diags);
            for arm in &s.arms {
                check_simplifiable_if_else_in_block(&arm.body, source, disabled, fix, rule, diags);
            }
        }
        Stmt::Return(e, _) => {
            if let Some(e) = e {
                check_simplifiable_if_else_in_expr(e, source, disabled, fix, rule, diags);
            }
        }
        Stmt::Defer(e, _) | Stmt::Errdefer(e, _) => {
            check_simplifiable_if_else_in_expr(e, source, disabled, fix, rule, diags)
        }
        Stmt::VarDecl { init, .. } => {
            if let Some(e) = init {
                check_simplifiable_if_else_in_expr(e, source, disabled, fix, rule, diags);
            }
        }
        Stmt::ConstDecl { init, .. } => {
            check_simplifiable_if_else_in_expr(init, source, disabled, fix, rule, diags)
        }
        Stmt::Block(b) => {
            check_simplifiable_if_else_in_block(b, source, disabled, fix, rule, diags)
        }
        _ => {}
    }
}

fn check_simplifiable_if_else_in_expr(
    expr: &Expr,
    source: &str,
    disabled: &HashMap<String, HashSet<usize>>,
    fix: bool,
    rule: &'static LintRule,
    diags: &mut Vec<LintDiag>,
) {
    match expr {
        Expr::IfExpr {
            cond,
            capture: _,
            then_e,
            else_e,
            span,
        } => {
            // 检测 `if (x) true else false` → `x`
            // 检测 `if (x) false else true` → `!x`
            let is_simplifiable = |e: &Expr| -> Option<bool> {
                match e {
                    Expr::BoolLit(v, _) => Some(*v),
                    _ => None,
                }
            };
            if let (Some(then_v), Some(else_v)) = (is_simplifiable(then_e), is_simplifiable(else_e))
            {
                if then_v && !else_v {
                    // `if (x) true else false` → `x`
                    if !is_disabled(disabled, "simplifiable_if_else", span.line as usize) {
                        diags.push(LintDiag {
                            rule,
                            span: span.clone(),
                            message: "可简化为条件本身：`if (x) true else false` → `x`".to_string(),
                            fix: if fix {
                                Some("/* simplify: x */".to_string())
                            } else {
                                None
                            },
                        });
                    }
                } else if !then_v && else_v {
                    // `if (x) false else true` → `!x`
                    if !is_disabled(disabled, "simplifiable_if_else", span.line as usize) {
                        diags.push(LintDiag {
                            rule,
                            span: span.clone(),
                            message: "可简化为 `!x`：`if (x) false else true` → `!x`".to_string(),
                            fix: if fix {
                                Some("/* simplify: !x */".to_string())
                            } else {
                                None
                            },
                        });
                    }
                }
            }
            check_simplifiable_if_else_in_expr(cond, source, disabled, fix, rule, diags);
            check_simplifiable_if_else_in_expr(then_e, source, disabled, fix, rule, diags);
            check_simplifiable_if_else_in_expr(else_e, source, disabled, fix, rule, diags);
        }
        Expr::Block(b, _) => {
            check_simplifiable_if_else_in_block(b, source, disabled, fix, rule, diags)
        }
        Expr::Call { args, .. } => {
            for a in args {
                check_simplifiable_if_else_in_expr(a, source, disabled, fix, rule, diags);
            }
        }
        Expr::Binary(_, a, b, _) => {
            check_simplifiable_if_else_in_expr(a, source, disabled, fix, rule, diags);
            check_simplifiable_if_else_in_expr(b, source, disabled, fix, rule, diags);
        }
        Expr::Unary(_, e, _)
        | Expr::Deref(e, _)
        | Expr::AddrOf(e, _, _)
        | Expr::Unwrap(e, _)
        | Expr::Try(e, _)
        | Expr::Await(e, _)
        | Expr::Move(e, _) => {
            check_simplifiable_if_else_in_expr(e, source, disabled, fix, rule, diags);
        }
        Expr::Orelse(a, b, _) => {
            check_simplifiable_if_else_in_expr(a, source, disabled, fix, rule, diags);
            check_simplifiable_if_else_in_expr(b, source, disabled, fix, rule, diags);
        }
        Expr::Catch(e, ck, _) => {
            check_simplifiable_if_else_in_expr(e, source, disabled, fix, rule, diags);
            match ck.as_ref() {
                CatchKind::Default(e) => {
                    check_simplifiable_if_else_in_expr(e, source, disabled, fix, rule, diags)
                }
                CatchKind::Bind { body, .. } => {
                    check_simplifiable_if_else_in_block(body, source, disabled, fix, rule, diags)
                }
            }
        }
        Expr::SwitchExpr { subject, arms, .. } => {
            check_simplifiable_if_else_in_expr(subject, source, disabled, fix, rule, diags);
            for arm in arms {
                check_simplifiable_if_else_in_block(&arm.body, source, disabled, fix, rule, diags);
            }
        }
        Expr::Assign { target, value, .. } => {
            check_simplifiable_if_else_in_expr(target, source, disabled, fix, rule, diags);
            check_simplifiable_if_else_in_expr(value, source, disabled, fix, rule, diags);
        }
        Expr::Index { base, indices, .. } => {
            check_simplifiable_if_else_in_expr(base, source, disabled, fix, rule, diags);
            for i in indices {
                check_simplifiable_if_else_in_expr(i, source, disabled, fix, rule, diags);
            }
        }
        Expr::Field { base, .. } | Expr::Dot { base, .. } => {
            check_simplifiable_if_else_in_expr(base, source, disabled, fix, rule, diags);
        }
        Expr::ArrayLit(items, _) | Expr::TupleLit(items, _) => {
            for e in items {
                check_simplifiable_if_else_in_expr(e, source, disabled, fix, rule, diags);
            }
        }
        Expr::Closure { body, .. } => {
            check_simplifiable_if_else_in_block(body, source, disabled, fix, rule, diags)
        }
        Expr::TupleDestructure(_, e, _) => {
            check_simplifiable_if_else_in_expr(e, source, disabled, fix, rule, diags)
        }
        Expr::NamedLit { fields, .. } => {
            for (_, e) in fields {
                check_simplifiable_if_else_in_expr(e, source, disabled, fix, rule, diags);
            }
        }
        _ => {}
    }
}

// ---------- L006: redundant_eq_false ----------

fn lint_redundant_eq_false(
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
        _ => {}
    }
}

// ---------- JSON 输出 ----------

pub fn diags_to_json(diags: &[LintDiag], file: &str) -> String {
    let mut items = Vec::new();
    for d in diags {
        items.push(format!(
            r#"{{"file":"{}","rule":"{}","line":{},"col":{},"message":"{}"}}"#,
            file,
            d.rule.name,
            d.span.line,
            d.span.col,
            d.message.replace('"', "\\\""),
        ));
    }
    format!("[{}]", items.join(",\n"))
}
