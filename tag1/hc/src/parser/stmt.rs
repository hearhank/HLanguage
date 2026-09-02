//! 语句解析：变量声明、if、while、for、switch、return、break、continue、defer 等语句

use super::*;
use crate::ast::*;
use crate::diag::Diagnostic;
use crate::token::TokenKind;

impl Parser {
    pub(crate) fn parse_block(&mut self) -> Result<Block, Diagnostic> {
        let start = self.span();
        self.expect(&TokenKind::LBrace, "`{`")?;
        let mut stmts = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            stmts.push(self.parse_stmt()?);
        }
        self.expect(&TokenKind::RBrace, "`}`")?;
        let end = self.span();
        Ok(Block {
            stmts,
            span: start.merge(&end),
        })
    }

    pub(crate) fn parse_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        // 循环标签：`:label while (...)` / `:label for (...)`（break :label 的对称形式）
        let label = if self.at(&TokenKind::Colon) {
            let l = self.label_if_ident();
            if !matches!(self.peek(), TokenKind::KwWhile | TokenKind::KwFor) {
                return Err(self.error_at("`循环标签`后需跟 `while` 或 `for`"));
            }
            l
        } else {
            None
        };
        let start = self.span();
        match self.peek().clone() {
            TokenKind::LBrace => Ok(Stmt::Block(self.parse_block()?)),
            TokenKind::Semi => {
                self.advance();
                Ok(Stmt::Empty)
            }
            TokenKind::KwVar => {
                self.advance();
                self.parse_var_decl(start)
            }
            TokenKind::KwConst => {
                self.advance();
                let name = self.expect_ident()?;
                self.expect(&TokenKind::Eq, "`=` in const declaration")?;
                let init = self.parse_expr()?;
                self.expect(&TokenKind::Semi, "`;` after const")?;
                Ok(Stmt::ConstDecl {
                    name,
                    init,
                    span: start,
                })
            }
            TokenKind::KwIf => self.parse_if_stmt(),
            TokenKind::KwWhile => self.parse_while_stmt(label),
            TokenKind::KwFor => self.parse_for_stmt(label),
            TokenKind::KwSwitch => {
                let sw = self.parse_switch()?;
                Ok(Stmt::Switch(sw))
            }
            TokenKind::KwReturn => {
                self.advance();
                let e = if self.at(&TokenKind::Semi) {
                    None
                } else {
                    // 组 D：类型函数体 `return [n]T;` —— `[` 按数组类型值表达式解析
                    Some(if self.in_type_fn && self.at(&TokenKind::LBracket) {
                        self.parse_type_value_expr()?
                    } else {
                        self.parse_expr()?
                    })
                };
                let end = self.span();
                self.expect(&TokenKind::Semi, "`;` after return")?;
                Ok(Stmt::Return(e, start.merge(&end)))
            }
            TokenKind::KwBreak => {
                self.advance();
                let label = self.label_if_ident();
                let end = self.span();
                self.expect(&TokenKind::Semi, "`;` after break")?;
                Ok(Stmt::Break(label, start.merge(&end)))
            }
            TokenKind::KwContinue => {
                self.advance();
                let label = self.label_if_ident();
                let end = self.span();
                self.expect(&TokenKind::Semi, "`;` after continue")?;
                Ok(Stmt::Continue(label, start.merge(&end)))
            }
            TokenKind::KwDefer => {
                self.advance();
                let e = self.parse_expr()?;
                let end = self.span();
                self.expect(&TokenKind::Semi, "`;` after defer")?;
                Ok(Stmt::Defer(e, start.merge(&end)))
            }
            TokenKind::KwErrdefer => {
                self.advance();
                let e = self.parse_expr()?;
                let end = self.span();
                self.expect(&TokenKind::Semi, "`;` after errdefer")?;
                Ok(Stmt::Errdefer(e, start.merge(&end)))
            }
            _ => {
                let e = self.parse_assign_or_expr()?;
                // 赋值表达式（已在 parse_assign_or_expr 中构造 Assign）
                self.expect(&TokenKind::Semi, "`;` after expression")?;
                Ok(Stmt::Expr(e))
            }
        }
    }

    pub(crate) fn label_if_ident(&mut self) -> Option<String> {
        if let TokenKind::Colon = self.peek() {
            // break :label 形式——lexer 已将 : 与标识符分离
            self.advance();
            if let TokenKind::Ident(s) = self.peek() {
                let s = s.clone();
                self.advance();
                return Some(s);
            }
        }
        None
    }

    pub(crate) fn parse_var_decl(&mut self, start: Span) -> Result<Stmt, Diagnostic> {
        let mut_ = if self.at(&TokenKind::KwMut) {
            self.advance();
            true
        } else {
            false
        };
        // 元组解构：var (a, b) = f();（D6：不支持 mut——出现即报诊断，backlog #1）
        if self.at(&TokenKind::LParen) {
            if mut_ {
                return Err(self.error_at("解构声明不支持 `mut`（D6：元组命名、元组只读）"));
            }
            self.advance();
            let mut names = Vec::new();
            loop {
                if self.at(&TokenKind::Underscore) {
                    self.advance();
                    names.push("_".to_string());
                } else {
                    names.push(self.expect_ident()?);
                }
                if self.at(&TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(&TokenKind::RParen, "`)` to close destructure")?;
            self.expect(&TokenKind::Eq, "`=` in destructure")?;
            let init = self.parse_expr()?;
            self.expect(&TokenKind::Semi, "`;` after destructure")?;
            let end = self.span();
            return Ok(Stmt::Expr(Expr::TupleDestructure(
                names,
                Box::new(init),
                start.merge(&end),
            )));
        }
        let name = self.expect_ident()?;
        let ty = if self.at(&TokenKind::Colon) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };
        let init = if self.at(&TokenKind::Eq) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect(&TokenKind::Semi, "`;` after variable declaration")?;
        Ok(Stmt::VarDecl {
            name,
            mut_,
            ty,
            init,
            span: start,
        })
    }

    pub(crate) fn parse_if_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        self.advance(); // if
        self.expect(&TokenKind::LParen, "`(` after if")?;
        let cond = self.parse_expr()?;
        self.expect(&TokenKind::RParen, "`)` after if condition")?;
        let capture = if self.at(&TokenKind::Pipe) {
            let c = self.parse_capture()?;
            Some(c)
        } else {
            None
        };
        let then_b = self.parse_body_or_stmt()?;
        let mut err_capture = None;
        let else_b = if self.at(&TokenKind::KwElse) {
            self.advance();
            // 错误捕获：else |err| { ... }
            if self.at(&TokenKind::Pipe) {
                err_capture = Some(self.parse_capture()?);
            }
            if self.at(&TokenKind::KwIf) {
                let inner = self.parse_if_stmt()?;
                Some(Box::new(Stmt::Block(inner_as_block(inner))))
            } else {
                Some(Box::new(Stmt::Block(self.parse_body_or_stmt()?)))
            }
        } else {
            None
        };
        let bspan = then_b.span.clone();
        Ok(Stmt::If(IfStmt {
            cond,
            capture,
            err_capture,
            then_b,
            else_b,
            span: bspan,
        }))
    }

    /// 块或单语句（if/while/for 体允许单语句）
    pub(crate) fn parse_body_or_stmt(&mut self) -> Result<Block, Diagnostic> {
        if self.at(&TokenKind::LBrace) {
            self.parse_block()
        } else {
            let start = self.span();
            let s = self.parse_stmt()?;
            let end = self.span();
            Ok(Block {
                stmts: vec![s],
                span: start.merge(&end),
            })
        }
    }

    pub(crate) fn parse_while_stmt(&mut self, label: Option<String>) -> Result<Stmt, Diagnostic> {
        self.advance(); // while
        self.expect(&TokenKind::LParen, "`(` after while")?;
        let cond = self.parse_expr()?;
        self.expect(&TokenKind::RParen, "`)` after while condition")?;
        // optional 捕获：while (maybe) |v| { ... }（step 前后均可，对齐 Zig）
        let mut capture = None;
        if self.at(&TokenKind::Pipe) {
            capture = Some(self.parse_capture()?);
        }
        let step = if self.at(&TokenKind::Colon) {
            self.advance();
            self.expect(&TokenKind::LParen, "`(` in continue step")?;
            let e = self.parse_assign_or_expr()?;
            self.expect(&TokenKind::RParen, "`)` in continue step")?;
            Some(e)
        } else {
            None
        };
        if capture.is_none() && self.at(&TokenKind::Pipe) {
            capture = Some(self.parse_capture()?);
        }
        let body = self.parse_body_or_stmt()?;
        let bspan = body.span.clone();
        Ok(Stmt::While(WhileStmt {
            label,
            cond,
            capture,
            step,
            body,
            span: bspan,
        }))
    }

    pub(crate) fn parse_for_stmt(&mut self, label: Option<String>) -> Result<Stmt, Diagnostic> {
        self.advance(); // for
        self.expect(&TokenKind::LParen, "`(` after for")?;
        let iter = self.parse_expr()?;
        self.expect(&TokenKind::RParen, "`)` after for iterable")?;
        let (mode, name) = self.parse_capture()?;
        let body = self.parse_body_or_stmt()?;
        let bspan = body.span.clone();
        Ok(Stmt::For(ForStmt {
            label,
            iter,
            capture: mode,
            capture_name: name,
            body,
            span: bspan,
        }))
    }

    /// 解析表达式，若遇到复合赋值运算符则构造 Assign（continue step / 语句场景）
    pub(crate) fn parse_assign_or_expr(&mut self) -> Result<Expr, Diagnostic> {
        let e = self.parse_expr()?;
        let aop = match self.peek() {
            TokenKind::Eq => Some(AssignOp::Set),
            TokenKind::PlusEq => Some(AssignOp::Add),
            TokenKind::MinusEq => Some(AssignOp::Sub),
            TokenKind::StarEq => Some(AssignOp::Mul),
            TokenKind::SlashEq => Some(AssignOp::Div),
            TokenKind::PipeEq => Some(AssignOp::BitOr),
            TokenKind::AmpEq => Some(AssignOp::BitAnd),
            TokenKind::CaretEq => Some(AssignOp::BitXor),
            _ => None,
        };
        if let Some(op) = aop {
            self.advance();
            let rhs = self.parse_expr()?;
            let span = e.span().merge(&rhs.span());
            let target = match &e {
                Expr::Ident(_, _)
                | Expr::Index { .. }
                | Expr::Field { .. }
                | Expr::Dot { .. }
                | Expr::Deref(_, _) => e.clone(),
                _ => return Err(Diagnostic::error(e.span(), "invalid assignment target")),
            };
            return Ok(Expr::Assign {
                target: Box::new(target),
                op,
                value: Box::new(rhs),
                span,
            });
        }
        Ok(e)
    }

    /// switch 臂内的语句：return/break/continue 后不强制分号（臂以 `,` 分隔）
    pub(crate) fn parse_switch_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.span();
        match self.peek() {
            TokenKind::KwReturn => {
                self.advance();
                let e = if matches!(
                    self.peek(),
                    TokenKind::Comma | TokenKind::RBrace | TokenKind::Eof
                ) {
                    None
                } else {
                    // 组 D：类型函数体内 switch 臂 return `[n]T;` 同样走数组类型值
                    Some(if self.in_type_fn && self.at(&TokenKind::LBracket) {
                        self.parse_type_value_expr()?
                    } else {
                        self.parse_expr()?
                    })
                };
                let end = self.span();
                if self.at(&TokenKind::Semi) {
                    self.advance();
                }
                Ok(Stmt::Return(e, start.merge(&end)))
            }
            TokenKind::KwBreak => {
                self.advance();
                if self.at(&TokenKind::Semi) {
                    self.advance();
                }
                Ok(Stmt::Break(None, start))
            }
            TokenKind::KwContinue => {
                self.advance();
                if self.at(&TokenKind::Semi) {
                    self.advance();
                }
                Ok(Stmt::Continue(None, start))
            }
            _ => self.parse_stmt(),
        }
    }

    pub(crate) fn parse_capture(&mut self) -> Result<(CaptureMode, String), Diagnostic> {
        self.expect(&TokenKind::Pipe, "`|` to open capture")?;
        let mode = if self.at(&TokenKind::KwMut) {
            self.advance();
            CaptureMode::Mut
        } else if self.at(&TokenKind::KwMove) {
            self.advance();
            CaptureMode::Move
        } else {
            CaptureMode::Read
        };
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Pipe, "`|` to close capture")?;
        Ok((mode, name))
    }

    pub(crate) fn parse_switch(&mut self) -> Result<SwitchStmt, Diagnostic> {
        self.advance(); // switch
        self.expect(&TokenKind::LParen, "`(` after switch")?;
        let subject = self.parse_expr()?;
        self.expect(&TokenKind::RParen, "`)` after switch subject")?;
        self.expect(&TokenKind::LBrace, "`{` to open switch")?;
        let mut arms = Vec::new();
        let mut has_else = false;
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            let astart = self.span();
            let mut patterns = Vec::new();
            loop {
                patterns.push(self.parse_switch_pattern()?);
                if self.at(&TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            if matches!(patterns.last(), Some(SwitchPattern::Else)) {
                has_else = true;
            }
            // C3：switch 守卫——`pattern if guard => expr`
            let guard = if self.at(&TokenKind::KwIf) {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            self.expect(&TokenKind::FatArrow, "`=>` in switch arm")?;
            let capture = if self.at(&TokenKind::Pipe) {
                let (m, n) = self.parse_capture()?;
                Some((m, n))
            } else {
                None
            };
            // 臂体可以是块、表达式或语句（`=> return error.NotFound`）
            let body = if self.at(&TokenKind::LBrace) {
                self.parse_block()?
            } else if matches!(
                self.peek(),
                TokenKind::KwReturn
                    | TokenKind::KwBreak
                    | TokenKind::KwContinue
                    | TokenKind::KwVar
                    | TokenKind::KwConst
            ) {
                let start = self.span();
                let s = self.parse_switch_stmt()?;
                let end = self.span();
                Block {
                    stmts: vec![s],
                    span: start.merge(&end),
                }
            } else {
                let e = self.parse_expr()?;
                let span = e.span();
                Block {
                    stmts: vec![Stmt::Expr(e)],
                    span,
                }
            };
            let end = self.span();
            arms.push(SwitchArm {
                patterns,
                guard,
                capture,
                body,
                span: astart.merge(&end),
            });
            if self.at(&TokenKind::Comma) {
                self.advance();
            }
        }
        self.expect(&TokenKind::RBrace, "`}` to close switch")?;
        let end = self.span();
        Ok(SwitchStmt {
            subject,
            arms,
            has_else,
            span: end,
        })
    }

    pub(crate) fn parse_switch_pattern(&mut self) -> Result<SwitchPattern, Diagnostic> {
        let start = self.span();
        // error.NotFound
        if self.is_ident("error") && self.peek_n(1) == &TokenKind::Dot {
            self.advance();
            self.advance();
            let name = self.expect_ident()?;
            return Ok(SwitchPattern::Error(name));
        }
        // 枚举限定模式 Direction.north / Type.variant（变体名可为关键字，如 JsonValue.null）
        if let TokenKind::Ident(_) = self.peek() {
            if self.peek_n(1) == &TokenKind::Dot {
                let _ty = self.expect_ident()?;
                self.advance(); // .
                let variant = self.expect_name_or_keyword()?;
                return Ok(SwitchPattern::Ident(variant));
            }
        }
        match self.peek().clone() {
            TokenKind::KwElse => {
                self.advance();
                Ok(SwitchPattern::Else)
            }
            TokenKind::KwNull => {
                self.advance();
                Ok(SwitchPattern::Ident("null".into()))
            }
            TokenKind::KwTrue => {
                self.advance();
                Ok(SwitchPattern::Ident("true".into()))
            }
            TokenKind::KwFalse => {
                self.advance();
                Ok(SwitchPattern::Ident("false".into()))
            }
            TokenKind::Int(s) => {
                self.advance();
                Ok(SwitchPattern::Int(s))
            }
            TokenKind::Float(s) => {
                self.advance();
                Ok(SwitchPattern::Float(s))
            }
            TokenKind::Str(s) => {
                self.advance();
                Ok(SwitchPattern::Str(s))
            }
            TokenKind::Char(c) => {
                self.advance();
                Ok(SwitchPattern::Char(c))
            }
            TokenKind::Ident(s) => {
                self.advance();
                Ok(SwitchPattern::Ident(s))
            }
            other => Err(Diagnostic::error(
                start,
                format!("invalid switch pattern: {}", other.describe()),
            )),
        }
    }
}
