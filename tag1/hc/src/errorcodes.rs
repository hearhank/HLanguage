//! 错误码表（M2.6）：编译器维护「错误名 ↔ 码」全局唯一映射
//!
//! - 错误 = 全局唯一整数错误码（Q13 错误名全局唯一；跨包统一）
//! - 编码 = 「包 ID + 包内码」（L5 定案）：高位 16 位 = 编译单元包 ID，
//!   低位 16 位 = 包内错误序——静态链接与动态库/插件场景均无冲突
//! - 每个错误记录**首次出现位置**（span）——错误报告以原始错误的位置和
//!   路径为前提定位（不输出完整调用链；Release 根作用域记录输出后 panic 式中止）
//! - 运行时整数表示（码 + 成功标记）归 M4.2，本模块为编译期表

use crate::ast::*;
use crate::token::Span;
use std::collections::HashMap;

/// 包内码位数（低位）
pub const CODE_BITS: u32 = 16;
/// 包内码掩码
pub const CODE_MASK: u32 = (1 << CODE_BITS) - 1;

/// 单个错误条目（名 / 码 / 首次出现位置）
#[derive(Debug, Clone)]
pub struct ErrorEntry {
    pub name: String,
    pub code: u32,
    /// 首次出现位置（错误集声明成员 / error.X 字面量 / switch 模式）
    pub span: Span,
}

/// 错误码表（名 ↔ 码 双向 + 位置）
#[derive(Debug, Clone, Default)]
pub struct ErrorCodeTable {
    by_name: HashMap<String, ErrorEntry>,
    /// 码 → 错误名（反向；索引 = 包内码）
    by_code: Vec<String>,
    package_id: u16,
}

impl ErrorCodeTable {
    pub fn new(package_id: u16) -> Self {
        ErrorCodeTable {
            by_name: HashMap::new(),
            by_code: Vec::new(),
            package_id,
        }
    }

    /// 编码：高位 16 位 = 包 ID，低位 16 位 = 包内错误序
    pub fn encode(package_id: u16, index: u16) -> u32 {
        ((package_id as u32) << CODE_BITS) | (index as u32)
    }

    /// 解码：包 ID（高位）
    pub fn package_of(code: u32) -> u16 {
        (code >> CODE_BITS) as u16
    }

    /// 解码：包内码（低位）
    pub fn index_of(code: u32) -> u16 {
        (code & CODE_MASK) as u16
    }

    /// 当前包 ID
    pub fn package_id(&self) -> u16 {
        self.package_id
    }

    /// 注册错误名（同名复用已有码——全局唯一）；返回错误码
    pub fn register(&mut self, name: &str, span: &Span) -> u32 {
        if let Some(e) = self.by_name.get(name) {
            return e.code;
        }
        let index = self.by_code.len() as u16;
        let code = Self::encode(self.package_id, index);
        self.by_name.insert(
            name.to_string(),
            ErrorEntry {
                name: name.to_string(),
                code,
                span: span.clone(),
            },
        );
        self.by_code.push(name.to_string());
        code
    }

    /// 错误名 → 码
    pub fn code_of(&self, name: &str) -> Option<u32> {
        self.by_name.get(name).map(|e| e.code)
    }

    /// 码 → 错误名
    pub fn name_of(&self, code: u32) -> Option<&str> {
        if Self::package_of(code) != self.package_id {
            return None;
        }
        self.by_code
            .get(Self::index_of(code) as usize)
            .map(|s| s.as_str())
    }

    /// 错误名 → 首次出现位置
    pub fn span_of(&self, name: &str) -> Option<&Span> {
        self.by_name.get(name).map(|e| &e.span)
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// 按包内码序遍历（声明序）
    pub fn entries(&self) -> impl Iterator<Item = &ErrorEntry> {
        self.by_code.iter().filter_map(|n| self.by_name.get(n))
    }
}

/// 从 AST 收集全部错误名（声明序；包 ID 由调用方指定）
pub fn collect(program: &Program, package_id: u16) -> ErrorCodeTable {
    let mut table = ErrorCodeTable::new(package_id);
    for d in &program.decls {
        collect_decl(d, &mut table);
    }
    table
}

fn collect_decl(d: &Decl, table: &mut ErrorCodeTable) {
    match d {
        Decl::Const { name, ty, span, .. } => {
            // 错误集别名成员：const FileError = error{ NotFound, ... }
            if let Some(Type::Named(tn, _)) = ty {
                if let Some(rest) = tn.strip_prefix("error_set:") {
                    for member in rest.split(',') {
                        table.register(member.trim(), span);
                    }
                }
            }
            let _ = name;
        }
        Decl::Fn { body, .. } => collect_block(body, table),
        Decl::Global { init, .. } => {
            if let Some(e) = init {
                collect_expr(e, table);
            }
        }
        Decl::Class { methods, .. } => {
            for m in methods {
                collect_block(&m.body, table);
            }
        }
        Decl::Namespace { decls, .. } => {
            for inner in decls {
                collect_decl(inner, table);
            }
        }
        Decl::Script { body, .. } => {
            // script 块 = 第三块 E1（tag1 不执行）——不收集
            let _ = body;
        }
        _ => {}
    }
}

fn collect_block(b: &Block, table: &mut ErrorCodeTable) {
    for s in &b.stmts {
        collect_stmt(s, table);
    }
}

fn collect_stmt(s: &Stmt, table: &mut ErrorCodeTable) {
    match s {
        Stmt::VarDecl { init, .. } => {
            if let Some(e) = init {
                collect_expr(e, table);
            }
        }
        Stmt::ConstDecl { init, .. } => collect_expr(init, table),
        Stmt::Expr(e) => collect_expr(e, table),
        Stmt::If(ifs) => {
            collect_expr(&ifs.cond, table);
            collect_block(&ifs.then_b, table);
            if let Some(else_b) = &ifs.else_b {
                collect_stmt(else_b, table);
            }
        }
        Stmt::While(w) => {
            collect_expr(&w.cond, table);
            if let Some(step) = &w.step {
                collect_expr(step, table);
            }
            collect_block(&w.body, table);
        }
        Stmt::For(f) => {
            collect_expr(&f.iter, table);
            collect_block(&f.body, table);
        }
        Stmt::Switch(sw) => {
            collect_expr(&sw.subject, table);
            for arm in &sw.arms {
                for p in &arm.patterns {
                    if let SwitchPattern::Error(name) = p {
                        table.register(name, &arm.span);
                    }
                }
                collect_block(&arm.body, table);
            }
        }
        Stmt::Return(e, _) => {
            if let Some(e) = e {
                collect_expr(e, table);
            }
        }
        Stmt::Defer(e, _) | Stmt::Errdefer(e, _) => collect_expr(e, table),
        Stmt::Block(b) => collect_block(b, table),
        _ => {}
    }
}

fn collect_expr(e: &Expr, table: &mut ErrorCodeTable) {
    match e {
        Expr::ErrorLit(name, span) => {
            table.register(name, span);
        }
        Expr::ArrayLit(items, _) | Expr::TupleLit(items, _) => {
            for it in items {
                collect_expr(it, table);
            }
        }
        Expr::NamedLit { fields, .. } => {
            for (_, v) in fields {
                collect_expr(v, table);
            }
        }
        Expr::Dot { base, .. } | Expr::Field { base, .. } | Expr::Deref(base, _) => {
            collect_expr(base, table);
        }
        Expr::Index { base, indices, .. } => {
            collect_expr(base, table);
            for i in indices {
                collect_expr(i, table);
            }
        }
        Expr::AddrOf(inner, _, _)
        | Expr::Unary(_, inner, _)
        | Expr::Unwrap(inner, _)
        | Expr::Try(inner, _)
        | Expr::Orelse(inner, _, _) => {
            collect_expr(inner, table);
        }
        Expr::Binary(_, l, r, _) => {
            collect_expr(l, table);
            collect_expr(r, table);
        }
        Expr::Catch(inner, kind, _) => {
            collect_expr(inner, table);
            if let CatchKind::Default(d) = kind.as_ref() {
                collect_expr(d, table);
            } else if let CatchKind::Bind { body, .. } = kind.as_ref() {
                collect_block(body, table);
            }
        }
        Expr::Call { callee, args, .. } => {
            collect_expr(callee, table);
            for a in args {
                collect_expr(a, table);
            }
        }
        Expr::IfExpr {
            cond,
            then_e,
            else_e,
            ..
        } => {
            collect_expr(cond, table);
            collect_expr(then_e, table);
            collect_expr(else_e, table);
        }
        Expr::SwitchExpr { subject, arms, .. } => {
            collect_expr(subject, table);
            for arm in arms {
                for p in &arm.patterns {
                    if let SwitchPattern::Error(name) = p {
                        table.register(name, &arm.span);
                    }
                }
                collect_block(&arm.body, table);
            }
        }
        Expr::Block(b, _) => collect_block(b, table),
        Expr::Assign { target, value, .. } => {
            collect_expr(target, table);
            collect_expr(value, table);
        }
        Expr::TupleDestructure(_, value, _) => collect_expr(value, table),
        Expr::Closure { body, .. } => collect_block(body, table),
        _ => {}
    }
}
