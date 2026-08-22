//! Token 定义（M1.1 Lexer 产物）

/// 源码位置（行/列 1 基；字节偏移用于切片）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: u32,
    pub col: u32,
}

impl Span {
    pub fn new(start: usize, end: usize, line: u32, col: u32) -> Self {
        Self {
            start,
            end,
            line,
            col,
        }
    }
    pub fn merge(&self, other: &Span) -> Span {
        Span {
            start: self.start,
            end: other.end,
            line: self.line,
            col: self.col,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // 字面量
    Ident(String),
    Int(String), // 含进制前缀与后缀的原文；宽度在 parser 中定型
    Float(String),
    Str(String),    // 已解码（去引号、处理转义）
    RawStr(String), // """...""" 已去包裹
    Char(u8),

    // 关键字
    KwVar,
    KwConst,
    KwFn,
    KwGlobal,
    KwIf,
    KwElse,
    KwWhile,
    KwFor,
    KwBreak,
    KwContinue,
    KwReturn,
    KwSwitch,
    KwDefer,
    KwErrdefer,
    KwClass,
    KwEnum,
    KwUnion,
    KwTree,
    KwInterface,
    KwWhere,
    KwNamespace,
    KwUsing,
    KwImport,
    KwPub,
    KwExport,
    KwO,
    KwMove,
    KwMut,
    KwAnd,
    KwOr,
    KwTry,
    KwCatch,
    KwOrelse,
    KwScript,
    KwComptime,
    KwAnytype,
    KwType,
    KwAsync,
    KwAwait,
    KwSpawn,
    KwExtern,
    KwVoid,
    KwNull,
    KwTrue,
    KwFalse,

    // 标点 / 运算符
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Semi,
    Comma,
    Dot,
    Colon,
    FatArrow,
    Eq,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    PercentPercent,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    CaretEq,
    PipeEq,
    AmpEq,
    Amp,
    Pipe,
    Caret,
    Tilde,
    Shl,
    Shr,
    Bang,
    Question,
    DotDot,
    DotStar,
    PipePipe, // || 错误集联合
    Underscore,

    // @ 内建函数（M4.3 子集）
    AtBuiltin(String),

    Eof,
    Error(&'static str),
}

impl TokenKind {
    pub fn describe(&self) -> String {
        match self {
            TokenKind::Ident(s) => format!("identifier `{s}`"),
            TokenKind::Int(s) => format!("integer literal `{s}`"),
            TokenKind::Float(s) => format!("float literal `{s}`"),
            TokenKind::Str(_) => "string literal".into(),
            TokenKind::RawStr(_) => "raw string literal".into(),
            TokenKind::Char(_) => "char literal".into(),
            TokenKind::Eof => "end of file".into(),
            TokenKind::Error(m) => format!("error token ({m})"),
            other => format!("`{}`", other.punct()),
        }
    }
    pub fn punct(&self) -> &'static str {
        match self {
            TokenKind::LBrace => "{",
            TokenKind::RBrace => "}",
            TokenKind::LParen => "(",
            TokenKind::RParen => ")",
            TokenKind::LBracket => "[",
            TokenKind::RBracket => "]",
            TokenKind::Semi => ";",
            TokenKind::Comma => ",",
            TokenKind::Dot => ".",
            TokenKind::Colon => ":",
            TokenKind::FatArrow => "=>",
            TokenKind::Eq => "=",
            TokenKind::EqEq => "==",
            TokenKind::Ne => "!=",
            TokenKind::Lt => "<",
            TokenKind::Le => "<=",
            TokenKind::Gt => ">",
            TokenKind::Ge => ">=",
            TokenKind::Plus => "+",
            TokenKind::Minus => "-",
            TokenKind::Star => "*",
            TokenKind::Slash => "/",
            TokenKind::Percent => "%",
            TokenKind::PercentPercent => "%%",
            TokenKind::PlusEq => "+=",
            TokenKind::MinusEq => "-=",
            TokenKind::StarEq => "*=",
            TokenKind::SlashEq => "/=",
            TokenKind::CaretEq => "^=",
            TokenKind::Amp => "&",
            TokenKind::Pipe => "|",
            TokenKind::Caret => "^",
            TokenKind::Tilde => "~",
            TokenKind::Shl => "<<",
            TokenKind::Shr => ">>",
            TokenKind::Bang => "!",
            TokenKind::Question => "?",
            TokenKind::DotDot => "..",
            TokenKind::DotStar => ".*",
            TokenKind::PipePipe => "||",
            TokenKind::Underscore => "_",
            _ => "keyword",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}
