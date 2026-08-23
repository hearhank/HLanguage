//! Lexer（M1.1）：源码 → token 流
//!
//! 支持：关键字全集（tag1 子集）、`@` 内建前缀、运算符全集、字符/字符串
//! （含 `"""` 原始字符串）、数字字面量（0x/0b/0o + 惰性宽度后缀 + `_` 分隔）、
//! 注释 `//` `///` `/* */`。全 token 带位置。

use crate::token::{Span, Token, TokenKind};

pub fn lex(source: &str) -> Vec<Token> {
    let mut lx = Lexer {
        src: source,
        pos: 0,
        line: 1,
        col: 1,
        tokens: Vec::new(),
    };
    lx.run();
    lx.tokens
}

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    line: u32,
    col: u32,
    tokens: Vec<Token>,
}

impl<'a> Lexer<'a> {
    fn run(&mut self) {
        loop {
            self.skip_ws_and_comments();
            let start = self.pos;
            let Some(c) = self.peek() else {
                self.push(TokenKind::Eof, start);
                return;
            };
            let kind = match c {
                'a'..='z' | 'A'..='Z' | '_' => self.lex_ident_or_keyword(),
                '0'..='9' => self.lex_number(),
                '"' => self.lex_string(),
                '\'' => self.lex_char(),
                '@' => {
                    self.bump();
                    let name = self.lex_ident_text();
                    TokenKind::AtBuiltin(name)
                }
                _ => self.lex_punct(),
            };
            self.push(kind, start);
        }
    }

    fn push(&mut self, kind: TokenKind, start: usize) {
        let span = Span::new(start, self.pos, self.line, self.col);
        self.tokens.push(Token { kind, span });
    }

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }
    fn peek2(&self) -> Option<char> {
        let mut it = self.src[self.pos..].chars();
        it.next();
        it.next()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.bump();
                }
                Some('/') if self.peek2() == Some('/') => {
                    // 行注释（含 /// 文档注释，语法上等价）
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                Some('/') if self.peek2() == Some('*') => {
                    self.bump();
                    self.bump();
                    loop {
                        match self.peek() {
                            None => {
                                self.tokens.push(Token {
                                    kind: TokenKind::Error("unterminated block comment"),
                                    span: Span::new(self.pos, self.pos, self.line, self.col),
                                });
                                return;
                            }
                            Some('*') if self.peek2() == Some('/') => {
                                self.bump();
                                self.bump();
                                break;
                            }
                            _ => {
                                self.bump();
                            }
                        }
                    }
                }
                _ => return,
            }
        }
    }

    fn lex_ident_or_keyword(&mut self) -> TokenKind {
        let name = self.lex_ident_text();
        match name.as_str() {
            "var" => TokenKind::KwVar,
            "const" => TokenKind::KwConst,
            "fn" => TokenKind::KwFn,
            "global" => TokenKind::KwGlobal,
            "if" => TokenKind::KwIf,
            "else" => TokenKind::KwElse,
            "while" => TokenKind::KwWhile,
            "for" => TokenKind::KwFor,
            "break" => TokenKind::KwBreak,
            "continue" => TokenKind::KwContinue,
            "return" => TokenKind::KwReturn,
            "switch" => TokenKind::KwSwitch,
            "defer" => TokenKind::KwDefer,
            "errdefer" => TokenKind::KwErrdefer,
            "class" => TokenKind::KwClass,
            "struct" => TokenKind::KwClass, // struct/class 合并为一型（H1：特性标注 [continuous] 区分存储形态）
            "enum" => TokenKind::KwEnum,
            "union" => TokenKind::KwUnion, // K1（ADR-0014）：无标签 union——字段内存重叠、无判别标签
            "tree" => TokenKind::KwTree,
            "interface" => TokenKind::KwInterface,
            "where" => TokenKind::KwWhere,
            "namespace" => TokenKind::KwNamespace,
            "using" => TokenKind::KwUsing,
            "import" => TokenKind::KwImport,
            "pub" => TokenKind::KwPub,
            "export" => TokenKind::KwExport,
            "o" | "owned" => TokenKind::KwOwned,
            "move" => TokenKind::KwMove,
            "mut" => TokenKind::KwMut,
            "and" => TokenKind::KwAnd,
            "or" => TokenKind::KwOr,
            "try" => TokenKind::KwTry,
            "catch" => TokenKind::KwCatch,
            "orelse" => TokenKind::KwOrelse,
            "script" => TokenKind::KwScript,
            "comptime" => TokenKind::KwComptime,
            "anytype" => TokenKind::KwAnytype,
            "type" => TokenKind::KwType,
            "async" => TokenKind::KwAsync,
            "await" => TokenKind::KwAwait,
            "spawn" => TokenKind::KwSpawn,
            "extern" => TokenKind::KwExtern,
            "void" => TokenKind::KwVoid,
            "null" => TokenKind::KwNull,
            "true" => TokenKind::KwTrue,
            "false" => TokenKind::KwFalse,
            _ => TokenKind::Ident(name),
        }
    }

    fn lex_ident_text(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                s.push(c);
                self.bump();
            } else {
                break;
            }
        }
        s
    }

    fn lex_number(&mut self) -> TokenKind {
        // 前缀
        if self.peek() == Some('0') {
            match self.peek2() {
                Some('x') | Some('X') => {
                    self.bump();
                    self.bump();
                    let mut s = String::from("0x");
                    while let Some(c) = self.peek() {
                        if c.is_ascii_hexdigit() || c == '_' {
                            s.push(c);
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    self.maybe_suffix(&mut s);
                    return TokenKind::Int(s);
                }
                Some('b') | Some('B') => {
                    self.bump();
                    self.bump();
                    let mut s = String::from("0b");
                    while let Some(c) = self.peek() {
                        if c == '0' || c == '1' || c == '_' {
                            s.push(c);
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    self.maybe_suffix(&mut s);
                    return TokenKind::Int(s);
                }
                Some('o') | Some('O') => {
                    self.bump();
                    self.bump();
                    let mut s = String::from("0o");
                    while let Some(c) = self.peek() {
                        if ('0'..='7').contains(&c) || c == '_' {
                            s.push(c);
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    self.maybe_suffix(&mut s);
                    return TokenKind::Int(s);
                }
                _ => {}
            }
        }

        let mut s = String::new();
        let mut is_float = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '_' {
                s.push(c);
                self.bump();
            } else if c == '.' && self.peek2().map_or(false, |d| d.is_ascii_digit()) {
                is_float = true;
                s.push('.');
                self.bump();
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() || c == '_' {
                        s.push(c);
                        self.bump();
                    } else {
                        break;
                    }
                }
            } else {
                break;
            }
        }
        // 指数
        if let Some(c) = self.peek() {
            if (c == 'e' || c == 'E')
                && self
                    .peek2()
                    .map_or(false, |d| d.is_ascii_digit() || d == '+' || d == '-')
            {
                is_float = true;
                s.push('e');
                self.bump();
                if let Some(c) = self.peek() {
                    if c == '+' || c == '-' {
                        s.push(c);
                        self.bump();
                    }
                }
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() || c == '_' {
                        s.push(c);
                        self.bump();
                    } else {
                        break;
                    }
                }
            }
        }
        if is_float {
            self.maybe_suffix(&mut s);
            TokenKind::Float(s)
        } else {
            self.maybe_suffix(&mut s);
            TokenKind::Int(s)
        }
    }

    /// 惰性宽度后缀：42i32 / 255u8 / -1isize / 3.14f64
    fn maybe_suffix(&mut self, s: &mut String) {
        if let Some(c) = self.peek() {
            if c == 'i' || c == 'u' || c == 'f' {
                // 后随字母数字段（i32/u8/isize/usize/f64）视为后缀
                let rest: Vec<char> = self.src[self.pos..].chars().collect();
                if rest.len() >= 2 && (rest[1].is_ascii_digit() || rest[1].is_alphabetic()) {
                    let suffix: String = rest
                        .iter()
                        .take_while(|ch| ch.is_ascii_digit() || ch.is_alphabetic())
                        .collect();
                    // 仅当整体形如 iN / uN / fN / isize / usize 时消费
                    let valid = (suffix.starts_with(['i', 'u', 'f']) && suffix.len() >= 2)
                        && (suffix[1..]
                            .chars()
                            .next()
                            .map_or(false, |c| c.is_ascii_digit())
                            || suffix == "isize"
                            || suffix == "usize");
                    if valid {
                        for _ in 0..suffix.len() {
                            self.bump();
                        }
                        s.push_str(&suffix);
                    }
                }
            }
        }
    }

    fn lex_string(&mut self) -> TokenKind {
        self.bump(); // 开引号
                     // 原始多行字符串
        if self.peek() == Some('"') && self.peek2() == Some('"') {
            self.bump();
            self.bump();
            let mut s = String::new();
            loop {
                match self.peek() {
                    None => return TokenKind::Error("unterminated raw string"),
                    Some('"')
                        if self.peek2() == Some('"')
                            && self.src[self.pos + 2..].chars().next() == Some('"') =>
                    {
                        self.bump();
                        self.bump();
                        self.bump();
                        return TokenKind::RawStr(s);
                    }
                    Some(c) => {
                        s.push(c);
                        self.bump();
                    }
                }
            }
        }
        let mut s = String::new();
        loop {
            match self.peek() {
                None => return TokenKind::Error("unterminated string literal"),
                Some('"') => {
                    self.bump();
                    return TokenKind::Str(s);
                }
                Some('\\') => {
                    self.bump();
                    match self.bump() {
                        Some('n') => s.push('\n'),
                        Some('r') => s.push('\r'),
                        Some('t') => s.push('\t'),
                        Some('\\') => s.push('\\'),
                        Some('"') => s.push('"'),
                        Some('\'') => s.push('\''),
                        Some('x') => {
                            let hi = self.bump().and_then(|c| c.to_digit(16));
                            let lo = self.bump().and_then(|c| c.to_digit(16));
                            match (hi, lo) {
                                (Some(h), Some(l)) => s.push((h * 16 + l) as u8 as char),
                                _ => return TokenKind::Error("invalid \\x escape"),
                            }
                        }
                        Some('u') => {
                            if self.bump() != Some('{') {
                                return TokenKind::Error("invalid \\u escape");
                            }
                            let mut v: u32 = 0;
                            loop {
                                match self.bump() {
                                    Some('}') => break,
                                    Some(c) => match c.to_digit(16) {
                                        Some(d) => v = v * 16 + d,
                                        None => return TokenKind::Error("invalid \\u escape"),
                                    },
                                    None => return TokenKind::Error("invalid \\u escape"),
                                }
                            }
                            match char::from_u32(v) {
                                Some(ch) => s.push(ch),
                                None => return TokenKind::Error("\\u escape out of range"),
                            }
                        }
                        _ => return TokenKind::Error("invalid escape sequence"),
                    }
                }
                Some(c) => {
                    s.push(c);
                    self.bump();
                }
            }
        }
    }

    fn lex_char(&mut self) -> TokenKind {
        self.bump(); // 开引号
        let byte = match self.peek() {
            None => return TokenKind::Error("unterminated char literal"),
            Some('\\') => {
                self.bump();
                let c = self.bump();
                match c {
                    Some('n') => b'\n',
                    Some('r') => b'\r',
                    Some('t') => b'\t',
                    Some('\\') => b'\\',
                    Some('\'') => b'\'',
                    Some('x') => {
                        let hi = self.bump().and_then(|c| c.to_digit(16));
                        let lo = self.bump().and_then(|c| c.to_digit(16));
                        match (hi, lo) {
                            (Some(h), Some(l)) => (h * 16 + l) as u8,
                            _ => return TokenKind::Error("invalid \\x escape in char"),
                        }
                    }
                    _ => return TokenKind::Error("invalid escape in char literal"),
                }
            }
            Some(c) => {
                if c.len_utf8() > 1 {
                    return TokenKind::Error("char literal must be a single ASCII byte");
                }
                self.bump();
                c as u8
            }
        };
        if self.bump() != Some('\'') {
            return TokenKind::Error("char literal must be closed with '");
        }
        TokenKind::Char(byte)
    }

    fn lex_punct(&mut self) -> TokenKind {
        let c = self.bump().unwrap();
        let _two = |lx: &mut Lexer, a: char, b: char, both: TokenKind, single: TokenKind| {
            if lx.peek() == Some(b) {
                lx.bump();
                both
            } else {
                let _ = a;
                single
            }
        };
        match c {
            '{' => TokenKind::LBrace,
            '}' => TokenKind::RBrace,
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            '[' => TokenKind::LBracket,
            ']' => TokenKind::RBracket,
            ';' => TokenKind::Semi,
            ',' => TokenKind::Comma,
            '.' => {
                if self.peek() == Some('.') {
                    self.bump();
                    TokenKind::DotDot
                } else if self.peek() == Some('*') {
                    self.bump();
                    TokenKind::DotStar
                } else {
                    TokenKind::Dot
                }
            }
            ':' => TokenKind::Colon,
            '=' => {
                if self.peek() == Some('=') {
                    self.bump();
                    TokenKind::EqEq
                } else if self.peek() == Some('>') {
                    self.bump();
                    TokenKind::FatArrow
                } else {
                    TokenKind::Eq
                }
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.bump();
                    TokenKind::Ne
                } else {
                    TokenKind::Bang
                }
            }
            '<' => {
                if self.peek() == Some('=') {
                    self.bump();
                    TokenKind::Le
                } else if self.peek() == Some('<') {
                    self.bump();
                    TokenKind::Shl
                } else {
                    TokenKind::Lt
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.bump();
                    TokenKind::Ge
                } else if self.peek() == Some('>') {
                    self.bump();
                    TokenKind::Shr
                } else {
                    TokenKind::Gt
                }
            }
            '+' => {
                if self.peek() == Some('=') {
                    self.bump();
                    TokenKind::PlusEq
                } else {
                    TokenKind::Plus
                }
            }
            '-' => {
                if self.peek() == Some('=') {
                    self.bump();
                    TokenKind::MinusEq
                } else {
                    TokenKind::Minus
                }
            }
            '*' => {
                if self.peek() == Some('=') {
                    self.bump();
                    TokenKind::StarEq
                } else {
                    TokenKind::Star
                }
            }
            '/' => {
                if self.peek() == Some('=') {
                    self.bump();
                    TokenKind::SlashEq
                } else {
                    TokenKind::Slash
                }
            }
            '%' => {
                if self.peek() == Some('%') {
                    self.bump();
                    TokenKind::PercentPercent
                } else {
                    TokenKind::Percent
                }
            }
            '&' => {
                if self.peek() == Some('&') {
                    self.bump();
                    TokenKind::KwAnd // 兼容 && 写作 and 的别名
                } else if self.peek() == Some('=') {
                    self.bump();
                    TokenKind::AmpEq
                } else {
                    TokenKind::Amp
                }
            }
            '|' => {
                if self.peek() == Some('|') {
                    self.bump();
                    TokenKind::PipePipe
                } else if self.peek() == Some('=') {
                    self.bump();
                    TokenKind::PipeEq
                } else {
                    TokenKind::Pipe
                }
            }
            '^' => {
                if self.peek() == Some('=') {
                    self.bump();
                    TokenKind::CaretEq
                } else {
                    TokenKind::Caret
                }
            }
            '~' => TokenKind::Tilde,
            '?' => TokenKind::Question,
            '_' => TokenKind::Underscore,
            _ => {
                let msg: &'static str = "unexpected character";
                self.tokens.push(Token {
                    kind: TokenKind::Error(msg),
                    span: Span::new(self.pos - c.len_utf8(), self.pos, self.line, self.col),
                });
                TokenKind::Error(msg)
            }
        }
    }
}
