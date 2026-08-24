//! Parser 类型基础解析（命名类型 / 元组 / 数组 / 泛型实例化）。

use super::*;
use crate::ast::Type;
use crate::diag::Diagnostic;
use crate::token::TokenKind;

impl Parser {
    pub(crate) fn parse_type_base(&mut self) -> Result<Type, Diagnostic> {
        // void 关键字
        if self.at(&TokenKind::KwVoid) {
            self.advance();
            return Ok(Type::Named("void".into(), vec![]));
        }
        // anytype：调用点推断（Q-S9）
        if self.at(&TokenKind::KwAnytype) {
            self.advance();
            return Ok(Type::Infer);
        }
        // type（类型即值，E1 完整；tag1 占位）
        if self.at(&TokenKind::KwType) {
            self.advance();
            return Ok(Type::Named("type".into(), vec![]));
        }
        // (T1, T2) 元组
        if self.at(&TokenKind::LParen) {
            self.advance();
            let mut items = vec![self.parse_type()?];
            while self.at(&TokenKind::Comma) {
                self.advance();
                items.push(self.parse_type()?);
            }
            self.expect(&TokenKind::RParen, "`)` to close tuple type")?;
            return Ok(Type::Tuple(items));
        }
        // [N]T 定长数组
        if self.at(&TokenKind::LBracket) {
            self.advance();
            let n = match self.peek() {
                TokenKind::Int(s) => {
                    let n = s
                        .trim_end_matches(|c: char| c.is_alphabetic())
                        .replace('_', "")
                        .parse::<usize>()
                        .map_err(|_| self.error_at(format!("invalid array length `{s}`")))?;
                    self.advance();
                    n
                }
                other => {
                    return Err(Diagnostic::error(
                        self.span(),
                        format!("expected array length, found {}", other.describe()),
                    ))
                }
            };
            self.expect(&TokenKind::RBracket, "`]` in array type")?;
            let inner = self.parse_type()?;
            return Ok(Type::Array(n, Box::new(inner)));
        }
        let mut name = self.expect_ident()?;
        // M1.4 限定类型名：Orders.Line（命名空间限定；双注册后按全名查找）
        while self.at(&TokenKind::Dot) {
            self.advance();
            let part = self.expect_name_or_keyword()?;
            name = format!("{name}.{part}");
        }
        // 泛型实例化 Vec<i32> / IIterable<i32> / Fn1<i32> i32
        let args = if self.at(&TokenKind::Lt) {
            self.advance();
            let mut a = Vec::new();
            if !self.at(&TokenKind::Gt) {
                loop {
                    // 组 D：comptime_int 字面量实参（`ArrayLen<i32, 3>` 的 `3`）——编译期
                    // 整数值，非类型名。实例化时按 `n: comptime_int` 参数绑定。
                    if let TokenKind::Int(text) = self.peek() {
                        let n = text
                            .trim_end_matches(|c: char| c.is_alphabetic())
                            .replace('_', "")
                            .parse::<usize>()
                            .map_err(|_| {
                                self.error_at(format!("invalid comptime_int arg `{text}`"))
                            })?;
                        self.advance();
                        a.push(Type::ComptimeInt(n));
                    } else {
                        a.push(self.parse_type()?);
                    }
                    if self.at(&TokenKind::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            self.expect_gt_generic()?;
            // FnN(参数) 返回类型：Fn1<i32> i32——把返回类型并入参数列表
            if name.starts_with("Fn")
                && name.len() > 2
                && name[2..].chars().all(|c| c.is_ascii_digit())
            {
                if let Ok(rt) = self.parse_type() {
                    a.push(rt);
                }
            }
            a
        } else {
            Vec::new()
        };
        Ok(Type::Named(name, args))
    }
}
