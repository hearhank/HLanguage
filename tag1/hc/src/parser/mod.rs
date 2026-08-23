//! Parser（M1.2）：token 流 → AST
//!
//! 递归下降 + 运算符优先级表（Q4 定案）：
//! 后缀 > 前缀/一元 > `*` `/` `%` `%%` > `+` `-` > `<<` `>>` > `&` > `^` > `|` > 比较（非结合）> and/or

mod decl;
mod expr;
mod stmt;
mod r#type;
mod type_decl;
mod util;

use crate::ast::*;
use crate::diag::Diagnostic;
use crate::token::{Span, Token, TokenKind};
use crate::trait_registry::TraitRegistry;

use decl::register_system_trait_handlers;

pub type ParseResult<T> = Result<T, Vec<Diagnostic>>;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    diags: Vec<Diagnostic>,
    /// 当前函数返回 `type`（类型函数，组 D）：其体部 `return [n]T;` 的 `[` 按
    /// 数组类型值表达式解析（`Expr::ArrayType`），而非数组字面量。
    in_type_fn: bool,
    /// 特性注册表（Q24）：字典式查找特性名称 → 处理器
    trait_registry: TraitRegistry,
}

impl Parser {
    pub fn new(_source: &str, tokens: Vec<Token>) -> Self {
        let mut trait_registry = TraitRegistry::new();
        register_system_trait_handlers(&mut trait_registry);
        Self {
            tokens,
            pos: 0,
            diags: Vec::new(),
            in_type_fn: false,
            trait_registry,
        }
    }

    pub fn parse_program(mut self) -> ParseResult<Program> {
        let mut decls = Vec::new();
        while !self.at(&TokenKind::Eof) {
            match self.parse_decl() {
                Ok(d) => decls.push(d),
                Err(e) => {
                    self.diags.push(e);
                    self.synchronize();
                }
            }
        }
        if self.diags.is_empty() {
            Ok(Program { decls })
        } else {
            Err(self.diags)
        }
    }

    // ---------- 类型 ----------

    pub fn parse_type(&mut self) -> Result<Type, Diagnostic> {
        // owned T（所有权形态）
        if self.at(&TokenKind::KwOwned) {
            self.advance();
            let inner = self.parse_type()?;
            return Ok(Type::Owned(Box::new(inner)));
        }
        // *T / *mut T
        if self.at(&TokenKind::Star) {
            self.advance();
            let mut_ = if self.at(&TokenKind::KwMut) {
                self.advance();
                true
            } else {
                false
            };
            let inner = self.parse_type()?;
            return Ok(Type::Ptr(Box::new(inner), mut_));
        }
        // &[T] / &mut [T] 或 &T（引用/切片；tag1：&Vec(X)、&i32 等引用形态）
        if self.at(&TokenKind::Amp) {
            self.advance();
            let mut_ = if self.at(&TokenKind::KwMut) {
                self.advance();
                true
            } else {
                false
            };
            // 切片 &[T]
            if self.at(&TokenKind::LBracket) {
                self.advance();
                let inner = self.parse_type()?;
                self.expect(&TokenKind::RBracket, "`]` in slice type")?;
                return Ok(Type::Slice(Box::new(inner), mut_));
            }
            // 引用类型 &T（Vec 等）
            let inner = self.parse_type()?;
            return Ok(Type::Slice(Box::new(inner), mut_));
        }
        // ?T
        if self.at(&TokenKind::Question) {
            self.advance();
            let inner = self.parse_type()?;
            return Ok(Type::Optional(Box::new(inner)));
        }
        // !T（错误联合 anyerror!T）
        if self.at(&TokenKind::Bang) {
            self.advance();
            let t = self.parse_type()?;
            return Ok(Type::ErrorUnion(None, Box::new(t)));
        }
        // E!T（命名错误集）
        let base = self.parse_type_base()?;
        if self.at(&TokenKind::Bang) {
            self.advance();
            let t = self.parse_type()?;
            return Ok(Type::ErrorUnion(Some(Box::new(base)), Box::new(t)));
        }
        Ok(base)
    }

    // ---------- 表达式 ----------

    pub fn parse_expr(&mut self) -> Result<Expr, Diagnostic> {
        self.parse_or()
    }
}

fn inner_as_block(s: Stmt) -> Block {
    match s {
        Stmt::Block(b) => b,
        other => Block {
            stmts: vec![other],
            span: Span::new(0, 0, 0, 0),
        },
    }
}

// 兼容测试用
impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::IntLit { span, .. }
            | Expr::FloatLit { span, .. }
            | Expr::StrLit { span, .. }
            | Expr::CharLit(_, span)
            | Expr::BoolLit(_, span)
            | Expr::NullLit(span)
            | Expr::VoidLit(span)
            | Expr::Ident(_, span)
            | Expr::ArrayLit(_, span)
            | Expr::TupleLit(_, span)
            | Expr::NamedLit { span, .. }
            | Expr::StructType { span, .. }
            | Expr::ArrayType { span, .. }
            | Expr::Dot { span, .. }
            | Expr::Field { span, .. }
            | Expr::Index { span, .. }
            | Expr::Deref(_, span)
            | Expr::AddrOf(_, _, span)
            | Expr::Unary(_, _, span)
            | Expr::Binary(_, _, _, span)
            | Expr::Orelse(_, _, span)
            | Expr::Unwrap(_, span)
            | Expr::Try(_, span)
            | Expr::Await(_, span)
            | Expr::Catch(_, _, span)
            | Expr::Call { span, .. }
            | Expr::IfExpr { span, .. }
            | Expr::SwitchExpr { span, .. }
            | Expr::Block(_, span)
            | Expr::Assign { span, .. }
            | Expr::ErrorLit(_, span)
            | Expr::FnRef(_, span)
            | Expr::TupleDestructure(_, _, span)
            | Expr::Move(_, span)
            | Expr::Closure { span, .. } => span.clone(),
        }
    }
}
