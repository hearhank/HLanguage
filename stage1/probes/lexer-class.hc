class Lexer {
    src: &[u8],
    n: i32,
    mut pos: i32,
    mut line: i32,
    mut col: i32,

    fn bump(self: *mut Self) void {
        if (self.src[self.pos] == '\n') {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        self.pos += utf8_width(self.src[self.pos]);
    }

    // 追加一个完整字符（含多字节 UTF-8）到内容缓冲并前进
    fn append_char(self: *mut Self, var mut content: Vec<u8>) void {
        var w = utf8_width(self.src[self.pos]);
        var mut k: i32 = 0;
        while (k < w) {
            content.append(self.src[self.pos + k]);
            k += 1;
        }
        self.bump();
    }

    fn emit_simple(self: *mut Self, start: usize, kind: &[u8]) void {
        io.print("{} {} {} {} {}\n", start, self.pos, self.line, self.col, kind);
    }

    fn emit_slice_payload(self: *mut Self, start: usize, kind: &[u8], payload: &[u8]) void {
        io.print("{} {} {} {} {}(\"", start, self.pos, self.line, self.col, kind);
        io.print("{}\")\n", payload);
    }

    fn emit_error(self: *mut Self, start: usize, msg: &[u8]) void {
        io.print("{} {} {} {} Error(\"", start, self.pos, self.line, self.col);
        io.print("{}\")\n", msg);
    }

    fn emit_content(self: *mut Self, start: usize, kind: &[u8], content: Vec<u8>) void {
        io.print("{} {} {} {} {}(\"", start, self.pos, self.line, self.col, kind);
        var esc = dbg_escape(content);
        var s = esc.as_slice();
        io.print("{}\")\n", s);
    }

    fn run(self: *mut Self) void {
        while (true) {
            self.skip_ws();
            var start = self.pos;
            if (self.pos >= self.n) {
                self.emit_simple(start, "Eof");
                return;
            }
            var mut c = self.src[self.pos];
            if (is_ident_start(c)) {
                self.lex_ident(start);
            } else if (is_digit(c)) {
                self.lex_number(start);
            } else if (c == '"') {
                self.lex_string(start);
            } else if (c == '\'') {
                self.lex_char(start);
            } else if (c == '@') {
                self.bump();
                var s2 = self.pos;
                while (self.pos < self.n and is_ident_cont(self.src[self.pos])) { self.bump(); }
                self.emit_slice_payload(start, "AtBuiltin", self.src[s2..self.pos]);
            } else {
                self.lex_punct(start);
            }
        }
    }

    fn skip_ws(self: *mut Self) void {
        while (true) {
            if (self.pos >= self.n) { return; }
            var mut c = self.src[self.pos];
            if (is_ws(c)) {
                self.bump();
            } else if (c == '/' and self.pos + 1 < self.n and self.src[self.pos + 1] == '/') {
                // 行注释（含 /// 文档注释，语法上等价）
                while (self.pos < self.n and self.src[self.pos] != '\n') { self.bump(); }
            } else if (c == '/' and self.pos + 1 < self.n and self.src[self.pos + 1] == '*') {
                self.bump();
                self.bump();
                while (true) {
                    if (self.pos >= self.n) {
                        // 未闭合块注释：Error（span=EOF 位置）+ 随后 run() 补 Eof
                        self.emit_error(self.pos, "unterminated block comment");
                        return;
                    }
                    if (self.src[self.pos] == '*' and self.pos + 1 < self.n and self.src[self.pos + 1] == '/') {
                        self.bump();
                        self.bump();
                        break;
                    }
                    self.bump();
                }
            } else {
                return;
            }
        }
    }

    fn lex_ident(self: *mut Self, start: usize) void {
        var s2 = self.pos;
        while (self.pos < self.n and is_ident_cont(self.src[self.pos])) { self.bump(); }
        var name = self.src[s2..self.pos];
        var kw = kw_of(name);
        if (kw) |k| {
            self.emit_simple(start, k);
        } else {
            self.emit_slice_payload(start, "Ident", name);
        }
    }

    fn lex_number(self: *mut Self, start: usize) void {
        var buf = Vec<u8>.init(alloc);
        var mut is_float = false;
        // 前缀 0x/0b/0o
        if (self.src[self.pos] == '0' and self.pos + 1 < self.n) {
            var mut c1 = self.src[self.pos + 1];
            if (c1 == 'x' or c1 == 'X') {
                buf.append('0'); buf.append('x');
                self.bump(); self.bump();
                while (self.pos < self.n and (is_hex(self.src[self.pos]) or self.src[self.pos] == '_')) {
                    buf.append(self.src[self.pos]);
                    self.bump();
                }
                self.finish_number(start, "Int", buf);
                return;
            }
            if (c1 == 'b' or c1 == 'B') {
                buf.append('0'); buf.append('b');
                self.bump(); self.bump();
                while (self.pos < self.n and (is_bin(self.src[self.pos]) or self.src[self.pos] == '_')) {
                    buf.append(self.src[self.pos]);
                    self.bump();
                }
                self.finish_number(start, "Int", buf);
                return;
            }
            if (c1 == 'o' or c1 == 'O') {
                buf.append('0'); buf.append('o');
                self.bump(); self.bump();
                while (self.pos < self.n and (is_oct(self.src[self.pos]) or self.src[self.pos] == '_')) {
                    buf.append(self.src[self.pos]);
                    self.bump();
                }
                self.finish_number(start, "Int", buf);
                return;
            }
        }
        // 十进制 / 浮点
        while (self.pos < self.n) {
            var mut c = self.src[self.pos];
            if (is_digit(c) or c == '_') {
                buf.append(c);
                self.bump();
            } else if (c == '.' and self.pos + 1 < self.n and is_digit(self.src[self.pos + 1])) {
                is_float = true;
                buf.append('.');
                self.bump();
                while (self.pos < self.n and (is_digit(self.src[self.pos]) or self.src[self.pos] == '_')) {
                    buf.append(self.src[self.pos]);
                    self.bump();
                }
            } else {
                break;
            }
        }
        // 指数（e/E → 归一化小写 e）
        if (self.pos < self.n) {
            var mut c = self.src[self.pos];
            if ((c == 'e' or c == 'E') and self.pos + 1 < self.n) {
                var mut c2 = self.src[self.pos + 1];
                if (is_digit(c2) or c2 == '+' or c2 == '-') {
                    is_float = true;
                    buf.append('e');
                    self.bump();
                    if (self.pos < self.n and (self.src[self.pos] == '+' or self.src[self.pos] == '-')) {
                        buf.append(self.src[self.pos]);
                        self.bump();
                    }
                    while (self.pos < self.n and (is_digit(self.src[self.pos]) or self.src[self.pos] == '_')) {
                        buf.append(self.src[self.pos]);
                        self.bump();
                    }
                }
            }
        }
        if (is_float) { self.finish_number(start, "Float", buf); }
        else { self.finish_number(start, "Int", buf); }
    }

    // Rust maybe_suffix 收集：is_ascii_digit() || is_alphabetic()（CJK 表意文字近似 E4–E9；`_` 不含）
    fn is_suffix_cont(self: *mut Self, b: u8) bool {
        return is_digit(b) or is_alpha(b) or (b >= 0xE4 and b <= 0xE9);
    }

    fn detect_suffix(self: *mut Self) ?&[u8] {
        if (self.pos < self.n) {
            var mut c = self.src[self.pos];
            if (c == 'i' or c == 'u' or c == 'f') {
                var mut j = self.pos;
                while (j < self.n and self.is_suffix_cont(self.src[j])) { j += utf8_width(self.src[j]); }
                var mut suf = self.src[self.pos..j];
                var slen: i32 = @intCast(i32, suf.len);
                if (slen >= 2) {
                    var ok = is_digit(self.src[self.pos + 1]) or suf == "isize" or suf == "usize";
                    if (ok) return suf;
                }
            }
        }
        return null;
    }

    fn finish_number(self: *mut Self, start: usize, kind: &[u8], var mut buf: Vec<u8>) void {
        // 惰性宽度后缀
        if (self.pos < self.n) {
            var suf = self.detect_suffix();
            if (suf) |s| {
                var slen: i32 = @intCast(i32, s.len);
                var mut k: i32 = 0;
                while (k < slen) {
                    buf.append(s[k]);
                    self.bump();
                    k += 1;
                }
            }
        }
        var s = buf.as_slice();
        io.print("{} {} {} {} {}(\"", start, self.pos, self.line, self.col, kind);
        io.print("{}\")\n", s);
    }

    fn lex_string(self: *mut Self, start: usize) void {
        self.bump();  // 开引号
        // 原始多行字符串 """..."""
        if (self.pos + 1 < self.n and self.src[self.pos] == '"' and self.src[self.pos + 1] == '"') {
            self.bump();
            self.bump();
            var content = Vec<u8>.init(alloc);
            while (true) {
                if (self.pos >= self.n) {
                    self.emit_error(start, "unterminated raw string");
                    return;
                }
                if (self.src[self.pos] == '"' and self.pos + 2 < self.n and self.src[self.pos + 1] == '"' and self.src[self.pos + 2] == '"') {
                    self.bump();
                    self.bump();
                    self.bump();
                    break;
                }
                self.append_char(content);
            }
            self.emit_content(start, "RawStr", content);
            return;
        }
        // 普通字符串
        var content = Vec<u8>.init(alloc);
        while (true) {
            if (self.pos >= self.n) {
                self.emit_error(start, "unterminated string literal");
                return;
            }
            if (self.src[self.pos] == '"') {
                self.bump();
                break;
            }
            if (self.src[self.pos] == '\\') {
                self.bump();  // 反斜杠
                if (self.pos >= self.n) {
                    self.emit_error(start, "unterminated string literal");
                    return;
                }
                var mut ec = self.src[self.pos];
                self.bump();  // 转义字符本身（与 Rust 一致：总是先消费再判定）
                if (ec == 'n') { content.append('\n'); }
                else if (ec == 'r') { content.append('\r'); }
                else if (ec == 't') { content.append('\t'); }
                else if (ec == '\\') { content.append('\\'); }
                else if (ec == '"') { content.append('"'); }
                else if (ec == '\'') { content.append('\''); }
                else if (ec == 'x') {
                    var mut hi: i32 = -1;
                    var mut lo: i32 = -1;
                    if (self.pos < self.n) { hi = hexval(self.src[self.pos]); self.bump(); }
                    if (self.pos < self.n) { lo = hexval(self.src[self.pos]); self.bump(); }
                    if (hi < 0 or lo < 0) {
                        self.emit_error(start, "invalid \\\\x escape");
                        return;
                    }
                    var byte: i32 = hi * 16 + lo;
                    if (byte < 0x80) { content.append(@intCast(u8, byte)); }
                    else {
                        content.append(@intCast(u8, 0xC0 + (byte >> 6)));
                        content.append(@intCast(u8, 0x80 + (byte & 0x3F)));
                    }
                }
                else if (ec == 'u') {
                    // 消费下一个字符再判定是否 '{'（与 Rust `if self.bump() != Some('{')` 一致）
                    if (self.pos >= self.n) {
                        self.emit_error(start, "invalid \\\\u escape");
                        return;
                    }
                    var brace = self.src[self.pos];
                    self.bump();
                    if (brace != '{') {
                        self.emit_error(start, "invalid \\\\u escape");
                        return;
                    }
                    var mut v: i64 = 0;
                    var mut bad = false;
                    while (true) {
                        if (self.pos >= self.n) { bad = true; break; }
                        var mut ch = self.src[self.pos];
                        self.bump();  // 与 Rust 一致：先消费再判定
                        if (ch == '}') break;
                        var d: i64 = @intCast(i64, hexval(ch));
                        if (d < 0) { bad = true; break; }
                        v = v * 16 + d;
                    }
                    if (bad) {
                        self.emit_error(start, "invalid \\\\u escape");
                        return;
                    }
                    if (v > 0x10FFFF or (v >= 0xD800 and v <= 0xDFFF)) {
                        self.emit_error(start, "\\\\u escape out of range");
                        return;
                    }
                    // UTF-8 编码
                    if (v < 0x80) { content.append(@intCast(u8, v)); }
                    else if (v < 0x800) {
                        content.append(@intCast(u8, 0xC0 + (v >> 6)));
                        content.append(@intCast(u8, 0x80 + (v & 0x3F)));
                    }
                    else if (v < 0x10000) {
                        content.append(@intCast(u8, 0xE0 + (v >> 12)));
                        content.append(@intCast(u8, 0x80 + ((v >> 6) & 0x3F)));
                        content.append(@intCast(u8, 0x80 + (v & 0x3F)));
                    }
                    else {
                        content.append(@intCast(u8, 0xF0 + (v >> 18)));
                        content.append(@intCast(u8, 0x80 + ((v >> 12) & 0x3F)));
                        content.append(@intCast(u8, 0x80 + ((v >> 6) & 0x3F)));
                        content.append(@intCast(u8, 0x80 + (v & 0x3F)));
                    }
                }
                else {
                    self.emit_error(start, "invalid escape sequence");
                    return;
                }
            }
            else {
                self.append_char(content);
            }
        }
        self.emit_content(start, "Str", content);
    }

    fn lex_char(self: *mut Self, start: usize) void {
        self.bump();  // 开引号
        var mut val: i32 = -1;
        if (self.pos >= self.n) {
            self.emit_error(start, "unterminated char literal");
            return;
        }
        if (self.src[self.pos] == '\\') {
            self.bump();  // 反斜杠
            if (self.pos >= self.n) {
                self.emit_error(start, "unterminated char literal");
                return;
            }
            var mut c = self.src[self.pos];
            self.bump();  // 转义字符本身（与 Rust 一致：总是先消费再判定）
            if (c == 'n') { val = 0x0A; }
            else if (c == 'r') { val = 0x0D; }
            else if (c == 't') { val = 0x09; }
            else if (c == '\\') { val = 0x5C; }
            else if (c == '\'') { val = 0x27; }
            else if (c == 'x') {
                var mut hi: i32 = -1;
                var mut lo: i32 = -1;
                if (self.pos < self.n) { hi = hexval(self.src[self.pos]); self.bump(); }
                if (self.pos < self.n) { lo = hexval(self.src[self.pos]); self.bump(); }
                if (hi < 0 or lo < 0) {
                    self.emit_error(start, "invalid \\\\x escape in char");
                    return;
                }
                val = hi * 16 + lo;
            }
            else {
                self.emit_error(start, "invalid escape in char literal");
                return;
            }
        } else {
            if (self.src[self.pos] >= 0x80) {
                self.emit_error(start, "char literal must be a single ASCII byte");
                return;
            }
            val = @intCast(i32, self.src[self.pos]);
            self.bump();
        }
        // 读闭引号：与 Rust 一致，总是消费下一个字符再判定
        if (self.pos >= self.n) {
            self.emit_error(start, "char literal must be closed with '");
            return;
        }
        var close = self.src[self.pos];
        self.bump();
        if (close != '\'') {
            self.emit_error(start, "char literal must be closed with '");
            return;
        }
        io.print("{} {} {} {} Char({})\n", start, self.pos, self.line, self.col, val);
    }

    fn lex_punct(self: *mut Self, start: usize) void {
        var mut c = self.src[self.pos];
        self.bump();
        if (c == '{') { self.emit_simple(start, "LBrace"); }
        else if (c == '}') { self.emit_simple(start, "RBrace"); }
        else if (c == '(') { self.emit_simple(start, "LParen"); }
        else if (c == ')') { self.emit_simple(start, "RParen"); }
        else if (c == '[') { self.emit_simple(start, "LBracket"); }
        else if (c == ']') { self.emit_simple(start, "RBracket"); }
        else if (c == ';') { self.emit_simple(start, "Semi"); }
        else if (c == ',') { self.emit_simple(start, "Comma"); }
        else if (c == '.') {
            if (self.pos < self.n and self.src[self.pos] == '.') { self.bump(); self.emit_simple(start, "DotDot"); }
            else if (self.pos < self.n and self.src[self.pos] == '*') { self.bump(); self.emit_simple(start, "DotStar"); }
            else { self.emit_simple(start, "Dot"); }
        }
        else if (c == ':') { self.emit_simple(start, "Colon"); }
        else if (c == '=') {
            if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.emit_simple(start, "EqEq"); }
            else if (self.pos < self.n and self.src[self.pos] == '>') { self.bump(); self.emit_simple(start, "FatArrow"); }
            else { self.emit_simple(start, "Eq"); }
        }
        else if (c == '!') {
            if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.emit_simple(start, "Ne"); }
            else { self.emit_simple(start, "Bang"); }
        }
        else if (c == '<') {
            if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.emit_simple(start, "Le"); }
            else if (self.pos < self.n and self.src[self.pos] == '<') { self.bump(); self.emit_simple(start, "Shl"); }
            else { self.emit_simple(start, "Lt"); }
        }
        else if (c == '>') {
            if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.emit_simple(start, "Ge"); }
            else if (self.pos < self.n and self.src[self.pos] == '>') { self.bump(); self.emit_simple(start, "Shr"); }
            else { self.emit_simple(start, "Gt"); }
        }
        else if (c == '+') {
            if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.emit_simple(start, "PlusEq"); }
            else { self.emit_simple(start, "Plus"); }
        }
        else if (c == '-') {
            if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.emit_simple(start, "MinusEq"); }
            else { self.emit_simple(start, "Minus"); }
        }
        else if (c == '*') {
            if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.emit_simple(start, "StarEq"); }
            else { self.emit_simple(start, "Star"); }
        }
        else if (c == '/') {
            if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.emit_simple(start, "SlashEq"); }
            else { self.emit_simple(start, "Slash"); }
        }
        else if (c == '%') {
            if (self.pos < self.n and self.src[self.pos] == '%') { self.bump(); self.emit_simple(start, "PercentPercent"); }
            else { self.emit_simple(start, "Percent"); }
        }
        else if (c == '&') {
            if (self.pos < self.n and self.src[self.pos] == '&') { self.bump(); self.emit_simple(start, "KwAnd"); }
            else if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.emit_simple(start, "AmpEq"); }
            else { self.emit_simple(start, "Amp"); }
        }
        else if (c == '|') {
            if (self.pos < self.n and self.src[self.pos] == '|') { self.bump(); self.emit_simple(start, "PipePipe"); }
            else if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.emit_simple(start, "PipeEq"); }
            else { self.emit_simple(start, "Pipe"); }
        }
        else if (c == '^') {
            if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.emit_simple(start, "CaretEq"); }
            else { self.emit_simple(start, "Caret"); }
        }
        else if (c == '~') { self.emit_simple(start, "Tilde"); }
        else if (c == '?') { self.emit_simple(start, "Question"); }
        else if (c == '_') { self.emit_simple(start, "Underscore"); }
        else {
            // 未知字符：两个相同 Error token（与 Rust run() + lex_punct 双 push 一致）
            self.emit_error(start, "unexpected character");
            self.emit_error(start, "unexpected character");
        }
    }
}
