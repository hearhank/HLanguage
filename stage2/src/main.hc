// stage2/src/main.hc — H 编译器（H 实现）：入口 + 词法阶段（自包含单文件）
// S1/S2：读目标源文件 → lex（真实实现）→ parse（S3 起填充）。
// 自包含惯例对齐 stage1 四件套（各自内嵌所需组件）；多文件拆分待工具链支持
// namespace/跨文件调用后回归（见 stage2/README.md 缺陷登记）。
// 纪律自查清单见 stage2/README.md；运行方式：
//   包模式：hc run stage2 stage2/test/smoke.hc（Rust 包加载）
//   检查：hc run stage1/checker.hc stage2/src/main.hc
//   链路：hc run stage1/interp.hc stage2/src/main.hc <target.hc>
//   对照：hc run stage1/interp.hc stage2/src/main.hc --dump-tokens <target.hc>（= hc lex 格式）

// ============================================================
// 字符分类辅助
// ============================================================

fn is_digit(b: u8) bool {
    return b >= '0' and b <= '9';
}
fn is_hex(b: u8) bool {
    return is_digit(b) or (b >= 'a' and b <= 'f') or (b >= 'A' and b <= 'F');
}
fn is_alpha(b: u8) bool {
    return (b >= 'a' and b <= 'z') or (b >= 'A' and b <= 'Z');
}
fn is_alnum(b: u8) bool {
    return is_digit(b) or is_alpha(b);
}
fn is_ident_start(b: u8) bool {
    return is_alpha(b) or b == '_';
}
fn is_ident_cont(b: u8) bool {
    if (is_alnum(b) or b == '_') return true;
    if (b >= 0xE4 and b <= 0xE9) return true;
    return false;
}
fn is_ws(b: u8) bool {
    return b == 0x20 or b == 0x09 or b == 0x0A or b == 0x0D or b == 0x0B or b == 0x0C;
}
fn utf8_width(b: u8) i32 {
    if (b < 0x80) return 1;
    if (b < 0xE0) return 2;
    if (b < 0xF0) return 3;
    return 4;
}
fn is_oct(b: u8) bool {
    return b >= '0' and b <= '7';
}
fn is_bin(b: u8) bool {
    return b == '0' or b == '1';
}
fn hexval(b: u8) i32 {
    if (is_digit(b)) return @intCast(i32, b) - '0';
    if (b >= 'a' and b <= 'f') return @intCast(i32, b) - 'a' + 10;
    if (b >= 'A' and b <= 'F') return @intCast(i32, b) - 'A' + 10;
    return -1;
}
fn vec_from_slice(s: &[u8]) Vec<u8> {
    var v = Vec<u8>.init(alloc);
    var mut i: usize = 0;
    while (i < s.len) {
        v.append(s[i]);
        i += 1;
    }
    return v;
}

// ============================================================
// 关键字判定（if-chain——不用 Map：Map 值在 stage1 interp 下属 R2 重定位风险）
// ============================================================

fn kw_of(name: &[u8]) ?&[u8] {
    if (name == "var") return "KwVar";
    if (name == "const") return "KwConst";
    if (name == "fn") return "KwFn";
    if (name == "global") return "KwGlobal";
    if (name == "if") return "KwIf";
    if (name == "else") return "KwElse";
    if (name == "while") return "KwWhile";
    if (name == "for") return "KwFor";
    if (name == "break") return "KwBreak";
    if (name == "continue") return "KwContinue";
    if (name == "return") return "KwReturn";
    if (name == "switch") return "KwSwitch";
    if (name == "defer") return "KwDefer";
    if (name == "errdefer") return "KwErrdefer";
    if (name == "class") return "KwClass";
    if (name == "struct") return "KwStruct";
    if (name == "enum") return "KwEnum";
    if (name == "union") return "KwUnion";
    if (name == "void") return "KwVoid";
    if (name == "spawn") return "KwSpawn";
    if (name == "tree") return "KwTree";
    if (name == "interface") return "KwInterface";
    if (name == "where") return "KwWhere";
    if (name == "namespace") return "KwNamespace";
    if (name == "import") return "KwImport";
    if (name == "pub") return "KwPub";
    if (name == "export") return "KwExport";
    if (name == "owned") return "KwOwned";
    if (name == "move") return "KwMove";
    if (name == "mut") return "KwMut";
    if (name == "and") return "KwAnd";
    if (name == "or") return "KwOr";
    if (name == "try") return "KwTry";
    if (name == "catch") return "KwCatch";
    if (name == "orelse") return "KwOrelse";
    if (name == "script") return "KwScript";
    if (name == "comptime") return "KwComptime";
    if (name == "anytype") return "KwAnytype";
    if (name == "type") return "KwType";
    if (name == "async") return "KwAsync";
    if (name == "await") return "KwAwait";
    if (name == "true") return "KwTrue";
    if (name == "false") return "KwFalse";
    if (name == "null") return "KwNull";
    if (name == "extern") return "KwExtern";
    return null;
}

// ============================================================
// Rust Debug 转义（Str 载荷；与 hc lex 输出对齐）
// ============================================================

// 解码 content[i] 处的完整码点（i 必须在字符起始；content 恒为合法 UTF-8）
fn cp_at(content: Vec<u8>, i: i32) i32 {
    var b0: i32 = @intCast(i32, content[i]);
    if (b0 < 0x80) return b0;
    var b1: i32 = @intCast(i32, content[i + 1]);
    if (b0 < 0xE0) return (b0 & 0x1F) * 64 + (b1 & 0x3F);
    var b2: i32 = @intCast(i32, content[i + 2]);
    if (b0 < 0xF0) return (b0 & 0x0F) * 4096 + (b1 & 0x3F) * 64 + (b2 & 0x3F);
    var b3: i32 = @intCast(i32, content[i + 3]);
    return (b0 & 0x07) * 262144 + (b1 & 0x3F) * 4096 + (b2 & 0x3F) * 64 + (b3 & 0x3F);
}

// Rust `char::is_printable()` 近似（K1 实证对齐；排除表与 stage1/lexer.hc 同源）
fn is_printable(cp: i32) bool {
    if (cp >= 0x20 and cp <= 0x7E) return true;
    if (cp <= 0x1F) return false;
    if (cp >= 0x7F and cp <= 0xA0) return false;
    if (cp == 0x00AD) return false;
    if (cp == 0x034F) return false;
    if (cp == 0x061C) return false;
    if (cp == 0x17B4 or cp == 0x17B5) return false;
    if (cp == 0x180E) return false;
    if (cp >= 0x200B and cp <= 0x200F) return false;
    if (cp >= 0x2028 and cp <= 0x202E) return false;
    if (cp >= 0x2060 and cp <= 0x2064) return false;
    if (cp >= 0x206A and cp <= 0x206F) return false;
    if (cp >= 0xFE00 and cp <= 0xFE0F) return false;
    if (cp == 0xFEFF) return false;
    if (cp >= 0xFFF0 and cp <= 0xFFFB) return false;
    if (cp >= 0xFDD0 and cp <= 0xFDEF) return false;
    if (cp >= 0xE000 and cp <= 0xF8FF) return false;
    if (cp >= 0x1BCA0 and cp <= 0x1BCA3) return false;
    if (cp >= 0x1D173 and cp <= 0x1D17A) return false;
    if (cp >= 0xE0000 and cp <= 0xE0FFF) return false;
    if (cp >= 0xF0000 and cp <= 0x10FFFF) return false;
    return true;
}

// 追加 `\u{hex}`（hex 小写无前导零，对齐 Rust escape_unicode）
fn append_unicode_escape(var mut out: Vec<u8>, cp: i32) void {
    out.append('\\'); out.append('u'); out.append('{');
    var digits = "0123456789abcdef";
    var mut sh: i32 = 0;
    var mut tmp: i32 = cp;
    while (tmp >= 0x10) { tmp = tmp / 16; sh += 4; }
    while (sh >= 0) {
        var idx = (cp >> sh) & 0xF;
        var d = digits[idx..(idx + 1)];
        out.append(d[0]);
        sh -= 4;
    }
    out.append('}');
}

// Rust Debug 输出：字符串内容转义（`\n`/`\r`/`\t`/`\"`/`\\`/`\0`/不可打印 → `\u{..}`）
fn dbg_escape(content: Vec<u8>) Vec<u8> {
    var out = Vec<u8>.init(alloc);
    var n: i32 = @intCast(i32, content.len);
    var mut i: i32 = 0;
    while (i < n) {
        var mut cp = cp_at(content, i);
        var w = utf8_width(content[i]);
        if (cp == 0x09) { out.append('\\'); out.append('t'); }
        else if (cp == 0x0A) { out.append('\\'); out.append('n'); }
        else if (cp == 0x0D) { out.append('\\'); out.append('r'); }
        else if (cp == 0x22) { out.append('\\'); out.append('"'); }
        else if (cp == 0x5C) { out.append('\\'); out.append('\\'); }
        else if (cp == 0x00) { out.append('\\'); out.append('0'); }
        else if (is_printable(cp)) {
            var mut j: i32 = 0;
            while (j < w) { out.append(content[i + j]); j += 1; }
        }
        else { append_unicode_escape(out, cp); }
        i += w;
    }
    return out;
}

// ============================================================
// Token + 收集式 Lexer
// ============================================================

class Token {
    kind: &[u8],
    text: Vec<u8>,
    start: usize,
    end: usize,
    line: i32,
    col: i32,
}

class Lexer {
    src: &[u8],
    n: usize,
    mut pos: usize,
    mut line: i32,
    mut col: i32,
    mut tokens: Vec<Token>,

    fn bump(self: *mut Self) void {
        if (self.src[self.pos] == '\n') {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        var c = self.src[self.pos];
        if (c < 0x80) { self.pos += 1; }
        else if (c < 0xE0) { self.pos += 2; }
        else if (c < 0xF0) { self.pos += 3; }
        else { self.pos += 4; }
    }

    fn push_token(self: *mut Self, kind: &[u8], text: Vec<u8>, start: usize) void {
        var tok = Token{
            kind = kind,
            text = text,
            start = start,
            end = self.pos,
            line = self.line,
            col = self.col,
        };
        self.tokens.append(tok);
    }

    fn push_simple(self: *mut Self, kind: &[u8], start: usize) void {
        var empty = Vec<u8>.init(alloc);
        self.push_token(kind, empty, start);
    }

    fn run(self: *mut Self) void {
        while (true) {
            self.skip_ws();
            var start = self.pos;
            if (self.pos >= self.n) {
                self.push_simple("Eof", start);
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
                var txt = vec_from_slice(self.src[s2..self.pos]);
                self.push_token("AtBuiltin", txt, start);
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
                while (self.pos < self.n and self.src[self.pos] != '\n') { self.bump(); }
            } else if (c == '/' and self.pos + 1 < self.n and self.src[self.pos + 1] == '*') {
                self.bump();
                self.bump();
                while (true) {
                    if (self.pos >= self.n) { return; }
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
            self.push_simple(k, start);
        } else {
            var txt = vec_from_slice(name);
            self.push_token("Ident", txt, start);
        }
    }

    fn lex_number(self: *mut Self, start: usize) void {
        var buf = Vec<u8>.init(alloc);
        var mut is_float = false;
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

    // Rust maybe_suffix 收集：is_ascii_digit() || is_alphabetic()（CJK 近似 E4–E9；`_` 不含）
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
                if (suf.len >= 2) {
                    var ok = is_digit(self.src[self.pos + 1]) or suf == "isize" or suf == "usize";
                    if (ok) return suf;
                }
            }
        }
        return null;
    }

    fn finish_number(self: *mut Self, start: usize, kind: &[u8], buf: Vec<u8>) void {
        // 创建可变副本，绕过参数不能声明 mut 的限制
        var mut txt = vec_from_slice(buf);
        if (self.pos < self.n) {
            var suf = self.detect_suffix();
            if (suf) |s| {
                var mut k: usize = 0;
                while (k < s.len) {
                    txt.append(s[k]);
                    self.bump();
                    k += 1;
                }
            }
        }
        self.push_token(kind, txt, start);
    }

    fn lex_string(self: *mut Self, start: usize) void {
        self.bump();
        if (self.pos + 1 < self.n and self.src[self.pos] == '"' and self.src[self.pos + 1] == '"') {
            self.bump();
            self.bump();
            var content = Vec<u8>.init(alloc);
            while (true) {
                if (self.pos >= self.n) { return; }
                if (self.src[self.pos] == '"' and self.pos + 2 < self.n and self.src[self.pos + 1] == '"' and self.src[self.pos + 2] == '"') {
                    self.bump(); self.bump(); self.bump();
                    break;
                }
                var w = utf8_width(self.src[self.pos]);
                var mut k = 0;
                while (k < w) {
                    content.append(self.src[self.pos + k]);
                    k += 1;
                }
                self.bump();
            }
            self.push_token("Str", content, start);
            return;
        }
        var content = Vec<u8>.init(alloc);
        while (true) {
            if (self.pos >= self.n) { return; }
            if (self.src[self.pos] == '"') {
                self.bump();
                break;
            }
            if (self.src[self.pos] == '\\') {
                self.bump();
                if (self.pos >= self.n) { return; }
                var mut ec = self.src[self.pos];
                self.bump();
                if (ec == 'n') { content.append('\n'); }
                else if (ec == 'r') { content.append('\r'); }
                else if (ec == 't') { content.append('\t'); }
                else if (ec == '\\') { content.append('\\'); }
                else if (ec == '"') { content.append('"'); }
                else if (ec == '\'') { content.append('\''); }
                else if (ec == 'x') {
                    var mut hi: i32 = -1; var mut lo: i32 = -1;
                    if (self.pos < self.n) { hi = hexval(self.src[self.pos]); self.bump(); }
                    if (self.pos < self.n) { lo = hexval(self.src[self.pos]); self.bump(); }
                    if (hi >= 0 and lo >= 0) {
                        var byte: i32 = hi * 16 + lo;
                        if (byte < 0x80) { content.append(@intCast(u8, byte)); }
                        else {
                            content.append(@intCast(u8, 0xC0 + (byte >> 6)));
                            content.append(@intCast(u8, 0x80 + (byte & 0x3F)));
                        }
                    }
                }
                else if (ec == 'u') {
                    if (self.pos >= self.n) { return; }
                    var mut brace = self.src[self.pos]; self.bump();
                    if (brace == '{') {
                        var mut v: i64 = 0;
                        while (true) {
                            if (self.pos >= self.n) { break; }
                            var mut ch = self.src[self.pos]; self.bump();
                            if (ch == '}') break;
                            var d: i64 = @intCast(i64, hexval(@intCast(u8, ch)));
                            if (d < 0) break;
                            v = v * 16 + d;
                        }
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
                }
            }
            else {
                var w = utf8_width(self.src[self.pos]);
                var mut k = 0;
                while (k < w) {
                    content.append(self.src[self.pos + k]);
                    k += 1;
                }
                self.bump();
            }
        }
        self.push_token("Str", content, start);
    }

    fn lex_char(self: *mut Self, start: usize) void {
        self.bump();
        var mut val: i32 = -1;
        if (self.pos >= self.n) { return; }
        if (self.src[self.pos] == '\\') {
            self.bump();
            if (self.pos >= self.n) { return; }
            var mut c = self.src[self.pos]; self.bump();
            if (c == 'n') { val = 0x0A; }
            else if (c == 'r') { val = 0x0D; }
            else if (c == 't') { val = 0x09; }
            else if (c == '\\') { val = 0x5C; }
            else if (c == '\'') { val = 0x27; }
            else if (c == 'x') {
                var mut hi: i32 = -1; var mut lo: i32 = -1;
                if (self.pos < self.n) { hi = hexval(self.src[self.pos]); self.bump(); }
                if (self.pos < self.n) { lo = hexval(self.src[self.pos]); self.bump(); }
                if (hi >= 0 and lo >= 0) { val = hi * 16 + lo; }
            }
        } else {
            if (self.src[self.pos] >= 0x80) { return; }
            val = @intCast(i32, self.src[self.pos]);
            self.bump();
        }
        if (self.pos >= self.n) { return; }
        var mut close = self.src[self.pos]; self.bump();
        if (close == '\'') {
            var txt = Vec<u8>.init(alloc);
            txt.append(@intCast(u8, val));
            self.push_token("Char", txt, start);
        }
    }

    fn lex_punct(self: *mut Self, start: usize) void {
        var mut c = self.src[self.pos];
        self.bump();
        if (c == '{') { self.push_simple("LBrace", start); }
        else if (c == '}') { self.push_simple("RBrace", start); }
        else if (c == '(') { self.push_simple("LParen", start); }
        else if (c == ')') { self.push_simple("RParen", start); }
        else if (c == '[') { self.push_simple("LBracket", start); }
        else if (c == ']') { self.push_simple("RBracket", start); }
        else if (c == ';') { self.push_simple("Semi", start); }
        else if (c == ',') { self.push_simple("Comma", start); }
        else if (c == '.') {
            if (self.pos < self.n and self.src[self.pos] == '.') { self.bump(); self.push_simple("DotDot", start); }
            else if (self.pos < self.n and self.src[self.pos] == '*') { self.bump(); self.push_simple("DotStar", start); }
            else { self.push_simple("Dot", start); }
        }
        else if (c == ':') { self.push_simple("Colon", start); }
        else if (c == '=') {
            if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.push_simple("EqEq", start); }
            else if (self.pos < self.n and self.src[self.pos] == '>') { self.bump(); self.push_simple("FatArrow", start); }
            else { self.push_simple("Eq", start); }
        }
        else if (c == '!') {
            if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.push_simple("Ne", start); }
            else { self.push_simple("Bang", start); }
        }
        else if (c == '<') {
            if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.push_simple("Le", start); }
            else if (self.pos < self.n and self.src[self.pos] == '<') { self.bump(); self.push_simple("Shl", start); }
            else { self.push_simple("Lt", start); }
        }
        else if (c == '>') {
            if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.push_simple("Ge", start); }
            else if (self.pos < self.n and self.src[self.pos] == '>') { self.bump(); self.push_simple("Shr", start); }
            else { self.push_simple("Gt", start); }
        }
        else if (c == '+') {
            if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.push_simple("PlusEq", start); }
            else { self.push_simple("Plus", start); }
        }
        else if (c == '-') {
            if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.push_simple("MinusEq", start); }
            else { self.push_simple("Minus", start); }
        }
        else if (c == '*') {
            if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.push_simple("StarEq", start); }
            else { self.push_simple("Star", start); }
        }
        else if (c == '/') {
            if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.push_simple("SlashEq", start); }
            else { self.push_simple("Slash", start); }
        }
        else if (c == '%') {
            if (self.pos < self.n and self.src[self.pos] == '%') { self.bump(); self.push_simple("PercentPercent", start); }
            else { self.push_simple("Percent", start); }
        }
        else if (c == '&') {
            if (self.pos < self.n and self.src[self.pos] == '&') { self.bump(); self.push_simple("KwAnd", start); }
            else if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.push_simple("AmpEq", start); }
            else { self.push_simple("Amp", start); }
        }
        else if (c == '|') {
            if (self.pos < self.n and self.src[self.pos] == '|') { self.bump(); self.push_simple("PipePipe", start); }
            else if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.push_simple("PipeEq", start); }
            else { self.push_simple("Pipe", start); }
        }
        else if (c == '^') {
            if (self.pos < self.n and self.src[self.pos] == '=') { self.bump(); self.push_simple("CaretEq", start); }
            else { self.push_simple("Caret", start); }
        }
        else if (c == '~') { self.push_simple("Tilde", start); }
        else if (c == '?') { self.push_simple("Question", start); }
        else if (c == '_') { self.push_simple("Underscore", start); }
        else {
            self.push_simple("Error", start);
            self.push_simple("Error", start);
        }
    }
}

// ============================================================
// 词法对外接口
// ============================================================

// 词法入口：源码字节 → token 流（Eof 收尾）
fn lex_source(src: &[u8]) Vec<Token> {
    var lx: Lexer = alloc.init(Lexer{
        src = src, n = src.len,
        pos = 0, line = 1, col = 1,
        tokens = Vec<Token>.init(alloc),
    });
    lx.run();
    return lx.tokens;
}

// K1 对照格式转储（= `hc lex`：{start} {end} {line} {col} {kind}；payload 按 kind 分型）
fn dump_tokens(toks: Vec<Token>) void {
    var mut i: usize = 0;
    while (i < toks.len) {
        var t = toks[i];
        if (t.kind == "Char") {
            // Char 载荷 = 码点数值（text[0] 即码点低字节）
            io.print("{} {} {} {} Char({})\n", t.start, t.end, t.line, t.col, @intCast(i32, t.text[0]));
        } else if (t.kind == "Str") {
            // Str 载荷 = 解码内容过 Rust Debug 转义
            io.print("{} {} {} {} Str(\"", t.start, t.end, t.line, t.col);
            io.print("{}\")\n", dbg_escape(t.text).as_slice());
        } else if (t.kind == "Ident" or t.kind == "AtBuiltin" or t.kind == "Int" or t.kind == "Float") {
            // 原文载荷（不转义；两段式打印避免载荷进入格式串）
            io.print("{} {} {} {} {}(\"", t.start, t.end, t.line, t.col, t.kind);
            io.print("{}\")\n", t.text.as_slice());
        } else {
            io.print("{} {} {} {} {}\n", t.start, t.end, t.line, t.col, t.kind);
        }
        i += 1;
    }
}

// ============================================================
// 阶段 2：语法分析（S3 填充真实 Parser；当前为骨架占位）
// ============================================================

fn parse_tokens(ntoks: usize) usize {
    return 0;
}

// ============================================================
// 入口
// ============================================================

fn main(args: Vec<String>) !void {
    // S2：token 流转储（K1 对照模式）
    if (args.len >= 3 and args[1].as_slice() == "--dump-tokens") {
        var dsrc = try io.fs.read_file(args[2], alloc);
        dump_tokens(lex_source(dsrc));
        return;
    }
    if (args.len < 2) {
        io.print("usage: main [--dump-tokens] <source.hc>\n");
        return error.Usage;
    }
    var path = args[1];
    // 阶段 0：读源文件（宿主透传；缺失 → err 上浮，main 非零退出）
    var src = try io.fs.read_file(path, alloc);
    // 阶段 1：词法（S2 已落地）
    var toks = lex_source(src);
    // 阶段 2：语法（S3 填充真实 Parser）
    var nnodes = parse_tokens(toks.len);
    // 阶段 3+：语义检查 / lower / HBC2 编码（S4–S7 填充）
    io.print("stage2: {} bytes -> {} tokens -> {} nodes\n", src.len, toks.len, nnodes);
}
