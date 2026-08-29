// ============================================================
// stage1/checker.hc — H 版语义分析（K3：E7 自举渐进路线 · 语义阶段）
//
// 双实现对照：本文件与 Rust 参考语义分析（tag1/hc/src/semantic/）
// 对同一源码输出同一诊断格式，差异即 bug。
// Rust 参考实现长期保留（自举失败风险对策）。
//
// 用法：hc run stage1/checker.hc <file.hc>
//
// 输出格式：
//   成功：OK
//   失败：error:line:col: message
// ============================================================

import H.std.{io};

// ============================================================
// 辅助函数
// ============================================================

fn is_digit(b: u8) bool {
    return b >= '0' and b <= '9';
}
fn is_hex(b: u8) bool {
    return is_digit(b) or (b >= 'a' and b <= 'f') or (b >= 'A' and b <= 'F');
}
fn is_bin(b: u8) bool {
    return b == '0' or b == '1';
}
fn is_oct(b: u8) bool {
    return b >= '0' and b <= '7';
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
fn slice_eq(a: &[u8], b: &[u8]) bool {
    if (a.len != b.len) return false;
    var mut i: usize = 0;
    while (i < a.len) {
        if (a[i] != b[i]) return false;
        i += 1;
    }
    return true;
}

// 追加字节切片到 Vec<u8>（用于构建错误消息）
fn append_bytes(msg: *mut Vec<u8>, s: &[u8]) void {
    var mut i: usize = 0;
    while (i < s.len) {
        msg.*.append(s[i]);
        i += 1;
    }
}

// ============================================================
// 关键字字典
// ============================================================

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
// Token 与词法分析器（Lexer）
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


    fn kw_of(self: *mut Self, name: &[u8]) ?&[u8] {
        var len = name.len;
        if (len >= 2 and len <= 9) {
            if (len == 2) {
                if (slice_eq(name, "fn")) return "KwFn";
                if (slice_eq(name, "if")) return "KwIf";
                if (slice_eq(name, "or")) return "KwOr";
                return null;
            }
            if (len == 3) {
                if (slice_eq(name, "var")) return "KwVar";
                if (slice_eq(name, "for")) return "KwFor";
                if (slice_eq(name, "pub")) return "KwPub";
                if (slice_eq(name, "mut")) return "KwMut";
                if (slice_eq(name, "and")) return "KwAnd";
                if (slice_eq(name, "try")) return "KwTry";
                return null;
            }
            if (len == 4) {
                if (slice_eq(name, "else")) return "KwElse";
                if (slice_eq(name, "enum")) return "KwEnum";
                if (slice_eq(name, "tree")) return "KwTree";
                if (slice_eq(name, "move")) return "KwMove";
                if (slice_eq(name, "type")) return "KwType";
                if (slice_eq(name, "void")) return "KwVoid";
                if (slice_eq(name, "null")) return "KwNull";
                if (slice_eq(name, "true")) return "KwTrue";
                return null;
            }
            if (len == 5) {
                if (slice_eq(name, "const")) return "KwConst";
                if (slice_eq(name, "while")) return "KwWhile";
                if (slice_eq(name, "break")) return "KwBreak";
                if (slice_eq(name, "defer")) return "KwDefer";
                if (slice_eq(name, "class")) return "KwClass";
                if (slice_eq(name, "union")) return "KwUnion";
                if (slice_eq(name, "where")) return "KwWhere";
                if (slice_eq(name, "import")) return "KwImport";
                if (slice_eq(name, "owned")) return "KwOwned";
                if (slice_eq(name, "catch")) return "KwCatch";
                if (slice_eq(name, "async")) return "KwAsync";
                if (slice_eq(name, "await")) return "KwAwait";
                if (slice_eq(name, "spawn")) return "KwSpawn";
                if (slice_eq(name, "false")) return "KwFalse";
                return null;
            }
            if (len == 6) {
                if (slice_eq(name, "global")) return "KwGlobal";
                if (slice_eq(name, "return")) return "KwReturn";
                if (slice_eq(name, "switch")) return "KwSwitch";
                if (slice_eq(name, "struct")) return "KwStruct";
                if (slice_eq(name, "import")) return "KwImport";
                if (slice_eq(name, "export")) return "KwExport";
                if (slice_eq(name, "orelse")) return "KwOrelse";
                if (slice_eq(name, "script")) return "KwScript";
                if (slice_eq(name, "extern")) return "KwExtern";
                return null;
            }
            if (len == 7) {
                if (slice_eq(name, "anytype")) return "KwAnytype";
                return null;
            }
            if (len == 8) {
                if (slice_eq(name, "continue")) return "KwContinue";
                if (slice_eq(name, "errdefer")) return "KwErrdefer";
                if (slice_eq(name, "comptime")) return "KwComptime";
                return null;
            }
            if (len == 9) {
                if (slice_eq(name, "interface")) return "KwInterface";
                if (slice_eq(name, "namespace")) return "KwNamespace";
                return null;
            }
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
// AST 节点
// ============================================================

class AstNode {
    kind: &[u8],
    props: Vec<u8>,
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

// 从 props 中提取属性值（key=value 格式，用 | 分隔）
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
        var mut is_pub = false;
        if (self.at("KwPub")) { is_pub = true; self.advance(); }
        var mut is_export = false;
        if (self.at("KwExport")) { is_export = true; self.advance(); }
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
            self.parse_type();
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
        var mut i: usize = 0;
        while (i < traits.len) {
            if (traits[i] == "test") {
                node_add_prop(&f, "test", "true");
            }
            i += 1;
        }
        if (self.at("Lt")) {
            self.advance();
            while (!self.at("Gt") and !self.at("Eof")) {
                self.expect_ident();
                if (self.at("Comma")) { self.advance(); }
            }
            self.expect("Gt");
        }
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
            node_add_prop(&f, "ret_union", "true");
            if (self.at("Ident") or self.at("KwVoid")) {
                var mut ret_ty = self.peek_text();
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
        if (self.at("LParen")) {
            self.advance();
            while (!self.at("RParen") and !self.at("Eof")) {
                self.parse_type();
                if (self.at("Comma")) { self.advance(); }
                else { break; }
            }
            self.expect("RParen");
        }
        while (self.at("LBracket")) {
            self.parse_trait();
        }
        self.expect("LBrace");
        while (!self.at("RBrace") and !self.at("Eof")) {
            if (self.at("KwFn") or self.at("LBracket") or (self.at("KwPub") and self.peek_n(1) == "KwFn")) {
                var m = self.parse_method(name);
                node_add_child(&cls, m);
            } else {
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
        if (self.at("KwOwned")) {
            self.advance();
            self.parse_type();
            return;
        }
        if (self.at("Star")) {
            self.advance();
            if (self.at("KwMut")) { self.advance(); }
            self.parse_type();
            return;
        }
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
        if (self.at("Question")) {
            self.advance();
            self.parse_type();
            return;
        }
        if (self.at("Bang")) {
            self.advance();
            self.parse_type();
            return;
        }
        self.parse_type_base();
        if (self.at("Bang")) {
            self.advance();
            self.parse_type();
        }
    }

    fn parse_type_base(self: *mut Self) void {
        if (self.at("Ident")) {
            var name = self.peek_text();
            self.advance();
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
        } else if (self.at("LBracket")) {
            self.advance();
            self.parse_expr();
            self.expect("RBracket");
            self.parse_type();
        } else if (self.at("LParen")) {
            self.advance();
            while (!self.at("RParen") and !self.at("Eof")) {
                self.parse_type();
                if (self.at("Comma")) { self.advance(); }
                else { break; }
            }
            self.expect("RParen");
        } else if (self.at("KwClass")) {
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
        if (self.at("Colon")) {
            self.advance();
            if (self.at("Ident")) { self.advance(); }
            if (self.at("KwWhile") or self.at("KwFor")) { }
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
        var e = self.parse_expr();
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
                // 存储类型名作为子节点，kind 直接设为类型名
                var tn = AstNode{
                    kind = ty,
                    props = Vec<u8>.init(alloc),
                    children = Vec<AstNode>.init(alloc),
                };
                node_add_child(&v, tn);
            } else {
                self.parse_type();
            }
        }
        if (self.at("Eq")) {
            self.advance();
            var init_expr = self.parse_expr();
            node_add_child(&v, init_expr);
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
        while (!self.at("FatArrow") and !self.at("RBrace") and !self.at("Eof")) {
            var pat = self.parse_switch_pattern();
            node_add_child(&arm, pat);
            if (self.at("Comma")) { self.advance(); break; }
        }
        self.expect("FatArrow");
        if (self.at("KwIf")) {
            self.advance();
            self.parse_expr();
        }
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
    // 表达式解析
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
            node_add_child(&b, l);
            node_add_child(&b, r);
            var opn = make_node("Or");
            node_add_child(&b, opn);
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
            node_add_child(&b, l);
            node_add_child(&b, r);
            var opn = make_node("And");
            node_add_child(&b, opn);
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
            node_add_child(&b, l);
            node_add_child(&b, r);
            var opn = make_node("Range");
            node_add_child(&b, opn);
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
            node_add_child(&b, l);
            node_add_child(&b, r);
            if (cmp_op == "EqEq") { var opn = make_node("Eq"); node_add_child(&b, opn); }
            else { var opn = make_node(cmp_op); node_add_child(&b, opn); }
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
            node_add_child(&b, l);
            node_add_child(&b, r);
            var opn = make_node("BitOr");
            node_add_child(&b, opn);
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
            node_add_child(&b, l);
            node_add_child(&b, r);
            var opn = make_node("BitXor");
            node_add_child(&b, opn);
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
            node_add_child(&b, l);
            node_add_child(&b, r);
            var opn = make_node("BitAnd");
            node_add_child(&b, opn);
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
                node_add_child(&b, l);
                node_add_child(&b, r);
                var opn = make_node(opname);
                node_add_child(&b, opn);
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
                node_add_child(&b, l);
                node_add_child(&b, r);
                if (opname == "Plus") { var opn = make_node("Add"); node_add_child(&b, opn); }
                else { var opn = make_node("Sub"); node_add_child(&b, opn); }
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
                node_add_child(&b, l);
                node_add_child(&b, r);
                if (opname == "Star") { var opn = make_node("Mul"); node_add_child(&b, opn); }
                else if (opname == "Slash") { var opn = make_node("Div"); node_add_child(&b, opn); }
                else if (opname == "Percent") { var opn = make_node("Mod"); node_add_child(&b, opn); }
                else { var opn = make_node("ModMod"); node_add_child(&b, opn); }
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
            if (self.at("Dot")) {
                // 方法调用或字段访问
                self.advance();
                var name = self.expect_name_or_keyword();
                if (self.at("LParen")) {
                    // 方法调用
                    var args = self.parse_call_args();
                    var call = make_node("Call");
                    var dot = make_node("DotCall");
                    node_add_prop(&dot, "method", name);
                    node_add_child(&dot, e);
                    node_add_child(&call, dot);
                    var mut i: usize = 0;
                    while (i < args.len) {
                        node_add_child(&call, args[i]);
                        i += 1;
                    }
                    e = call;
                } else {
                    // 字段访问
                    var field = make_node("Field");
                    node_add_prop(&field, "name", name);
                    node_add_child(&field, e);
                    e = field;
                }
            } else if (self.at("LParen")) {
                var args = self.parse_call_args();
                var call = make_node("Call");
                node_add_child(&call, e);
                var mut i: usize = 0;
                while (i < args.len) {
                    node_add_child(&call, args[i]);
                    i += 1;
                }
                e = call;
            } else if (self.at("LBracket")) {
                self.advance();
                var idx = self.parse_expr();
                self.expect("RBracket");
                var index = make_node("Index");
                node_add_child(&index, e);
                node_add_child(&index, idx);
                e = index;
            } else if (self.at("Bang")) {
                self.advance();
                var unwrap = make_node("Unwrap");
                node_add_child(&unwrap, e);
                e = unwrap;
            } else {
                break;
            }
        }
        return e;
    }

    fn parse_call_args(self: *mut Self) Vec<AstNode> {
        self.expect("LParen");
        var args = Vec<AstNode>.init(alloc);
        if (!self.at("RParen")) {
            while (true) {
                var arg = self.parse_expr();
                args.append(arg);
                if (self.at("Comma")) { self.advance(); }
                else { break; }
            }
        }
        self.expect("RParen");
        return args;
    }

    fn parse_primary(self: *mut Self) AstNode {
        var k = self.peek();
        if (k == "Int") {
            var txt = self.peek_text();
            self.advance();
            var n = make_node("IntLit");
            quoted_add_prop(&n, "text", txt);
            return n;
        }
        if (k == "Float") {
            var txt = self.peek_text();
            self.advance();
            var n = make_node("FloatLit");
            quoted_add_prop(&n, "text", txt);
            return n;
        }
        if (k == "Str") {
            var txt = self.peek_text();
            self.advance();
            var s = make_node("StrLit");
            quoted_add_prop(&s, "text", txt);
            return s;
        }
        if (k == "Char") {
            var txt = self.peek_text();
            self.advance();
            var c = make_node("CharLit");
            quoted_add_prop(&c, "text", txt);
            return c;
        }
        if (k == "KwTrue") {
            self.advance();
            var b = make_node("BoolLit");
            node_add_prop(&b, "val", "true");
            return b;
        }
        if (k == "KwFalse") {
            self.advance();
            var b = make_node("BoolLit");
            node_add_prop(&b, "val", "false");
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
        if (k == "AtBuiltin") {
            var txt = self.peek_text();
            self.advance();
            var b = make_node("AtBuiltin");
            quoted_add_prop(&b, "name", txt);
            if (self.at("LParen")) {
                var args = self.parse_call_args();
                var mut i: usize = 0;
                while (i < args.len) {
                    node_add_child(&b, args[i]);
                    i += 1;
                }
            }
            return b;
        }
        if (k == "Ident") {
            var name = self.peek_text();
            self.advance();
            // 类字面量：Type{field = val, ...}
            if (self.at("LBrace")) {
                self.advance();
                var cl = make_node("ClassLit");
                quoted_add_prop(&cl, "name", name);
                while (!self.at("RBrace") and !self.at("Eof")) {
                    var fname = self.expect_name_or_keyword();
                    var fi = make_node("FieldInit");
                    quoted_add_prop(&fi, "name", fname);
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
            var id = make_node("Ident");
            quoted_add_prop(&id, "name", name);
            return id;
        }
        if (k == "LParen") {
            self.advance();
            var e = self.parse_expr();
            self.expect("RParen");
            return e;
        }
        if (k == "LBracket") {
            self.advance();
            var arr = make_node("ArrayLit");
            if (!self.at("RBracket")) {
                while (true) {
                    var elem = self.parse_expr();
                    node_add_child(&arr, elem);
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
            }
            self.expect("RBracket");
            return arr;
        }
        if (k == "KwIf") {
            self.advance();
            var ife = make_node("IfExpr");
            self.expect("LParen");
            var cond = self.parse_expr();
            node_add_child(&ife, cond);
            self.expect("RParen");
            var then_e = self.parse_expr();
            node_add_child(&ife, then_e);
            self.expect("KwElse");
            var else_e = self.parse_expr();
            node_add_child(&ife, else_e);
            return ife;
        }
        if (k == "KwSwitch") {
            self.advance();
            var switch_expr = make_node("SwitchExpr");
            self.expect("LParen");
            var subj = self.parse_expr();
            node_add_child(&switch_expr, subj);
            self.expect("RParen");
            self.expect("LBrace");
            while (!self.at("RBrace") and !self.at("Eof")) {
                var arm = self.parse_switch_arm();
                node_add_child(&switch_expr, arm);
            }
            self.expect("RBrace");
            return switch_expr;
        }
        // 未知 → 空节点
        self.advance();
        return make_node("Unknown");
    }
}

// ============================================================
// 核心类型系统
// ============================================================

// 整数宽度
enum IntWidth {
    I8,
    I16,
    I32,
    I64,
    I128,
    ISize,
    U8,
    U16,
    U32,
    U64,
    U128,
    USize,
    Comptime,
}

// 类型种类（简化版：用 &[u8] 标记类型名，运行时用 kind 字符串匹配）
// 具体类型：int|float|bool|void|str|named|ptr|slice|optional|error_union|tuple|array|infer|generic|unknown
class SType {
    kind: Vec<u8>,
    // 命名类型
    type_name: Vec<u8>,
    type_args: Vec<SType>,
    // 指针类型
    pointee: ?SType,
    ptr_mut: bool,
    // 切片类型
    elem_type: ?SType,
    // 数组类型
    array_len: i64,
    // 错误联合类型
    error_set_type: ?SType,
    inner_type: ?SType,
    // 元组类型
    elem_types: Vec<SType>,
}

// 创建基本类型
fn make_ty(kind: &[u8]) SType {
    return SType{
        kind = vec_from_slice(kind),
        type_name = Vec<u8>.init(alloc),
        type_args = Vec<SType>.init(alloc),
        pointee = null,
        ptr_mut = false,
        elem_type = null,
        array_len = 0,
        error_set_type = null,
        inner_type = null,
        elem_types = Vec<SType>.init(alloc),
    };
}

// 分配来源
enum AllocSource {
    None,
    NonArena,
    Arena,
    Global,
    Unknown,
}

// 变量信息
class VarInfo {
    ty: SType,
    source: AllocSource,
    mut_: bool,
}

// 函数签名
class FnSig {
    param_types: Vec<SType>,
    ret_type: SType,
}

// ============================================================
// 语义检查器（Checker）
// ============================================================

// 作用域条目
class ScopeEntry {
    name: &[u8],
    info: VarInfo,
}

// 检查器状态
class Checker {
    // 诊断信息（错误消息列表）
    diags: Vec<Vec<u8>>,
    // 源码（用于行号定位）
    src: Vec<u8>,
    // 行号表（从源码构建）
    line_starts: Vec<usize>,
    // 作用域条目（扁平存储，从后向前查找）
    scopes: Vec<ScopeEntry>,
    // 每个作用域边界（push 时的 scopes.len）
    scope_sizes: Vec<usize>,
    // 类型注册表（名字→类型信息）
    types: Map<&[u8], SType>,
    // 函数注册表（名字→函数签名）
    funcs: Map<&[u8], FnSig>,
    // 当前函数是否声明了错误联合返回类型
    current_fn_ret_is_error_union: bool,
    // 当前正在检查的类名（空串 = 不在类内；用于 self 注册与 Self 解析）
    mut current_class: &[u8],
    // ADR-0030（2026-08-29）：已 move 的变量名（use-after-move 冻结）——
    // 存标量 bool（R2：Map 只存标量避免重定位损坏）；pop_scope 时按绑定名注销
    moved: Map<&[u8], bool>,

    // 初始化：从源码构建行号表
    fn init(self: *mut Self, src: Vec<u8>) void {
        self.src = src;
        self.line_starts.append(0);
        var mut i: usize = 0;
        while (i < src.len) {
            if (src[i] == '\n') {
                self.line_starts.append(i + 1);
            }
            i += 1;
        }
        self.push_scope();
    }

    // 添加错误
    fn error(self: *mut Self, msg: Vec<u8>) void {
        self.diags.append(msg);
    }

    // 推入新作用域
    fn push_scope(self: *mut Self) void {
        self.scope_sizes.append(self.scopes.len);
    }

    // 弹出作用域
    fn pop_scope(self: *mut Self) void {
        if (self.scope_sizes.len > 0) {
            var mut target = self.scope_sizes[self.scope_sizes.len - 1];
            self.scope_sizes.remove(self.scope_sizes.len - 1);
            while (self.scopes.len > target) {
                var entry = self.scopes[self.scopes.len - 1];
                // ADR-0030：变量死亡 → 注销冻结标记（本作用域声明的绑定）
                self.moved.remove(entry.name);
                self.scopes.remove(self.scopes.len - 1);
            }
        }
    }

    // 在当前作用域注册名字
    fn register(self: *mut Self, name: &[u8], info: VarInfo) void {
        var entry = ScopeEntry{name = name, info = info};
        self.scopes.append(entry);
    }

    // 从当前作用域栈查找名字（从最内层向外查找）
    fn lookup(self: *mut Self, name: &[u8]) ?VarInfo {
        var mut i: i64 = @intCast(i64, self.scopes.len) - 1;
        while (i >= 0) {
            var entry = self.scopes[@intCast(usize, i)];
            if (entry.name == name) {
                return entry.info;
            }
            i -= 1;
        }
        return null;
    }

    // 注册类型
    fn register_type(self: *mut Self, name: &[u8], ty: SType) void {
        self.types.put(name, ty);
    }

    // 查找类型
    fn lookup_type(self: *mut Self, name: &[u8]) ?SType {
        if (self.types.contains(name)) {
            return self.types.get(name);
        }
        return null;
    }

    // 注册函数
    fn register_func(self: *mut Self, name: &[u8], sig: FnSig) void {
        self.funcs.put(name, sig);
    }

    // 查找函数
    fn lookup_func(self: *mut Self, name: &[u8]) ?FnSig {
        if (self.funcs.contains(name)) {
            return self.funcs.get(name);
        }
        return null;
    }

    // 类型解析：将类型名字符串转换为 SType
    fn ty_of(self: *mut Self, name: &[u8]) SType {
        // 内建整数类型
        if (name == "i8") return make_ty("i8");
        if (name == "i16") return make_ty("i16");
        if (name == "i32") return make_ty("i32");
        if (name == "i64") return make_ty("i64");
        if (name == "i128") return make_ty("i128");
        if (name == "isize") return make_ty("isize");
        if (name == "u8") return make_ty("u8");
        if (name == "u16") return make_ty("u16");
        if (name == "u32") return make_ty("u32");
        if (name == "u64") return make_ty("u64");
        if (name == "u128") return make_ty("u128");
        if (name == "usize") return make_ty("usize");
        if (name == "comptime_int") return make_ty("comptime_int");
        // 内建浮点类型
        if (name == "f16" or name == "f32" or name == "f64" or name == "f128") return make_ty("float");
        // 内建其他类型
        if (name == "bool") return make_ty("bool");
        if (name == "void") return make_ty("void");
        if (name == "String") return make_ty("str");
        if (name == "type" or name == "anytype") return make_ty("type");
        // 内建集合类型
        if (name == "Vec" or name == "Deque" or name == "Map" or name == "Table") return make_ty(name);
        if (name == "Allocator" or name == "ExitType") return make_ty(name);
        if (name == "Future") return make_ty(name);
        // 在类型注册表中查找
        if (self.types.contains(name)) {
            var t = self.types.get(name);
            if (t) |tt| { return tt; }
        }
        // 大写开头 → 泛型参数
        if (name.len > 0 and name[0] >= 'A' and name[0] <= 'Z') {
            return make_ty("generic");
        }
        // 未知类型
        return make_ty("unknown");
    }

    // 从 props 中解析类型注解
    fn resolve_ty(self: *mut Self, props: &[u8]) SType {
        var ty_prop = get_prop(props, "ty");
        if (ty_prop) |t| {
            return self.ty_of(t);
        }
        return make_ty("unknown");
    }

    // 从子节点中解析类型注解
    fn resolve_ty_children(self: *mut Self, children: Vec<AstNode>) SType {
        var mut ci: usize = 0;
        while (ci < children.len) {
            var child = children[ci];
            if (child.kind == "TypeName") {
                var tn = get_prop(child.props, "name");
                if (tn) |t| {
                    return self.ty_of(t);
                }
                break;
            }
            ci += 1;
        }
        return make_ty("unknown");
    }

    // 类型兼容性检查：值类型是否兼容于期望类型
    fn is_compatible(self: *mut Self, val_ty: SType, expect_ty: SType) bool {
        var vk = val_ty.kind.as_slice();
        var ek = expect_ty.kind.as_slice();
        // comptime_int 兼容任何整数类型
        if (vk == "comptime_int") {
            if (ek == "i8" or ek == "i16" or
                ek == "i32" or ek == "i64" or
                ek == "i128" or ek == "isize" or
                ek == "u8" or ek == "u16" or
                ek == "u32" or ek == "u64" or
                ek == "u128" or ek == "usize" or
                ek == "comptime_int") return true;
        }
        // 相同类型
        if (vk == ek) return true;
        return false;
    }

    // 推断分配来源
    fn infer_source(self: *mut Self, expr: AstNode) AllocSource {
        var k = expr.kind;
        // 字面量 = 无所有权
        if (k == "IntLit" or k == "FloatLit" or k == "BoolLit" or
            k == "StrLit" or k == "CharLit" or k == "NullLit" or k == "VoidLit") {
            return AllocSource.None;
        }
        // 数组字面量 = 堆分配
        if (k == "ArrayLit") {
            return AllocSource.NonArena;
        }
        // 函数调用 = 堆分配
        if (k == "Call") {
            return AllocSource.NonArena;
        }
        // 标识符 = 继承来源
        if (k == "Ident") {
            var name = get_prop(expr.props, "name");
            if (name) |n| {
                var found = self.lookup(n);
                if (found) |info| {
                    return info.source;
                }
            }
        }
        return AllocSource.Unknown;
    }

    // 获取表达式类型
    fn type_of_expr(self: *mut Self, expr: AstNode) SType {
        var k = expr.kind;
        if (k == "IntLit") { return make_ty("comptime_int"); }
        if (k == "FloatLit") { return make_ty("float"); }
        if (k == "BoolLit") { return make_ty("bool"); }
        if (k == "StrLit") { return make_ty("str"); }
        if (k == "CharLit") { return make_ty("u8"); }
        if (k == "NullLit") { return make_ty("null"); }
        if (k == "VoidLit") { return make_ty("void"); }
        if (k == "Ident") {
            var name = get_prop(expr.props, "name");
            if (name) |n| {
                // 在作用域中查找变量类型
                var found = self.lookup(n);
                if (found) |info| { return info.ty; }
                // 在类型注册表中查找
                if (self.types.contains(n)) {
                    var t = self.types.get(n);
                    if (t) |tt| { return tt; }
                }
                // 函数名 → 函数类型
                if (self.funcs.contains(n)) { return make_ty("fn"); }
                // 内建名称
                if (self.is_builtin_name(n)) {
                    if (n == "true" or n == "false") return make_ty("bool");
                    if (n == "null") return make_ty("null");
                    if (n == "void") return make_ty("void");
                    // 其他内建名（alloc, io 等）→ unknown
                }
            }
            return make_ty("unknown");
        }
        if (k == "Binary") {
            if (expr.children.len >= 3) {
                var op = expr.children[2].kind;
                var l = self.type_of_expr(expr.children[0]);
                var r = self.type_of_expr(expr.children[1]);
                // 逻辑运算符返回 bool
                if (op == "And" or op == "Or") {
                    return make_ty("bool");
                }
                // 比较运算符返回 bool
                if (op == "Eq" or op == "Ne" or op == "Lt" or
                    op == "Le" or op == "Gt" or op == "Ge") {
                    return make_ty("bool");
                }
                // 算术运算符：如果任一操作数是 float，结果 float
                if (l.kind.as_slice() == "float" or r.kind.as_slice() == "float") return make_ty("float");
                // 否则 return comptime_int（后续会收窄）
                return make_ty("comptime_int");
            }
            return make_ty("unknown");
        }
        if (k == "Unary") {
            if (expr.children.len > 0) {
                return self.type_of_expr(expr.children[0]);
            }
            return make_ty("unknown");
        }
        if (k == "Call") {
            // 检查是否是函数调用
            if (expr.children.len > 0) {
                var callee = expr.children[0];
                if (callee.kind == "Ident") {
                    var name = get_prop(callee.props, "name");
                    if (name) |n| {
                        // 在函数注册表中查找
                        if (self.funcs.contains(n)) {
                            var sig = self.funcs.get(n);
                            if (sig) |s| { return s.ret_type; }
                        }
                        // 内建函数
                        if (n == "expect" or n == "expect_eq") return make_ty("void");
                        if (n == "@intCast" or n == "@floatCast") return make_ty("unknown");
                    }
                }
            }
            return make_ty("unknown");
        }
        if (k == "ArrayLit") { return make_ty("array"); }
        if (k == "AtBuiltin") { return make_ty("unknown"); }
        if (k == "Field") {
            // error.NotFound → 错误类型
            if (expr.children.len > 0) {
                var base = expr.children[0];
                if (base.kind == "Ident") {
                    var name = get_prop(base.props, "name");
                    if (name) |n| {
                        if (slice_eq(n, "error")) {
                            return make_ty("error_type");
                        }
                    }
                }
            }
            return make_ty("unknown");
        }
        if (k == "Index") { return make_ty("unknown"); }
        if (k == "Unwrap") {
            if (expr.children.len > 0) {
                return self.type_of_expr(expr.children[0]);
            }
            return make_ty("unknown");
        }
        return make_ty("unknown");
    }

    // 检查程序（两遍：收集 + 检查）
    fn check_program(self: *mut Self, prog: AstNode) void {
        // 第一遍：收集所有声明
        self.collect_program(prog);
        // 第二遍：检查
        var mut i: usize = 0;
        while (i < prog.children.len) {
            self.check_decl(prog.children[i]);
            i += 1;
        }
    }

    // ========== 收集阶段（第一遍） ==========

    // 收集所有声明
    fn collect_program(self: *mut Self, prog: AstNode) void {
        var mut i: usize = 0;
        while (i < prog.children.len) {
            self.collect_decl(prog.children[i]);
            i += 1;
        }
    }

    // 收集单个声明
    fn collect_decl(self: *mut Self, decl: AstNode) void {
        var k = decl.kind;
        if (k == "Class") { self.collect_class(decl); }
        else if (k == "Enum") { self.collect_enum(decl); }
        else if (k == "Union") { self.collect_union(decl); }
        else if (k == "Interface") { self.collect_interface(decl); }
        else if (k == "Fn") { self.collect_fn(decl); }
        else if (k == "Namespace") {
            var mut i: usize = 0;
            while (i < decl.children.len) {
                self.collect_decl(decl.children[i]);
                i += 1;
            }
        }
    }

    // 收集 class 声明
    fn collect_class(self: *mut Self, decl: AstNode) void {
        var name = get_prop(decl.props, "name");
        if (name) |n| {
            var ty = make_ty(n);
            self.register_type(n, ty);
        }
    }

    // 收集 enum 声明
    fn collect_enum(self: *mut Self, decl: AstNode) void {
        var name = get_prop(decl.props, "name");
        if (name) |n| {
            var ty = make_ty(n);
            self.register_type(n, ty);
        }
    }

    // 收集 union 声明
    fn collect_union(self: *mut Self, decl: AstNode) void {
        var name = get_prop(decl.props, "name");
        if (name) |n| {
            var ty = make_ty(n);
            self.register_type(n, ty);
        }
    }

    // 收集 interface 声明
    fn collect_interface(self: *mut Self, decl: AstNode) void {
        var name = get_prop(decl.props, "name");
        if (name) |n| {
            var ty = make_ty(n);
            self.register_type(n, ty);
        }
    }

    // 收集 fn 声明
    fn collect_fn(self: *mut Self, decl: AstNode) void {
        var name = get_prop(decl.props, "name");
        if (name) |n| {
            var sig = FnSig{
                param_types = Vec<SType>.init(alloc),
                ret_type = make_ty("unknown"),
            };
            self.register_func(n, sig);
        }
    }

    // ========== 检查阶段（第二遍） ==========

    // 检查声明
    fn check_decl(self: *mut Self, decl: AstNode) void {
        var k = decl.kind;
        if (k == "Fn") { self.check_fn(decl); }
        else if (k == "Class") { self.check_class(decl); }
        else if (k == "Namespace") {
            var mut i: usize = 0;
            while (i < decl.children.len) {
                self.check_decl(decl.children[i]);
                i += 1;
            }
        }
    }

    // 检查类声明：逐个检查方法体（self 由 current_class 在 check_fn 内注册）
    fn check_class(self: *mut Self, decl: AstNode) void {
        var cname = get_prop(decl.props, "name");
        if (cname) |c| {
            self.current_class = c;
            var mut i: usize = 0;
            while (i < decl.children.len) {
                var child = decl.children[i];
                if (child.kind == "Fn") { self.check_fn(child); }
                i += 1;
            }
            self.current_class = "";
        }
    }

    // 检查函数声明
    fn check_fn(self: *mut Self, decl: AstNode) void {
        self.push_scope();
        // 解析返回类型是否是错误联合
        var ru = get_prop(decl.props, "ret_union");
        if (ru) |_| { self.current_fn_ret_is_error_union = true; }
        else { self.current_fn_ret_is_error_union = false; }
        // 方法体：注册 self（显式 self 参数会在下方参数循环中覆盖）
        if (self.current_class.len > 0) {
            var self_info = VarInfo{
                ty = make_ty(self.current_class),
                source = AllocSource.Unknown,
                mut_ = true,
            };
            self.register("self", self_info);
        }
        var mut i: usize = 0;
        while (i < decl.children.len) {
            var child = decl.children[i];
            if (child.kind == "Param") {
                var pname = get_prop(child.props, "name");
                if (pname) |n| {
                    var param_ty = self.resolve_ty(child.props);
                    var info = VarInfo{
                        ty = param_ty,
                        source = AllocSource.Unknown,
                        mut_ = false,
                    };
                    self.register(n, info);
                }
            }
            i += 1;
        }
        i = 0;
        while (i < decl.children.len) {
            var child = decl.children[i];
            if (child.kind == "Block") {
                self.check_block(child);
            }
            i += 1;
        }
        self.pop_scope();
        self.current_fn_ret_is_error_union = false;
    }

    // 检查块
    fn check_block(self: *mut Self, block: AstNode) void {
        self.push_scope();
        var mut i: usize = 0;
        while (i < block.children.len) {
            self.check_stmt(block.children[i]);
            i += 1;
        }
        self.pop_scope();
    }

    // 检查语句
    fn check_stmt(self: *mut Self, stmt: AstNode) void {
        var k = stmt.kind;
        if (k == "Block") {
            self.check_block(stmt);
        } else if (k == "VarDecl") {
            self.check_var_decl(stmt);
        } else if (k == "If") {
            self.check_if(stmt);
        } else if (k == "While") {
            self.check_while(stmt);
        } else if (k == "For") {
            self.check_for(stmt);
        } else if (k == "Switch") {
            self.check_switch(stmt);
        } else if (k == "Return") {
            self.check_return(stmt);
        } else if (k == "ExprStmt") {
            if (stmt.children.len > 0) {
                self.check_expr(stmt.children[0]);
            }
        } else if (k == "Defer" or k == "Errdefer") {
        } else if (k == "Empty" or k == "Break" or k == "Continue") {
        } else if (k == "ConstDecl") {
            var name = get_prop(stmt.props, "name");
            if (name) |n| {
                var info = VarInfo{
                    ty = make_ty("unknown"),
                    source = AllocSource.Unknown,
                    mut_ = false,
                };
                self.register(n, info);
            }
        }
    }

    // 检查变量声明
    fn check_var_decl(self: *mut Self, stmt: AstNode) void {
        var name = get_prop(stmt.props, "name");
        // 解析类型注解：第一个子节点的 kind 是类型名（如 "i32"）
        var mut ty = make_ty("unknown");
        if (stmt.children.len > 0) {
            var first = stmt.children[0];
            var candidate = self.ty_of(first.kind);
            var ck = candidate.kind.as_slice();
            if (ck != "unknown" and ck != "generic") {
                ty = candidate;
            }
        }
        // 判断是否有初始值表达式
        var mut has_init = false;
        if (stmt.children.len > 1) {
            has_init = true;
        } else if (stmt.children.len == 1) {
            var first = stmt.children[0];
            var candidate = self.ty_of(first.kind);
            var ck = candidate.kind.as_slice();
            if (ck == "unknown" or ck == "generic") {
                has_init = true;
            }
        }
        // 检查 mut
        var mut is_mut = false;
        var m = get_prop(stmt.props, "mut");
        if (m) |_| { is_mut = true; }
        // 推断分配来源
        var mut source = AllocSource.Unknown;
        if (has_init and stmt.children.len > 0) {
            var last_idx = stmt.children.len - 1;
            source = self.infer_source(stmt.children[last_idx]);
        }
        if (name) |n| {
            var info = VarInfo{
                ty = ty,
                source = source,
                mut_ = is_mut,
            };
            self.register(n, info);
            // ADR-0030：重新赋值/新声明 → 复活已 move 的同名变量
            self.moved.remove(n);
        }
        // 检查初始值表达式类型（初始值是最后一个子节点）
        if (has_init and stmt.children.len > 0) {
            var last_idx = stmt.children.len - 1;
            var init_expr = stmt.children[last_idx];
            // 检查 move 操作
            if (init_expr.kind == "Move" and init_expr.children.len > 0) {
                var inner = init_expr.children[0];
                if (inner.kind == "Ident") {
                    var name = get_prop(inner.props, "name");
                    if (name) |n| {
                        var found = self.lookup(n);
                        if (found) |info| {
                            if (info.source == AllocSource.None) {
                                var msg = Vec<u8>.init(alloc);
                                msg.append('e'); msg.append('r'); msg.append('r'); msg.append('o'); msg.append('r');
                                msg.append(':'); msg.append(' ');
                                msg.append('c'); msg.append('a'); msg.append('n'); msg.append('n'); msg.append('o'); msg.append('t');
                                msg.append(' '); msg.append('m'); msg.append('o'); msg.append('v'); msg.append('e');
                                msg.append(' '); msg.append('`');
                                var mut j: usize = 0;
                                while (j < n.len) { msg.append(n[j]); j += 1; }
                                msg.append('`'); msg.append(':'); msg.append(' ');
                                msg.append('v'); msg.append('a'); msg.append('l'); msg.append('u'); msg.append('e');
                                msg.append(' '); msg.append('t'); msg.append('y'); msg.append('p'); msg.append('e');
                                msg.append(' '); msg.append('h'); msg.append('a'); msg.append('s');
                                msg.append(' '); msg.append('n'); msg.append('o'); msg.append(' ');
                                msg.append('o'); msg.append('w'); msg.append('n'); msg.append('e'); msg.append('r'); msg.append('s'); msg.append('h'); msg.append('i'); msg.append('p');
                                self.error(msg);
                            }
                        }
                    }
                }
            }
            var init_type = self.type_of_expr(init_expr);
            var tk = ty.kind.as_slice();
            var ik = init_type.kind.as_slice();
            if (tk != "unknown" and ik != "unknown") {
                if (!self.is_compatible(init_type, ty)) {
                    var msg = Vec<u8>.init(alloc);
                    msg.append('t'); msg.append('y'); msg.append('p'); msg.append('e');
                    msg.append(' '); msg.append('m'); msg.append('i'); msg.append('s');
                    msg.append('m'); msg.append('a'); msg.append('t'); msg.append('c');
                    msg.append('h'); msg.append(':'); msg.append(' ');
                    msg.append('e'); msg.append('x'); msg.append('p'); msg.append('e');
                    msg.append('c'); msg.append('t'); msg.append('e'); msg.append('d');
                    msg.append(' ');
                    var mut ki: usize = 0;
                    while (ki < ty.kind.len) { msg.append(ty.kind[ki]); ki += 1; }
                    msg.append(','); msg.append(' ');
                    msg.append('g'); msg.append('o'); msg.append('t'); msg.append(' ');
                    ki = 0;
                    while (ki < init_type.kind.len) { msg.append(init_type.kind[ki]); ki += 1; }
                    self.error(msg);
                }
            }
        }
    }

    // 检查条件表达式类型（与 Rust 参考保持一致：接受大多数类型）
    fn check_condition(self: *mut Self, cond: AstNode) void {
        // 当前阶段：条件表达式已在 check_expr 中检查，
        // 此处保留扩展点（未来可添加更严格的类型检查）
    }

    // 检查 if 语句
    fn check_if(self: *mut Self, stmt: AstNode) void {
        if (stmt.children.len > 0) {
            self.check_expr(stmt.children[0]);
            self.check_condition(stmt.children[0]);
        }
        if (stmt.children.len > 1) {
            var then_block = stmt.children[1];
            if (then_block.kind == "Block") {
                var p = get_prop(stmt.props, "payload");
                if (p) |pn| {
                    self.push_scope();
                    var info = VarInfo{
                        ty = make_ty("unknown"),
                        source = AllocSource.Unknown,
                        mut_ = false,
                    };
                    self.register(pn, info);
                    self.check_block(then_block);
                    self.pop_scope();
                } else {
                    self.check_block(then_block);
                }
            }
        }
        if (stmt.children.len > 2) {
            var else_block = stmt.children[2];
            if (else_block.kind == "Block") {
                self.check_block(else_block);
            } else if (else_block.kind == "If") {
                self.check_if(else_block);
            }
        }
    }

    // 检查 while 语句
    fn check_while(self: *mut Self, stmt: AstNode) void {
        if (stmt.children.len > 0) {
            self.check_expr(stmt.children[0]);
            self.check_condition(stmt.children[0]);
        }
        if (stmt.children.len > 1) {
            var body = stmt.children[1];
            if (body.kind == "Block") {
                var p = get_prop(stmt.props, "payload");
                if (p) |pn| {
                    self.push_scope();
                    var info = VarInfo{
                        ty = make_ty("unknown"),
                        source = AllocSource.Unknown,
                        mut_ = false,
                    };
                    self.register(pn, info);
                    self.check_block(body);
                    self.pop_scope();
                } else {
                    self.check_block(body);
                }
            }
        }
    }

    // 检查 for 语句
    fn check_for(self: *mut Self, stmt: AstNode) void {
        if (stmt.children.len > 0) {
            self.check_expr(stmt.children[0]);
            self.check_condition(stmt.children[0]);
        }
        if (stmt.children.len > 1) {
            var body = stmt.children[1];
            if (body.kind == "Block") {
                // 迭代载荷 `for (xs) \|x\| {...}`：载荷绑定仅限循环体作用域
                var p = get_prop(stmt.props, "payload");
                if (p) |pn| {
                    self.push_scope();
                    var info = VarInfo{
                        ty = make_ty("unknown"),
                        source = AllocSource.Unknown,
                        mut_ = false,
                    };
                    self.register(pn, info);
                    self.check_block(body);
                    self.pop_scope();
                } else {
                    self.check_block(body);
                }
            }
        }
    }

    // 检查 switch 语句
    fn check_switch(self: *mut Self, stmt: AstNode) void {
        if (stmt.children.len > 0) {
            self.check_expr(stmt.children[0]);
        }
        var mut i: usize = 1;
        while (i < stmt.children.len) {
            var arm = stmt.children[i];
            if (arm.kind == "SwitchArm") {
                var mut j: usize = 0;
                while (j < arm.children.len) {
                    self.check_expr(arm.children[j]);
                    j += 1;
                }
            }
            i += 1;
        }
    }

    // 检查 return 语句
    fn check_return(self: *mut Self, stmt: AstNode) void {
        if (stmt.children.len > 0) {
            var expr = stmt.children[0];
            self.check_expr(expr);
            // 检查是否返回局部变量引用（引用逃逸检测）
            if (expr.kind == "AddrOf" and expr.children.len > 0) {
                var inner = expr.children[0];
                if (inner.kind == "Ident") {
                    var name = get_prop(inner.props, "name");
                    if (name) |n| {
                        // 若标识符为作用域内局部变量/参数 → 引用逃逸
                        var found = self.lookup(n);
                        if (found) |_| {
                            var msg = Vec<u8>.init(alloc);
                            append_bytes(&msg, "error: cannot return reference to `");
                            var mut j: usize = 0;
                            while (j < n.len) { msg.append(n[j]); j += 1; }
                            append_bytes(&msg, "`: reference escapes function scope");
                            self.error(msg);
                        }
                    }
                }
            }
            // 检查是否返回错误字面量但函数没有声明错误联合返回类型
            if (expr.kind == "Field" and expr.children.len > 0) {
                var base = expr.children[0];
                if (base.kind == "Ident") {
                    var name = get_prop(base.props, "name");
                    if (name) |n| {
                        if (slice_eq(n, "error")) {
                            if (!self.current_fn_ret_is_error_union) {
                                var msg = Vec<u8>.init(alloc);
                                append_bytes(&msg, "error: cannot return error literal: function does not declare error union");
                                self.error(msg);
                            }
                        }
                    }
                }
            }
        }
    }

    // 检查表达式
    fn check_expr(self: *mut Self, expr: AstNode) void {
        var k = expr.kind;
        if (k == "Ident") {
            self.check_ident(expr);
        } else if (k == "ClassLit") {
            // 只检查各字段初始化值；字段名不作标识符查析（宽容，避免与参考实现诊断分歧）
            var mut i: usize = 0;
            while (i < expr.children.len) {
                var fi = expr.children[i];
                var mut j: usize = 0;
                while (j < fi.children.len) {
                    self.check_expr(fi.children[j]);
                    j += 1;
                }
                i += 1;
            }
        } else if (k == "Binary") {
            if (expr.children.len >= 2) {
                self.check_expr(expr.children[0]);
                self.check_expr(expr.children[1]);
            }
        } else if (k == "Unary") {
            if (expr.children.len > 0) {
                self.check_expr(expr.children[0]);
            }
        } else if (k == "Call") {
            var mut i: usize = 0;
            while (i < expr.children.len) {
                self.check_expr(expr.children[i]);
                i += 1;
            }
        } else if (k == "Field") {
            if (expr.children.len > 0) {
                self.check_expr(expr.children[0]);
            }
        } else if (k == "Index") {
            var mut i: usize = 0;
            while (i < expr.children.len) {
                self.check_expr(expr.children[i]);
                i += 1;
            }
        } else if (k == "Move") {
            if (expr.children.len > 0) {
                var inner = expr.children[0];
                self.check_expr(inner);
                // ADR-0030：目标解包 —— Move[AddrOf[Ident]]（move &t / move &mut t）
                // 与 Move[Ident]（裸别名 ≡ move &mut t）
                var mut tgt: ?AstNode = null;
                var mut move_mut = false;
                if (inner.kind == "AddrOf") {
                    if (inner.children.len > 0) { tgt = inner.children[0]; }
                    if (has_prop(inner.props, "mut")) { move_mut = true; }
                } else if (inner.kind == "Ident") {
                    tgt = inner;
                    move_mut = true;
                }
                if (tgt) |tn| {
                    if (tn.kind == "Ident") {
                        var name = get_prop(tn.props, "name");
                        if (name) |n| {
                            var found = self.lookup(n);
                            if (found) |info| {
                                // 可写形态（&mut / 裸别名）要求 `mut`
                                if (move_mut and !info.mut_) {
                                    var msg = Vec<u8>.init(alloc);
                                    append_bytes(&msg, "error: cannot move `");
                                    append_bytes(&msg, n);
                                    append_bytes(&msg, "` because it is not declared `mut`; use `move &");
                                    append_bytes(&msg, n);
                                    append_bytes(&msg, "` for read-only ownership transfer");
                                    self.error(msg);
                                }
                                // 分配来源检查（None = 无所有权；Arena/global 判定 K6 同步）
                                if (info.source == AllocSource.None) {
                                    var msg = Vec<u8>.init(alloc);
                                    append_bytes(&msg, "error: cannot move `");
                                    append_bytes(&msg, n);
                                    append_bytes(&msg, "`: value type has no ownership (move transfers destroy responsibility)");
                                    self.error(msg);
                                }
                                // 冻结登记：move 后原变量禁止使用（重新赋值复活）
                                self.moved.put(n, true);
                            }
                        }
                    }
                }
            }
        } else if (k == "Assign") {
            // ADR-0030：赋值复活 + 目标/值递归检查
            if (expr.children.len >= 2) {
                var tgt = expr.children[0];
                if (tgt.kind == "Ident") {
                    var name = get_prop(tgt.props, "name");
                    if (name) |n| { self.moved.remove(n); }
                }
                self.check_expr(tgt);
                self.check_expr(expr.children[1]);
            }
        } else if (k == "AddrOf" or k == "Try" or k == "Await") {
            if (expr.children.len > 0) {
                self.check_expr(expr.children[0]);
            }
        } else if (k == "ArrayLit") {
            var mut i: usize = 0;
            while (i < expr.children.len) {
                self.check_expr(expr.children[i]);
                i += 1;
            }
        } else if (k == "IfExpr") {
            var mut i: usize = 0;
            while (i < expr.children.len) {
                self.check_expr(expr.children[i]);
                i += 1;
            }
        } else if (k == "SwitchExpr") {
            var mut i: usize = 0;
            while (i < expr.children.len) {
                self.check_expr(expr.children[i]);
                i += 1;
            }
        } else if (k == "Closure") {
            var mut i: usize = 0;
            while (i < expr.children.len) {
                if (expr.children[i].kind == "Block") {
                    self.check_block(expr.children[i]);
                } else {
                    self.check_expr(expr.children[i]);
                }
                i += 1;
            }
        } else if (k == "DotCall") {
            if (expr.children.len > 0) {
                self.check_expr(expr.children[0]);
            }
        } else if (k == "Unwrap") {
            if (expr.children.len > 0) {
                self.check_expr(expr.children[0]);
            }
        } else if (k == "AtBuiltin") {
            var mut i: usize = 0;
            while (i < expr.children.len) {
                self.check_expr(expr.children[i]);
                i += 1;
            }
        }
    }

    // 检查标识符引用
    fn check_ident(self: *mut Self, expr: AstNode) void {
        var name = get_prop(expr.props, "name");
        if (name) |n| {
            if (self.is_builtin_name(n)) { return; }
            if (self.types.contains(n)) { return; }
            if (self.funcs.contains(n)) { return; }
            var found = self.lookup(n);
            if (found) |_| {
                // ADR-0030：use-after-move 冻结——move 后使用原变量编译错误
                if (self.moved.contains(n)) {
                    var msg = Vec<u8>.init(alloc);
                    append_bytes(&msg, "error: use of moved variable `");
                    append_bytes(&msg, n);
                    append_bytes(&msg, "`; assign to it again to revive");
                    self.error(msg);
                }
                return;
            }
            var msg = Vec<u8>.init(alloc);
            msg.append('e'); msg.append('r'); msg.append('r'); msg.append('o'); msg.append('r');
            msg.append(':'); msg.append(' ');
            msg.append('u'); msg.append('n'); msg.append('d'); msg.append('e'); msg.append('f');
            msg.append('i'); msg.append('n'); msg.append('e'); msg.append('d');
            msg.append(' '); msg.append('n'); msg.append('a'); msg.append('m'); msg.append('e');
            msg.append(' '); msg.append('`');
            var mut j: usize = 0;
            while (j < n.len) {
                msg.append(n[j]);
                j += 1;
            }
            msg.append('`');
            self.error(msg);
        }
    }

    // 判断是否为内建名称（使用 slice_eq 避免 &[u8] 指针比较问题）
    fn is_builtin_name(self: *mut Self, name: &[u8]) bool {
        if (slice_eq(name, "alloc") or slice_eq(name, "page_allocator")) return true;
        if (slice_eq(name, "io") or slice_eq(name, "stdout") or slice_eq(name, "stderr")) return true;
        if (slice_eq(name, "true") or slice_eq(name, "false") or slice_eq(name, "null") or slice_eq(name, "void")) return true;
        if (slice_eq(name, "pi")) return true;
        if (slice_eq(name, "Vec") or slice_eq(name, "Deque") or slice_eq(name, "Map") or slice_eq(name, "Table")) return true;
        if (slice_eq(name, "String") or slice_eq(name, "Allocator") or slice_eq(name, "ExitType")) return true;
        if (slice_eq(name, "Pipe") or slice_eq(name, "Tee") or slice_eq(name, "Funnel") or slice_eq(name, "Hub")) return true;
        if (slice_eq(name, "i8") or slice_eq(name, "i16") or slice_eq(name, "i32") or slice_eq(name, "i64") or slice_eq(name, "i128")) return true;
        if (slice_eq(name, "u8") or slice_eq(name, "u16") or slice_eq(name, "u32") or slice_eq(name, "u64") or slice_eq(name, "u128")) return true;
        if (slice_eq(name, "isize") or slice_eq(name, "usize")) return true;
        if (slice_eq(name, "f16") or slice_eq(name, "f32") or slice_eq(name, "f64") or slice_eq(name, "f128")) return true;
        if (slice_eq(name, "bool") or slice_eq(name, "void")) return true;
        if (slice_eq(name, "comptime_int") or slice_eq(name, "comptime_float")) return true;
        if (slice_eq(name, "type") or slice_eq(name, "anytype")) return true;
        if (slice_eq(name, "Future")) return true;
        if (slice_eq(name, "expect") or slice_eq(name, "expect_eq")) return true;
        if (slice_eq(name, "error")) return true;
        return false;
    }

    // 输出诊断结果
    fn report(self: *mut Self) void {
        if (self.diags.len == 0) {
            io.print("OK\n");
        } else {
            var mut i: usize = 0;
            while (i < self.diags.len) {
                io.print("{}\n", self.diags[i].as_slice());
                i += 1;
            }
        }
    }
}

// ============================================================
// 入口
// ============================================================

// ============================================================
// AST dump 调试工具（--dump-ast）
// ============================================================

fn dump_indent(depth: usize) void {
    var mut i: usize = 0;
    while (i < depth) { io.print("  "); i += 1; }
}

fn dump_ast_rec(n: AstNode, depth: usize) void {
    dump_indent(depth);
    io.print("{} props[{}]{}\n", n.kind, n.props.len, n.props.as_slice());
    var mut i: usize = 0;
    while (i < n.children.len) {
        dump_ast_rec(n.children[i], depth + 1);
        i += 1;
    }
}

fn main(args: Vec<String>) !void {
    // --dump-ast 调试模式：args[1] 为模式开关（args[0] 是程序自身路径）
    if (args.len >= 3 and args[1].as_slice() == "--dump-ast") {
        var mut dsrc = try io.fs.read_file(args[2], alloc);
        var dkw = build_rev_kw_map();
        var dlx: Lexer = alloc.init(Lexer{
            src = dsrc, n = dsrc.len,
            pos = 0, line = 1, col = 1,
            tokens = Vec<Token>.init(alloc),
        });
        dlx.run();
        var dparser: Parser = alloc.init(Parser{
            tokens = dlx.tokens, pos = 0,
            n = dlx.tokens.len,
            rev_kw_map = dkw,
        });
        var dast = dparser.parse_program();
        dump_ast_rec(dast, 0);
        return;
    }

    var mut path = args[0];
    if (args.len >= 2) { path = args[1]; }
    var mut src = try io.fs.read_file(path, alloc);

    // 构建关键字字典
    var rev_kw_map = build_rev_kw_map();

    // 词法分析
    var lx: Lexer = alloc.init(Lexer{
        src = src, n = src.len,
        pos = 0, line = 1, col = 1,
        tokens = Vec<Token>.init(alloc),
    });
    lx.run();

    // 语法分析
    var parser: Parser = alloc.init(Parser{
        tokens = lx.tokens, pos = 0,
        n = lx.tokens.len,
        rev_kw_map = rev_kw_map,
    });
    var ast = parser.parse_program();

    // 语义检查
    var checker: Checker = alloc.init(Checker{
        diags = Vec<Vec<u8>>.init(alloc),
        src = Vec<u8>.init(alloc),
        line_starts = Vec<usize>.init(alloc),
        scopes = Vec<ScopeEntry>.init(alloc),
        scope_sizes = Vec<usize>.init(alloc),
        types = Map<&[u8], SType>.init(alloc),
        funcs = Map<&[u8], FnSig>.init(alloc),
        current_fn_ret_is_error_union = false,
        current_class = "",
        moved = Map<&[u8], bool>.init(alloc),
    });
    checker.init(src);
    checker.check_program(ast);
    checker.report();
}
