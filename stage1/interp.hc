// ============================================================
// stage1/interp.hc — H 版执行引擎（K4，E7 自举渐进路线 · 执行阶段）
//
// 树遍历解释器：内嵌 lexer/parser（源自 parser.hc，A 组修复同步副本），
// 对目标程序求值，输出与 Rust 参考 `hc run` 逐字节一致。
// 对照验收：tag1/hc-tools/tests/k4_interp.rs × stage1/exec-corpus/。
//
// 当前状态（C1 骨架）：读入 → parse → AST 就绪（无求值）。
// 用法：
//   hc run stage1/interp.hc <file.hc>            → 解析成功打印 OK
//   hc run stage1/interp.hc --dump-ast <file.hc> → 转储 AST（复用 K2 dump）
// ============================================================

import H.std.{io};

// ============================================================
// 辅助函数（复用 lexer.hc 逻辑）
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

// ============================================================
// 关键字字典表（Map<关键字名, 关键字种类>）
// 使用 Map 替代 if-else 链，O(1) 查找，更易维护
// ============================================================

fn build_kw_map() Map<&[u8], &[u8]> {
    var m = Map<&[u8], &[u8]>.init(alloc);
    m.put("and", "KwAnd");
    m.put("anytype", "KwAnytype");
    m.put("async", "KwAsync");
    m.put("await", "KwAwait");
    m.put("break", "KwBreak");
    m.put("catch", "KwCatch");
    m.put("class", "KwClass");
    m.put("comptime", "KwComptime");
    m.put("const", "KwConst");
    m.put("continue", "KwContinue");
    m.put("defer", "KwDefer");
    m.put("else", "KwElse");
    m.put("enum", "KwEnum");
    m.put("errdefer", "KwErrdefer");
    m.put("export", "KwExport");
    m.put("extern", "KwExtern");
    m.put("false", "KwFalse");
    m.put("fn", "KwFn");
    m.put("for", "KwFor");
    m.put("global", "KwGlobal");
    m.put("if", "KwIf");
    m.put("import", "KwImport");
    m.put("interface", "KwInterface");
    m.put("move", "KwMove");
    m.put("mut", "KwMut");
    m.put("namespace", "KwNamespace");
    m.put("null", "KwNull");
    m.put("or", "KwOr");
    m.put("orelse", "KwOrelse");
    m.put("owned", "KwOwned");
    m.put("pub", "KwPub");
    m.put("return", "KwReturn");
    m.put("script", "KwScript");
    m.put("spawn", "KwSpawn");
    m.put("switch", "KwSwitch");
    m.put("tree", "KwTree");
    m.put("true", "KwTrue");
    m.put("try", "KwTry");
    m.put("type", "KwType");
    m.put("union", "KwUnion");
    m.put("import", "KwImport");
    m.put("var", "KwVar");
    m.put("void", "KwVoid");
    m.put("where", "KwWhere");
    m.put("while", "KwWhile");
    return m;
}

// 反向字典：关键字种类 -> 关键字名（用于 expect_name_or_keyword）
fn build_rev_kw_map() Map<&[u8], &[u8]> {
    var m = Map<&[u8], &[u8]>.init(alloc);
    m.put("KwAnd", "and");
    m.put("KwAnytype", "anytype");
    m.put("KwAsync", "async");
    m.put("KwAwait", "await");
    m.put("KwBreak", "break");
    m.put("KwCatch", "catch");
    m.put("KwClass", "class");
    m.put("KwComptime", "comptime");
    m.put("KwConst", "const");
    m.put("KwContinue", "continue");
    m.put("KwDefer", "defer");
    m.put("KwElse", "else");
    m.put("KwEnum", "enum");
    m.put("KwErrdefer", "errdefer");
    m.put("KwExport", "export");
    m.put("KwExtern", "extern");
    m.put("KwFalse", "false");
    m.put("KwFn", "fn");
    m.put("KwFor", "for");
    m.put("KwGlobal", "global");
    m.put("KwIf", "if");
    m.put("KwImport", "import");
    m.put("KwInterface", "interface");
    m.put("KwMove", "move");
    m.put("KwMut", "mut");
    m.put("KwNamespace", "namespace");
    m.put("KwNull", "null");
    m.put("KwOr", "or");
    m.put("KwOrelse", "orelse");
    m.put("KwOwned", "owned");
    m.put("KwPub", "pub");
    m.put("KwReturn", "return");
    m.put("KwScript", "script");
    m.put("KwSpawn", "spawn");
    m.put("KwSwitch", "switch");
    m.put("KwTree", "tree");
    m.put("KwTrue", "true");
    m.put("KwTry", "try");
    m.put("KwType", "type");
    m.put("KwUnion", "union");
    m.put("KwImport", "import");
    m.put("KwVar", "var");
    m.put("KwVoid", "void");
    m.put("KwWhere", "where");
    m.put("KwWhile", "while");
    return m;
}

// ============================================================
// 词法分析器（Token 流）
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
    kw_map: Map<&[u8], &[u8]>,

    fn kw_of(self: *mut Self, name: &[u8]) ?&[u8] {
        if (self.kw_map.contains(name)) {
            return self.kw_map.get(name).?;
        }
        return null;
    }

    fn bump(self: *mut Self) void {
        if (self.src[self.pos] == '\n') {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        // inline utf8_width for performance (most chars are ASCII)
        var c = self.src[self.pos];
        if (c < 0x80) { self.pos += 1; }
        else if (c < 0xE0) { self.pos += 2; }
        else if (c < 0xF0) { self.pos += 3; }
        else { self.pos += 4; }
    }

    // append_char 已内联到调用处（避免参数 mut 关键字）

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
        var kw = self.kw_of(name);
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
// AST 类型定义
// ============================================================

// 简单的 AST：用 Vec<u8> 存储节点类型名，属性用 Vec<Prop> 字符串键值对
// 输出格式：NodeType|key=val|key=val\n  children (indented)

// 简单的 AST：用 Vec<u8> 存储节点类型名，属性用 Vec<u8> 字符串键值对
// 输出格式：NodeType|key=val|key=val\n  children (indented)

class AstNode {
    kind: &[u8],
    // props: flat Vec<u8> with key=value pairs separated by null
    props: Vec<u8>,
    // children
    children: Vec<AstNode>,
}

fn make_node(kind: &[u8]) AstNode {
    var n = AstNode{
        kind = kind,
        props = Vec<u8>.init(alloc),
        children = Vec<AstNode>.init(alloc),
    };
    return n;
}

fn node_add_prop(node: *mut AstNode, key: &[u8], val: &[u8]) void {
    // encode: |key=value
    node.props.append('|');
    var mut i: usize = 0;
    while (i < key.len) {
        node.props.append(key[i]);
        i += 1;
    }
    node.props.append('=');
    i = 0;
    while (i < val.len) {
        node.props.append(val[i]);
        i += 1;
    }
}

fn node_add_child(node: *mut AstNode, child: AstNode) void {
    node.children.append(child);
}

fn quoted_add_prop(node: *mut AstNode, key: &[u8], val: &[u8]) void {
    node.props.append('|');
    var mut i: usize = 0;
    while (i < key.len) {
        node.props.append(key[i]);
        i += 1;
    }
    node.props.append('=');
    node.props.append('"');
    i = 0;
    while (i < val.len) {
        node.props.append(val[i]);
        i += 1;
    }
    node.props.append('"');
}

// 逐字节切片比较（与 checker.hc 同实现；== 对运行时堆子切片不可靠）
fn slice_eq(a: &[u8], b: &[u8]) bool {
    if (a.len != b.len) return false;
    var mut i: usize = 0;
    while (i < a.len) {
        if (a[i] != b[i]) return false;
        i += 1;
    }
    return true;
}

// 从 props 中提取属性值（key=value 格式，用 | 分隔；与 checker.hc 同实现）
fn get_prop(props: &[u8], key: &[u8]) ?&[u8] {
    var mut i: usize = 0;
    var n = props.len;
    while (i < n) {
        if (props[i] == '|') { i += 1; }
        if (i + key.len < n) {
            var mut match_key = true;
            var mut j: usize = 0;
            while (j < key.len) {
                if (props[i + j] != key[j]) { match_key = false; break; }
                j += 1;
            }
            if (match_key and props[i + key.len] == '=') {
                var mut val_start = i + key.len + 1;
                var mut skip_quote = false;
                if (val_start < n and props[val_start] == '"') {
                    skip_quote = true;
                    val_start += 1;
                }
                var mut val_end = val_start;
                while (val_end < n) {
                    if (skip_quote and props[val_end] == '"') { break; }
                    if (!skip_quote and props[val_end] == '|') { break; }
                    val_end += 1;
                }
                return props[val_start..val_end];
            }
        }
        while (i < n and props[i] != '|') { i += 1; }
    }
    return null;
}

// 检查属性是否存在
fn has_prop(props: &[u8], key: &[u8]) bool {
    var v = get_prop(props, key);
    return v != null;
}

// ============================================================
// 解析器（Parser）
// ============================================================

class Parser {
    tokens: Vec<Token>,
    mut pos: usize,
    n: usize,
    rev_kw_map: Map<&[u8], &[u8]>,

    fn peek(self: *mut Self) &[u8] {
        var tok = self.tokens[self.pos];
        return tok.kind;
    }

    fn peek_n(self: *mut Self, n: usize) &[u8] {
        var mut idx = self.pos + n;
        if (idx >= self.n) { idx = self.n - 1; }
        var tok = self.tokens[idx];
        return tok.kind;
    }

    // 判定 `Ident<` 是否为泛型实参（匹配 `>` 后跟 . ( { 时为真；否则视为小于号）
    // 遇语句边界（; { } and or if/while/for/return 等）即判定非泛型，避免跨语句误扫；
    // Shr（>>）在深度 ≥2 时视为嵌套泛型闭合
    fn generic_args_ahead(self: *mut Self) bool {
        var mut i: usize = self.pos + 1;
        var mut depth: usize = 1;
        while (i < self.n and depth > 0) {
            var k2 = self.tokens[i].kind;
            if (k2 == "Lt") { depth += 1; }
            else if (k2 == "Gt") { depth -= 1; }
            else if (k2 == "Shr") {
                if (depth < 2) { return false; }
                depth -= 2;
            }
            else if (k2 == "Semi" or k2 == "LBrace" or k2 == "RBrace" or k2 == "KwAnd" or k2 == "KwOr" or k2 == "KwIf" or k2 == "KwWhile" or k2 == "KwFor" or k2 == "KwReturn" or k2 == "KwFn" or k2 == "KwClass") { return false; }
            i += 1;
        }
        if (depth != 0) { return false; }
        if (i >= self.n) { return false; }
        var nk = self.tokens[i].kind;
        return nk == "Dot" or nk == "LParen" or nk == "LBrace";
    }

    fn peek_text(self: *mut Self) &[u8] {
        var tok = self.tokens[self.pos];
        return tok.text.as_slice();
    }

    fn at(self: *mut Self, kind: &[u8]) bool {
        return self.peek() == kind;
    }

    fn text_eq(self: *mut Self, s: &[u8]) bool {
        return self.peek_text() == s;
    }

    fn at_any(self: *mut Self, kinds: &[&[u8]]) bool {
        var mut i: usize = 0;
        while (i < kinds.len) {
            if (self.at(kinds[i])) return true;
            i += 1;
        }
        return false;
    }

    fn advance(self: *mut Self) Token {
        var t = self.tokens[self.pos];
        if (self.pos < self.n - 1) { self.pos += 1; }
        return t;
    }

    fn expect(self: *mut Self, kind: &[u8]) bool {
        if (self.at(kind)) {
            self.advance();
            return true;
        }
        return false;
    }

    fn expect_ident(self: *mut Self) &[u8] {
        if (self.at("Ident")) {
            var txt = self.peek_text();
            self.advance();
            return txt;
        }
        // 关键字也可作标识符（如 `type` 作字段名）
        var txt = self.peek_text();
        self.advance();
        return txt;
    }

    fn expect_name_or_keyword(self: *mut Self) &[u8] {
        var k = self.peek();
        if (k == "Ident") {
            var txt = self.peek_text();
            self.advance();
            return txt;
        }
        // 关键字可作点号字段名，用反向字典 O(1) 查找
        var txt = self.peek_text();
        if (self.rev_kw_map.contains(k)) {
            var name = self.rev_kw_map.get(k).?;
            self.advance();
            return name;
        }
        return txt;
    }

    // ---------- 程序入口 ----------

    fn parse_program(self: *mut Self) AstNode {
        var prog = make_node("Program");
        while (!self.at("Eof")) {
            var decl = self.parse_decl();
            node_add_child(&prog, decl);
        }
        return prog;
    }

    // ---------- 声明解析 ----------

    fn parse_decl(self: *mut Self) AstNode {
        // pub
        var mut is_pub = false;
        if (self.at("KwPub")) { is_pub = true; self.advance(); }
        // export
        var mut is_export = false;
        if (self.at("KwExport")) { is_export = true; self.advance(); }
        // [pad] [align(T)] [Test]
        var traits = Vec<&[u8]>.init(alloc);
        while (self.at("LBracket")) {
            var t = self.parse_trait();
            if (t) |tr| { traits.append(tr); }
        }

        var k = self.peek();
        if (k == "KwGlobal") {
            self.advance();
            return self.parse_global(is_pub);
        }
        if (k == "KwConst") {
            self.advance();
            return self.parse_const(is_pub);
        }
        if (k == "KwAsync") {
            self.advance();
            self.expect("KwFn");
            return self.finish_fn_decl(traits, is_pub, true, is_export);
        }
        if (k == "KwExtern") {
            self.advance();
            return self.parse_extern_fn(is_pub);
        }
        if (k == "KwFn") {
            self.advance();
            return self.finish_fn_decl(traits, is_pub, false, is_export);
        }
        if (k == "KwClass" or k == "KwTree") {
            self.advance();
            return self.parse_class(is_pub);
        }
        if (k == "KwEnum") {
            self.advance();
            return self.parse_enum(is_pub);
        }
        if (k == "KwUnion") {
            self.advance();
            return self.parse_union(is_pub);
        }
        if (k == "KwInterface") {
            self.advance();
            return self.parse_interface(is_pub);
        }
        if (k == "KwNamespace") {
            self.advance();
            var name = self.expect_ident();
            self.expect("LBrace");
            var ns = make_node("Namespace");
            node_add_prop(&ns, "name", name);
            if (is_pub) { node_add_prop(&ns, "pub", "true"); }
            while (!self.at("RBrace") and !self.at("Eof")) {
                var d = self.parse_decl();
                node_add_child(&ns, d);
            }
            self.expect("RBrace");
            return ns;
        }
        if (k == "KwImport") {
            self.advance();
            var path = self.parse_path();
            // 选择集：import H.std.{io}（parse_path 已消费 `{` 前的点）
            if (self.at("LBrace")) {
                self.advance();
                while (!self.at("RBrace") and !self.at("Eof")) {
                    var _ = self.expect_name_or_keyword();
                    if (self.at("Ident") and self.peek_text() == "as") {
                        self.advance();
                        var _a = self.expect_ident();
                    }
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
                self.expect("RBrace");
            }
            var mut alias: ?&[u8] = null;
            if (self.at("Ident") and self.peek_text() == "as") {
                self.advance();
                alias = self.expect_ident();
            }
            self.expect("Semi");
            var u = make_node("Import");
            node_add_prop(&u, "path", path);
            if (alias) |a| { node_add_prop(&u, "alias", a); }
            return u;
        }
        if (k == "KwScript") {
            self.advance();
            self.parse_block();
            var sc = make_node("Script");
            return sc;
        }
        if (k == "KwComptime") {
            self.advance();
            self.parse_block();
            var cp = make_node("Comptime");
            return cp;
        }
        // 未知声明 → 空节点并且推进当前 token 防止无限循环
        self.advance();
        return make_node("UnknownDecl");
    }

    fn parse_trait(self: *mut Self) ?&[u8] {
        self.expect("LBracket");
        var name = self.expect_ident();
        if (name == "continuous") { self.expect("RBracket"); return "continuous"; }
        if (name == "pad") { self.expect("RBracket"); return "pad"; }
        if (name == "module") { self.expect("RBracket"); return "module"; }
        if (name == "test") {
            if (self.at("LParen")) {
                self.advance();
                if (self.at("Str")) { self.advance(); }
                self.expect("RParen");
            }
            self.expect("RBracket");
            return "test";
        }
        if (name == "align") {
            self.expect("LParen");
            self.parse_type();
            self.expect("RParen");
            self.expect("RBracket");
            return "align";
        }
        self.expect("RBracket");
        return null;
    }

    fn parse_global(self: *mut Self, is_pub: bool) AstNode {
        var name = self.expect_ident();
        var g = make_node("Global");
        node_add_prop(&g, "name", name);
        if (is_pub) { node_add_prop(&g, "pub", "true"); }
        if (self.at("Colon")) {
            self.advance();
            self.parse_type(); // ty info consumed
        }
        if (self.at("Eq")) {
            self.advance();
            self.parse_expr();
            node_add_prop(&g, "has_init", "true");
        }
        self.expect("Semi");
        return g;
    }

    fn parse_const(self: *mut Self, is_pub: bool) AstNode {
        var name = self.expect_ident();
        var c = make_node("Const");
        node_add_prop(&c, "name", name);
        if (is_pub) { node_add_prop(&c, "pub", "true"); }
        //  error{...}
        if (self.at("Ident") and self.peek_text() == "error" and self.peek_n(1) == "LBrace") {
            self.advance();
            self.advance();
            while (!self.at("RBrace") and !self.at("Eof")) {
                self.expect_ident();
                if (self.at("Comma")) { self.advance(); }
            }
            self.expect("RBrace");
            self.expect("Semi");
            return c;
        }
        self.expect("Eq");
        self.parse_expr();
        self.expect("Semi");
        return c;
    }

    fn finish_fn_decl(self: *mut Self, traits: Vec<&[u8]>, is_pub: bool, is_async: bool, is_export: bool) AstNode {
        var name = self.expect_ident();
        var f = make_node("Fn");
        node_add_prop(&f, "name", name);
        if (is_pub) { node_add_prop(&f, "pub", "true"); }
        if (is_async) { node_add_prop(&f, "async", "true"); }
        if (is_export) { node_add_prop(&f, "exported", "true"); }
        // 检查 [test] 特性
        var mut i: usize = 0;
        while (i < traits.len) {
            if (traits[i] == "test") {
                node_add_prop(&f, "test", "true");
            }
            i += 1;
        }
        // 泛型参数 <T>
        if (self.at("Lt")) {
            self.advance();
            while (!self.at("Gt") and !self.at("Eof")) {
                self.expect_ident();
                if (self.at("Comma")) { self.advance(); }
            }
            self.expect("Gt");
        }
        // 参数 (params)
        self.expect("LParen");
        if (!self.at("RParen")) {
            while (true) {
                var p = self.parse_param();
                node_add_child(&f, p);
                if (self.at("Comma")) { self.advance(); }
                else { break; }
            }
        }
        self.expect("RParen");
        // 返回类型
        if (self.at("Bang")) {
            self.advance();
            if (self.at("Ident") or self.at("KwVoid")) {
                var mut ret_ty = self.peek_text();
                self.advance();
                self.consume_type_args();
                var r = make_node("ret:");
                var mut k: usize = 0;
                while (k < ret_ty.len) {
                    r.props.append(ret_ty[k]);
                    k += 1;
                }
                node_add_child(&f, r);
            } else {
                self.parse_type();
            }
        } else if (self.at("KwVoid") or self.at("Ident")) {
            var mut ret_ty = self.peek_text();
            // 关键字（如 void）的 text 为空，直接用关键字名
            if (ret_ty.len == 0) {
                if (self.at("KwVoid")) { ret_ty = "void"; }
            }
            self.advance();
            self.consume_type_args();
            var r = make_node("ret:");
            var mut k: usize = 0;
            while (k < ret_ty.len) {
                r.props.append(ret_ty[k]);
                k += 1;
            }
            node_add_child(&f, r);
        } else if (!self.at("LBrace") and !self.at("Semi") and !self.at("Eof")) {
            // 复杂类型（如 ?&[u8]、*[4]u8 等）：仅消费 token
            self.parse_type();
        }
        // where 子句
        if (self.at("KwWhere")) {
            self.advance();
            while (!self.at("LBrace") and !self.at("Semi") and !self.at("Eof")) {
                self.parse_type();
                if (self.at("Comma")) { self.advance(); }
                else { break; }
            }
        }
        // 体部（extern fn 无 body）
        if (self.at("Semi")) {
            self.advance();
            node_add_prop(&f, "extern", "true");
            return f;
        }
        var body = self.parse_block();
        node_add_child(&f, body);
        return f;
    }

    fn parse_extern_fn(self: *mut Self, is_pub: bool) AstNode {
        self.expect("KwFn");
        var name = self.expect_ident();
        var f = make_node("Fn");
        node_add_prop(&f, "name", name);
        node_add_prop(&f, "extern", "true");
        if (is_pub) { node_add_prop(&f, "pub", "true"); }
        self.expect("LParen");
        if (!self.at("RParen")) {
            while (true) {
                var p = self.parse_param();
                node_add_child(&f, p);
                if (self.at("Comma")) { self.advance(); }
                else { break; }
            }
        }
        self.expect("RParen");
        if (self.at("Bang")) {
            self.advance();
            if (self.at("Ident") or self.at("KwVoid")) {
                var ret_ty = self.peek_text();
                self.advance();
                var r = make_node("ret:");
                var mut k: usize = 0;
                while (k < ret_ty.len) {
                    r.props.append(ret_ty[k]);
                    k += 1;
                }
                node_add_child(&f, r);
            } else {
                self.parse_type();
            }
        } else if (self.at("KwVoid") or self.at("Ident")) {
            var ret_ty = self.peek_text();
            self.advance();
            var r = make_node("ret:");
            var mut k: usize = 0;
            while (k < ret_ty.len) {
                r.props.append(ret_ty[k]);
                k += 1;
            }
            node_add_child(&f, r);
        }
        self.expect("Semi");
        return f;
    }

    fn parse_param(self: *mut Self) AstNode {
        // var/mut 前缀（如 var mut out: Vec<u8>）
        var mut is_mut = false;
        if (self.at("KwVar")) { self.advance(); is_mut = true; }
        if (self.at("KwMut")) { self.advance(); is_mut = true; }
        var name = self.expect_ident();
        self.expect("Colon");
        var p = make_node("Param");
        node_add_prop(&p, "name", name);
        if (is_mut) { node_add_prop(&p, "mut", "true"); }
        if (self.at("Ident") or self.at("KwVoid")) {
            var ty = self.peek_text();
            self.advance();
            if (ty.len > 0) {
                quoted_add_prop(&p, "ty", ty);
            } else {
                quoted_add_prop(&p, "ty", "void");
            }
            // 泛型实参仅消费：Type(T1,T2) / Type<T1,T2>
            if (self.at("LParen")) {
                self.advance();
                while (!self.at("RParen") and !self.at("Eof")) {
                    self.parse_type();
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
                self.expect("RParen");
            }
            if (self.at("Lt")) {
                self.advance();
                while (!self.at("Gt") and !self.at("Eof")) {
                    self.parse_type();
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
                self.expect("Gt");
            }
        } else {
            self.parse_type();
        }
        if (self.at("Eq")) {
            self.advance();
            self.parse_expr();
            node_add_prop(&p, "has_default", "true");
        }
        return p;
    }

    fn parse_class(self: *mut Self, is_pub: bool) AstNode {
        var name = self.expect_ident();
        var cls = make_node("Class");
        node_add_prop(&cls, "name", name);
        if (is_pub) { node_add_prop(&cls, "pub", "true"); }
        // 接口
        if (self.at("LParen")) {
            self.advance();
            while (!self.at("RParen") and !self.at("Eof")) {
                self.parse_type();
                if (self.at("Comma")) { self.advance(); }
                else { break; }
            }
            self.expect("RParen");
        }
        // traits
        while (self.at("LBracket")) {
            self.parse_trait();
        }
        self.expect("LBrace");
        // 字段和方法
        while (!self.at("RBrace") and !self.at("Eof")) {
            if (self.at("KwFn") or self.at("LBracket") or (self.at("KwPub") and self.peek_n(1) == "KwFn")) {
                // 方法
                var m = self.parse_method(name);
                node_add_child(&cls, m);
            } else {
                // 字段
                var f = self.parse_field();
                node_add_child(&cls, f);
            }
        }
        self.expect("RBrace");
        return cls;
    }

    fn parse_field(self: *mut Self) AstNode {
        var mut is_fpub = false;
        if (self.at("KwPub")) { is_fpub = true; self.advance(); }
        var mut is_mut = false;
        if (self.at("KwMut")) { is_mut = true; self.advance(); }
        var name = self.expect_ident();
        var f = make_node("FieldDecl");
        node_add_prop(&f, "name", name);
        if (is_mut) { node_add_prop(&f, "mut", "true"); }
        if (is_fpub) { node_add_prop(&f, "pub", "true"); }
        self.expect("Colon");
        // 简单 Ident 类型存 ty prop（对齐 Param 模式）；其余类型仅消费 token
        if (self.at("Ident")) {
            var ty = self.peek_text();
            self.advance();
            if (ty.len > 0) {
                quoted_add_prop(&f, "ty", ty);
            }
            // 泛型实参仅消费：Type(T1,T2) / Type<T1,T2>
            if (self.at("LParen")) {
                self.advance();
                while (!self.at("RParen") and !self.at("Eof")) {
                    self.parse_type();
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
                self.expect("RParen");
            }
            if (self.at("Lt")) {
                self.advance();
                while (!self.at("Gt") and !self.at("Eof")) {
                    self.parse_type();
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
                self.expect("Gt");
            }
        } else {
            self.parse_type();
        }
        // 分隔容错：逗号/分号均可
        if (self.at("Comma") or self.at("Semi")) { self.advance(); }
        return f;
    }

    fn parse_method(self: *mut Self, cls_name: &[u8]) AstNode {
        // traits
        var mut traits = Vec<&[u8]>.init(alloc);
        while (self.at("LBracket")) {
            var t = self.parse_trait();
            if (t) |tn| { traits.append(tn); }
        }
        var mut is_pub = false;
        if (self.at("KwPub")) { is_pub = true; self.advance(); }
        self.expect("KwFn");
        var f = self.finish_fn_decl(traits, is_pub, false, false);
        node_add_prop(&f, "method", cls_name);
        return f;
    }

    fn parse_enum(self: *mut Self, is_pub: bool) AstNode {
        var name = self.expect_ident();
        var e = make_node("Enum");
        node_add_prop(&e, "name", name);
        if (is_pub) { node_add_prop(&e, "pub", "true"); }
        self.expect("LBrace");
        while (!self.at("RBrace") and !self.at("Eof")) {
            var vname = self.expect_ident();
            var v = make_node("Variant");
            node_add_prop(&v, "name", vname);
            if (self.at("LParen")) {
                self.advance();
                self.parse_type();
                self.expect("RParen");
            }
            node_add_child(&e, v);
            if (self.at("Comma")) { self.advance(); }
        }
        self.expect("RBrace");
        return e;
    }

    fn parse_union(self: *mut Self, is_pub: bool) AstNode {
        var name = self.expect_ident();
        var u = make_node("Union");
        node_add_prop(&u, "name", name);
        if (is_pub) { node_add_prop(&u, "pub", "true"); }
        self.expect("LBrace");
        while (!self.at("RBrace") and !self.at("Eof")) {
            var fname = self.expect_ident();
            self.expect("Colon");
            self.parse_type();
            self.expect("Semi");
        }
        self.expect("RBrace");
        return u;
    }

    fn parse_interface(self: *mut Self, is_pub: bool) AstNode {
        var name = self.expect_ident();
        var iface = make_node("Interface");
        node_add_prop(&iface, "name", name);
        if (is_pub) { node_add_prop(&iface, "pub", "true"); }
        self.expect("LBrace");
        while (!self.at("RBrace") and !self.at("Eof")) {
            self.expect("KwFn");
            var mname = self.expect_ident();
            self.expect("LParen");
            if (!self.at("RParen")) {
                while (true) {
                    var _ = self.parse_param();
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
            }
            self.expect("RParen");
            if (self.at("Bang")) {
                self.advance();
                self.parse_type();
            } else if (self.at("KwVoid") or self.at("Ident")) {
                self.advance();
            }
            self.expect("Semi");
        }
        self.expect("RBrace");
        return iface;
    }

    fn parse_path(self: *mut Self) Vec<u8> {
        var parts = Vec<u8>.init(alloc);
        var first = self.expect_ident();
        var mut i: usize = 0;
        while (i < first.len) {
            parts.append(first[i]);
            i += 1;
        }
        while (self.at("Dot")) {
            self.advance();
            parts.append('.');
            var seg = self.expect_name_or_keyword();
            var mut j: usize = 0;
            while (j < seg.len) {
                parts.append(seg[j]);
                j += 1;
            }
        }
        return parts;
    }

    fn parse_import_path(self: *mut Self) Vec<u8> {
        return self.parse_path();
    }

    // ============================================================
    // 类型解析
    // ============================================================

    fn parse_type(self: *mut Self) void {
        // owned T
        if (self.at("KwOwned")) {
            self.advance();
            self.parse_type();
            return;
        }
        // *T / *mut T
        if (self.at("Star")) {
            self.advance();
            if (self.at("KwMut")) { self.advance(); }
            self.parse_type();
            return;
        }
        // &[T] / &mut [T] 或 &T
        if (self.at("Amp")) {
            self.advance();
            if (self.at("KwMut")) { self.advance(); }
            if (self.at("LBracket")) {
                self.advance();
                self.parse_type();
                self.expect("RBracket");
            } else {
                self.parse_type();
            }
            return;
        }
        // ?T
        if (self.at("Question")) {
            self.advance();
            self.parse_type();
            return;
        }
        // !T（anyerror）
        if (self.at("Bang")) {
            self.advance();
            self.parse_type();
            return;
        }
        // 基础类型
        self.parse_type_base();
        // E!T（命名错误集）
        if (self.at("Bang")) {
            self.advance();
            self.parse_type();
        }
    }

    fn parse_type_base(self: *mut Self) void {
        if (self.at("Ident")) {
            var name = self.peek_text();
            self.advance();
            // 泛型实参：Type(T1, T2)
            if (self.at("LParen")) {
                self.advance();
                while (!self.at("RParen") and !self.at("Eof")) {
                    self.parse_type();
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
                self.expect("RParen");
            }
            // 泛型实参：Type<T1, T2>
            if (self.at("Lt")) {
                self.advance();
                while (!self.at("Gt") and !self.at("Eof")) {
                    self.parse_type();
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
                self.expect("Gt");
            }
        } else if (self.at("LBracket")) {
            // [N]T 定长数组
            self.advance();
            self.parse_expr();
            self.expect("RBracket");
            self.parse_type();
        } else if (self.at("LParen")) {
            // 元组
            self.advance();
            while (!self.at("RParen") and !self.at("Eof")) {
                self.parse_type();
                if (self.at("Comma")) { self.advance(); }
                else { break; }
            }
            self.expect("RParen");
        } else if (self.at("KwClass")) {
            // struct { ... } 类型字面量
            self.advance();
            self.expect("LBrace");
            if (!self.at("RBrace")) {
                while (true) {
                    self.expect_ident();
                    self.expect("Colon");
                    self.parse_type();
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
            }
            self.expect("RBrace");
        } else {
            // 关键字作类型名（如 void, type 等）
            self.advance();
        }
    }

    // ============================================================
    // 语句解析
    // ============================================================

    fn parse_block(self: *mut Self) AstNode {
        var b = make_node("Block");
        // `{` 缺失时返回空块且不消费（防失控吞并；无括号体由 parse_block_or_stmt 包装）
        if (!self.at("LBrace")) { return b; }
        self.advance();
        while (!self.at("RBrace") and !self.at("Eof")) {
            var stmt = self.parse_stmt();
            node_add_child(&b, stmt);
        }
        self.expect("RBrace");
        return b;
    }

    // 块或单语句体：`if (c) stmt;` 无括号形式包装成 Block
    fn parse_block_or_stmt(self: *mut Self) AstNode {
        if (self.at("LBrace")) { return self.parse_block(); }
        var b = make_node("Block");
        var stmt = self.parse_stmt();
        node_add_child(&b, stmt);
        return b;
    }

    // 泛型实参仅消费：Type(T1,T2) / Type<T1,T2>
    fn consume_type_args(self: *mut Self) void {
        if (self.at("LParen")) {
            self.advance();
            while (!self.at("RParen") and !self.at("Eof")) {
                self.parse_type();
                if (self.at("Comma")) { self.advance(); }
                else { break; }
            }
            self.expect("RParen");
        }
        if (self.at("Lt")) {
            self.advance();
            while (!self.at("Gt") and !self.at("Eof")) {
                self.parse_type();
                if (self.at("Comma")) { self.advance(); }
                else { break; }
            }
            self.expect("Gt");
        }
    }

    fn parse_stmt(self: *mut Self) AstNode {
        // 循环标签
        if (self.at("Colon")) {
            self.advance();
            if (self.at("Ident")) { self.advance(); }
            if (self.at("KwWhile") or self.at("KwFor")) {
                // 标签后跟 while/for
            }
        }
        var k = self.peek();
        if (k == "LBrace") {
            return self.parse_block();
        }
        if (k == "Semi") {
            self.advance();
            return make_node("Empty");
        }
        if (k == "KwVar") {
            self.advance();
            return self.parse_var_decl();
        }
        if (k == "KwConst") {
            self.advance();
            var name = self.expect_ident();
            self.expect("Eq");
            self.parse_expr();
            self.expect("Semi");
            var c = make_node("ConstDecl");
            node_add_prop(&c, "name", name);
            return c;
        }
        if (k == "KwIf") {
            return self.parse_if_stmt();
        }
        if (k == "KwWhile") {
            return self.parse_while_stmt();
        }
        if (k == "KwFor") {
            return self.parse_for_stmt();
        }
        if (k == "KwSwitch") {
            return self.parse_switch_stmt();
        }
        if (k == "KwReturn") {
            self.advance();
            var r = make_node("Return");
            if (!self.at("Semi")) {
                var val = self.parse_expr();
                node_add_child(&r, val);
            }
            self.expect("Semi");
            return r;
        }
        if (k == "KwBreak") {
            self.advance();
            var b = make_node("Break");
            self.expect("Semi");
            return b;
        }
        if (k == "KwContinue") {
            self.advance();
            var c = make_node("Continue");
            self.expect("Semi");
            return c;
        }
        if (k == "KwDefer") {
            self.advance();
            self.parse_expr();
            self.expect("Semi");
            return make_node("Defer");
        }
        if (k == "KwErrdefer") {
            self.advance();
            self.parse_expr();
            self.expect("Semi");
            return make_node("Errdefer");
        }
        // 默认：表达式语句（含赋值：target = / += / -= / *= / /= value）
        var e = self.parse_expr();
        var ak = self.peek();
        if (ak == "Eq" or ak == "PlusEq" or ak == "MinusEq" or ak == "StarEq" or ak == "SlashEq") {
            self.advance();
            var rhs = self.parse_expr();
            var a = make_node("Assign");
            if (ak == "Eq") { node_add_prop(&a, "op", "Eq"); }
            else if (ak == "PlusEq") { node_add_prop(&a, "op", "PlusEq"); }
            else if (ak == "MinusEq") { node_add_prop(&a, "op", "MinusEq"); }
            else if (ak == "StarEq") { node_add_prop(&a, "op", "StarEq"); }
            else { node_add_prop(&a, "op", "SlashEq"); }
            node_add_child(&a, e);
            node_add_child(&a, rhs);
            self.expect("Semi");
            var aes = make_node("ExprStmt");
            node_add_child(&aes, a);
            return aes;
        }
        self.expect("Semi");
        var es = make_node("ExprStmt");
        node_add_child(&es, e);
        return es;
    }

    fn parse_var_decl(self: *mut Self) AstNode {
        var mut is_mut = false;
        if (self.at("KwMut")) { is_mut = true; self.advance(); }
        var name = self.expect_ident();
        var v = make_node("VarDecl");
        node_add_prop(&v, "name", name);
        if (is_mut) { node_add_prop(&v, "mut", "true"); }
        if (self.at("Colon")) {
            self.advance();
            if (self.at("Ident") or self.at("KwVoid")) {
                var ty = self.peek_text();
                self.advance();
                if (ty.len > 0) {
                    quoted_add_prop(&v, "ty", ty);
                } else {
                    quoted_add_prop(&v, "ty", "void");
                }
            } else {
                self.parse_type();
            }
        }
        if (self.at("Eq")) {
            self.advance();
            var init = self.parse_expr();
            node_add_child(&v, init);
            node_add_prop(&v, "has_init", "true");
        }
        self.expect("Semi");
        return v;
    }

    fn parse_if_stmt(self: *mut Self) AstNode {
        self.advance();
        var ifn = make_node("If");
        self.expect("LParen");
        var cond = self.parse_expr();
        node_add_child(&ifn, cond);
        self.expect("RParen");
        // 载荷捕获（后置）：if (opt) |v| / if (x) |v| |e|
        if (self.at("Pipe")) {
            self.advance();
            var cap = self.expect_ident();
            node_add_prop(&ifn, "payload", cap);
            self.expect("Pipe");
        }
        if (self.at("Pipe")) {
            self.advance();
            var err = self.expect_ident();
            node_add_prop(&ifn, "payload_err", err);
            self.expect("Pipe");
        }
        var then_b = self.parse_block_or_stmt();
        node_add_child(&ifn, then_b);
        if (self.at("KwElse")) {
            self.advance();
            if (self.at("KwIf")) {
                var else_if = self.parse_if_stmt();
                node_add_child(&ifn, else_if);
            } else {
                var else_b = self.parse_block_or_stmt();
                node_add_child(&ifn, else_b);
            }
        }
        return ifn;
    }

    fn parse_while_stmt(self: *mut Self) AstNode {
        self.advance();
        var wn = make_node("While");
        self.expect("LParen");
        var cond = self.parse_expr();
        node_add_child(&wn, cond);
        self.expect("RParen");
        // 载荷捕获（后置）：while (it.next()) |x|
        if (self.at("Pipe")) {
            self.advance();
            var cap = self.expect_ident();
            node_add_prop(&wn, "payload", cap);
            self.expect("Pipe");
        }
        // step 子句
        if (self.at("Colon") and self.peek_n(1) == "LParen") {
            self.advance();
            self.expect("LParen");
            self.parse_expr();
            self.expect("RParen");
        }
        var body = self.parse_block_or_stmt();
        node_add_child(&wn, body);
        return wn;
    }

    fn parse_for_stmt(self: *mut Self) AstNode {
        self.advance();
        var for_node = make_node("For");
        self.expect("LParen");
        if (self.at("KwMut")) { self.advance(); }
        var iter = self.parse_expr();
        node_add_child(&for_node, iter);
        self.expect("RParen");
        // 载荷捕获（后置）：for (iter) |item|
        if (self.at("Pipe")) {
            self.advance();
            var cap = self.expect_ident();
            node_add_prop(&for_node, "payload", cap);
            self.expect("Pipe");
        }
        var body = self.parse_block_or_stmt();
        node_add_child(&for_node, body);
        return for_node;
    }

    fn parse_switch_stmt(self: *mut Self) AstNode {
        self.advance();
        var sn = make_node("Switch");
        self.expect("LParen");
        var subj = self.parse_expr();
        node_add_child(&sn, subj);
        self.expect("RParen");
        self.expect("LBrace");
        while (!self.at("RBrace") and !self.at("Eof")) {
            var arm = self.parse_switch_arm();
            node_add_child(&sn, arm);
        }
        self.expect("RBrace");
        return sn;
    }

    fn parse_switch_arm(self: *mut Self) AstNode {
        var arm = make_node("SwitchArm");
        // 模式列表
        while (!self.at("FatArrow") and !self.at("RBrace") and !self.at("Eof")) {
            var pat = self.parse_switch_pattern();
            node_add_child(&arm, pat);
            if (self.at("Comma")) { self.advance(); break; }
        }
        self.expect("FatArrow");
        // 守卫
        if (self.at("KwIf")) {
            self.advance();
            self.parse_expr();
        }
        // 体（块或表达式）
        if (self.at("LBrace")) {
            var body = self.parse_block();
            node_add_child(&arm, body);
        } else {
            var e = self.parse_expr();
            var es = make_node("ExprStmt");
            node_add_child(&es, e);
            node_add_child(&arm, es);
        }
        if (self.at("Comma")) { self.advance(); }
        return arm;
    }

    fn parse_switch_pattern(self: *mut Self) AstNode {
        var p = make_node("Pattern");
        if (self.at("KwElse")) {
            self.advance();
            node_add_prop(&p, "else", "true");
        } else if (self.at("Dot")) {
            self.advance();
            var name = self.expect_ident();
            node_add_prop(&p, "error", name);
        } else if (self.at("Ident")) {
            var name = self.peek_text();
            self.advance();
            if (self.at("Dot")) {
                // error.NotFound
                self.advance();
                var err = self.expect_ident();
                node_add_prop(&p, "error", err);
            } else {
                node_add_prop(&p, "ident", name);
            }
        } else if (self.at("Int")) {
            var txt = self.peek_text();
            self.advance();
            node_add_prop(&p, "int", txt);
        } else if (self.at("Float")) {
            var txt = self.peek_text();
            self.advance();
            node_add_prop(&p, "float", txt);
        } else if (self.at("Str")) {
            var txt = self.peek_text();
            self.advance();
            node_add_prop(&p, "str", txt);
        } else if (self.at("Char")) {
            var txt = self.peek_text();
            self.advance();
            node_add_prop(&p, "char", txt);
        } else {
            self.advance();
        }
        return p;
    }

    // ============================================================
    // 表达式解析（递归下降 + 优先级表）
    // ============================================================

    fn parse_expr(self: *mut Self) AstNode {
        return self.parse_or();
    }

    fn parse_or(self: *mut Self) AstNode {
        var mut l = self.parse_and();
        while (self.at("KwOr") or self.at("PipePipe")) {
            self.advance();
            var r = self.parse_and();
            var b = make_node("Binary");
            node_add_prop(&b, "op", "Or");
            node_add_child(&b, l);
            node_add_child(&b, r);
            l = b;
        }
        return l;
    }

    fn parse_and(self: *mut Self) AstNode {
        var mut l = self.parse_range();
        while (self.at("KwAnd")) {
            self.advance();
            var r = self.parse_range();
            var b = make_node("Binary");
            node_add_prop(&b, "op", "And");
            node_add_child(&b, l);
            node_add_child(&b, r);
            l = b;
        }
        return l;
    }

    fn parse_range(self: *mut Self) AstNode {
        var mut l = self.parse_comparison();
        if (self.at("DotDot")) {
            self.advance();
            var r = self.parse_comparison();
            var b = make_node("Binary");
            node_add_prop(&b, "op", "Range");
            node_add_child(&b, l);
            node_add_child(&b, r);
            l = b;
        }
        return l;
    }

    fn parse_comparison(self: *mut Self) AstNode {
        var mut l = self.parse_bitor();
        var cmp_op = self.peek();
        if (cmp_op == "EqEq" or cmp_op == "Ne" or cmp_op == "Lt" or cmp_op == "Le" or cmp_op == "Gt" or cmp_op == "Ge") {
            self.advance();
            var r = self.parse_bitor();
            var b = make_node("Binary");
            if (cmp_op == "EqEq") { node_add_prop(&b, "op", "Eq"); }
            else { node_add_prop(&b, "op", cmp_op); }
            node_add_child(&b, l);
            node_add_child(&b, r);
            l = b;
        }
        return l;
    }

    fn parse_bitor(self: *mut Self) AstNode {
        var mut l = self.parse_bitxor();
        while (self.at("Pipe")) {
            self.advance();
            var r = self.parse_bitxor();
            var b = make_node("Binary");
            node_add_prop(&b, "op", "BitOr");
            node_add_child(&b, l);
            node_add_child(&b, r);
            l = b;
        }
        return l;
    }

    fn parse_bitxor(self: *mut Self) AstNode {
        var mut l = self.parse_bitand();
        while (self.at("Caret")) {
            self.advance();
            var r = self.parse_bitand();
            var b = make_node("Binary");
            node_add_prop(&b, "op", "BitXor");
            node_add_child(&b, l);
            node_add_child(&b, r);
            l = b;
        }
        return l;
    }

    fn parse_bitand(self: *mut Self) AstNode {
        var mut l = self.parse_shift();
        while (self.at("Amp")) {
            self.advance();
            var r = self.parse_shift();
            var b = make_node("Binary");
            node_add_prop(&b, "op", "BitAnd");
            node_add_child(&b, l);
            node_add_child(&b, r);
            l = b;
        }
        return l;
    }

    fn parse_shift(self: *mut Self) AstNode {
        var mut l = self.parse_addsub();
        while (true) {
            var opname = self.peek();
            if (opname == "Shl" or opname == "Shr") {
                self.advance();
                var r = self.parse_addsub();
                var b = make_node("Binary");
                node_add_prop(&b, "op", opname);
                node_add_child(&b, l);
                node_add_child(&b, r);
                l = b;
            } else { break; }
        }
        return l;
    }

    fn parse_addsub(self: *mut Self) AstNode {
        var mut l = self.parse_muldiv();
        while (true) {
            var opname = self.peek();
            if (opname == "Plus" or opname == "Minus") {
                self.advance();
                var r = self.parse_muldiv();
                var b = make_node("Binary");
                if (opname == "Plus") { node_add_prop(&b, "op", "Add"); }
                else { node_add_prop(&b, "op", "Sub"); }
                node_add_child(&b, l);
                node_add_child(&b, r);
                l = b;
            } else { break; }
        }
        return l;
    }

    fn parse_muldiv(self: *mut Self) AstNode {
        var mut l = self.parse_unary();
        while (true) {
            var opname = self.peek();
            if (opname == "Star" or opname == "Slash" or opname == "Percent" or opname == "PercentPercent") {
                self.advance();
                var r = self.parse_unary();
                var b = make_node("Binary");
                if (opname == "Star") { node_add_prop(&b, "op", "Mul"); }
                else if (opname == "Slash") { node_add_prop(&b, "op", "Div"); }
                else if (opname == "Percent") { node_add_prop(&b, "op", "Mod"); }
                else { node_add_prop(&b, "op", "ModMod"); }
                node_add_child(&b, l);
                node_add_child(&b, r);
                l = b;
            } else { break; }
        }
        return l;
    }

    fn parse_unary(self: *mut Self) AstNode {
        var k = self.peek();
        if (k == "Minus") {
            self.advance();
            var mut e = self.parse_unary();
            var u = make_node("Unary");
            node_add_prop(&u, "op", "Neg");
            node_add_child(&u, e);
            return u;
        }
        if (k == "Bang") {
            self.advance();
            var mut e = self.parse_unary();
            var u = make_node("Unary");
            node_add_prop(&u, "op", "Not");
            node_add_child(&u, e);
            return u;
        }
        if (k == "Tilde") {
            self.advance();
            var mut e = self.parse_unary();
            var u = make_node("Unary");
            node_add_prop(&u, "op", "BitNot");
            node_add_child(&u, e);
            return u;
        }
        if (k == "Amp") {
            self.advance();
            var mut is_mut = false;
            if (self.at("KwMut")) { is_mut = true; self.advance(); }
            var mut e = self.parse_unary();
            var a = make_node("AddrOf");
            if (is_mut) { node_add_prop(&a, "mut", "true"); }
            node_add_child(&a, e);
            return a;
        }
        if (k == "KwTry") {
            self.advance();
            var mut e = self.parse_unary();
            var t = make_node("Try");
            node_add_child(&t, e);
            return t;
        }
        if (k == "KwAwait") {
            self.advance();
            var mut e = self.parse_unary();
            var a = make_node("Await");
            node_add_child(&a, e);
            return a;
        }
        if (k == "KwSpawn") {
            self.advance();
            var args = self.parse_call_args();
            var c = make_node("Call");
            var callee = make_node("Ident");
            node_add_prop(&callee, "name", "spawn");
            node_add_child(&c, callee);
            var mut i: usize = 0;
            while (i < args.len) {
                node_add_child(&c, args[i]);
                i += 1;
            }
            return c;
        }
        if (k == "KwMove") {
            self.advance();
            // 闭包
            if (self.at("Pipe") or (self.at("KwMut") and self.peek_n(1) == "Pipe")) {
                return self.parse_closure();
            }
            var mut e = self.parse_unary();
            var m = make_node("Move");
            node_add_child(&m, e);
            return m;
        }
        return self.parse_postfix();
    }

    fn parse_closure(self: *mut Self) AstNode {
        var c = make_node("Closure");
        var mut is_mut = false;
        var is_move = false;
        if (self.at("KwMut")) { is_mut = true; self.advance(); }
        self.expect("Pipe");
        if (!self.at("Pipe")) {
            while (true) {
                var p = self.expect_ident();
                if (self.at("Comma")) { self.advance(); }
                else { break; }
            }
        }
        self.expect("Pipe");
        // 体部
        if (self.at("LBrace")) {
            var body = self.parse_block();
            node_add_child(&c, body);
        } else {
            var mut e = self.parse_expr();
            var es = make_node("ExprStmt");
            node_add_child(&es, e);
            node_add_child(&c, es);
        }
        return c;
    }

    fn parse_postfix(self: *mut Self) AstNode {
        var mut e = self.parse_primary();
        while (true) {
            var kk = self.peek();
            if (kk == "Dot") {
                self.advance();
                if (self.at("Question")) {
                    // .? 链式解包
                    self.advance();
                    var u = make_node("Unwrap");
                    node_add_child(&u, e);
                    e = u;
                } else {
                    var field = self.expect_name_or_keyword();
                    if (self.at("LParen")) {
                        // 方法调用
                        var args = self.parse_call_args();
                        var call = make_node("Call");
                        var fe = make_node("Field");
                        node_add_prop(&fe, "field", field);
                        node_add_child(&fe, e);
                        node_add_child(&call, fe);
                        var mut i: usize = 0;
                        while (i < args.len) {
                            node_add_child(&call, args[i]);
                            i += 1;
                        }
                        e = call;
                    } else {
                        var fe = make_node("Field");
                        node_add_prop(&fe, "field", field);
                        node_add_child(&fe, e);
                        e = fe;
                    }
                }
            } else if (kk == "LBracket") {
                self.advance();
                var idx = self.parse_expr();
                self.expect("RBracket");
                var ie = make_node("Index");
                node_add_child(&ie, e);
                node_add_child(&ie, idx);
                e = ie;
            } else if (kk == "DotStar") {
                self.advance();
                var d = make_node("Deref");
                node_add_child(&d, e);
                e = d;
            } else if (kk == "Question") {
                // 后缀 ? 解包
                self.advance();
                var u = make_node("Unwrap");
                node_add_child(&u, e);
                e = u;
            } else if (kk == "LParen") {
                var args = self.parse_call_args();
                var call = make_node("Call");
                node_add_child(&call, e);
                var mut i: usize = 0;
                while (i < args.len) {
                    node_add_child(&call, args[i]);
                    i += 1;
                }
                e = call;
                // 泛型字面量
                // 泛型字面量：Pair<i32>{...}
                if (self.at("LBrace")) {
                    // 简单处理：跳过字面量字段
                    self.advance();
                    if (!self.at("RBrace")) {
                        while (true) {
                            self.expect_ident();
                            self.expect("Eq");
                            self.parse_expr();
                            if (self.at("Comma")) { self.advance(); }
                            else { break; }
                        }
                    }
                    self.expect("RBrace");
                }
            } else if (kk == "KwOrelse") {
                self.advance();
                var r = self.parse_expr();
                var orelse_node = make_node("Orelse");
                node_add_child(&orelse_node, e);
                node_add_child(&orelse_node, r);
                e = orelse_node;
            } else if (kk == "KwCatch") {
                self.advance();
                var c = make_node("Catch");
                node_add_child(&c, e);
                if (self.at("Pipe")) {
                    self.advance();
                    var name = self.expect_ident();
                    self.expect("Pipe");
                    var body = self.parse_block();
                    var bnode = make_node("Bind");
                    node_add_prop(&bnode, "name", name);
                    node_add_child(&bnode, body);
                    node_add_child(&c, bnode);
                } else {
                    var d = self.parse_expr();
                    var dnode = make_node("Default");
                    node_add_child(&dnode, d);
                    node_add_child(&c, dnode);
                }
                e = c;
            } else {
                break;
            }
        }
        return e;
    }

    fn parse_call_args(self: *mut Self) Vec<AstNode> {
        var args = Vec<AstNode>.init(alloc);
        self.expect("LParen");
        if (!self.at("RParen")) {
            while (true) {
                var a = self.parse_expr();
                args.append(a);
                if (self.at("Comma")) {
                    self.advance();
                    if (self.at("RParen")) { break; }
                } else { break; }
            }
        }
        self.expect("RParen");
        return args;
    }

    fn parse_primary(self: *mut Self) AstNode {
        var k = self.peek();
        // 闭包
        if (k == "Pipe" or (k == "KwMut" and self.peek_n(1) == "Pipe")) {
            return self.parse_closure();
        }
        // 推断枚举值 .variant
        if (k == "Dot") {
            self.advance();
            var variant = self.expect_name_or_keyword();
            var d = make_node("Dot");
            node_add_prop(&d, "field", variant);
            return d;
        }
        // @内建
        if (k == "AtBuiltin") {
            var txt = self.peek_text();
            self.advance();
            var args = self.parse_call_args();
            var call = make_node("Call");
            var callee = make_node("Ident");
            node_add_prop(&callee, "name", txt[0..txt.len]);
            node_add_child(&call, callee);
            var mut i: usize = 0;
            while (i < args.len) {
                node_add_child(&call, args[i]);
                i += 1;
            }
            return call;
        }
        // struct { ... } 类型字面量
        if (k == "KwClass") {
            self.advance();
            self.expect("LBrace");
            var st = make_node("StructType");
            if (!self.at("RBrace")) {
                while (true) {
                    var name = self.expect_ident();
                    if (self.at("Colon")) {
                        self.advance();
                        self.parse_type();
                    } else if (self.at("Eq")) {
                        self.advance();
                        self.parse_expr();
                    }
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
            }
            self.expect("RBrace");
            return st;
        }
        // 字面量
        if (k == "Int") {
            var txt = self.peek_text();
            self.advance();
            var mut n = make_node("IntLit");
            node_add_prop(&n, "text", txt[0..txt.len]);
            return n;
        }
        if (k == "Float") {
            var txt = self.peek_text();
            self.advance();
            var mut n = make_node("FloatLit");
            node_add_prop(&n, "text", txt[0..txt.len]);
            return n;
        }
        if (k == "Str") {
            var txt = self.peek_text();
            self.advance();
            var s = make_node("StrLit");
            node_add_prop(&s, "value", txt[0..txt.len]);
            return s;
        }
        if (k == "RawStr") {
            var txt = self.peek_text();
            self.advance();
            var s = make_node("StrLit");
            node_add_prop(&s, "value", txt[0..txt.len]);
            node_add_prop(&s, "raw", "true");
            return s;
        }
        if (k == "Char") {
            var txt = self.peek_text();
            self.advance();
            var c = make_node("CharLit");
            node_add_prop(&c, "value", txt[0..txt.len]);
            return c;
        }
        if (k == "KwTrue") {
            self.advance();
            var b = make_node("BoolLit");
            node_add_prop(&b, "value", "true");
            return b;
        }
        if (k == "KwFalse") {
            self.advance();
            var b = make_node("BoolLit");
            node_add_prop(&b, "value", "false");
            return b;
        }
        if (k == "KwNull") {
            self.advance();
            return make_node("NullLit");
        }
        if (k == "KwVoid") {
            self.advance();
            return make_node("VoidLit");
        }
        // 标识符
        if (k == "Ident") {
            var name = self.peek_text();
            self.advance();
            // 枚举常量 error.NotFound
            if (name == "error" and self.at("Dot")) {
                self.advance();
                var err = self.expect_ident();
                var e = make_node("ErrorLit");
                node_add_prop(&e, "name", err);
                return e;
            }
            var id = make_node("Ident");
            node_add_prop(&id, "name", name[0..name.len]);
            // 类字面量：Type{field = val, ...}
            if (self.at("LBrace")) {
                self.advance();
                var cl = make_node("ClassLit");
                node_add_prop(&cl, "name", name[0..name.len]);
                while (!self.at("RBrace") and !self.at("Eof")) {
                    var fname = self.expect_name_or_keyword();
                    var fi = make_node("FieldInit");
                    node_add_prop(&fi, "name", fname);
                    if (self.at("Eq")) {
                        self.advance();
                        var vexpr = self.parse_expr();
                        node_add_child(&fi, vexpr);
                    }
                    node_add_child(&cl, fi);
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
                self.expect("RBrace");
                return cl;
            }
            // 泛型类型表达式：Vec<u8>.init(...) / Vec<Vec<u8>>.init（仅当匹配 `>` 后跟 . ( { 时消费，避免误吞小于号）
            if (self.at("Lt") and self.generic_args_ahead()) {
                self.advance();
                while (!self.at("Gt") and !self.at("Shr") and !self.at("Eof")) {
                    self.parse_type();
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
                if (self.at("Shr")) { self.advance(); }
                else { self.expect("Gt"); }
            }
            // 它后面可能跟泛型实参：Type(T1)
            if (self.at("LParen") and self.peek_n(1) != "RParen" and self.peek_n(1) != "Star" and self.peek_n(1) != "Slash" and self.peek_n(1) != "Plus" and self.peek_n(1) != "Minus") {
                // 可能是类型构造或函数调用，由 parse_postfix 处理
                // 但这里不做超前判断，交给调用者
            }
            return id;
        }
        // 错误字面量
        if (k == "KwScript") {
            self.advance();
            self.parse_block();
            return make_node("Script");
        }
        // 块表达式
        if (k == "LBrace") {
            return self.parse_block();
        }
        // 元组/括号表达式
        if (k == "LParen") {
            self.advance();
            var e = self.parse_expr();
            if (self.at("Comma")) {
                // 元组
                var t = make_node("TupleLit");
                node_add_child(&t, e);
                while (self.at("Comma")) {
                    self.advance();
                    var el = self.parse_expr();
                    node_add_child(&t, el);
                }
                self.expect("RParen");
                return t;
            }
            self.expect("RParen");
            return e;
        }
        // 数组字面量
        if (k == "LBracket") {
            self.advance();
            var arr = make_node("ArrayLit");
            if (!self.at("RBracket")) {
                while (true) {
                    var e = self.parse_expr();
                    node_add_child(&arr, e);
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
            }
            self.expect("RBracket");
            return arr;
        }
        // 错误：跳过
        self.advance();
        return make_node("Unknown");
    }
}

// ============================================================
// AST 输出（dump 函数，与 Rust `hc parse` 格式一致）
// ============================================================

class AstDumper {
    mut buf: Vec<u8>,

    fn dump(self: *mut Self, node: AstNode, depth: i32) void {
        var mut i = 0;
        while (i < depth * 2) {
            self.buf.append(' ');
            i += 1;
        }
        var mut kind_str = node.kind;
        // Handle ret: nodes specially
        if (kind_str == "ret:") {
            self.buf.append('r'); self.buf.append('e'); self.buf.append('t'); self.buf.append(':'); self.buf.append(' ');
            if (node.props.len > 0) {
                var s = node.props.as_slice();
                self.buf.append('"');
                var mut j: usize = 0;
                while (j < s.len) {
                    self.buf.append(s[j]);
                    j += 1;
                }
                self.buf.append('"');
            }
            self.buf.append('\n');
            return;
        }
        // kind
        var mut j: usize = 0;
        while (j < kind_str.len) {
            self.buf.append(kind_str[j]);
            j += 1;
        }
        // props
        if (node.props.len > 0) {
            var s = node.props.as_slice();
            j = 0;
            while (j < s.len) {
                self.buf.append(s[j]);
                j += 1;
            }
        }
        self.buf.append('\n');
        // children
        var mut ci: usize = 0;
        while (ci < node.children.len) {
            self.dump(node.children[ci], depth + 1);
            ci += 1;
        }
    }
}

// ============================================================
// C2：值模型与环境
// ============================================================

// 运行时值（class+kind 字符串分发，对齐 checker.hc 现行模式；
// 不用枚举负载，规避免 HO 限制）
class Value {
    kind: &[u8],          // "int"|"float"|"bool"|"str"|"void"|"vec"|"map"|"obj"|"fnref"|"null"
    i: i64,               // int / bool（0|1）
    f: f64,               // float
    s: &[u8],             // str（指向存活内存的切片）
    vec: Vec<Value>,      // vec（未用为空）
    map: Map<&[u8], Value>,   // map（未用为空）
    obj: ?ObjInst,        // obj（C8 填充）
    name: &[u8],          // fnref 目标函数名
}

// class 实例（字段平行数组，避免 Value→Env→Value 环）
class ObjInst {
    cls: &[u8],
    field_names: Vec<&[u8]>,
    field_vals: Vec<Value>,
}

fn mk_int(v: i64) Value {
    return Value{
        kind = "int", i = v, f = 0.0, s = "",
        vec = Vec<Value>.init(alloc), map = Map<&[u8], Value>.init(alloc),
        obj = null, name = "",
    };
}
fn mk_float(v: f64) Value {
    return Value{
        kind = "float", i = 0, f = v, s = "",
        vec = Vec<Value>.init(alloc), map = Map<&[u8], Value>.init(alloc),
        obj = null, name = "",
    };
}
fn mk_bool(b: bool) Value {
    var mut iv: i64 = 0;
    if (b) { iv = 1; }
    return Value{
        kind = "bool", i = iv, f = 0.0, s = "",
        vec = Vec<Value>.init(alloc), map = Map<&[u8], Value>.init(alloc),
        obj = null, name = "",
    };
}
fn mk_str(s: &[u8]) Value {
    return Value{
        kind = "str", i = 0, f = 0.0, s = s,
        vec = Vec<Value>.init(alloc), map = Map<&[u8], Value>.init(alloc),
        obj = null, name = "",
    };
}
fn mk_void() Value {
    return Value{
        kind = "void", i = 0, f = 0.0, s = "",
        vec = Vec<Value>.init(alloc), map = Map<&[u8], Value>.init(alloc),
        obj = null, name = "",
    };
}
fn mk_null() Value {
    return Value{
        kind = "null", i = 0, f = 0.0, s = "",
        vec = Vec<Value>.init(alloc), map = Map<&[u8], Value>.init(alloc),
        obj = null, name = "",
    };
}
fn mk_vec(items: Vec<Value>) Value {
    return Value{
        kind = "vec", i = 0, f = 0.0, s = "",
        vec = items, map = Map<&[u8], Value>.init(alloc),
        obj = null, name = "",
    };
}
fn mk_map() Value {
    return Value{
        kind = "map", i = 0, f = 0.0, s = "",
        vec = Vec<Value>.init(alloc), map = Map<&[u8], Value>.init(alloc),
        obj = null, name = "",
    };
}
// 可选值（?T）：i 作 some/none 标志，vec 作 0/1 元素盒（避免 Value 自嵌套类型）
fn mk_opt_some(v: Value) Value {
    var bx = Vec<Value>.init(alloc);
    bx.append(v);
    return Value{
        kind = "opt", i = 1, f = 0.0, s = "",
        vec = bx, map = Map<&[u8], Value>.init(alloc),
        obj = null, name = "",
    };
}
fn mk_opt_none() Value {
    return Value{
        kind = "opt", i = 0, f = 0.0, s = "",
        vec = Vec<Value>.init(alloc), map = Map<&[u8], Value>.init(alloc),
        obj = null, name = "",
    };
}
fn mk_obj(inst: ObjInst) Value {
    return Value{
        kind = "obj", i = 0, f = 0.0, s = "",
        vec = Vec<Value>.init(alloc), map = Map<&[u8], Value>.init(alloc),
        obj = inst, name = "",
    };
}
fn mk_err(ename: &[u8]) Value {
    return Value{
        kind = "err", i = 0, f = 0.0, s = ename,
        vec = Vec<Value>.init(alloc), map = Map<&[u8], Value>.init(alloc),
        obj = null, name = "",
    };
}

// 环境条目
class EnvEntry {
    name: &[u8],
    val: Value,
}

// 作用域栈环境（对齐 checker.hc：扁平存储 + size 回滚 + 逆序线性查找）
class Env {
    entries: Vec<EnvEntry>,
    scope_sizes: Vec<usize>,

    // 推入新作用域
    fn push_scope(self: *mut Self) void {
        self.scope_sizes.append(self.entries.len);
    }

    // 弹出作用域
    fn pop_scope(self: *mut Self) void {
        if (self.scope_sizes.len > 0) {
            var mut target = self.scope_sizes[self.scope_sizes.len - 1];
            self.scope_sizes.remove(self.scope_sizes.len - 1);
            while (self.entries.len > target) {
                self.entries.remove(self.entries.len - 1);
            }
        }
    }

    // 在当前作用域声明名字
    fn declare(self: *mut Self, name: &[u8], val: Value) void {
        self.entries.append(EnvEntry{name = name, val = val});
    }

    // 查找名字（从最内层向外）
    fn lookup(self: *mut Self, name: &[u8]) ?Value {
        var mut i: i64 = @intCast(i64, self.entries.len) - 1;
        while (i >= 0) {
            var entry = self.entries[@intCast(usize, i)];
            if (slice_eq(entry.name, name)) {
                return entry.val;
            }
            i -= 1;
        }
        return null;
    }

    // 就地赋值（找到同名绑定即覆盖；返回是否成功）
    fn assign(self: *mut Self, name: &[u8], val: Value) bool {
        var mut i: i64 = @intCast(i64, self.entries.len) - 1;
        while (i >= 0) {
            if (slice_eq(self.entries[@intCast(usize, i)].name, name)) {
                self.entries[@intCast(usize, i)] = EnvEntry{name = name, val = val};
                return true;
            }
            i -= 1;
        }
        return false;
    }
}

// ============================================================
// C3：表达式求值（字面量/标识符/算术/比较/逻辑短路/赋值 + io.print）
// ============================================================

// 十进制（含 0x 十六进制、下划线分隔）文本 → i64
fn parse_int_text(txt: &[u8]) i64 {
    var mut i: usize = 0;
    var mut neg = false;
    if (i < txt.len and txt[i] == '-') { neg = true; i += 1; }
    var mut base: i64 = 10;
    if (i + 1 < txt.len and txt[i] == '0' and (txt[i + 1] == 'x' or txt[i + 1] == 'X')) {
        base = 16;
        i += 2;
    }
    var mut v: i64 = 0;
    while (i < txt.len) {
        var b = txt[i];
        if (b == '_') { i += 1; continue; }
        var d = hexval(b);
        if (d < 0 or @intCast(i64, d) >= base) { break; }
        v = v * base + @intCast(i64, d);
        i += 1;
    }
    if (neg) { v = -v; }
    return v;
}

// 十进制浮点文本 → f64（不含指数；语料所需切片）
fn parse_float_text(txt: &[u8]) f64 {
    var mut i: usize = 0;
    var mut neg = false;
    if (i < txt.len and txt[i] == '-') { neg = true; i += 1; }
    var mut v: f64 = 0.0;
    while (i < txt.len and txt[i] >= '0' and txt[i] <= '9') {
        v = v * 10.0 + @intCast(f64, txt[i] - '0');
        i += 1;
    }
    if (i < txt.len and txt[i] == '.') {
        i += 1;
        var mut scale: f64 = 0.1;
        while (i < txt.len and txt[i] >= '0' and txt[i] <= '9') {
            v = v + @intCast(f64, txt[i] - '0') * scale;
            scale = scale * 0.1;
            i += 1;
        }
    }
    if (neg) { v = -v; }
    return v;
}

// 字节串追加（与 checker.hc 同款辅助）
fn append_bytes(out: *mut Vec<u8>, s: &[u8]) void {
    var mut i: usize = 0;
    while (i < s.len) {
        out.*.append(s[i]);
        i += 1;
    }
}

// 字节串字典序比较（String.compare；-1/0/1）
fn str_compare(a: &[u8], b: &[u8]) i64 {
    var mut n: usize = a.len;
    if (b.len < n) { n = b.len; }
    var mut i: usize = 0;
    while (i < n) {
        if (a[i] != b[i]) {
            if (a[i] < b[i]) { return -1; }
            return 1;
        }
        i += 1;
    }
    if (a.len < b.len) { return -1; }
    if (a.len > b.len) { return 1; }
    return 0;
}

// i64 追加为十进制字节
fn append_int(v: i64, out: *mut Vec<u8>) void {
    var mut u = v;
    if (u < 0) {
        out.*.append('-');
        u = -u;
    }
    if (u == 0) {
        out.*.append('0');
        return;
    }
    var tmp = Vec<u8>.init(alloc);
    while (u > 0) {
        tmp.append(@intCast(u8, u % 10) + '0');
        u = u / 10;
    }
    var mut i: i64 = @intCast(i64, tmp.len) - 1;
    while (i >= 0) {
        out.*.append(tmp[@intCast(usize, i)]);
        i -= 1;
    }
}

// f64 位数字 → 字符（H 无 float→int 转换内建，用比较链替代）
fn digit_ch(d: f64) u8 {
    if (d < 0.5) { return '0'; }
    if (d < 1.5) { return '1'; }
    if (d < 2.5) { return '2'; }
    if (d < 3.5) { return '3'; }
    if (d < 4.5) { return '4'; }
    if (d < 5.5) { return '5'; }
    if (d < 6.5) { return '6'; }
    if (d < 7.5) { return '7'; }
    if (d < 8.5) { return '8'; }
    return '9';
}

// f64 追加为最短十进制（整数部分 + 去尾零小数；与 Rust Display 对齐到语料所需切片）。
// 无 float→int cast：10 的幂标定 + 逐位减法提取
fn append_float(v: f64, out: *mut Vec<u8>) void {
    var mut x = v;
    if (x < 0.0) {
        out.*.append('-');
        x = -x;
    }
    if (x < 1.0) {
        out.*.append('0');
    } else {
        var mut p = 1.0;
        while (p * 10.0 <= x) { p *= 10.0; }
        while (p >= 1.0) {
            var mut d = 0.0;
            while (x >= p) { x -= p; d += 1.0; }
            out.*.append(digit_ch(d));
            p /= 10.0;
        }
    }
    if (x <= 0.0) { return; }
    // 小数位先入局部缓冲再统一去尾零
    var frac_buf = Vec<u8>.init(alloc);
    var mut n: usize = 0;
    while (n < 12 and x > 0.0000000000001) {
        x *= 10.0;
        var mut d = 0.0;
        while (x >= 1.0) { x -= 1.0; d += 1.0; }
        frac_buf.append(digit_ch(d));
        n += 1;
    }
    while (frac_buf.len > 0 and frac_buf[frac_buf.len - 1] == '0') {
        frac_buf.remove(frac_buf.len - 1);
    }
    if (frac_buf.len > 0) {
        out.*.append('.');
        var mut i: usize = 0;
        while (i < frac_buf.len) {
            out.*.append(frac_buf[i]);
            i += 1;
        }
    }
}

// 值 → 可打印字节（对齐 Rust 参考 stdout 格式）
fn append_value(v: Value, out: *mut Vec<u8>) void {
    if (v.kind == "int") { append_int(v.i, out); }
    else if (v.kind == "float") { append_float(v.f, out); }
    else if (v.kind == "bool") {
        if (v.i == 1) { append_bytes(out, "true"); }
        else { append_bytes(out, "false"); }
    }
    else if (v.kind == "str") { append_bytes(out, v.s); }
    else if (v.kind == "null") { append_bytes(out, "null"); }
    else if (v.kind == "void") { }
}

// 取 Value 的 f64 视图（int 提升为 float）——须在 Interp 之前定义（单遍编译）
fn as_f(v: Value) f64 {
    if (v.kind == "float") { return v.f; }
    return @intCast(f64, v.i);
}

// 执行器（C3：表达式面；控制流 C4、函数 C5、容器 C6+ 递增）
class Interp {
    prog: AstNode,
    env: Env,
    mut flow: &[u8],   // "" 正常 | "break" | "continue"（循环消费）| "return"（函数边界消费）
    mut retv: Value,   // return 载荷
    fns: Map<&[u8], usize>,   // 顶层 fn 注册表（存 prog.children 索引；Map 存类实例跨 put 会被重定位损坏，标量安全）
    classes: Map<&[u8], usize>,   // 类注册表（类名 → prog.children 索引）

    // 找 main 并执行其体（先收集顶层 fn 注册表）
    fn run_main(self: *mut Self) void {
        var mut i: usize = 0;
        while (i < self.prog.children.len) {
            var decl = self.prog.children[i];
            if (decl.kind == "Class") {
                var cn = get_prop(decl.props, "name");
                if (cn) |c| { self.classes.put(c, i); }
            } else if (decl.kind == "Fn") {
                var name = get_prop(decl.props, "name");
                if (name) |nm| {
                    self.fns.put(nm, i);
                    if (slice_eq(nm, "main") and decl.children.len > 0) {
                        var body = decl.children[decl.children.len - 1];
                        if (body.kind == "Block") {
                            self.exec_block(body);
                            if (slice_eq(self.flow, "return")) { self.flow = ""; }
                        }
                        return;
                    }
                }
            }
            i += 1;
        }
    }

    fn exec_block(self: *mut Self, blk: AstNode) void {
        self.env.push_scope();
        var mut i: usize = 0;
        while (i < blk.children.len) {
            if (self.flow.len > 0) { break; }
            self.exec_stmt(blk.children[i]);
            i += 1;
        }
        self.env.pop_scope();
    }

    // 执行块或单语句（if/while/for 体两种形态）
    fn exec_sub(self: *mut Self, node: AstNode) void {
        if (node.kind == "Block") {
            self.exec_block(node);
        } else {
            self.exec_stmt(node);
        }
    }

    fn exec_stmt(self: *mut Self, stmt: AstNode) void {
        var k = stmt.kind;
        if (k == "VarDecl") {
            var name = get_prop(stmt.props, "name");
            if (name) |nm| {
                var mut v = mk_void();
                if (has_prop(stmt.props, "has_init") and stmt.children.len > 0) {
                    v = self.eval_expr(stmt.children[stmt.children.len - 1]);
                }
                self.env.declare(nm, v);
            }
        } else if (k == "ExprStmt") {
            if (stmt.children.len > 0) {
                self.eval_expr(stmt.children[0]);
            }
        } else if (k == "If") {
            var cv = self.eval_expr(stmt.children[0]);
            if (self.truthy(cv)) {
                self.exec_sub(stmt.children[1]);
            } else if (stmt.children.len >= 3) {
                self.exec_sub(stmt.children[2]);
            }
        } else if (k == "While") {
            while (true) {
                var cv = self.eval_expr(stmt.children[0]);
                if (!self.truthy(cv)) { break; }
                self.exec_sub(stmt.children[1]);
                if (self.flow.len > 0) {
                    if (slice_eq(self.flow, "break")) { self.flow = ""; break; }
                    if (slice_eq(self.flow, "continue")) { self.flow = ""; continue; }
                    break;
                }
            }
        } else if (k == "For") {
            var itv = self.eval_expr(stmt.children[0]);
            if (itv.kind == "vec") {
                var pl = get_prop(stmt.props, "payload");
                var mut n: usize = 0;
                while (n < itv.vec.len) {
                    self.env.push_scope();
                    if (pl) |p| { self.env.declare(p, itv.vec[n]); }
                    self.exec_sub(stmt.children[1]);
                    self.env.pop_scope();
                    if (self.flow.len > 0) {
                        if (slice_eq(self.flow, "break")) { self.flow = ""; break; }
                        if (slice_eq(self.flow, "continue")) { self.flow = ""; n += 1; continue; }
                        break;
                    }
                    n += 1;
                }
            }
        } else if (k == "Break") {
            self.flow = "break";
        } else if (k == "Continue") {
            self.flow = "continue";
        } else if (k == "Return") {
            if (stmt.children.len > 0) {
                self.retv = self.eval_expr(stmt.children[0]);
            } else {
                self.retv = mk_void();
            }
            self.flow = "return";
        }
    }

    fn truthy(self: *mut Self, v: Value) bool {
        if (v.kind == "bool") { return v.i == 1; }
        if (v.kind == "int") { return v.i != 0; }
        return false;
    }

    fn eval_expr(self: *mut Self, e: AstNode) Value {
        var k = e.kind;
        if (k == "IntLit") {
            var t = get_prop(e.props, "text");
            if (t) |txt| { return mk_int(parse_int_text(txt)); }
            return mk_int(0);
        }
        if (k == "FloatLit") {
            var t = get_prop(e.props, "text");
            if (t) |txt| { return mk_float(parse_float_text(txt)); }
            return mk_float(0.0);
        }
        if (k == "StrLit") {
            var t = get_prop(e.props, "value");
            if (t) |txt| { return mk_str(txt); }
            return mk_str("");
        }
        if (k == "BoolLit") {
            var t = get_prop(e.props, "value");
            if (t) |txt| { return mk_bool(slice_eq(txt, "true")); }
            return mk_bool(false);
        }
        if (k == "NullLit") { return mk_null(); }
        if (k == "ArrayLit") {
            var items = Vec<Value>.init(alloc);
            var mut ai: usize = 0;
            while (ai < e.children.len) {
                items.append(self.eval_expr(e.children[ai]));
                ai += 1;
            }
            return mk_vec(items);
        }
        if (k == "Ident") {
            var t = get_prop(e.props, "name");
            if (t) |nm| {
                var v = self.env.lookup(nm);
                if (v) |val| { return val; }
            }
            return mk_void();
        }
        if (k == "Unary") {
            if (e.children.len > 0) {
                var v = self.eval_expr(e.children[0]);
                var op = get_prop(e.props, "op");
                if (op) |o| {
                    if (slice_eq(o, "Neg")) {
                        if (v.kind == "float") { return mk_float(-v.f); }
                        return mk_int(-v.i);
                    }
                    if (slice_eq(o, "Not")) { return mk_bool(!self.truthy(v)); }
                }
                return v;
            }
            return mk_void();
        }
        if (k == "Binary") {
            return self.eval_binary(e);
        }
        if (k == "Assign") {
            return self.eval_assign(e);
        }
        if (k == "Call") {
            return self.eval_call(e);
        }
        if (k == "Field") {
            return self.eval_field(e);
        }
        if (k == "Index") {
            return self.eval_index(e);
        }
        if (k == "ClassLit") {
            // 类字面量 → 对象（字段平行数组）
            var cname = get_prop(e.props, "name");
            var mut inst = ObjInst{
                cls = "",
                field_names = Vec<&[u8]>.init(alloc),
                field_vals = Vec<Value>.init(alloc),
            };
            if (cname) |cn| { inst.cls = cn; }
            var mut ci2: usize = 0;
            while (ci2 < e.children.len) {
                var fin = e.children[ci2];
                var fn2 = get_prop(fin.props, "name");
                var mut fv = mk_void();
                if (fin.children.len > 0) { fv = self.eval_expr(fin.children[0]); }
                if (fn2) |f| {
                    inst.field_names.append(f);
                    inst.field_vals.append(fv);
                }
                ci2 += 1;
            }
            return mk_obj(inst);
        }
        if (k == "Unwrap") {
            // .? 解包：some → 内值；none → 原样上浮（语料不对 none 取 .?）
            if (e.children.len > 0) {
                var uv = self.eval_expr(e.children[0]);
                if (uv.kind == "opt") {
                    if (uv.i == 1 and uv.vec.len > 0) { return uv.vec[0]; }
                }
                return uv;
            }
            return mk_void();
        }
        if (k == "Orelse") {
            // orelse 兔底：some → 内值；none/缺失 → 兔底表达式
            if (e.children.len >= 2) {
                var lv = self.eval_expr(e.children[0]);
                if (lv.kind == "opt") {
                    if (lv.i == 1 and lv.vec.len > 0) { return lv.vec[0]; }
                    return self.eval_expr(e.children[1]);
                }
                return lv;
            }
            return mk_void();
        }
        if (k == "ErrorLit") {
            var en = get_prop(e.props, "name");
            if (en) |en2| { return mk_err(en2); }
            return mk_err("unknown");
        }
        if (k == "Move") {
            // ADR-0030：move 是编译期所有权转移——运行时穿透求值取引用目标
            if (e.children.len > 0) {
                return self.eval_expr(e.children[0]);
            }
            return mk_void();
        }
        if (k == "Try") {
            // try 传播：err → flow=return 向函数边界冒泡
            if (e.children.len > 0) {
                var tv = self.eval_expr(e.children[0]);
                if (tv.kind == "err") {
                    self.retv = tv;
                    self.flow = "return";
                    return mk_void();
                }
                return tv;
            }
            return mk_void();
        }
        if (k == "Catch") {
            // catch 兔底：err → 求默认值（Default 包裹节点）；否则原值
            if (e.children.len >= 2) {
                var cv = self.eval_expr(e.children[0]);
                if (cv.kind == "err") {
                    var dn = e.children[1];
                    if (dn.children.len > 0) { return self.eval_expr(dn.children[0]); }
                    return mk_void();
                }
                return cv;
            }
            return mk_void();
        }
        return mk_void();
    }

    // 属性读：容器 .len / obj 字段读
    fn eval_field(self: *mut Self, e: AstNode) Value {
        if (e.children.len == 0) { return mk_void(); }
        var fname = get_prop(e.props, "field");
        if (fname) |f| {
            var bv = self.eval_expr(e.children[0]);
            if (slice_eq(f, "len")) {
                if (bv.kind == "vec") { return mk_int(@intCast(i64, bv.vec.len)); }
                if (bv.kind == "map") { return mk_int(@intCast(i64, bv.map.len)); }
                if (bv.kind == "str") { return mk_int(@intCast(i64, bv.s.len)); }
            }
            if (bv.kind == "obj") {
                if (bv.obj) |inst| {
                    var mut fi2: usize = 0;
                    while (fi2 < inst.field_names.len) {
                        if (slice_eq(inst.field_names[fi2], f)) {
                            return inst.field_vals[fi2];
                        }
                        fi2 += 1;
                    }
                }
            }
        }
        return mk_void();
    }

    // 索引读：vec[i] / str[i]（str[a..b] 切片属 C7）
    fn eval_index(self: *mut Self, e: AstNode) Value {
        if (e.children.len < 2) { return mk_void(); }
        var bv = self.eval_expr(e.children[0]);
        var iv = self.eval_expr(e.children[1]);
        if (bv.kind == "vec") {
            var n: i64 = @intCast(i64, bv.vec.len);
            if (iv.i >= 0 and iv.i < n) { return bv.vec[@intCast(usize, iv.i)]; }
            return mk_void();
        }
        if (bv.kind == "str") {
            // 切片 s[a..b]（Range 二元节点）→ 子串
            var rn = e.children[1];
            if (rn.kind == "Binary") {
                var rop = get_prop(rn.props, "op");
                if (rop) |ro| {
                    if (slice_eq(ro, "Range")) {
                        var a2 = self.eval_expr(rn.children[0]);
                        var b2 = self.eval_expr(rn.children[1]);
                        var sn: i64 = @intCast(i64, bv.s.len);
                        if (a2.i >= 0 and b2.i <= sn and a2.i <= b2.i) {
                            return mk_str(bv.s[@intCast(usize, a2.i)..@intCast(usize, b2.i)]);
                        }
                        return mk_str("");
                    }
                }
            }
            var n: i64 = @intCast(i64, bv.s.len);
            if (iv.i >= 0 and iv.i < n) { return mk_int(bv.s[@intCast(usize, iv.i)]); }
            return mk_void();
        }
        return mk_void();
    }

    fn eval_binary(self: *mut Self, e: AstNode) Value {
        var op = get_prop(e.props, "op");
        if (op) |o| {
            // 逻辑短路：And/Or 先算左值即判
            if (slice_eq(o, "And")) {
                var l = self.eval_expr(e.children[0]);
                if (!self.truthy(l)) { return mk_bool(false); }
                var r = self.eval_expr(e.children[1]);
                return mk_bool(self.truthy(r));
            }
            if (slice_eq(o, "Or")) {
                var l = self.eval_expr(e.children[0]);
                if (self.truthy(l)) { return mk_bool(true); }
                var r = self.eval_expr(e.children[1]);
                return mk_bool(self.truthy(r));
            }
            var l = self.eval_expr(e.children[0]);
            var r = self.eval_expr(e.children[1]);
            return self.binop(o, l, r);
        }
        return mk_void();
    }

    fn binop(self: *mut Self, o: &[u8], l: Value, r: Value) Value {
        var lfl = l.kind == "float";
        var rfl = r.kind == "float";
        if (slice_eq(o, "Eq")) {
            if (l.kind == "str" and r.kind == "str") { return mk_bool(slice_eq(l.s, r.s)); }
            if (lfl or rfl) { return mk_bool(as_f(l) == as_f(r)); }
            return mk_bool(l.i == r.i);
        }
        if (slice_eq(o, "Ne")) {
            if (l.kind == "str" and r.kind == "str") { return mk_bool(!slice_eq(l.s, r.s)); }
            if (lfl or rfl) { return mk_bool(!(as_f(l) == as_f(r))); }
            return mk_bool(!(l.i == r.i));
        }
        if (lfl or rfl) {
            var a = as_f(l);
            var b = as_f(r);
            if (slice_eq(o, "Lt")) { return mk_bool(a < b); }
            if (slice_eq(o, "Le")) { return mk_bool(a <= b); }
            if (slice_eq(o, "Gt")) { return mk_bool(a > b); }
            if (slice_eq(o, "Ge")) { return mk_bool(a >= b); }
            if (slice_eq(o, "Add")) { return mk_float(a + b); }
            if (slice_eq(o, "Sub")) { return mk_float(a - b); }
            if (slice_eq(o, "Mul")) { return mk_float(a * b); }
            if (slice_eq(o, "Div")) { return mk_float(a / b); }
            return mk_void();
        }
        if (slice_eq(o, "Lt")) { return mk_bool(l.i < r.i); }
        if (slice_eq(o, "Le")) { return mk_bool(l.i <= r.i); }
        if (slice_eq(o, "Gt")) { return mk_bool(l.i > r.i); }
        if (slice_eq(o, "Ge")) { return mk_bool(l.i >= r.i); }
        if (slice_eq(o, "Add")) { return mk_int(l.i + r.i); }
        if (slice_eq(o, "Sub")) { return mk_int(l.i - r.i); }
        if (slice_eq(o, "Mul")) { return mk_int(l.i * r.i); }
        if (slice_eq(o, "Div")) { return mk_int(l.i / r.i); }
        if (slice_eq(o, "Mod")) { return mk_int(l.i % r.i); }
        return mk_void();
    }

    fn eval_assign(self: *mut Self, e: AstNode) Value {
        var op = get_prop(e.props, "op");
        if (op) |o| {
            var target = e.children[0];
            if (target.kind == "Field") {
                // obj 字段写（Eq / 复合赋值）：重建 ObjInst 并变量写回
                var fname = get_prop(target.props, "field");
                if (fname) |f| {
                    var bnode = target.children[0];
                    var mut basev = self.eval_expr(bnode);
                    if (basev.kind == "obj") {
                        var mut val: Value = mk_void();
                        if (slice_eq(o, "Eq")) {
                            val = self.eval_expr(e.children[1]);
                        } else {
                            var cur = self.eval_field(target);
                            var rv = self.eval_expr(e.children[1]);
                            var mut op2: &[u8] = "Add";
                            if (slice_eq(o, "MinusEq")) { op2 = "Sub"; }
                            else if (slice_eq(o, "StarEq")) { op2 = "Mul"; }
                            else if (slice_eq(o, "SlashEq")) { op2 = "Div"; }
                            val = self.binop(op2, cur, rv);
                        }
                        if (basev.obj) |inst| {
                            var names2 = Vec<&[u8]>.init(alloc);
                            var vals2 = Vec<Value>.init(alloc);
                            var mut i2: usize = 0;
                            var mut hit = false;
                            while (i2 < inst.field_names.len) {
                                names2.append(inst.field_names[i2]);
                                if (slice_eq(inst.field_names[i2], f)) {
                                    vals2.append(val);
                                    hit = true;
                                } else {
                                    vals2.append(inst.field_vals[i2]);
                                }
                                i2 += 1;
                            }
                            if (hit and bnode.kind == "Ident") {
                                var ni = ObjInst{
                                    cls = inst.cls,
                                    field_names = names2,
                                    field_vals = vals2,
                                };
                                basev.obj = ni;
                                var bn2 = get_prop(bnode.props, "name");
                                if (bn2) |bn3| {
                                    self.env.assign(bn3, basev);
                                    return val;
                                }
                            }
                        }
                    }
                }
                return mk_void();
            }
            var name = get_prop(target.props, "name");
            if (name) |nm| {
                if (slice_eq(o, "Eq")) {
                    var v = self.eval_expr(e.children[1]);
                    self.env.assign(nm, v);
                    return v;
                }
                // 复合赋值：target = target OP value
                var cur = self.eval_expr(target);
                var rv = self.eval_expr(e.children[1]);
                if (slice_eq(o, "PlusEq")) {
                    var v = self.binop("Add", cur, rv);
                    self.env.assign(nm, v);
                    return v;
                }
                if (slice_eq(o, "MinusEq")) {
                    var v = self.binop("Sub", cur, rv);
                    self.env.assign(nm, v);
                    return v;
                }
                if (slice_eq(o, "StarEq")) {
                    var v = self.binop("Mul", cur, rv);
                    self.env.assign(nm, v);
                    return v;
                }
                if (slice_eq(o, "SlashEq")) {
                    var v = self.binop("Div", cur, rv);
                    self.env.assign(nm, v);
                    return v;
                }
            }
        }
        return mk_void();
    }

    // 调用：io.print/println 内建 + 顶层用户函数（C5）
    fn eval_call(self: *mut Self, e: AstNode) Value {
        if (e.children.len == 0) { return mk_void(); }
        var head = e.children[0];
        if (head.kind == "Field") {
            var method = get_prop(head.props, "field");
            if (method) |m| {
                // io.print/println 内建
                if (head.children.len > 0 and head.children[0].kind == "Ident") {
                    var bn = get_prop(head.children[0].props, "name");
                    if (bn) |bnm| {
                        if (slice_eq(bnm, "io") and (slice_eq(m, "print") or slice_eq(m, "println"))) {
                            self.builtin_print(e, slice_eq(m, "println"));
                            return mk_void();
                        }
                    }
                }
                // 静态方法：String.compare / String.fromInt（base 为类型名）
                if (head.children[0].kind == "Ident") {
                    var st = get_prop(head.children[0].props, "name");
                    if (st) |t| {
                        if (slice_eq(t, "String")) {
                            if (slice_eq(m, "compare") and e.children.len > 2) {
                                var av = self.eval_expr(e.children[1]);
                                var bv = self.eval_expr(e.children[2]);
                                return mk_int(str_compare(av.s, bv.s));
                            }
                            if (slice_eq(m, "fromInt") and e.children.len > 1) {
                                var iv = self.eval_expr(e.children[1]);
                                var buf = Vec<u8>.init(alloc);
                                append_int(iv.i, &mut buf);
                                return mk_str(buf.as_slice());
                            }
                        }
                    }
                }
                if (head.children.len > 0) {
                    // 类型构造：Vec<T>.init / Map<K,V>.init（泛型实参已被解析器消费）
                    if (slice_eq(m, "init") and head.children[0].kind == "Ident") {
                        var tn = get_prop(head.children[0].props, "name");
                        if (tn) |t| {
                            if (slice_eq(t, "Vec")) { return mk_vec(Vec<Value>.init(alloc)); }
                            if (slice_eq(t, "Map")) { return mk_map(); }
                            if (slice_eq(t, "alloc") and e.children.len > 1 and e.children[1].kind == "ClassLit") {
                                return self.eval_expr(e.children[1]);
                            }
                        }
                    }
                    // 实例方法：求值 base，按值类型分发；容器变更写回变量
                    //（Vec/Map 结构体按值拷贝，不写回则 len/内容对后续语句不可见）
                    var basev = self.eval_expr(head.children[0]);
                    var is_var = head.children[0].kind == "Ident";
                    var mut bname: ?&[u8] = null;
                    if (is_var) { bname = get_prop(head.children[0].props, "name"); }
                    if (basev.kind == "vec") {
                        if (slice_eq(m, "append")) {
                            if (e.children.len > 1) {
                                basev.vec.append(self.eval_expr(e.children[1]));
                                if (bname) |bn| { self.env.assign(bn, basev); }
                            }
                            return mk_void();
                        }
                        if (slice_eq(m, "get")) {
                            if (e.children.len > 1) {
                                var iv = self.eval_expr(e.children[1]);
                                var n: i64 = @intCast(i64, basev.vec.len);
                                if (iv.i >= 0 and iv.i < n) {
                                    return mk_opt_some(basev.vec[@intCast(usize, iv.i)]);
                                }
                            }
                            return mk_opt_none();
                        }
                    }
                    if (basev.kind == "str") {
                        if (slice_eq(m, "concat")) {
                            if (e.children.len > 1) {
                                var ov = self.eval_expr(e.children[1]);
                                var buf = Vec<u8>.init(alloc);
                                append_bytes(&mut buf, basev.s);
                                append_bytes(&mut buf, ov.s);
                                return mk_str(buf.as_slice());
                            }
                            return basev;
                        }
                    }
                    if (basev.kind == "map") {
                        if (slice_eq(m, "put")) {
                            if (e.children.len > 2) {
                                var kv = self.eval_expr(e.children[1]);
                                var vv = self.eval_expr(e.children[2]);
                                basev.map.put(kv.s, vv);
                                if (bname) |bn| { self.env.assign(bn, basev); }
                            }
                            return mk_void();
                        }
                        if (slice_eq(m, "get")) {
                            if (e.children.len > 1) {
                                var kv = self.eval_expr(e.children[1]);
                                var hit = basev.map.get(kv.s);
                                if (hit) |val| { return mk_opt_some(val); }
                            }
                            return mk_opt_none();
                        }
                        if (slice_eq(m, "contains")) {
                            if (e.children.len > 1) {
                                var kv = self.eval_expr(e.children[1]);
                                return mk_bool(basev.map.contains(kv.s));
                            }
                            return mk_bool(false);
                        }
                    }
                    if (basev.kind == "obj") {
                        // 方法分发：obj.cls → 类注册表 → 类体 Fn（name==m）
                        if (basev.obj) |inst| {
                            var cip = self.classes.get(inst.cls);
                            if (cip) |ci| {
                                var cnode = self.prog.children[ci];
                                var mut mi: usize = 0;
                                while (mi < cnode.children.len) {
                                    var mnode = cnode.children[mi];
                                    if (mnode.kind == "Fn") {
                                        var mn = get_prop(mnode.props, "name");
                                        if (mn) |mn2| {
                                            if (slice_eq(mn2, m)) {
                                                return self.call_method(mnode, basev, e);
                                            }
                                        }
                                    }
                                    mi += 1;
                                }
                            }
                        }
                    }
                }
            }
        } else if (head.kind == "Ident") {
            var hname = get_prop(head.props, "name");
            if (hname) |hn| {
                if (self.fns.contains(hn)) {
                    var fip = self.fns.get(hn);
                    if (fip) |fi| {

                        // 调用入口一次性取 fd（本地副本跨 eval 读，同 For 的 itv 模式）。
                        // 注意：循环变量不得命名 pi——pi 是 H 内置常量（π=3.14159...），
                        // 同名声明会被遮蔽，pi<flen 恒 false，参数绑定静默跳过（C5.1 根因）。
                        var fd = self.prog.children[fi];
                        var flen = fd.children.len;
                        var cclen = e.children.len;
                        self.env.push_scope();
                        var mut pidx: usize = 0;
                        var mut ai: usize = 1;
                        while (pidx < flen) {
                            if (ai >= cclen) { break; }
                            var ch = fd.children[pidx];
                            if (ch.kind != "Param") { break; }
                            var pname = get_prop(ch.props, "name");
                            var av = self.eval_expr(e.children[ai]);
                            if (pname) |pn| { self.env.declare(pn, av); }
                            pidx += 1;
                            ai += 1;
                        }
                        var body = fd.children[flen - 1];
                        self.flow = "";
                        self.retv = mk_void();
                        if (body.kind == "Block") {
                            self.exec_block(body);
                        }
                        var out = self.retv;
                        self.flow = "";
                        self.env.pop_scope();
                        return out;
                    }
                }
            }
        }
        return mk_void();
    }

    // 方法调用：绑定 self（不占实参位）+ 其余参数，执行方法体
    fn call_method(self: *mut Self, mnode: AstNode, recv: Value, e: AstNode) Value {
        var flen = mnode.children.len;
        self.env.push_scope();
        self.env.declare("self", recv);
        var mut pidx: usize = 0;
        var mut ai: usize = 1;
        while (pidx < flen) {
            var ch = mnode.children[pidx];
            if (ch.kind != "Param") { break; }
            var pname = get_prop(ch.props, "name");
            if (pname) |pn| {
                if (!slice_eq(pn, "self")) {
                    if (ai >= e.children.len) { break; }
                    var pv = self.eval_expr(e.children[ai]);
                    self.env.declare(pn, pv);
                    ai += 1;
                }
            }
            pidx += 1;
        }
        var body = mnode.children[flen - 1];
        self.flow = "";
        self.retv = mk_void();
        if (body.kind == "Block") {
            self.exec_block(body);
        }
        var out = self.retv;
        self.flow = "";
        self.env.pop_scope();
        return out;
    }

    // "{}" 占位符格式化输出（与 Rust 参考 stdout 对齐）
    fn builtin_print(self: *mut Self, call: AstNode, newline: bool) void {
        var out = Vec<u8>.init(alloc);
        var mut fmtv = mk_str("");
        var mut argi: usize = 2;
        if (call.children.len > 1) {
            fmtv = self.eval_expr(call.children[1]);
        }
        var mut i: usize = 0;
        var s = fmtv.s;
        while (i < s.len) {
            if (s[i] == '{' and i + 1 < s.len and s[i + 1] == '}') {
                if (argi < call.children.len) {
                    append_value(self.eval_expr(call.children[argi]), &mut out);
                    argi += 1;
                }
                i += 2;
                continue;
            }
            out.append(s[i]);
            i += 1;
        }
        if (newline) { out.append('\n'); }
        io.print("{}", out.as_slice());
    }
}

// ============================================================
// 入口
// ============================================================

fn main(args: Vec<String>) !void {
    // --self-test：C2 值模型 + 环境单元自检
    if (args.len >= 2 and args[1].as_slice() == "--self-test") {
        var env: Env = alloc.init(Env{
            entries = Vec<EnvEntry>.init(alloc),
            scope_sizes = Vec<usize>.init(alloc),
        });
        env.push_scope();
        env.declare("a", mk_int(42));
        env.push_scope();
        env.declare("b", mk_str("hi"));
        // 内层可见外层
        var va = env.lookup("a");
        if (va) |v| {
            if (v.kind == "int" and v.i == 42) { io.print("lookup-outer ok\n"); }
            else { io.print("lookup-outer FAIL\n"); }
        } else { io.print("lookup-outer missing FAIL\n"); }
        // 就地赋值（内层覆盖外层绑定）
        var ok = env.assign("a", mk_int(43));
        if (ok) {
            var va2 = env.lookup("a");
            if (va2) |v2| {
                if (v2.i == 43) { io.print("assign ok\n"); }
                else { io.print("assign FAIL\n"); }
            } else { io.print("assign missing FAIL\n"); }
        } else { io.print("assign FAIL\n"); }
        // 内层独有绑定随 pop 消失
        env.pop_scope();
        var vb = env.lookup("b");
        if (vb) |v| {
            io.print("scope-escape FAIL\n");
        } else { io.print("scope-pop ok\n"); }
        // 外层赋值结果保留（43），遮蔽语义：pop 后仍见 43
        var va3 = env.lookup("a");
        if (va3) |v3| {
            if (v3.i == 43) { io.print("outer-preserved ok\n"); }
            else { io.print("outer-preserved FAIL\n"); }
        } else { io.print("outer-preserved missing FAIL\n"); }
        // 容器值构造
        var vv = Vec<Value>.init(alloc);
        vv.append(mk_int(1));
        vv.append(mk_int(2));
        if (vv.len == 2 and vv[1].i == 2) { io.print("vec-value ok\n"); }
        else { io.print("vec-value FAIL\n"); }
        var mm = Map<&[u8], Value>.init(alloc);
        mm.put("k", mk_str("v"));
        var mv = mm.get("k");
        if (mv) |mval| {
            if (mval.kind == "str") { io.print("map-value ok\n"); }
            else { io.print("map-value FAIL\n"); }
        } else { io.print("map-value missing FAIL\n"); }
        // 其余构造器烟雾
        var vf = mk_float(1.5);
        var vbool = mk_bool(true);
        var vn = mk_null();
        var vd = mk_void();
        if (vf.f == 1.5 and vbool.i == 1 and vn.kind == "null" and vd.kind == "void") {
            io.print("ctors ok\n");
        } else { io.print("ctors FAIL\n"); }
        return;
    }
    // --dump-ast 调试模式：args[1] 为模式开关（args[0] 是程序自身路径），与 checker.hc 约定一致
    if (args.len >= 3 and args[1].as_slice() == "--dump-ast") {
        var mut dsrc = try io.fs.read_file(args[2], alloc);
        var dkw = build_kw_map();
        var drev = build_rev_kw_map();
        var dlx: Lexer = alloc.init(Lexer{
            src = dsrc, n = dsrc.len,
            pos = 0, line = 1, col = 1,
            tokens = Vec<Token>.init(alloc),
            kw_map = dkw,
        });
        dlx.run();
        var dparser: Parser = alloc.init(Parser{
            tokens = dlx.tokens, pos = 0,
            n = dlx.tokens.len,
            rev_kw_map = drev,
        });
        var dast = dparser.parse_program();
        var ddumper: AstDumper = alloc.init(AstDumper{
            buf = Vec<u8>.init(alloc),
        });
        ddumper.dump(dast, 0);
        io.print("{}", ddumper.buf.as_slice());
        return;
    }
    var mut path = args[0];
    if (args.len >= 2) { path = args[1]; }
    var mut src = try io.fs.read_file(path, alloc);
    // 构建关键字字典
    var kw_map = build_kw_map();
    var rev_kw_map = build_rev_kw_map();
    // 词法分析
    var lx: Lexer = alloc.init(Lexer{
        src = src, n = src.len,
        pos = 0, line = 1, col = 1,
        tokens = Vec<Token>.init(alloc),
        kw_map = kw_map,
    });
    lx.run();
    // 语法分析（C1：AST 就绪即成功，无求值）
    var parser: Parser = alloc.init(Parser{
        tokens = lx.tokens, pos = 0,
        n = lx.tokens.len,
        rev_kw_map = rev_kw_map,
    });
    var ast = parser.parse_program();
    // C3：求值（main 入口；控制流 C4、函数 C5 递增）
    var it: Interp = alloc.init(Interp{
        prog = ast,
        env = alloc.init(Env{
            entries = Vec<EnvEntry>.init(alloc),
            scope_sizes = Vec<usize>.init(alloc),
        }),
        flow = "",
        retv = mk_void(),
        fns = Map<&[u8], usize>.init(alloc),
        classes = Map<&[u8], usize>.init(alloc),
    });
    it.run_main();
}
