//! Parser 基础工具：token 游标 / 期望 / 同步。

use super::*;
use crate::diag::Diagnostic;
use crate::token::{Span, Token, TokenKind};

impl Parser {
    pub(crate) fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }
    pub(crate) fn peek_n(&self, n: usize) -> &TokenKind {
        let i = (self.pos + n).min(self.tokens.len() - 1);
        &self.tokens[i].kind
    }
    pub(crate) fn span(&self) -> Span {
        self.tokens[self.pos].span.clone()
    }
    pub(crate) fn at(&self, k: &TokenKind) -> bool {
        self.peek() == k
    }
    pub(crate) fn advance(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        t
    }
    pub(crate) fn expect(&mut self, k: &TokenKind, what: &str) -> Result<Token, Diagnostic> {
        if self.at(k) {
            Ok(self.advance())
        } else {
            Err(Diagnostic::error(
                self.span(),
                format!("expected {what}, found {}", self.peek().describe()),
            ))
        }
    }

    /// 泛型上下文关闭 `>`：处理嵌套泛型的 `>>`（词法为单个 Shr）分裂为两个 `>`。
    /// 当前消耗第一个 `>`，第二个以 Gt token 插入当前位置供外层泛型关闭。
    pub(crate) fn expect_gt_generic(&mut self) -> Result<(), Diagnostic> {
        match self.peek() {
            TokenKind::Gt => {
                self.advance();
                Ok(())
            }
            TokenKind::Shr => {
                let sp = self.tokens[self.pos].span.clone();
                self.tokens[self.pos].kind = TokenKind::Gt;
                self.advance();
                self.tokens.insert(
                    self.pos,
                    Token {
                        kind: TokenKind::Gt,
                        span: sp,
                    },
                );
                Ok(())
            }
            other => Err(Diagnostic::error(
                self.span(),
                format!(
                    "expected `>` to close generic args, found {}",
                    other.describe()
                ),
            )),
        }
    }
    pub(crate) fn error_at(&self, msg: impl Into<String>) -> Diagnostic {
        Diagnostic::error(self.span(), msg)
    }
    pub(crate) fn synchronize(&mut self) {
        while !self.at(&TokenKind::Eof) {
            if matches!(
                self.peek(),
                TokenKind::KwFn
                    | TokenKind::KwClass
                    | TokenKind::KwEnum
                    | TokenKind::KwUnion
                    | TokenKind::KwInterface
                    | TokenKind::KwNamespace
                    | TokenKind::KwUsing
                    | TokenKind::KwImport
                    | TokenKind::KwGlobal
            ) {
                return;
            }
            self.advance();
        }
    }
}
