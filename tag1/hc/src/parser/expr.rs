//! 表达式解析：二元/一元/调用/字段/索引/闭包/if/switch 等表达式

use super::*;
use crate::ast::*;
use crate::diag::Diagnostic;
use crate::token::TokenKind;

impl Parser {
    pub(crate) fn parse_or(&mut self) -> Result<Expr, Diagnostic> {
        let mut l = self.parse_and()?;
        while self.at(&TokenKind::KwOr) || self.at(&TokenKind::PipePipe) {
            let op = self.advance();
            let r = self.parse_and()?;
            let span = l.span().merge(&r.span());
            l = Expr::Binary(BinOp::Or, Box::new(l), Box::new(r), span);
            let _ = op;
        }
        Ok(l)
    }

    pub(crate) fn parse_and(&mut self) -> Result<Expr, Diagnostic> {
        let mut l = self.parse_range()?;
        while self.at(&TokenKind::KwAnd) {
            self.advance();
            let r = self.parse_range()?;
            let span = l.span().merge(&r.span());
            l = Expr::Binary(BinOp::And, Box::new(l), Box::new(r), span);
        }
        Ok(l)
    }

    /// 区间糖 0..10（Q29：复用 .. 记号）；无上界 `1..`（切片到末尾）
    pub(crate) fn parse_range(&mut self) -> Result<Expr, Diagnostic> {
        let l = self.parse_comparison()?;
        if self.at(&TokenKind::DotDot) {
            self.advance();
            // 无上界：后随 ]/)/,/; 等边界符
            let open_end = matches!(
                self.peek(),
                TokenKind::RBracket | TokenKind::RParen | TokenKind::Comma | TokenKind::Semi
            );
            if open_end {
                let span = l.span().clone();
                let end_marker = Expr::IntLit {
                    text: "__end__".into(),
                    span: span.clone(),
                };
                return Ok(Expr::Binary(
                    BinOp::Range,
                    Box::new(l),
                    Box::new(end_marker),
                    span,
                ));
            }
            let r = self.parse_comparison()?;
            let span = l.span().merge(&r.span());
            return Ok(Expr::Binary(BinOp::Range, Box::new(l), Box::new(r), span));
        }
        Ok(l)
    }

    pub(crate) fn parse_comparison(&mut self) -> Result<Expr, Diagnostic> {
        let l = self.parse_bitor()?;
        let op = match self.peek() {
            TokenKind::EqEq => Some(BinOp::Eq),
            TokenKind::Ne => Some(BinOp::Ne),
            TokenKind::Lt => Some(BinOp::Lt),
            TokenKind::Le => Some(BinOp::Le),
            TokenKind::Gt => Some(BinOp::Gt),
            TokenKind::Ge => Some(BinOp::Ge),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let r = self.parse_bitor()?;
            let span = l.span().merge(&r.span());
            return Ok(Expr::Binary(op, Box::new(l), Box::new(r), span));
        }
        Ok(l)
    }

    pub(crate) fn parse_bitor(&mut self) -> Result<Expr, Diagnostic> {
        let mut l = self.parse_bitxor()?;
        while self.at(&TokenKind::Pipe) {
            self.advance();
            let r = self.parse_bitxor()?;
            let span = l.span().merge(&r.span());
            l = Expr::Binary(BinOp::BitOr, Box::new(l), Box::new(r), span);
        }
        Ok(l)
    }

    pub(crate) fn parse_bitxor(&mut self) -> Result<Expr, Diagnostic> {
        let mut l = self.parse_bitand()?;
        while self.at(&TokenKind::Caret) {
            self.advance();
            let r = self.parse_bitand()?;
            let span = l.span().merge(&r.span());
            l = Expr::Binary(BinOp::BitXor, Box::new(l), Box::new(r), span);
        }
        Ok(l)
    }

    pub(crate) fn parse_bitand(&mut self) -> Result<Expr, Diagnostic> {
        let mut l = self.parse_shift()?;
        while self.at(&TokenKind::Amp) {
            self.advance();
            let r = self.parse_shift()?;
            let span = l.span().merge(&r.span());
            l = Expr::Binary(BinOp::BitAnd, Box::new(l), Box::new(r), span);
        }
        Ok(l)
    }

    pub(crate) fn parse_shift(&mut self) -> Result<Expr, Diagnostic> {
        let mut l = self.parse_addsub()?;
        loop {
            let op = match self.peek() {
                TokenKind::Shl => BinOp::Shl,
                TokenKind::Shr => BinOp::Shr,
                _ => break,
            };
            self.advance();
            let r = self.parse_addsub()?;
            let span = l.span().merge(&r.span());
            l = Expr::Binary(op, Box::new(l), Box::new(r), span);
        }
        Ok(l)
    }

    pub(crate) fn parse_addsub(&mut self) -> Result<Expr, Diagnostic> {
        let mut l = self.parse_muldiv()?;
        loop {
            let op = match self.peek() {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let r = self.parse_muldiv()?;
            let span = l.span().merge(&r.span());
            l = Expr::Binary(op, Box::new(l), Box::new(r), span);
        }
        Ok(l)
    }

    pub(crate) fn parse_muldiv(&mut self) -> Result<Expr, Diagnostic> {
        let mut l = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Mod,
                TokenKind::PercentPercent => BinOp::EucMod,
                _ => break,
            };
            self.advance();
            let r = self.parse_unary()?;
            let span = l.span().merge(&r.span());
            l = Expr::Binary(op, Box::new(l), Box::new(r), span);
        }
        Ok(l)
    }

    pub(crate) fn parse_unary(&mut self) -> Result<Expr, Diagnostic> {
        let start = self.span();
        match self.peek() {
            TokenKind::Minus => {
                self.advance();
                let e = self.parse_unary()?;
                Ok(Expr::Unary(UnaryOp::Neg, Box::new(e), start))
            }
            TokenKind::Bang => {
                self.advance();
                let e = self.parse_unary()?;
                Ok(Expr::Unary(UnaryOp::Not, Box::new(e), start))
            }
            TokenKind::Tilde => {
                self.advance();
                let e = self.parse_unary()?;
                Ok(Expr::Unary(UnaryOp::BitNot, Box::new(e), start))
            }
            TokenKind::Amp => {
                self.advance();
                let mut_ = if self.at(&TokenKind::KwMut) {
                    self.advance();
                    true
                } else {
                    false
                };
                let e = self.parse_unary()?;
                Ok(Expr::AddrOf(Box::new(e), mut_, start))
            }
            TokenKind::KwTry => {
                self.advance();
                let e = self.parse_unary()?;
                Ok(Expr::Try(Box::new(e), start))
            }
            TokenKind::KwAwait => {
                // 组 E E1：`await expr`——Future(R) 值 → R（协作式 Future，ADR-0011）
                self.advance();
                let e = self.parse_unary()?;
                Ok(Expr::Await(Box::new(e), start))
            }
            TokenKind::KwSpawn => {
                // E2.2：`spawn(f, args...) owned Thread(T)`——以普通调用形态解析
                // （callee = Ident("spawn")），语义层按内建处理（is_builtin_fn "spawn"）。
                // 第三块 E2 的 async/await/spawn 并发语法不在此列，保持 out-of-scope。
                self.advance();
                let args = self.parse_call_args()?;
                let end = self.span();
                Ok(Expr::Call {
                    callee: Box::new(Expr::Ident("spawn".to_string(), start.clone())),
                    args,
                    span: start.merge(&end),
                })
            }
            TokenKind::KwMove => {
                // move |v| ... = move 捕获闭包；否则 move x（所有权转移标记，M2.4——
                // 保留供语义检查器验证唯一约束 = 拥有所有权；原绑定仍可访问，悬垂/冲突由用户负责）
                if self.peek_n(1) == &TokenKind::Pipe
                    || (self.peek_n(1) == &TokenKind::KwMut && self.peek_n(2) == &TokenKind::Pipe)
                {
                    return self.parse_closure(start);
                }
                self.advance();
                let inner = self.parse_unary()?;
                let end = self.span();
                Ok(Expr::Move(Box::new(inner), start.merge(&end)))
            }
            _ => self.parse_postfix(),
        }
    }

    pub(crate) fn parse_postfix(&mut self) -> Result<Expr, Diagnostic> {
        let mut e = self.parse_primary()?;
        loop {
            let start = self.span();
            match self.peek() {
                TokenKind::Dot => {
                    self.advance();
                    // `.?` 链式解包（m.get("apple").?）
                    if self.at(&TokenKind::Question) {
                        self.advance();
                        e = Expr::Unwrap(Box::new(e), start);
                        continue;
                    }
                    let field = self.expect_name_or_keyword()?;
                    // 调用：p.dist(q)
                    if self.at(&TokenKind::LParen) {
                        let args = self.parse_call_args()?;
                        let end = self.span();
                        e = Expr::Call {
                            callee: Box::new(Expr::Field {
                                base: Box::new(e),
                                field,
                                span: start.clone(),
                            }),
                            args,
                            span: start.merge(&end),
                        };
                    } else {
                        e = Expr::Field {
                            base: Box::new(e),
                            field,
                            span: start,
                        };
                    }
                }
                TokenKind::LBracket => {
                    self.advance();
                    let mut indices = vec![self.parse_expr()?];
                    while self.at(&TokenKind::Comma) {
                        self.advance();
                        indices.push(self.parse_expr()?);
                    }
                    self.expect(&TokenKind::RBracket, "`]` after index")?;
                    let end = self.span();
                    e = Expr::Index {
                        base: Box::new(e),
                        indices,
                        span: start.merge(&end),
                    };
                }
                TokenKind::DotStar => {
                    self.advance();
                    e = Expr::Deref(Box::new(e), start);
                }
                TokenKind::Question => {
                    self.advance();
                    e = Expr::Unwrap(Box::new(e), start);
                }
                TokenKind::LParen => {
                    let args = self.parse_call_args()?;
                    let end = self.span();
                    e = Expr::Call {
                        callee: Box::new(e),
                        args,
                        span: start.merge(&end),
                    };
                    // 泛型类型实例化后字面量：Pair<i32>{ first = 1, ... }。
                    // call 实参按类型实参收集（E1.2 组 D comptime 类型应用——不再丢弃）。
                    if self.at(&TokenKind::LBrace) {
                        match &e {
                            Expr::Call { callee, args, .. }
                                if matches!(callee.as_ref(), Expr::Ident(_, _)) =>
                            {
                                if let Expr::Ident(tyname, _) = callee.as_ref() {
                                    let ty_args: Vec<Type> =
                                        args.iter().filter_map(|a| self.expr_to_type(a)).collect();
                                    let fields = self.parse_named_lit_fields()?;
                                    e = Expr::NamedLit {
                                        ty: tyname.clone(),
                                        ty_args,
                                        fields,
                                        span: start,
                                    };
                                }
                            }
                            _ => {
                                let _ = self.parse_named_lit_fields()?;
                            }
                        }
                    }
                }
                TokenKind::KwOrelse => {
                    self.advance();
                    // orelse return/continue/break（tag1：控制流兜底 → Block 表达式；
                    // return 不消费分号——由外层声明消费）
                    if self.at(&TokenKind::KwReturn) {
                        self.advance();
                        let rv = if self.at(&TokenKind::Semi) {
                            None
                        } else {
                            Some(self.parse_expr()?)
                        };
                        let sp = start.clone();
                        let block = Block {
                            stmts: vec![Stmt::Return(rv, sp.clone())],
                            span: sp.clone(),
                        };
                        let end = self.span();
                        e = Expr::Orelse(
                            Box::new(e),
                            Box::new(Expr::Block(block, sp)),
                            start.merge(&end),
                        );
                    } else if self.at(&TokenKind::KwContinue) || self.at(&TokenKind::KwBreak) {
                        let kw = self.advance();
                        let stmt = match kw.kind {
                            TokenKind::KwContinue => Stmt::Continue(None, kw.span.clone()),
                            _ => Stmt::Break(None, kw.span.clone()),
                        };
                        // 不消费分号——由外层声明消费
                        let sp = start.clone();
                        let block = Block {
                            stmts: vec![stmt],
                            span: sp.clone(),
                        };
                        let end = self.span();
                        e = Expr::Orelse(
                            Box::new(e),
                            Box::new(Expr::Block(block, sp)),
                            start.merge(&end),
                        );
                    } else {
                        let r = self.parse_expr()?;
                        let end = self.span();
                        e = Expr::Orelse(Box::new(e), Box::new(r), start.merge(&end));
                    }
                }
                TokenKind::KwCatch => {
                    self.advance();
                    let kind = if self.at(&TokenKind::Pipe) {
                        let (_, name) = self.parse_capture()?;
                        // 体：块或表达式（`catch |err| switch (err) { ... }`）
                        let body = if self.at(&TokenKind::LBrace) {
                            self.parse_block()?
                        } else {
                            let e = self.parse_expr()?;
                            let sp = e.span();
                            Block {
                                stmts: vec![Stmt::Expr(e)],
                                span: sp,
                            }
                        };
                        CatchKind::Bind { name, body }
                    } else if self.at(&TokenKind::KwReturn) {
                        // catch return error.X（不消费分号——外层 return 消费）
                        self.advance();
                        let rv = if self.at(&TokenKind::Semi) {
                            None
                        } else {
                            Some(self.parse_expr()?)
                        };
                        let body = Block {
                            stmts: vec![Stmt::Return(rv, start.clone())],
                            span: start.clone(),
                        };
                        CatchKind::Bind {
                            name: "__catch_flow__".into(),
                            body,
                        }
                    } else if self.at(&TokenKind::KwBreak) || self.at(&TokenKind::KwContinue) {
                        let stmt = self.parse_stmt()?;
                        let body = Block {
                            stmts: vec![stmt],
                            span: start.clone(),
                        };
                        CatchKind::Bind {
                            name: "__catch_flow__".into(),
                            body,
                        }
                    } else {
                        let default = self.parse_expr()?;
                        CatchKind::Default(Box::new(default))
                    };
                    let end = self.span();
                    e = Expr::Catch(Box::new(e), Box::new(kind), start.merge(&end));
                }
                _ => break,
            }
        }
        Ok(e)
    }

    pub(crate) fn parse_call_args(&mut self) -> Result<Vec<Expr>, Diagnostic> {
        self.expect(&TokenKind::LParen, "`(`")?;
        let mut args = Vec::new();
        if !self.at(&TokenKind::RParen) {
            loop {
                args.push(self.parse_expr()?);
                if self.at(&TokenKind::Comma) {
                    self.advance();
                    // 尾随逗号
                    if self.at(&TokenKind::RParen) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RParen, "`)`")?;
        Ok(args)
    }

    /// 组 D：类型值表达式（类型函数体 `return` 用）。`[` → 数组类型 `[n]T`；
    /// 其它形态走普通表达式（`T` 标识符 / `struct { ... }` 类型字面量等）。
    pub(crate) fn parse_type_value_expr(&mut self) -> Result<Expr, Diagnostic> {
        if self.at(&TokenKind::LBracket) {
            return self.parse_array_type_expr();
        }
        self.parse_expr()
    }

    /// 组 D：数组类型值 `[len]elem`（类型函数体 `return [n]T;`）。
    /// `len` = 编译期整数表达式（标识符 `n` / 字面量 `3`）；`elem` = 元素类型值表达式
    /// （标识符 / 嵌套数组 / struct 类型字面量）。
    pub(crate) fn parse_array_type_expr(&mut self) -> Result<Expr, Diagnostic> {
        let start = self.span();
        self.expect(&TokenKind::LBracket, "`[` in array type expression")?;
        let len = self.parse_expr()?;
        self.expect(&TokenKind::RBracket, "`]` in array type expression")?;
        let elem = self.parse_type_value_expr()?;
        let end = self.span();
        Ok(Expr::ArrayType {
            len: Box::new(len),
            elem: Box::new(elem),
            span: start.merge(&end),
        })
    }

    pub(crate) fn parse_primary(&mut self) -> Result<Expr, Diagnostic> {
        let start = self.span();
        // 闭包：|v| expr / |v, w| expr / mut |v| { ... }
        if self.at(&TokenKind::Pipe)
            || (self.at(&TokenKind::KwMut) && self.peek_n(1) == &TokenKind::Pipe)
        {
            return self.parse_closure(start);
        }
        // 推断枚举值字面量（L1）：.shallow ≡ CopyMode.shallow——参数/字段类型已知时
        if self.at(&TokenKind::Dot) {
            self.advance();
            let variant = self.expect_name_or_keyword()?;
            let end = self.span();
            return Ok(Expr::Dot {
                base: Box::new(Expr::VoidLit(start.clone())),
                field: variant,
                span: start.merge(&end),
            });
        }
        // @ 内建表达式：@intFromEnum(k)
        if let TokenKind::AtBuiltin(name) = self.peek().clone() {
            self.advance();
            let args = self.parse_call_args()?;
            let end = self.span();
            return Ok(Expr::Call {
                callee: Box::new(Expr::Ident(format!("@{name}"), start.clone())),
                args,
                span: start.merge(&end),
            });
        }
        match self.peek().clone() {
            // struct { ... } 类型字面量（H1：struct/class 合并；E1.2 组 D type-as-value）。
            // 字段为 `name: Type`（类型标注——保留，构成 `Expr::StructType` 类型值）或
            // `name = expr`（值——tag1 仅解析不执行，NamedLit 占位）。
            TokenKind::KwClass | TokenKind::KwStruct => {
                self.advance();
                self.expect(&TokenKind::LBrace, "`{` after struct literal")?;
                let mut type_fields: Vec<(String, Type)> = Vec::new();
                let mut value_fields: Vec<(String, Expr)> = Vec::new();
                let mut all_typed = true;
                if !self.at(&TokenKind::RBrace) {
                    loop {
                        let name = self.expect_ident()?;
                        if self.at(&TokenKind::Colon) {
                            self.advance();
                            let ty = self.parse_type()?;
                            type_fields.push((name, ty));
                        } else if self.at(&TokenKind::Eq) {
                            self.advance();
                            let v = self.parse_expr()?;
                            value_fields.push((name, v));
                            all_typed = false;
                        } else {
                            return Err(self.error_at(format!(
                                "expected `:` or `=` in struct literal field `{name}`"
                            )));
                        }
                        if self.at(&TokenKind::Comma) {
                            self.advance();
                            if self.at(&TokenKind::RBrace) {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RBrace, "`}` after struct literal")?;
                let end = self.span();
                if all_typed {
                    Ok(Expr::StructType {
                        fields: type_fields,
                        span: start.merge(&end),
                    })
                } else {
                    Ok(Expr::NamedLit {
                        ty: "struct".into(),
                        ty_args: vec![],
                        fields: value_fields,
                        span: start.merge(&end),
                    })
                }
            }
            TokenKind::Int(s) => {
                self.advance();
                Ok(Expr::IntLit {
                    text: s,
                    span: start,
                })
            }
            TokenKind::Float(s) => {
                self.advance();
                Ok(Expr::FloatLit {
                    text: s,
                    span: start,
                })
            }
            TokenKind::Str(s) => {
                self.advance();
                Ok(Expr::StrLit {
                    value: s,
                    raw: false,
                    span: start,
                })
            }
            TokenKind::RawStr(s) => {
                self.advance();
                Ok(Expr::StrLit {
                    value: s,
                    raw: true,
                    span: start,
                })
            }
            TokenKind::Char(c) => {
                self.advance();
                Ok(Expr::CharLit(c, start))
            }
            TokenKind::KwTrue => {
                self.advance();
                Ok(Expr::BoolLit(true, start))
            }
            TokenKind::KwFalse => {
                self.advance();
                Ok(Expr::BoolLit(false, start))
            }
            TokenKind::KwNull => {
                self.advance();
                Ok(Expr::NullLit(start))
            }
            TokenKind::KwVoid => {
                self.advance();
                Ok(Expr::VoidLit(start))
            }
            TokenKind::KwIf => {
                self.advance();
                self.expect(&TokenKind::LParen, "`(` after if")?;
                let cond = self.parse_expr()?;
                self.expect(&TokenKind::RParen, "`)`")?;
                // 捕获形式 if (opt) |v| ...
                let capture = if self.at(&TokenKind::Pipe) {
                    let c = self.parse_capture()?;
                    Some(c)
                } else {
                    None
                };
                let then_e = self.parse_expr()?;
                self.expect(&TokenKind::KwElse, "`else` in if expression")?;
                let else_e = self.parse_expr()?;
                let end = self.span();
                Ok(Expr::IfExpr {
                    cond: Box::new(cond),
                    capture,
                    then_e: Box::new(then_e),
                    else_e: Box::new(else_e),
                    span: start.merge(&end),
                })
            }
            TokenKind::KwSwitch => {
                let sw = self.parse_switch()?;
                Ok(Expr::SwitchExpr {
                    subject: Box::new(sw.subject),
                    arms: sw.arms,
                    span: sw.span,
                })
            }
            TokenKind::LBrace => {
                let b = self.parse_block()?;
                Ok(Expr::Block(b, start))
            }
            TokenKind::LBracket => {
                self.advance();
                let mut items = Vec::new();
                if !self.at(&TokenKind::RBracket) {
                    loop {
                        items.push(self.parse_expr()?);
                        if self.at(&TokenKind::Comma) {
                            self.advance();
                            // 尾逗号：`[a, b, ]`
                            if self.at(&TokenKind::RBracket) {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RBracket, "`]` after array literal")?;
                let end = self.span();
                Ok(Expr::ArrayLit(items, start.merge(&end)))
            }
            TokenKind::LParen => {
                self.advance();
                if self.at(&TokenKind::RParen) {
                    self.advance();
                    return Ok(Expr::TupleLit(vec![], start));
                }
                let first = self.parse_expr()?;
                if self.at(&TokenKind::Comma) {
                    self.advance();
                    let mut items = vec![first];
                    while !self.at(&TokenKind::RParen) && !self.at(&TokenKind::Eof) {
                        items.push(self.parse_expr()?);
                        if self.at(&TokenKind::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RParen, "`)` to close tuple")?;
                    let end = self.span();
                    Ok(Expr::TupleLit(items, start.merge(&end)))
                } else {
                    self.expect(&TokenKind::RParen, "`)`")?;
                    Ok(first) // 括号分组：直接返回内层表达式（短路语义依赖）
                }
            }
            TokenKind::KwTry => {
                self.advance();
                let e = self.parse_expr()?;
                Ok(Expr::Try(Box::new(e), start))
            }
            TokenKind::Ident(name) => {
                self.advance();
                // struct 关键字（已合并入 class，2026-08-14）：匿名类型占位跳过
                if name == "struct" && self.at(&TokenKind::LBrace) {
                    self.skip_anon_struct()?;
                    let end = self.span();
                    return Ok(Expr::VoidLit(start.merge(&end)));
                }
                // error.NotFound 错误字面量
                if name == "error" && self.at(&TokenKind::Dot) {
                    self.advance();
                    let ename = self.expect_ident()?;
                    return Ok(Expr::ErrorLit(ename, start));
                }
                // 集合类型实例化 Vec<i32>/Map<&[u8], i32>/Table<i32>：泛型参数为类型（跳过），
                // 返回 Ident 以便 postfix `.init` 继续（tag1：类型实例化 = 空容器）
                if matches!(
                    name.as_str(),
                    "Vec"
                        | "Map"
                        | "Deque"
                        | "Table"
                        | "List"
                        | "Pipe"
                        | "Tee"
                        | "Funnel"
                        | "Hub"
                        | "Pair"
                        | "PairPair"
                        | "LinkedList"
                        | "Opt"
                ) && self.at(&TokenKind::Lt)
                {
                    self.advance();
                    let mut ty_args: Vec<Type> = Vec::new();
                    if !self.at(&TokenKind::Gt) {
                        loop {
                            ty_args.push(self.parse_type()?);
                            if self.at(&TokenKind::Comma) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect_gt_generic()?;
                    // 泛型类型字面量：Pair<i32>{ first = 1, ... } → NamedLit（ty_args 收集）
                    if self.at(&TokenKind::LBrace) {
                        let fields = self.parse_named_lit_fields()?;
                        let end = self.span();
                        return Ok(Expr::NamedLit {
                            ty: name,
                            ty_args,
                            fields,
                            span: start.merge(&end),
                        });
                    }
                    // 容器字面量：Vec<i32>[1, 2, 3] → ContainerLit（ADR-0027）
                    if self.at(&TokenKind::LBracket) {
                        self.advance();
                        let mut items = Vec::new();
                        if !self.at(&TokenKind::RBracket) {
                            loop {
                                items.push(self.parse_expr()?);
                                if self.at(&TokenKind::Comma) {
                                    self.advance();
                                } else {
                                    break;
                                }
                            }
                        }
                        self.expect(&TokenKind::RBracket, "`]` after container literal")?;
                        let end = self.span();
                        return Ok(Expr::ContainerLit {
                            ty: name,
                            ty_args,
                            items,
                            span: start.merge(&end),
                        });
                    }
                    return Ok(Expr::Ident(name, start));
                }
                // Type.name（枚举常量）——解释器统一处理
                // 字面量构造 Type{...} 或调用 Type(...)
                if self.at(&TokenKind::LBrace) {
                    let fields = self.parse_named_lit_fields()?;
                    let end = self.span();
                    return Ok(Expr::NamedLit {
                        ty: name,
                        ty_args: vec![],
                        fields,
                        span: start.merge(&end),
                    });
                }
                if self.at(&TokenKind::Dot) {
                    self.advance();
                    // `x.?` 链式解包（对齐 parse_postfix 形态）：裸标识符解包
                    if self.at(&TokenKind::Question) {
                        self.advance();
                        let end = self.span();
                        return Ok(Expr::Unwrap(
                            Box::new(Expr::Ident(name, start.clone())),
                            start.merge(&end),
                        ));
                    }
                    let field = self.expect_name_or_keyword()?;
                    let end = self.span();
                    // M1.4 限定名类型字面量：Orders.Line{ ... }
                    if self.at(&TokenKind::LBrace) {
                        let fields = self.parse_named_lit_fields()?;
                        let end = self.span();
                        return Ok(Expr::NamedLit {
                            ty: format!("{name}.{field}"),
                            ty_args: vec![],
                            fields,
                            span: start.merge(&end),
                        });
                    }
                    if self.at(&TokenKind::LParen) {
                        let args = self.parse_call_args()?;
                        let span = start.merge(&end);
                        return Ok(Expr::Call {
                            callee: Box::new(Expr::Dot {
                                base: Box::new(Expr::Ident(name, start.clone())),
                                field,
                                span: start.clone(),
                            }),
                            args,
                            span,
                        });
                    }
                    return Ok(Expr::Dot {
                        base: Box::new(Expr::Ident(name, start.clone())),
                        field,
                        span: start.merge(&end),
                    });
                }
                Ok(Expr::Ident(name, start))
            }
            other => Err(Diagnostic::error(
                start,
                format!("expected expression, found {}", other.describe()),
            )),
        }
    }

    /// 闭包：|v| expr / |v, w| { ... } / mut |v| ... / move |v| ...（Q13：FnN 调用接口）
    pub(crate) fn parse_closure(&mut self, start: Span) -> Result<Expr, Diagnostic> {
        // 捕获模式前缀：mut（可写）/ move（转移捕获）——可叠放，粒度 = 整个闭包
        let mut is_mut = false;
        let mut is_move = false;
        loop {
            if self.at(&TokenKind::KwMut) {
                self.advance();
                is_mut = true;
            } else if self.at(&TokenKind::KwMove) {
                self.advance();
                is_move = true;
            } else {
                break;
            }
        }
        self.expect(&TokenKind::Pipe, "`|` to open closure params")?;
        let mut params = Vec::new();
        if !self.at(&TokenKind::Pipe) {
            loop {
                params.push(self.expect_ident()?);
                if self.at(&TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect(&TokenKind::Pipe, "`|` to close closure params")?;
        let body = if self.at(&TokenKind::LBrace) {
            self.parse_block()?
        } else {
            let e = self.parse_expr()?;
            let sp = e.span();
            Block {
                stmts: vec![Stmt::Expr(e)],
                span: sp,
            }
        };
        let end = self.span();
        Ok(Expr::Closure {
            params,
            body,
            is_mut,
            is_move,
            span: start.merge(&end),
        })
    }

    pub(crate) fn parse_named_lit_fields(&mut self) -> Result<Vec<(String, Expr)>, Diagnostic> {
        self.expect(&TokenKind::LBrace, "`{`")?;
        let mut fields = Vec::new();
        if !self.at(&TokenKind::RBrace) {
            loop {
                let name = self.expect_ident()?;
                self.expect(&TokenKind::Eq, "`=` in literal field")?;
                let value = self.parse_expr()?;
                fields.push((name, value));
                if self.at(&TokenKind::Comma) {
                    self.advance();
                    if self.at(&TokenKind::RBrace) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RBrace, "`}` after literal")?;
        Ok(fields)
    }

    /// 表达式 → 类型实参（E1.2 组 D）：`Pair<i32>{...}` 的 call 实参转为泛型实参。
    /// 支持裸标识符 `i32`、限定名 `Math.Vec`、嵌套泛型 `Vec<i32>`；非类型形态返回 None
    /// （调用方按「无类型实参」处理——tag1 泛型实参必须可解析为类型）。
    pub(crate) fn expr_to_type(&self, e: &Expr) -> Option<Type> {
        match e {
            Expr::Ident(n, _) => Some(Type::Named(n.clone(), vec![])),
            Expr::Dot { base, field, .. } => match self.expr_to_type(base) {
                Some(Type::Named(bn, bargs)) if bargs.is_empty() => {
                    Some(Type::Named(format!("{bn}.{field}"), vec![]))
                }
                _ => None,
            },
            Expr::Call { callee, args, .. } => match callee.as_ref() {
                Expr::Ident(n, _) => {
                    let targs: Option<Vec<Type>> =
                        args.iter().map(|a| self.expr_to_type(a)).collect();
                    targs.map(|t| Type::Named(n.clone(), t))
                }
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn expect_ident(&mut self) -> Result<String, Diagnostic> {
        match self.peek() {
            TokenKind::Ident(s) => {
                let s = s.clone();
                self.advance();
                Ok(s)
            }
            other => Err(Diagnostic::error(
                self.span(),
                format!("expected identifier, found {}", other.describe()),
            )),
        }
    }

    /// 函数/方法名：允许关键字（如方法名 where / 变体名 null）
    pub(crate) fn expect_name_or_keyword(&mut self) -> Result<String, Diagnostic> {
        match self.peek() {
            TokenKind::Ident(s) => {
                let s = s.clone();
                self.advance();
                Ok(s)
            }
            // 方法名可为关键字（tag1：where 等）
            TokenKind::KwWhere => {
                self.advance();
                Ok("where".to_string())
            }
            TokenKind::KwNull => {
                self.advance();
                Ok("null".to_string())
            }
            // M7.2：`Kind.script`（build.zon kind 值）——script 为关键字，作名称放行
            TokenKind::KwScript => {
                self.advance();
                Ok("script".to_string())
            }
            // E1（ADR-0013）：`types.type`（当前类型名元数据）——type 为关键字，点号后作名称放行
            TokenKind::KwType => {
                self.advance();
                Ok("type".to_string())
            }
            other => Err(Diagnostic::error(
                self.span(),
                format!("expected identifier, found {}", other.describe()),
            )),
        }
    }

    pub(crate) fn is_ident(&self, name: &str) -> bool {
        matches!(self.peek(), TokenKind::Ident(s) if s == name)
    }
}
