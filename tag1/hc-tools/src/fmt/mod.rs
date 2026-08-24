//! hc fmt 命令：代码格式化器（token 级重排 + AST 保真）
//!
//! 定义：枚举：ParenKind, BraceKind, CKind
//! 定义：结构体：Comment, Fmt

pub mod emit;

use hc::lexer;
use hc::token::{Token, TokenKind};

const INDENT: &str = "    ";

/// 格式化源码；lex 失败返回错误信息。输出换行风格跟随源码（CRLF 源 → CRLF 输出）。
pub fn format_source(source: &str) -> Result<String, String> {
    let tokens = lexer::lex(source);
    let mut f = Fmt {
        src: source,
        tokens,
        out: String::new(),
        line_started: false,
        indent: 0,
        prev: None,
        prev_nospace: false,
        last_end: 0,
        cur_idx: 0,
        parens: Vec::new(),
        braces: Vec::new(),
        brackets: Vec::new(),
        bracket_multiline: Vec::new(),
        paren_multiline: Vec::new(),
        brace_multiline: Vec::new(),
        empty_brace: Vec::new(),
        last_word_after_dot: false,
        last_paren: None,
        last_bracket_type: false,
        in_capture: false,
        in_switch_arm: false,
        in_chain: false,
        cur_line: 1,
        brace_paren_depth: Vec::new(),
        brace_bracket_depth: Vec::new(),
        force_block: false,
        type_decl_pending: false,
        switch_pending: false,
        fn_pending: false,
        last_amp_unary: false,
        last_star_unary: false,
        comments: Vec::new(),
        ci: 0,
    };
    let out = f.run()?;
    Ok(if source.contains("\r\n") {
        out.replace('\n', "\r\n")
    } else {
        out
    })
}

/// 提取 token 序列（忽略注释/空白），用于 CLI 的 AST 保真自检。
pub fn token_signature(source: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for t in lexer::lex(source) {
        if matches!(t.kind, TokenKind::Eof) {
            break;
        }
        if let TokenKind::Error(m) = &t.kind {
            return Err(format!("lex error: {m}"));
        }
        out.push(t.text(source));
    }
    Ok(out)
}

/// token 原文（span 切片）——对全部 token 类型均覆盖源码字面量（含引号/后缀）。
trait TokenText {
    fn text(&self, src: &str) -> String;
}
impl TokenText for Token {
    fn text(&self, src: &str) -> String {
        src[self.span.start..self.span.end].to_string()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ParenKind {
    Control(&'static str),
    FnParams,
    Step,
    Call,
    Group,
}

#[derive(Clone, Copy, PartialEq)]
enum BraceKind {
    Block,
    TypeDecl,
    Switch,
    Literal,
    Import,
}

#[derive(Clone, Copy, PartialEq)]
enum CKind {
    Standalone,
    Inline,
    Trailing,
}

#[derive(Clone)]
struct Comment {
    start: usize,
    end: usize,
    text: String,
    kind: CKind,
    /// 注释所在源码行（1 基）——行尾注释挂载须与最后 token 同行。
    line: usize,
}

struct Fmt<'a> {
    src: &'a str,
    tokens: Vec<Token>,
    out: String,
    line_started: bool,
    indent: usize,
    prev: Option<TokenKind>,
    /// 上一个 token 后不应有空格（`(`/`[`/`.`/一元前缀/捕获开 `|` 等）。
    prev_nospace: bool,
    last_end: usize,
    cur_idx: usize,
    parens: Vec<ParenKind>,
    braces: Vec<BraceKind>,
    /// 每个 `[` 是否为数组类型括号。
    brackets: Vec<bool>,
    /// 每个 `[` 是否跨多行（源内从 `[` 到匹配 `]` 含换行）——多行数组保留垂直布局。
    bracket_multiline: Vec<bool>,
    /// 每个 `(` 是否跨多行——多行调用/参数/条件括号保留垂直布局。
    paren_multiline: Vec<bool>,
    /// 每个 `{` 是否跨多行（多行 struct 字面量保留字段垂直布局）。
    brace_multiline: Vec<bool>,
    /// 每个 `{` 是否空块（紧跟 `}`）——保持 `{}` 行内，不展开。
    empty_brace: Vec<bool>,
    /// 上一个词 token 是否为成员名（紧随 `.`/`.*`）——关键字（如方法名 `where`）作标识符用。
    last_word_after_dot: bool,
    last_paren: Option<ParenKind>,
    last_bracket_type: bool,
    in_capture: bool,
    in_switch_arm: bool,
    /// 方法链垂直延续中（`.concat(...)` 跨行）——延续行缩进 +1，语句/表达式结束恢复。
    in_chain: bool,
    /// 最后发射 token 所在源码行（1 基）。
    cur_line: usize,
    /// 每个花括号打开时的括号/方括号深度——switch/类型声明逗号规则须用相对深度。
    brace_paren_depth: Vec<usize>,
    brace_bracket_depth: Vec<usize>,
    /// 下一个 `{` 强制为语句块（控制头 / fn 体 / else / => / comptime）。
    force_block: bool,
    /// 下一个 `{` 为类型声明体（class/enum/union/interface/tree/namespace）。
    type_decl_pending: bool,
    /// 下一个 `{` 为 switch 体。
    switch_pending: bool,
    /// `fn` 后待解析的参数 `(`。
    fn_pending: bool,
    last_amp_unary: bool,
    last_star_unary: bool,
    comments: Vec<Comment>,
    ci: usize,
}

/// 统计 `\n` 数量（用于空行检测；兼容 CRLF）。
fn count_nl(s: &str) -> usize {
    s.bytes().filter(|&b| b == b'\n').count()
}

fn is_operand(prev: &Option<TokenKind>) -> bool {
    prev.as_ref().is_some_and(is_operand_kind)
}

fn is_operand_kind(k: &TokenKind) -> bool {
    matches!(
        k,
        TokenKind::Ident(_)
            | TokenKind::Int(_)
            | TokenKind::Float(_)
            | TokenKind::Str(_)
            | TokenKind::RawStr(_)
            | TokenKind::Char(_)
            | TokenKind::AtBuiltin(_)
            | TokenKind::Underscore
            | TokenKind::RBrace
            | TokenKind::RParen
            | TokenKind::RBracket
            | TokenKind::DotStar
            | TokenKind::KwVoid
            | TokenKind::KwNull
            | TokenKind::KwTrue
            | TokenKind::KwFalse
    )
}

fn is_binary_op(k: &TokenKind) -> bool {
    matches!(
        k,
        TokenKind::Eq
            | TokenKind::EqEq
            | TokenKind::Ne
            | TokenKind::Lt
            | TokenKind::Le
            | TokenKind::Gt
            | TokenKind::Ge
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Slash
            | TokenKind::Percent
            | TokenKind::PercentPercent
            | TokenKind::PlusEq
            | TokenKind::MinusEq
            | TokenKind::StarEq
            | TokenKind::SlashEq
            | TokenKind::CaretEq
            | TokenKind::PipeEq
            | TokenKind::AmpEq
            | TokenKind::Amp
            | TokenKind::Pipe
            | TokenKind::Caret
            | TokenKind::Shl
            | TokenKind::Shr
            | TokenKind::PipePipe
    )
}
