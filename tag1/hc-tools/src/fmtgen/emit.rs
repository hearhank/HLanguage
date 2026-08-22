//! token 发射器：驱动 `Fmt` 状态机，按 token 种类排版输出。

use super::*;

impl<'a> Fmt<'a> {
    pub(super) fn run(&mut self) -> Result<String, String> {
        self.extract_comments();
        let n = self.tokens.len();
        let mut i = 0;
        while i < n {
            let tok = self.tokens[i].clone();
            if matches!(tok.kind, TokenKind::Eof) {
                break;
            }
            if let TokenKind::Error(m) = &tok.kind {
                return Err(format!("lex error: {m}"));
            }
            let next = self.tokens.get(i + 1).map(|t| t.kind.clone());
            self.cur_idx = i;
            self.emit_gap(tok.span.start);
            let text = tok.text(self.src);
            // 先记录本 token 的结束偏移/行号：emit_token 内 end_line→flush_trailing
            // 需要以本 token（而非前一 token）为基准测量行尾注释前的对齐空白。
            self.last_end = tok.span.end;
            self.cur_line = tok.span.line as usize;
            self.emit_token(&tok.kind, &text, next.as_ref());
            self.prev = Some(tok.kind.clone());
            i += 1;
        }
        self.emit_gap(self.src.len());
        self.end_line();
        Ok(self.out.clone())
    }

    // ---------- 基础发射 ----------

    fn raw(&mut self, s: &str) {
        if !self.line_started {
            for _ in 0..self.indent {
                self.out.push_str(INDENT);
            }
            self.line_started = true;
        }
        self.out.push_str(s);
    }

    fn space(&mut self) {
        if self.line_started && !self.out.ends_with(' ') {
            self.out.push(' ');
        }
    }

    /// 结束当前行；行已空则无操作（避免空块前多空行）。
    fn nl(&mut self) {
        if self.line_started {
            self.out.push('\n');
            self.line_started = false;
        }
    }

    /// 强制换行（即使当前行已空——用于制造空行）。
    fn force_nl(&mut self) {
        self.out.push('\n');
        self.line_started = false;
    }

    /// 行尾：先挂载行尾注释，再换行。
    fn end_line(&mut self) {
        self.flush_trailing();
        self.nl();
    }

    /// 挂载行尾注释：仅消费与最后 token 同行（源码行号）的注释，避免跨行聚合；
    /// 注释前的源码对齐空白（全空格/tab）原样保留。
    fn flush_trailing(&mut self) {
        let mut gap_from = self.last_end;
        loop {
            let c = match self.comments.get(self.ci) {
                Some(c) if c.kind == CKind::Trailing && c.line == self.cur_line => c.clone(),
                _ => break,
            };
            self.emit_comment_gap_from(gap_from, c.start);
            self.raw(&c.text);
            self.ci += 1;
            gap_from = c.end;
        }
    }

    /// 方法链结束：恢复延续缩进。
    fn leave_chain(&mut self) {
        if self.in_chain {
            self.indent = self.indent.saturating_sub(1);
            self.in_chain = false;
        }
    }

    fn emit_punct(&mut self, text: &str, space_before: bool, nospace_after: bool) {
        if space_before {
            self.space();
        }
        self.raw(text);
        self.prev_nospace = nospace_after;
    }

    /// 词 token（标识符/关键字/字面量/`@` 内建）：默认空格分隔，`prev_nospace` 覆盖。
    fn emit_word(&mut self, text: &str) {
        // 紧随 `.`/`.*` 的词 = 成员名：关键字（如方法名 `where`）此刻作标识符用（LParen 作调用接收者）。
        self.last_word_after_dot =
            matches!(self.prev, Some(TokenKind::Dot) | Some(TokenKind::DotStar));
        if !self.prev_nospace {
            self.space();
        }
        self.raw(text);
        self.prev_nospace = false;
    }

    // ---------- 注释与空行 ----------

    fn extract_comments(&mut self) {
        let spans: Vec<(usize, usize)> = self
            .tokens
            .iter()
            .filter(|t| !matches!(t.kind, TokenKind::Eof))
            .map(|t| (t.span.start, t.span.end))
            .collect();
        let mut prev_end = 0;
        for (start, end) in spans {
            self.scan_gap(prev_end, start);
            prev_end = end;
        }
        self.scan_gap(prev_end, self.src.len());
    }

    fn scan_gap(&mut self, start: usize, end: usize) {
        let seg = &self.src[start..end];
        let mut i = 0;
        while i < seg.len() {
            let rest = &seg[i..];
            if rest.starts_with("//") {
                let line_end = rest.find('\n').map(|p| i + p).unwrap_or(seg.len());
                self.push_comment(start + i, start + line_end);
                i = line_end;
            } else if rest.starts_with("/*") {
                match rest.find("*/") {
                    Some(p) => {
                        self.push_comment(start + i, start + i + p + 2);
                        i += p + 2;
                    }
                    None => i = seg.len(),
                }
            } else {
                i += 1;
            }
        }
    }

    fn push_comment(&mut self, cs: usize, ce: usize) {
        let text = self.src[cs..ce].trim_end().to_string();
        let line = self.src[..cs].matches('\n').count() + 1;
        let own_line = {
            let line_start = self.src[..cs].rfind('\n').map(|p| p + 1).unwrap_or(0);
            self.src[line_start..cs]
                .chars()
                .all(|c| c == ' ' || c == '\t')
        };
        let followed_by_code = {
            let line_end = self.src[ce..]
                .find('\n')
                .map(|p| ce + p)
                .unwrap_or(self.src.len());
            self.src[ce..line_end].chars().any(|c| !c.is_whitespace())
        };
        let kind = if followed_by_code {
            CKind::Inline
        } else if own_line {
            CKind::Standalone
        } else {
            CKind::Trailing
        };
        self.comments.push(Comment {
            start: cs,
            end: ce,
            text,
            kind,
            line,
        });
    }

    /// 处理 next token 之前的 gap：空行保留 + 独立/行内注释。
    fn emit_gap(&mut self, offset: usize) {
        let mut seg_start = self.last_end;
        loop {
            let next_ci_start = self
                .comments
                .get(self.ci)
                .map(|c| c.start)
                .unwrap_or(offset);
            if next_ci_start >= offset {
                break;
            }
            if count_nl(&self.src[seg_start..next_ci_start]) >= 2 {
                self.emit_blank();
            }
            let c = self.comments[self.ci].clone();
            match c.kind {
                CKind::Standalone => {
                    self.raw(&c.text);
                    self.nl();
                }
                CKind::Inline => {
                    self.space();
                    self.raw(&c.text);
                    self.space();
                }
                // 行尾注释未被 end_line 消费 → 处于表达式中间：强制行断（`//` 会吃掉后续）
                CKind::Trailing => {
                    self.emit_comment_gap(c.start);
                    self.raw(&c.text);
                    self.nl();
                }
            }
            self.ci += 1;
            seg_start = c.end;
        }
        let nl_count = count_nl(&self.src[seg_start..offset]);
        if nl_count >= 2 {
            self.emit_blank();
        } else if nl_count >= 1
            && ((!self.brackets.is_empty() && *self.bracket_multiline.last().unwrap_or(&false))
                || (!self.parens.is_empty() && *self.paren_multiline.last().unwrap_or(&false))
                || (!self.braces.is_empty() && *self.brace_multiline.last().unwrap_or(&false)))
        {
            // 多行数组字面量 / 多行调用（参数）括号 / 多行 struct 字面量：
            // 元素/实参/字段/闭括号随源码换行（行内逗号仍同行）。
            self.nl();
        }
    }

    fn emit_blank(&mut self) {
        self.nl();
        if !self.out.ends_with("\n\n") {
            self.force_nl();
        }
    }

    /// 行尾注释前的源码间隙空白：全为空格/tab 时原样保留（对齐 padding，注释表逐列对齐一致），否则单空格。
    fn emit_comment_gap(&mut self, cs: usize) {
        self.emit_comment_gap_from(self.last_end, cs);
    }

    fn emit_comment_gap_from(&mut self, from: usize, cs: usize) {
        let gap = &self.src[from..cs];
        if !gap.is_empty() && gap.chars().all(|c| c == ' ' || c == '\t') {
            self.raw(gap);
        } else {
            self.space();
        }
    }

    // ---------- token 发射 ----------

    fn emit_token(&mut self, kind: &TokenKind, text: &str, next: Option<&TokenKind>) {
        match kind {
            TokenKind::Ident(_)
            | TokenKind::Int(_)
            | TokenKind::Float(_)
            | TokenKind::Str(_)
            | TokenKind::RawStr(_)
            | TokenKind::Char(_)
            | TokenKind::AtBuiltin(_)
            | TokenKind::Underscore => self.emit_word(text),

            TokenKind::KwFn => {
                self.emit_word(text);
                self.fn_pending = true;
            }
            TokenKind::KwIf => {
                self.force_block = false;
                self.emit_word(text);
            }
            TokenKind::KwElse => {
                // else 后：`{` / `|err|` → 块体；`if`/表达式 → 不强制块
                if matches!(next, Some(TokenKind::LBrace) | Some(TokenKind::Pipe)) {
                    self.force_block = true;
                }
                self.emit_word(text);
            }
            TokenKind::KwClass
            | TokenKind::KwEnum
            | TokenKind::KwUnion
            | TokenKind::KwTree
            | TokenKind::KwInterface
            | TokenKind::KwNamespace => {
                self.emit_word(text);
                self.type_decl_pending = true;
            }
            TokenKind::KwComptime => {
                self.emit_word(text);
                self.force_block = true;
            }
            TokenKind::KwWhile
            | TokenKind::KwFor
            | TokenKind::KwSwitch
            | TokenKind::KwBreak
            | TokenKind::KwContinue
            | TokenKind::KwImport
            | TokenKind::KwVar
            | TokenKind::KwConst
            | TokenKind::KwGlobal
            | TokenKind::KwReturn
            | TokenKind::KwDefer
            | TokenKind::KwErrdefer
            | TokenKind::KwWhere
            | TokenKind::KwUsing
            | TokenKind::KwPub
            | TokenKind::KwExport
            | TokenKind::KwO
            | TokenKind::KwMove
            | TokenKind::KwMut
            | TokenKind::KwAnd
            | TokenKind::KwOr
            | TokenKind::KwTry
            | TokenKind::KwCatch
            | TokenKind::KwOrelse
            | TokenKind::KwScript
            | TokenKind::KwAnytype
            | TokenKind::KwType
            | TokenKind::KwAsync
            | TokenKind::KwAwait
            | TokenKind::KwSpawn
            | TokenKind::KwVoid
            | TokenKind::KwNull
            | TokenKind::KwTrue
            | TokenKind::KwFalse => self.emit_word(text),

            TokenKind::LParen => {
                // 空格：左操作数（词/`)`/`]`/`}`）→ 调用紧跟无空格；前缀/开括号/`,` → 无空格；
                // 其余（控制关键字/return/var/`=`/`and`/`:` 等）→ 空格。
                // `spawn(f, …)` 为调用形态：关键字 `spawn` 后紧跟 `(` 无空格。
                // 返回类型元组 `fn f() (i32, i32)`：FnParams 闭括号后、强制块前 → 空格 + Group。
                let return_tuple = self.force_block && matches!(self.prev, Some(TokenKind::RParen));
                let space_before = return_tuple
                    || (self.prev.is_some()
                        && !self.prev_nospace
                        && !is_operand(&self.prev)
                        && !self.last_word_after_dot
                        && !self.fn_pending
                        && !matches!(self.prev, Some(TokenKind::KwSpawn)));
                let kind = if matches!(
                    self.prev,
                    Some(TokenKind::KwIf)
                        | Some(TokenKind::KwWhile)
                        | Some(TokenKind::KwFor)
                        | Some(TokenKind::KwSwitch)
                ) {
                    let kw = match &self.prev {
                        Some(TokenKind::KwIf) => "if",
                        Some(TokenKind::KwWhile) => "while",
                        Some(TokenKind::KwFor) => "for",
                        _ => "switch",
                    };
                    ParenKind::Control(kw)
                } else if return_tuple {
                    ParenKind::Group
                } else if self.fn_pending {
                    self.fn_pending = false;
                    ParenKind::FnParams
                } else if matches!(self.prev, Some(TokenKind::Colon)) {
                    ParenKind::Step
                } else if matches!(self.prev, Some(TokenKind::KwSpawn)) {
                    ParenKind::Call
                } else if is_operand(&self.prev) || self.last_word_after_dot {
                    ParenKind::Call
                } else {
                    ParenKind::Group
                };
                self.parens.push(kind);
                let paren_ml = self.paren_is_multiline();
                self.paren_multiline.push(paren_ml);
                if paren_ml {
                    self.indent += 1;
                }
                self.emit_punct("(", space_before, true);
            }
            TokenKind::RParen => {
                self.leave_chain();
                if self.paren_multiline.pop().unwrap_or(false) {
                    self.indent = self.indent.saturating_sub(1);
                }
                let popped = self.parens.pop();
                self.last_paren = popped;
                match &popped {
                    Some(ParenKind::Control(kw)) => match *kw {
                        "switch" => {
                            self.switch_pending = true;
                            self.force_block = true;
                        }
                        "if" => {
                            if matches!(
                                next,
                                Some(TokenKind::LBrace)
                                    | Some(TokenKind::Pipe)
                                    | Some(TokenKind::Colon)
                            ) {
                                self.force_block = true;
                            }
                        }
                        _ => self.force_block = true,
                    },
                    Some(ParenKind::FnParams) => self.force_block = true,
                    _ => {}
                }
                self.emit_punct(")", false, false);
            }
            TokenKind::LBracket => {
                let space_before = match &self.prev {
                    None => false,
                    Some(k) if is_operand_kind(k) => false,
                    Some(TokenKind::RBracket) | Some(TokenKind::RParen) | Some(TokenKind::Dot) => {
                        false
                    }
                    _ => !self.prev_nospace,
                };
                self.emit_punct("[", space_before, true);
                self.brackets.push(self.bracket_is_type());
                // 多行数组字面量：元素垂直排布 + 缩进一级，直至匹配 `]`。
                let ml = self.bracket_is_multiline();
                self.bracket_multiline.push(ml);
                if ml {
                    self.indent += 1;
                }
            }
            TokenKind::RBracket => {
                if self.bracket_multiline.pop().unwrap_or(false) {
                    self.indent = self.indent.saturating_sub(1);
                }
                self.leave_chain();
                self.raw("]");
                self.last_bracket_type = self.brackets.pop().unwrap_or(false);
                // 类型括号 `[5]i32`/`[2][2]i32`/`[n]*mut T`：元素类型紧跟 `]` 无空格；
                // 其余（如 `where` 子句、`;` 等）恢复空格分隔。
                self.prev_nospace = self.last_bracket_type
                    && matches!(
                        next,
                        Some(TokenKind::Ident(_))
                            | Some(TokenKind::LBracket)
                            | Some(TokenKind::Star)
                            | Some(TokenKind::Amp)
                    );
            }
            TokenKind::LBrace => {
                let (kind, space_before) = if self.type_decl_pending {
                    self.type_decl_pending = false;
                    (BraceKind::TypeDecl, true)
                } else if self.switch_pending {
                    self.switch_pending = false;
                    self.force_block = false;
                    (BraceKind::Switch, true)
                } else if self.force_block {
                    self.force_block = false;
                    (BraceKind::Block, true)
                } else if matches!(self.prev, Some(TokenKind::Dot)) {
                    (BraceKind::Import, false)
                } else if is_operand(&self.prev) {
                    (BraceKind::Literal, false)
                } else {
                    (BraceKind::Block, true)
                };
                self.emit_punct(
                    "{",
                    space_before,
                    kind == BraceKind::Import || kind == BraceKind::Literal,
                );
                self.braces.push(kind);
                self.brace_paren_depth.push(self.parens.len());
                self.brace_bracket_depth.push(self.brackets.len());
                let brace_ml = matches!(kind, BraceKind::Literal | BraceKind::Import)
                    && self.brace_is_multiline();
                self.brace_multiline.push(brace_ml);
                // 空块判定：下一 token 即 `}` 且 `{` 到 `}` 之间无注释。仅含注释的块
                // 不折叠（`{` 换行 → 注释独占行 → `}` 独立行），否则 `{}` 保持行内。
                let empty =
                    matches!(next, Some(TokenKind::RBrace)) && !self.cur_brace_has_comment();
                self.empty_brace.push(empty);
                match kind {
                    BraceKind::Block | BraceKind::TypeDecl | BraceKind::Switch => {
                        self.indent += 1;
                        if !empty {
                            self.end_line();
                        }
                    }
                    BraceKind::Literal | BraceKind::Import => {
                        if brace_ml {
                            self.indent += 1;
                        }
                    }
                }
            }
            TokenKind::RBrace => {
                self.leave_chain();
                let kind = self.braces.pop().unwrap_or(BraceKind::Block);
                self.brace_paren_depth.pop();
                self.brace_bracket_depth.pop();
                if self.brace_multiline.pop().unwrap_or(false) {
                    self.indent = self.indent.saturating_sub(1);
                }
                let empty = self.empty_brace.pop().unwrap_or(false);
                match kind {
                    BraceKind::Block | BraceKind::TypeDecl | BraceKind::Switch => {
                        self.indent = self.indent.saturating_sub(1);
                        if !empty {
                            self.end_line();
                        }
                        self.raw("}");
                        self.force_block = false;
                        match next {
                            Some(TokenKind::KwElse)
                            | Some(TokenKind::KwCatch)
                            | Some(TokenKind::KwOrelse)
                            | Some(TokenKind::Comma)
                            | Some(TokenKind::RParen)
                            | Some(TokenKind::RBracket)
                            | Some(TokenKind::Semi)
                            | Some(TokenKind::Dot) => {
                                self.prev_nospace = false;
                            }
                            Some(k) if is_binary_op(k) => {
                                self.prev_nospace = false;
                            }
                            _ => self.end_line(),
                        }
                    }
                    BraceKind::Literal | BraceKind::Import => {
                        self.raw("}");
                        self.prev_nospace = false;
                    }
                }
            }
            TokenKind::Semi => {
                self.raw(";");
                self.force_block = false;
                self.type_decl_pending = false;
                self.leave_chain();
                self.end_line();
                self.prev_nospace = true;
            }
            TokenKind::Comma => {
                self.leave_chain();
                self.raw(",");
                // switch 臂/类型声明字段的逗号：以打开该花括号时的括号/方括号深度为准
                // （相对深度），故 switch 位于调用括号内（如 `expect(switch …)`）也能换行。
                let at_brace_level = self.parens.len()
                    == *self.brace_paren_depth.last().unwrap_or(&0)
                    && self.brackets.len() == *self.brace_bracket_depth.last().unwrap_or(&0);
                if at_brace_level {
                    match self.braces.last() {
                        Some(BraceKind::Switch) if self.in_switch_arm => {
                            self.in_switch_arm = false;
                            self.force_block = false;
                            self.end_line();
                            self.prev_nospace = true;
                            return;
                        }
                        Some(BraceKind::TypeDecl) => {
                            self.end_line();
                            self.prev_nospace = true;
                            return;
                        }
                        _ => {}
                    }
                }
                self.prev_nospace = false;
            }
            TokenKind::Dot | TokenKind::DotStar => {
                // 方法链延续：原源码 `.` 位于下一行时保留垂直排布（延续行缩进 +1）。
                let tok_line = self.tokens[self.cur_idx].span.line as usize;
                let prev_line = self
                    .tokens
                    .get(self.cur_idx.saturating_sub(1))
                    .map(|t| t.span.line as usize)
                    .unwrap_or(tok_line);
                if is_operand(&self.prev) && tok_line > prev_line {
                    if !self.in_chain {
                        self.indent += 1;
                        self.in_chain = true;
                    }
                    self.nl();
                }
                self.raw(text);
                // DotStar（后缀解引用 `b.*`）结果为操作数：后续二元运算符留空格（`b.* + n`）。
                self.prev_nospace = !matches!(kind, TokenKind::DotStar);
            }
            TokenKind::DotDot => {
                self.raw("..");
                self.prev_nospace = true;
            }
            TokenKind::Colon => {
                // while 步进 `: (s)`：`)`/捕获闭 `|` 之后；循环标签 `:label`：语句起始；
                // `break :label`：break/continue 之后；否则为类型标注。
                let is_step = matches!(self.prev, Some(TokenKind::RParen))
                    || matches!(self.prev, Some(TokenKind::Pipe));
                if is_step {
                    self.space();
                    self.raw(":");
                    self.prev_nospace = false;
                } else if matches!(
                    self.prev,
                    None | Some(TokenKind::Semi)
                        | Some(TokenKind::LBrace)
                        | Some(TokenKind::RBrace)
                ) {
                    self.raw(":");
                    self.prev_nospace = true;
                } else if matches!(
                    self.prev,
                    Some(TokenKind::KwBreak) | Some(TokenKind::KwContinue)
                ) {
                    self.space();
                    self.raw(":");
                    self.prev_nospace = true;
                } else {
                    self.raw(":");
                    self.prev_nospace = false;
                }
            }
            TokenKind::FatArrow => {
                self.space();
                self.raw("=>");
                self.force_block = true;
                self.in_switch_arm = true;
                self.prev_nospace = false;
            }
            TokenKind::Pipe => {
                if self.in_capture {
                    self.raw("|");
                    self.in_capture = false;
                    self.prev_nospace = false;
                } else {
                    let open_capture = match &self.prev {
                        Some(TokenKind::RParen) => matches!(
                            self.last_paren,
                            Some(ParenKind::Control(_)) | Some(ParenKind::Step)
                        ),
                        Some(TokenKind::FatArrow)
                        | Some(TokenKind::KwElse)
                        | Some(TokenKind::KwCatch) => true,
                        Some(p) => !is_operand_kind(p),
                        None => true,
                    };
                    if open_capture {
                        // 调用括号内捕获紧跟 `(`（`filter(|v| …)`）；其余触发位置（`)`/`=>`/`else` 等）留空格。
                        if !matches!(
                            self.prev,
                            Some(TokenKind::LParen) | Some(TokenKind::LBracket)
                        ) {
                            self.space();
                        }
                        self.raw("|");
                        self.in_capture = true;
                        self.prev_nospace = true;
                    } else {
                        self.space();
                        self.raw("|");
                        self.prev_nospace = false;
                    }
                }
            }
            TokenKind::Star => {
                let unary = !is_operand(&self.prev)
                    || matches!(self.prev, Some(TokenKind::RParen))
                        && matches!(self.last_paren, Some(ParenKind::FnParams))
                    || matches!(self.prev, Some(TokenKind::RBracket)) && self.last_bracket_type;
                if unary {
                    if !self.prev_nospace {
                        self.space();
                    }
                    self.raw("*");
                    self.prev_nospace = true;
                    self.last_star_unary = true;
                } else {
                    self.space();
                    self.raw("*");
                    self.prev_nospace = false;
                }
            }
            TokenKind::Amp => {
                let unary = !is_operand(&self.prev)
                    || matches!(self.prev, Some(TokenKind::RParen))
                        && matches!(self.last_paren, Some(ParenKind::FnParams))
                    || matches!(self.prev, Some(TokenKind::RBracket)) && self.last_bracket_type;
                if unary {
                    if !self.prev_nospace {
                        self.space();
                    }
                    self.raw("&");
                    self.prev_nospace = true;
                    self.last_amp_unary = true;
                } else {
                    self.space();
                    self.raw("&");
                    self.prev_nospace = false;
                }
            }
            TokenKind::Minus | TokenKind::Plus => {
                if is_operand(&self.prev) {
                    self.space();
                    self.raw(text);
                    self.prev_nospace = false;
                } else {
                    if !self.prev_nospace {
                        self.space();
                    }
                    self.raw(text);
                    self.prev_nospace = true;
                }
            }
            TokenKind::Bang => {
                // `E!T` 错误联合：操作数后 `!` 紧跟无空格（`FileError!&[u8]`）；
                // 返回类型 `fn f() !void`：FnParams 闭括号后 `!` 前留空格。
                let union_nospace = is_operand(&self.prev)
                    && !(matches!(self.prev, Some(TokenKind::RParen))
                        && matches!(self.last_paren, Some(ParenKind::FnParams)));
                if !union_nospace && !self.prev_nospace {
                    self.space();
                }
                self.raw("!");
                self.prev_nospace = true;
            }
            TokenKind::Question => {
                if !self.prev_nospace {
                    self.space();
                }
                self.raw("?");
                self.prev_nospace = true;
            }
            TokenKind::Tilde => {
                if !self.prev_nospace {
                    self.space();
                }
                self.raw("~");
                self.prev_nospace = true;
            }
            TokenKind::Eq
            | TokenKind::EqEq
            | TokenKind::Ne
            | TokenKind::Lt
            | TokenKind::Le
            | TokenKind::Gt
            | TokenKind::Ge
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
            | TokenKind::Caret
            | TokenKind::Shl
            | TokenKind::Shr
            | TokenKind::PipePipe => {
                self.space();
                self.raw(text);
                self.prev_nospace = false;
            }
            TokenKind::Eof | TokenKind::Error(_) => {}
        }
    }

    /// `[` 是否为数组类型括号（`[5]i32` / `[n]T` / `&[u8]` / `[2][2]i32`）。
    /// 向后看匹配 `]` 后的 token + 打开位置上下文综合判断。
    fn bracket_is_type(&self) -> bool {
        let mut depth = 0;
        let mut j = self.cur_idx;
        let mut after = None;
        while j < self.tokens.len() {
            match &self.tokens[j].kind {
                TokenKind::LBracket => depth += 1,
                TokenKind::RBracket => {
                    depth -= 1;
                    if depth == 0 {
                        after = self.tokens.get(j + 1).map(|t| &t.kind);
                        break;
                    }
                }
                TokenKind::Eof => break,
                _ => {}
            }
            j += 1;
        }
        let after_is_type = matches!(after, Some(TokenKind::Ident(_)) | Some(TokenKind::LBracket));
        let typeish_ctx = matches!(self.prev, Some(TokenKind::Colon) | Some(TokenKind::Bang))
            || matches!(self.prev, Some(TokenKind::Amp)) && self.last_amp_unary
            || matches!(self.prev, Some(TokenKind::Star)) && self.last_star_unary
            || matches!(self.prev, Some(TokenKind::RBracket)) && self.last_bracket_type;
        after_is_type || typeish_ctx
    }

    /// 当前 `[` 是否跨多行（源内从 `[` 到匹配 `]` 含换行）。多行数组字面量保留垂直布局。
    fn bracket_is_multiline(&self) -> bool {
        let start = self.tokens[self.cur_idx].span.start;
        let mut depth = 0;
        let mut j = self.cur_idx;
        while j < self.tokens.len() {
            match &self.tokens[j].kind {
                TokenKind::LBracket => depth += 1,
                TokenKind::RBracket => {
                    depth -= 1;
                    if depth == 0 {
                        return count_nl(&self.src[start..self.tokens[j].span.end]) >= 1;
                    }
                }
                TokenKind::Eof => break,
                _ => {}
            }
            j += 1;
        }
        false
    }

    /// 当前 `(` 是否为「垂直实参」式跨行：紧随 `(` 的首个 token 位于下一行
    /// （`f(\n  a, b,\n)`）。实参从 `(` 同 行开始（如闭包块 `f(|x| {`）不算——嵌套块
    /// 自管换行，括号缩进只给真正的逐实参垂直排布。
    fn paren_is_multiline(&self) -> bool {
        let next = self.tokens.get(self.cur_idx + 1);
        matches!(next, Some(t) if !matches!(t.kind, TokenKind::RParen)
            && t.span.line > self.tokens[self.cur_idx].span.line)
    }

    /// 当前 `{` 与紧随的 `}` 之间是否含注释（`{` 与 `}` 相邻时中间只可能有空白或注释）。
    /// 用于空块判定：`fn f() { // 说明\n}` 按非空处理，避免行尾注释被压到 `{` 同行。
    fn cur_brace_has_comment(&self) -> bool {
        let next_start = self.tokens[self.cur_idx + 1].span.start;
        let gap = &self.src[self.last_end..next_start];
        gap.contains("//") || gap.contains("/*")
    }

    /// 当前 `{` 是否跨多行（源内从 `{` 到匹配 `}` 含换行）。多行 struct 字面量保留字段垂直布局。
    fn brace_is_multiline(&self) -> bool {
        let start = self.tokens[self.cur_idx].span.start;
        let mut depth = 0;
        let mut j = self.cur_idx;
        while j < self.tokens.len() {
            match &self.tokens[j].kind {
                TokenKind::LBrace => depth += 1,
                TokenKind::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        return count_nl(&self.src[start..self.tokens[j].span.end]) >= 1;
                    }
                }
                TokenKind::Eof => break,
                _ => {}
            }
            j += 1;
        }
        false
    }
}
