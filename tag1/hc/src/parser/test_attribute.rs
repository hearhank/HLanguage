//! 测试属性扩展：`[test]` / `[test("name")]` / `[test(async)]` / `[test(thread)]` / `[test(timeout=N)]` 的解析与构建
//!
//! 通过 `TestExt` 扩展 trait 为 `Parser` 添加测试属性解析方法。
//! 定义：结构体：Test

use crate::ast::*;
use crate::diag::Diagnostic;
use crate::lexer::token::TokenKind;
use crate::parser::Parser;

/// 测试属性数据模型
#[derive(Debug, Clone, Default)]
pub struct Test {
    /// 测试显示名称
    pub name: Option<String>,
    /// 测试执行模式（Serial / Async / Thread）
    pub mode: TestMode,
    /// 超时秒数
    pub timeout: Option<u64>,
}

/// Parser 扩展 trait：为 `Parser` 添加测试属性解析方法
pub trait TestExt {
    /// 解析 `[test(...)]` 括号风格语法
    ///
    /// 支持形式：
    /// - `[test]` — 默认串行
    /// - `[test("名称")]` — 带显示名
    /// - `[test(async)]` — 异步模式
    /// - `[test(thread)]` — OS 线程模式
    /// - `[test(timeout=5)]` — 超时秒数
    /// - `[test("名称", async, timeout=5)]` — 组合
    fn parse_test_attr(&mut self) -> Result<Trait, Diagnostic>;

    /// 从 struct 字面量构建 `[test{name="foo", mode=async, timeout=5}]`
    fn build_test_from_attr(&self, fields: Vec<(String, Expr)>) -> Result<Trait, Diagnostic>;
}

impl TestExt for Parser {
    fn parse_test_attr(&mut self) -> Result<Trait, Diagnostic> {
        let mut attr = Test::default();

        if self.at(&TokenKind::LParen) {
            self.advance();
            if !self.at(&TokenKind::RParen) {
                let tok = self.peek().clone();
                match tok {
                    TokenKind::Str(s) => {
                        self.advance();
                        attr.name = Some(s);
                    }
                    TokenKind::KwAsync => {
                        self.advance();
                        attr.mode = TestMode::Async;
                    }
                    TokenKind::Ident(ref id) if id == "thread" => {
                        self.advance();
                        attr.mode = TestMode::Thread;
                    }
                    TokenKind::Ident(ref id) if id == "timeout" => {
                        self.advance();
                        self.expect(&TokenKind::Eq, "`=` after timeout")?;
                        attr.timeout = self.parse_timeout_value()?;
                    }
                    _ => return Err(self.error_at("expected test name, mode, or timeout")),
                }
                while self.at(&TokenKind::Comma) {
                    self.advance();
                    let tok = self.peek().clone();
                    match tok {
                        TokenKind::KwAsync => {
                            self.advance();
                            attr.mode = TestMode::Async;
                        }
                        TokenKind::Ident(ref id) if id == "thread" => {
                            self.advance();
                            attr.mode = TestMode::Thread;
                        }
                        TokenKind::Ident(ref id) if id == "timeout" => {
                            self.advance();
                            self.expect(&TokenKind::Eq, "`=` after timeout")?;
                            attr.timeout = self.parse_timeout_value()?;
                        }
                        _ => return Err(self.error_at("expected async, thread, or timeout=N")),
                    }
                }
            }
            self.expect(&TokenKind::RParen, "`)")?;
        }

        Ok(attr.into_trait())
    }

    fn build_test_from_attr(&self, fields: Vec<(String, Expr)>) -> Result<Trait, Diagnostic> {
        let mut attr = Test::default();

        for (fname, fval) in &fields {
            match fname.as_str() {
                "name" => {
                    if let Expr::StrLit { value, .. } = fval {
                        attr.name = Some(value.clone());
                    } else {
                        return Err(self.error_at("test.name must be a string literal"));
                    }
                }
                "mode" => match fval {
                    Expr::Ident(s, _) if s == "async" => attr.mode = TestMode::Async,
                    Expr::Ident(s, _) if s == "thread" => attr.mode = TestMode::Thread,
                    _ => return Err(self.error_at("test.mode must be `async` or `thread`")),
                },
                "timeout" => {
                    if let Expr::IntLit { text, .. } = fval {
                        let n = text
                            .trim_end_matches(|c: char| c.is_alphabetic())
                            .replace('_', "")
                            .parse::<u64>()
                            .map_err(|_| {
                                self.error_at(format!("invalid timeout value `{text}`"))
                            })?;
                        attr.timeout = Some(n);
                    } else {
                        return Err(self.error_at("test.timeout must be an integer"));
                    }
                }
                _ => {
                    return Err(self.error_at(format!("unknown field `{fname}` in test attribute")));
                }
            }
        }

        Ok(attr.into_trait())
    }
}

impl Test {
    /// 转换为 `Trait::Test` 枚举值
    fn into_trait(self) -> Trait {
        Trait::Test {
            name: self.name,
            mode: self.mode,
            timeout: self.timeout,
        }
    }
}

// Helper: 解析超时值（供 Parser 内部使用）
impl Parser {
    fn parse_timeout_value(&mut self) -> Result<Option<u64>, Diagnostic> {
        if let TokenKind::Int(n) = self.peek().clone() {
            self.advance();
            let value = n
                .trim_end_matches(|c: char| c.is_alphabetic())
                .replace('_', "")
                .parse::<u64>()
                .map_err(|_| self.error_at(format!("invalid timeout value `{n}`")))?;
            Ok(Some(value))
        } else {
            Err(self.error_at("expected integer timeout value"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::Parser;

    fn parse_test_attr(source: &str) -> Result<Trait, Diagnostic> {
        let tokens = lex(source);
        let mut parser = Parser::new(source, tokens);
        parser.advance(); // skip `[`
        parser.advance(); // skip `test`
        parser.parse_test_attr()
    }

    #[test]
    fn test_bare_test() {
        let tr = parse_test_attr("[test]").unwrap();
        match tr {
            Trait::Test {
                name,
                mode,
                timeout,
            } => {
                assert!(name.is_none());
                assert_eq!(mode, TestMode::Serial);
                assert!(timeout.is_none());
            }
            _ => panic!("expected Trait::Test"),
        }
    }

    #[test]
    fn test_test_with_name() {
        let tr = parse_test_attr(r#"[test("my_test")]"#).unwrap();
        match tr {
            Trait::Test {
                name,
                mode,
                timeout,
            } => {
                assert_eq!(name.as_deref(), Some("my_test"));
                assert_eq!(mode, TestMode::Serial);
                assert!(timeout.is_none());
            }
            _ => panic!("expected Trait::Test"),
        }
    }

    #[test]
    fn test_test_async() {
        let tr = parse_test_attr("[test(async)]").unwrap();
        match tr {
            Trait::Test {
                name,
                mode,
                timeout,
            } => {
                assert!(name.is_none());
                assert_eq!(mode, TestMode::Async);
                assert!(timeout.is_none());
            }
            _ => panic!("expected Trait::Test"),
        }
    }

    #[test]
    fn test_test_thread() {
        let tr = parse_test_attr("[test(thread)]").unwrap();
        match tr {
            Trait::Test { mode, .. } => {
                assert_eq!(mode, TestMode::Thread);
            }
            _ => panic!("expected Trait::Test"),
        }
    }

    #[test]
    fn test_test_timeout() {
        let tr = parse_test_attr("[test(timeout=5)]").unwrap();
        match tr {
            Trait::Test { timeout, .. } => {
                assert_eq!(timeout, Some(5));
            }
            _ => panic!("expected Trait::Test"),
        }
    }

    #[test]
    fn test_test_combined() {
        let tr = parse_test_attr(r#"[test("my_test", async, timeout=10)]"#).unwrap();
        match tr {
            Trait::Test {
                name,
                mode,
                timeout,
            } => {
                assert_eq!(name.as_deref(), Some("my_test"));
                assert_eq!(mode, TestMode::Async);
                assert_eq!(timeout, Some(10));
            }
            _ => panic!("expected Trait::Test"),
        }
    }
}
