import H.std.{io};

// ============================================================
// stage1/parser.hc — H 版 parser（K2：AST 转储，双实现对照）
//
// 双实现对照：本文件与 Rust 参考 parser（tag1/hc/src/parser.rs）输出
// 同一格式 `{depth} {Tag} {payload} {start} {end}`，逐行 diff，差异即 bug。
// 后序（子节点先于父节点；Program 不占行）；Type 节点无 span。
//
// 用法：hc run stage1/parser.hc <file.hc>
//
// 分阶段落地（K2a）：词法→Token 适配 + 类型系统 + 声明骨架
//   - 声明：global / const（含 error{...} 与 A || B 错误集别名）/ namespace
//     / using / import（含符号选择）/ enum / union / interface
//   - 表达式：最小集合（字面量 + Ident），供 const/global 初值
//   - 语句：尚未引入（K2b/c 补全 fn 体 / 语句 / 类方法）
// ============================================================

// ---------- 基础字符函数（与 lexer.hc 同源） ----------

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

// 关键字表（45 项；struct 并入 KwClass）
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
    if (name == "struct") return "KwClass";
    if (name == "enum") return "KwEnum";
    if (name == "union") return "KwUnion";
    if (name == "tree") return "KwTree";
    if (name == "interface") return "KwInterface";
    if (name == "where") return "KwWhere";
    if (name == "namespace") return "KwNamespace";
    if (name == "using") return "KwUsing";
    if (name == "import") return "KwImport";
    if (name == "pub") return "KwPub";
    if (name == "export") return "KwExport";
    if (name == "o") return "KwO";
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
    if (name == "spawn") return "KwSpawn";
    if (name == "void") return "KwVoid";
    if (name == "null") return "KwNull";
    if (name == "true") return "KwTrue";
    if (name == "false") return "KwFalse";
    return null;
}

// ---------- Unicode 工具（与 lexer.hc 同源） ----------

fn cp_at(content: Vec(u8), i: i32) i32 {
    var b0: i32 = @intCast(i32, content[i]);
    if (b0 < 0x80) return b0;
    var b1: i32 = @intCast(i32, content[i + 1]);
    if (b0 < 0xE0) return (b0 & 0x1F) * 64 + (b1 & 0x3F);
    var b2: i32 = @intCast(i32, content[i + 2]);
    if (b0 < 0xF0) return (b0 & 0x0F) * 4096 + (b1 & 0x3F) * 64 + (b2 & 0x3F);
    var b3: i32 = @intCast(i32, content[i + 3]);
    return (b0 & 0x07) * 262144 + (b1 & 0x3F) * 4096 + (b2 & 0x3F) * 64 + (b3 & 0x3F);
}

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

fn append_unicode_escape(out: Vec(u8), cp: i32) void {
    out.append('\\'); out.append('u'); out.append('{');
    var digits = "0123456789abcdef";
    var sh: i32 = 0;
    var tmp: i32 = cp;
    while (tmp >= 0x10) { tmp = tmp / 16; sh += 4; }
    while (sh >= 0) {
        var idx = (cp >> sh) & 0xF;
        var d = digits[idx..(idx + 1)];
        out.append(d[0]);
        sh -= 4;
    }
    out.append('}');
}

// Rust Debug 输出：字符串内容转义（与 K1 验证一致）
fn dbg_escape(content: Vec(u8)) Vec(u8) {
    var out = Vec(u8).init(alloc);
    var n: i32 = @intCast(i32, content.len);
    var i: i32 = 0;
    while (i < n) {
        var cp = cp_at(content, i);
        var w = utf8_width(content[i]);
        if (cp == 0x09) { out.append('\\'); out.append('t'); }
        else if (cp == 0x0A) { out.append('\\'); out.append('n'); }
        else if (cp == 0x0D) { out.append('\\'); out.append('r'); }
        else if (cp == 0x22) { out.append('\\'); out.append('"'); }
        else if (cp == 0x5C) { out.append('\\'); out.append('\\'); }
        else if (cp == 0x00) { out.append('\\'); out.append('0'); }
        else if (is_printable(cp)) {
            var j: i32 = 0;
            while (j < w) { out.append(content[i + j]); j += 1; }
        }
        else { append_unicode_escape(out, cp); }
        i += w;
    }
    return out;
}

fn dbg_escape_str(s: String) String {
    var bytes = Vec(u8).init(alloc);
    var n: i32 = @intCast(i32, s.len);
    var i: i32 = 0;
    while (i < n) {
        bytes.append(s[i]);
        i += 1;
    }
    var esc = dbg_escape(bytes);
    return String.from_slice(esc, alloc);
}

// ---------- 字符串工具 ----------

// int → String
fn i2s(v: i32) String {
    var out = Vec(u8).init(alloc);
    var neg = false;
    var n: i32 = v;
    if (n < 0) { neg = true; n = -n; }
    var cnt: i32 = 1;
    var tmp: i32 = n;
    while (tmp >= 10) { tmp = tmp / 10; cnt += 1; }
    if (neg) { out.append('-'); }
    var place: i32 = cnt;
    while (place > 0) {
        var pow: i32 = 1;
        var k: i32 = 1;
        while (k < place) { pow *= 10; k += 1; }
        var d = n / pow;
        out.append(@intCast(u8, '0' + d));
        n = n % pow;
        place -= 1;
    }
    return String.from_slice(out, alloc);
}

fn s_of(b: &[u8]) String { return String.from_slice(b, alloc); }
fn cat_b(a: String, b: &[u8]) String { return a.concat(String.from_slice(b, alloc)); }
fn cat_s(a: String, b: String) String { return a.concat(b); }
fn cat_i(a: String, n: i32) String { return a.concat(i2s(n)); }
fn seq(a: String, b: &[u8]) bool { return a == String.from_slice(b, alloc); }

// `{:?}` 字符串（带引号、已转义）——alias/select 用
fn qq(s: String) String {
    return cat_b(cat_b(s_of("\""), dbg_escape_str(s)), "\"");
}

// 逗号连接 Vec(String)
fn join_c(v: Vec(String)) String {
    var out = String.from_slice("", alloc);
    var i: i32 = 0;
    while (i < @intCast(i32, v.len)) {
        if (i > 0) { out = cat_b(out, ","); }
        out = cat_s(out, v[i]);
        i += 1;
    }
    return out;
}

// 点号连接路径 Vec(String)
fn join_d(v: Vec(String)) String {
    var out = String.from_slice("", alloc);
    var i: i32 = 0;
    while (i < @intCast(i32, v.len)) {
        if (i > 0) { out = cat_b(out, "."); }
        out = cat_s(out, v[i]);
        i += 1;
    }
    return out;
}

// ---------- Token ----------

class Token {
    kind: &[u8],
    text: String,
    val: i32,
    start: i32,
    end: i32,
}

// ---------- Lexer（产出 Token 流；源逻辑同 lexer.hc） ----------

class Lexer {
    src: &[u8],
    n: i32,
    mut pos: i32,
    out: Vec(Token),

    fn bump(self: *mut Self) void {
        self.pos += utf8_width(self.src[self.pos]);
    }

    fn push(self: *mut Self, kind: &[u8], text: String, val: i32, start: i32) void {
        self.out.append(Token{ kind = kind, text = text, val = val, start = start, end = self.pos });
    }

    fn append_char(self: *mut Self, content: Vec(u8)) void {
        var w = utf8_width(self.src[self.pos]);
        var k: i32 = 0;
        while (k < w) {
            content.append(self.src[self.pos + k]);
            k += 1;
        }
        self.bump();
    }

    fn emit_simple(self: *mut Self, start: i32, kind: &[u8]) void {
        self.push(kind, String.from_slice("", alloc), 0, start);
    }
    fn emit_slice_payload(self: *mut Self, start: i32, kind: &[u8], payload: &[u8]) void {
        self.push(kind, String.from_slice(payload, alloc), 0, start);
    }
    fn emit_error(self: *mut Self, start: i32, msg: &[u8]) void {
        self.push("Error", String.from_slice(msg, alloc), 0, start);
    }
    fn emit_content(self: *mut Self, start: i32, kind: &[u8], content: Vec(u8)) void {
        self.push(kind, String.from_slice(content, alloc), 0, start);
    }

    fn run(self: *mut Self) void {
        while (true) {
            self.skip_ws();
            var start = self.pos;
            if (self.pos >= self.n) {
                self.emit_simple(start, "Eof");
                return;
            }
            var c = self.src[self.pos];
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
            var c = self.src[self.pos];
            if (is_ws(c)) {
                self.bump();
            } else if (c == '/' and self.pos + 1 < self.n and self.src[self.pos + 1] == '/') {
                while (self.pos < self.n and self.src[self.pos] != '\n') { self.bump(); }
            } else if (c == '/' and self.pos + 1 < self.n and self.src[self.pos + 1] == '*') {
                self.bump();
                self.bump();
                while (true) {
                    if (self.pos >= self.n) {
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

    fn lex_ident(self: *mut Self, start: i32) void {
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

    fn lex_number(self: *mut Self, start: i32) void {
        var buf = Vec(u8).init(alloc);
        var is_float = false;
        if (self.src[self.pos] == '0' and self.pos + 1 < self.n) {
            var c1 = self.src[self.pos + 1];
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
            var c = self.src[self.pos];
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
            var c = self.src[self.pos];
            if ((c == 'e' or c == 'E') and self.pos + 1 < self.n) {
                var c2 = self.src[self.pos + 1];
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
            var c = self.src[self.pos];
            if (c == 'i' or c == 'u' or c == 'f') {
                var j = self.pos;
                while (j < self.n and self.is_suffix_cont(self.src[j])) { j += utf8_width(self.src[j]); }
                var suf = self.src[self.pos..j];
                var slen: i32 = @intCast(i32, suf.len);
                if (slen >= 2) {
                    var ok = is_digit(self.src[self.pos + 1]) or suf == "isize" or suf == "usize";
                    if (ok) return suf;
                }
            }
        }
        return null;
    }

    fn finish_number(self: *mut Self, start: i32, kind: &[u8], buf: Vec(u8)) void {
        if (self.pos < self.n) {
            var suf = self.detect_suffix();
            if (suf) |s| {
                var slen: i32 = @intCast(i32, s.len);
                var k: i32 = 0;
                while (k < slen) {
                    buf.append(s[k]);
                    self.bump();
                    k += 1;
                }
            }
        }
        self.push(kind, String.from_slice(buf, alloc), 0, start);
    }

    fn lex_string(self: *mut Self, start: i32) void {
        self.bump();
        if (self.pos + 1 < self.n and self.src[self.pos] == '"' and self.src[self.pos + 1] == '"') {
            self.bump();
            self.bump();
            var content = Vec(u8).init(alloc);
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
        var content = Vec(u8).init(alloc);
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
                self.bump();
                if (self.pos >= self.n) {
                    self.emit_error(start, "unterminated string literal");
                    return;
                }
                var ec = self.src[self.pos];
                self.bump();
                if (ec == 'n') { content.append('\n'); }
                else if (ec == 'r') { content.append('\r'); }
                else if (ec == 't') { content.append('\t'); }
                else if (ec == '\\') { content.append('\\'); }
                else if (ec == '"') { content.append('"'); }
                else if (ec == '\'') { content.append('\''); }
                else if (ec == 'x') {
                    var hi: i32 = -1;
                    var lo: i32 = -1;
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
                    var v: i64 = 0;
                    var bad = false;
                    while (true) {
                        if (self.pos >= self.n) { bad = true; break; }
                        var ch = self.src[self.pos];
                        self.bump();
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

    fn lex_char(self: *mut Self, start: i32) void {
        self.bump();
        var val: i32 = -1;
        if (self.pos >= self.n) {
            self.emit_error(start, "unterminated char literal");
            return;
        }
        if (self.src[self.pos] == '\\') {
            self.bump();
            if (self.pos >= self.n) {
                self.emit_error(start, "unterminated char literal");
                return;
            }
            var c = self.src[self.pos];
            self.bump();
            if (c == 'n') { val = 0x0A; }
            else if (c == 'r') { val = 0x0D; }
            else if (c == 't') { val = 0x09; }
            else if (c == '\\') { val = 0x5C; }
            else if (c == '\'') { val = 0x27; }
            else if (c == 'x') {
                var hi: i32 = -1;
                var lo: i32 = -1;
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
        self.push("Char", String.from_slice("", alloc), val, start);
    }

    fn lex_punct(self: *mut Self, start: i32) void {
        var c = self.src[self.pos];
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
            self.emit_error(start, "unexpected character");
            self.emit_error(start, "unexpected character");
        }
    }
}

// ---------- 输出缓冲 ----------

class BufLine {
    mut depth: i32,
    text: String,
}

// ---------- Parser ----------

class Parser {
    toks: Vec(Token),
    mut pos: i32,
    lines: Vec(BufLine),
    field_buf: Vec(BufLine),
    method_buf: Vec(BufLine),
    mut buf_mode: i32,
    mut silent: i32,
    mut last_span_s: i32,
    mut last_span_e: i32,

    // ---- token 辅助 ----

    fn at(self: *mut Self, k: &[u8]) bool {
        if (self.pos >= @intCast(i32, self.toks.len)) { return k == "Eof"; }
        return self.toks[self.pos].kind == k;
    }
    fn peek_kind(self: *mut Self, n: i32) &[u8] {
        if (self.pos + n >= @intCast(i32, self.toks.len)) { return "Eof"; }
        return self.toks[self.pos + n].kind;
    }
    fn cur_start(self: *mut Self) i32 { return self.toks[self.pos].start; }
    fn cur_end(self: *mut Self) i32 { return self.toks[self.pos].end; }
    fn advance(self: *mut Self) void {
        self.pos += 1;
    }
    fn expect(self: *mut Self, k: &[u8]) !void {
        if (!self.at(k)) { return error.Expected; }
        self.pos += 1;
    }
    fn expect_ident(self: *mut Self) !String {
        if (self.toks[self.pos].kind != "Ident") { return error.Expected; }
        var t = self.toks[self.pos];
        self.pos += 1;
        self.last_span_s = t.start;
        self.last_span_e = t.end;
        return t.text;
    }
    fn expect_name_or_keyword(self: *mut Self) !String {
        var k = self.toks[self.pos].kind;
        if (k == "Ident") { return self.expect_ident(); }
        if (k == "KwWhere") { self.pos += 1; return s_of("where"); }
        if (k == "KwNull") { self.pos += 1; return s_of("null"); }
        if (k == "KwScript") { self.pos += 1; return s_of("script"); }
        if (k == "KwType") { self.pos += 1; return s_of("type"); }
        return error.Expected;
    }
    fn is_ident(self: *mut Self, name: &[u8]) bool {
        if (self.toks[self.pos].kind != "Ident") { return false; }
        return self.toks[self.pos].text == String.from_slice(name, alloc);
    }
    fn at_type_start(self: *mut Self) bool {
        var k = self.toks[self.pos].kind;
        if (k == "KwO" or k == "Star" or k == "Amp" or k == "Question" or k == "Bang") { return true; }
        if (k == "KwVoid" or k == "KwAnytype" or k == "KwType") { return true; }
        if (k == "LParen" or k == "LBracket") { return true; }
        if (k == "Ident") { return true; }
        return false;
    }

    // ---- 输出 ----

    fn emit_line(self: *mut Self, d: i32, text: String) void {
        if (self.silent != 0) { return; }
        if (self.buf_mode == 0) {
            self.lines.append(BufLine{ depth = d, text = text });
        } else if (self.buf_mode == 1) {
            self.field_buf.append(BufLine{ depth = d, text = text });
        } else {
            self.method_buf.append(BufLine{ depth = d, text = text });
        }
    }
    fn emit_sp(self: *mut Self, d: i32, text: String, s: i32, e: i32) void {
        var t = cat_b(text, " ");
        t = cat_i(t, s);
        t = cat_b(t, " ");
        t = cat_i(t, e);
        self.emit_line(d, t);
    }
    fn cur_len(self: *mut Self) i32 {
        if (self.buf_mode == 0) { return @intCast(i32, self.lines.len); }
        else if (self.buf_mode == 1) { return @intCast(i32, self.field_buf.len); }
        else { return @intCast(i32, self.method_buf.len); }
    }
    fn fix_lines(self: *mut Self, from: i32, to: i32, delta: i32) void {
        var i = from;
        if (self.buf_mode == 0) {
            while (i < to) { self.lines[i].depth += delta; i += 1; }
        } else if (self.buf_mode == 1) {
            while (i < to) { self.field_buf[i].depth += delta; i += 1; }
        } else {
            while (i < to) { self.method_buf[i].depth += delta; i += 1; }
        }
    }
    fn flush(self: *mut Self) void {
        var i: i32 = 0;
        while (i < @intCast(i32, self.lines.len)) {
            io.print("{} {}\n", self.lines[i].depth, self.lines[i].text);
            i += 1;
        }
    }

    // ---- 程序 ----

    fn parse_program(self: *mut Self) !void {
        while (!self.at("Eof")) {
            try self.parse_decl(0);
        }
    }

    fn parse_decl(self: *mut Self, d: i32) !void {
        var is_pub: i32 = 0;
        if (self.at("KwPub")) { self.advance(); is_pub = 1; }
        if (self.at("KwExport")) { self.advance(); }
        var has_module: i32 = 0;
        while (self.at("LBracket")) {
            var code = try self.parse_trait();
            if (code == 2) { has_module = 1; }
        }
        var ks = self.cur_start();
        var ke = self.cur_end();
        if (self.at("KwGlobal")) {
            self.advance();
            try self.parse_global(d, ks, ke, is_pub);
            return;
        }
        if (self.at("KwConst")) {
            self.advance();
            try self.parse_const(d, ks, ke, is_pub);
            return;
        }
        if (self.at("KwNamespace")) {
            self.advance();
            try self.parse_namespace(d, ks, is_pub, has_module);
            return;
        }
        if (self.at("KwUsing")) {
            self.advance();
            try self.parse_using(d, ks);
            return;
        }
        if (self.at("KwImport")) {
            self.advance();
            try self.parse_import(d, ks);
            return;
        }
        if (self.at("KwEnum")) {
            self.advance();
            try self.parse_enum(d, ks, is_pub);
            return;
        }
        if (self.at("KwUnion")) {
            self.advance();
            try self.parse_union(d, ks, is_pub);
            return;
        }
        if (self.at("KwInterface")) {
            self.advance();
            try self.parse_interface(d, ks, is_pub);
            return;
        }
        return error.UnknownDecl;
    }

    fn parse_trait(self: *mut Self) !i32 {
        try self.expect("LBracket");
        var name = try self.expect_ident();
        var code: i32 = 0;
        if (seq(name, "continuous")) { code = 0; }
        else if (seq(name, "pad")) { code = 1; }
        else if (seq(name, "module")) { code = 2; }
        else if (seq(name, "align")) {
            try self.expect("LParen");
            self.silent = 1;
            try self.parse_type(0);
            self.silent = 0;
            try self.expect("RParen");
            code = 3;
        }
        else if (seq(name, "test")) {
            if (self.at("LParen")) {
                self.advance();
                if (self.toks[self.pos].kind == "Str") { self.advance(); }
                try self.expect("RParen");
            }
            code = 4;
        }
        else { return error.UnknownTrait; }
        try self.expect("RBracket");
        return code;
    }

    fn parse_global(self: *mut Self, d: i32, ks: i32, ke: i32, is_pub: i32) !void {
        var name = try self.expect_ident();
        if (self.at("Colon")) {
            self.advance();
            try self.parse_type(d + 1);
        }
        if (self.at("Eq")) {
            self.advance();
            try self.parse_expr(d + 1);
        }
        try self.expect("Semi");
        var t = cat_b(s_of("Global "), name);
        t = cat_b(t, " pub=");
        t = cat_i(t, is_pub);
        self.emit_sp(d, t, ks, ke);
    }

    fn parse_const(self: *mut Self, d: i32, ks: i32, ke: i32, is_pub: i32) !void {
        var name = try self.expect_ident();
        if (self.at("Colon")) {
            self.advance();
            try self.parse_type(d + 1);
        }
        try self.expect("Eq");
        // error{ ... } 错误集别名
        if (self.is_ident("error") and self.peek_kind(1) == "LBrace") {
            self.advance();
            self.advance();
            var names = Vec(String).init(alloc);
            while (!self.at("RBrace") and !self.at("Eof")) {
                var e = try self.expect_ident();
                names.append(e);
                if (self.at("Comma")) { self.advance(); }
            }
            try self.expect("RBrace");
            try self.expect("Semi");
            var end = self.cur_end();
            var tn = cat_b(s_of("Named error_set:"), join_c(names));
            tn = cat_b(tn, " 0");
            self.emit_line(d + 1, tn);
            self.emit_sp(d + 1, s_of("Void"), ks, end);
            var t = cat_b(s_of("Const "), name);
            t = cat_b(t, " pub=");
            t = cat_i(t, is_pub);
            self.emit_sp(d, t, ks, end);
            return;
        }
        // A || B 错误集联合别名
        if (self.toks[self.pos].kind == "Ident" and self.peek_kind(1) == "PipePipe") {
            var parts = Vec(String).init(alloc);
            parts.append(self.toks[self.pos].text);
            while (self.toks[self.pos].kind == "Ident") {
                if (self.peek_kind(1) != "PipePipe") { break; }
                self.advance();
                self.advance();
                if (self.toks[self.pos].kind == "Ident") {
                    parts.append(self.toks[self.pos].text);
                }
            }
            if (self.toks[self.pos].kind == "Ident") {
                parts.append(self.toks[self.pos].text);
                self.advance();
            }
            try self.expect("Semi");
            var end = self.cur_end();
            var tn = cat_b(s_of("Named error_set:"), join_c(parts));
            tn = cat_b(tn, " 0");
            self.emit_line(d + 1, tn);
            self.emit_sp(d + 1, s_of("Void"), ks, end);
            var t = cat_b(s_of("Const "), name);
            t = cat_b(t, " pub=");
            t = cat_i(t, is_pub);
            self.emit_sp(d, t, ks, end);
            return;
        }
        try self.parse_expr(d + 1);
        try self.expect("Semi");
        var t = cat_b(s_of("Const "), name);
        t = cat_b(t, " pub=");
        t = cat_i(t, is_pub);
        self.emit_sp(d, t, ks, ke);
    }

    fn parse_namespace(self: *mut Self, d: i32, ks: i32, is_pub: i32, is_module: i32) !void {
        var name = try self.expect_ident();
        try self.expect("LBrace");
        while (!self.at("RBrace") and !self.at("Eof")) {
            try self.parse_decl(d + 1);
        }
        try self.expect("RBrace");
        var end = self.cur_end();
        var t = cat_b(s_of("Namespace "), name);
        t = cat_b(t, " pub=");
        t = cat_i(t, is_pub);
        t = cat_b(t, " module=");
        t = cat_i(t, is_module);
        self.emit_sp(d, t, ks, end);
    }

    fn parse_using(self: *mut Self, d: i32, ks: i32) !void {
        var path = try self.parse_path();
        var has_alias = false;
        var alias = String.from_slice("", alloc);
        if (self.is_ident("as")) {
            self.advance();
            var a = try self.expect_ident();
            alias = a;
            has_alias = true;
        }
        try self.expect("Semi");
        var end = self.cur_end();
        var t = cat_b(s_of("Using path="), join_d(path));
        t = cat_b(t, " alias=");
        if (has_alias) { t = cat_s(t, qq(alias)); } else { t = cat_b(t, "_"); }
        self.emit_sp(d, t, ks, end);
    }

    fn parse_import(self: *mut Self, d: i32, ks: i32) !void {
        var path = try self.parse_import_path();
        var select: i32 = 0;
        if (self.at("Dot") and self.peek_kind(1) == "LBrace") {
            self.advance();
            self.advance();
            while (true) {
                var sname = try self.expect_ident();
                var has_a = false;
                var alias = String.from_slice("", alloc);
                if (self.is_ident("as")) {
                    self.advance();
                    var a = try self.expect_ident();
                    alias = a;
                    has_a = true;
                }
                var st = cat_b(s_of("Sel "), sname);
                st = cat_b(st, " alias=");
                if (has_a) { st = cat_s(st, qq(alias)); } else { st = cat_b(st, "_"); }
                self.emit_line(d + 1, st);
                if (self.at("Comma")) { self.advance(); } else { break; }
            }
            try self.expect("RBrace");
            select = 1;
        }
        var has_alias = false;
        var alias = String.from_slice("", alloc);
        if (select == 0 and self.is_ident("as")) {
            self.advance();
            var a = try self.expect_ident();
            alias = a;
            has_alias = true;
        }
        try self.expect("Semi");
        var end = self.cur_end();
        var t = cat_b(s_of("Import path="), join_d(path));
        t = cat_b(t, " alias=");
        if (has_alias) { t = cat_s(t, qq(alias)); } else { t = cat_b(t, "_"); }
        t = cat_b(t, " select=");
        t = cat_i(t, select);
        self.emit_sp(d, t, ks, end);
    }

    fn parse_enum(self: *mut Self, d: i32, ks: i32, is_pub: i32) !void {
        var name = try self.expect_ident();
        try self.expect("LBrace");
        while (!self.at("RBrace") and !self.at("Eof")) {
            var vname = String.from_slice("", alloc);
            var vs: i32 = 0;
            var ve: i32 = 0;
            if (self.at("KwNull")) {
                vs = self.cur_start();
                ve = self.cur_end();
                self.advance();
                vname = s_of("null");
            } else {
                vname = try self.expect_ident();
                vs = self.last_span_s;
                ve = self.last_span_e;
            }
            if (self.at("Colon")) {
                self.advance();
                try self.parse_type(d + 2);
            }
            var t = cat_b(s_of("EnumVariant "), vname);
            self.emit_sp(d + 1, t, vs, ve);
            if (self.at("Comma")) { self.advance(); }
        }
        try self.expect("RBrace");
        var end = self.cur_end();
        var t = cat_b(s_of("Enum "), name);
        t = cat_b(t, " pub=");
        t = cat_i(t, is_pub);
        self.emit_sp(d, t, ks, end);
    }

    fn parse_union(self: *mut Self, d: i32, ks: i32, is_pub: i32) !void {
        var name = try self.expect_ident();
        try self.expect("LBrace");
        while (!self.at("RBrace") and !self.at("Eof")) {
            var mpub: i32 = 0;
            if (self.at("KwPub")) { self.advance(); mpub = 1; }
            if (self.at("KwMut")) { self.advance(); }
            var fname = try self.expect_ident();
            var fs = self.last_span_s;
            var fe = self.last_span_e;
            try self.expect("Colon");
            try self.parse_type(d + 2);
            var t = cat_b(s_of("FieldDecl "), fname);
            t = cat_b(t, " pub=");
            t = cat_i(t, mpub);
            self.emit_sp(d + 1, t, fs, fe);
            if (self.at("Comma")) { self.advance(); }
        }
        try self.expect("RBrace");
        var end = self.cur_end();
        var t = cat_b(s_of("Union "), name);
        t = cat_b(t, " pub=");
        t = cat_i(t, is_pub);
        self.emit_sp(d, t, ks, end);
    }

    fn parse_interface(self: *mut Self, d: i32, ks: i32, is_pub: i32) !void {
        var name = try self.expect_ident();
        if (self.at("Colon")) {
            self.advance();
            while (true) {
                try self.parse_type(d + 1);
                if (self.at("Comma")) { self.advance(); } else { break; }
            }
        }
        try self.expect("LBrace");
        while (!self.at("RBrace") and !self.at("Eof")) {
            if (self.at("KwFn")) {
                self.advance();
                try self.parse_interface_method(d + 1);
            } else {
                return error.Expected;
            }
        }
        try self.expect("RBrace");
        var end = self.cur_end();
        var t = cat_b(s_of("Interface "), name);
        t = cat_b(t, " pub=");
        t = cat_i(t, is_pub);
        self.emit_sp(d, t, ks, end);
    }

    fn parse_interface_method(self: *mut Self, d: i32) !void {
        var mname = try self.expect_ident();
        var ms = self.last_span_s;
        try self.expect("LParen");
        if (!self.at("RParen")) {
            while (true) {
                try self.parse_param(d + 1);
                if (self.at("Comma")) {
                    self.advance();
                    if (self.at("RParen")) { break; }
                } else {
                    break;
                }
            }
        }
        try self.expect("RParen");
        if (!self.at("Semi") and !self.at("RBrace")) {
            try self.parse_type(d + 1);
        }
        if (self.at("KwWhere")) {
            self.advance();
            while (true) {
                var tn = try self.expect_ident();
                try self.expect("Colon");
                try self.parse_type(d + 1);
                var wt = cat_b(s_of("Where "), tn);
                self.emit_line(d + 1, wt);
                if (self.at("Comma")) { self.advance(); } else { break; }
            }
        }
        if (self.at("Semi")) { self.advance(); }
        var mend = self.cur_end();
        self.emit_sp(d + 1, s_of("Block"), ms, mend);
        var mt = cat_b(s_of("Method "), mname);
        self.emit_sp(d, mt, ms, mend);
    }

    fn parse_param(self: *mut Self, d: i32) !void {
        var pname = try self.expect_ident();
        var ps = self.last_span_s;
        var pe = self.last_span_e;
        try self.expect("Colon");
        try self.parse_type(d + 1);
        if (self.at("Eq")) {
            self.advance();
            try self.parse_expr(d + 1);
        }
        var t = cat_b(s_of("Param "), pname);
        self.emit_sp(d, t, ps, pe);
    }

    fn parse_path(self: *mut Self) !Vec(String) {
        var path = Vec(String).init(alloc);
        var seg = try self.expect_ident();
        path.append(seg);
        while (self.at("Dot")) {
            self.advance();
            var s2 = try self.expect_ident();
            path.append(s2);
        }
        return path;
    }

    fn parse_import_path(self: *mut Self) !Vec(String) {
        var path = Vec(String).init(alloc);
        var seg = try self.expect_ident();
        path.append(seg);
        while (true) {
            if (self.at("Dot") and self.peek_kind(1) == "LBrace") { break; }
            if (self.at("Dot")) {
                self.advance();
                var s2 = try self.expect_ident();
                path.append(s2);
            } else {
                break;
            }
        }
        return path;
    }

    // ---- 类型 ----

    fn parse_type(self: *mut Self, d: i32) !void {
        if (self.at("KwO")) {
            self.advance();
            try self.parse_type(d + 1);
            self.emit_line(d, s_of("Owned"));
            return;
        }
        if (self.at("Star")) {
            self.advance();
            var m: i32 = 0;
            if (self.at("KwMut")) { self.advance(); m = 1; }
            try self.parse_type(d + 1);
            var t = cat_b(cat_i(s_of("Ptr mut="), m), "");
            self.emit_line(d, t);
            return;
        }
        if (self.at("Amp")) {
            self.advance();
            var m: i32 = 0;
            if (self.at("KwMut")) { self.advance(); m = 1; }
            if (self.at("LBracket")) {
                self.advance();
                try self.parse_type(d + 1);
                try self.expect("RBracket");
                var t = cat_b(cat_i(s_of("Slice mut="), m), "");
                self.emit_line(d, t);
                return;
            }
            try self.parse_type(d + 1);
            var t = cat_b(cat_i(s_of("Slice mut="), m), "");
            self.emit_line(d, t);
            return;
        }
        if (self.at("Question")) {
            self.advance();
            try self.parse_type(d + 1);
            self.emit_line(d, s_of("Optional"));
            return;
        }
        if (self.at("Bang")) {
            self.advance();
            try self.parse_type(d + 1);
            self.emit_line(d, s_of("ErrorUnion err=0"));
            return;
        }
        var base_start = self.cur_len();
        try self.parse_type_base(d + 1);
        if (self.at("Bang")) {
            self.advance();
            try self.parse_type(d + 1);
            self.emit_line(d, s_of("ErrorUnion err=1"));
            return;
        }
        self.fix_lines(base_start, self.cur_len(), -1);
    }

    fn parse_type_base(self: *mut Self, d: i32) !void {
        if (self.at("KwVoid")) {
            self.advance();
            self.emit_line(d, s_of("Named void 0"));
            return;
        }
        if (self.at("KwAnytype")) {
            self.advance();
            self.emit_line(d, s_of("Infer"));
            return;
        }
        if (self.at("KwType")) {
            self.advance();
            self.emit_line(d, s_of("Named type 0"));
            return;
        }
        if (self.at("LParen")) {
            self.advance();
            try self.parse_type(d + 1);
            var cnt: i32 = 1;
            while (self.at("Comma")) {
                self.advance();
                try self.parse_type(d + 1);
                cnt += 1;
            }
            try self.expect("RParen");
            var t = cat_b(cat_i(s_of("Tuple "), cnt), "");
            self.emit_line(d, t);
            return;
        }
        if (self.at("LBracket")) {
            self.advance();
            var n = try self.parse_int_lit();
            try self.expect("RBracket");
            try self.parse_type(d + 1);
            var t = cat_b(cat_i(s_of("Array "), n), "");
            self.emit_line(d, t);
            return;
        }
        var name = try self.expect_ident();
        while (self.at("Dot")) {
            self.advance();
            var part = try self.expect_name_or_keyword();
            name = cat_s(name, s_of("."));
            name = cat_s(name, part);
        }
        var argc: i32 = 0;
        if (self.at("LParen")) {
            self.advance();
            if (!self.at("RParen")) {
                while (true) {
                    if (self.toks[self.pos].kind == "Int") {
                        var n = try self.parse_int_lit();
                        var t = cat_b(cat_i(s_of("ComptimeInt "), n), "");
                        self.emit_line(d + 1, t);
                    } else {
                        try self.parse_type(d + 1);
                    }
                    argc += 1;
                    if (self.at("Comma")) { self.advance(); } else { break; }
                }
            }
            try self.expect("RParen");
            if (self.is_fnn(name) and self.at_type_start()) {
                try self.parse_type(d + 1);
                argc += 1;
            }
        }
        var t = cat_b(s_of("Named "), name);
        t = cat_b(t, " ");
        t = cat_i(t, argc);
        self.emit_line(d, t);
    }

    fn parse_int_lit(self: *mut Self) !i32 {
        if (self.toks[self.pos].kind != "Int") { return error.Expected; }
        var t = self.toks[self.pos];
        self.pos += 1;
        var s = t.text;
        var n: i32 = @intCast(i32, s.len);
        var v: i32 = 0;
        var i: i32 = 0;
        while (i < n) {
            var c = s[i];
            if (c >= '0' and c <= '9') {
                v = v * 10 + (@intCast(i32, c) - '0');
            }
            i += 1;
        }
        return v;
    }

    fn is_fnn(self: *mut Self, name: String) bool {
        var n: i32 = @intCast(i32, name.len);
        if (n < 3) { return false; }
        if (name[0] != 'F' or name[1] != 'n') { return false; }
        var i: i32 = 2;
        while (i < n) {
            if (!is_digit(name[i])) { return false; }
            i += 1;
        }
        return true;
    }

    // ---- 表达式（K2a：最小集合） ----

    fn parse_expr(self: *mut Self, d: i32) !void {
        try self.parse_primary(d);
    }

    fn parse_primary(self: *mut Self, d: i32) !void {
        var t = self.toks[self.pos];
        if (t.kind == "Int") {
            self.pos += 1;
            self.emit_sp(d, cat_s(s_of("Int "), t.text), t.start, t.end);
            return;
        }
        if (t.kind == "Float") {
            self.pos += 1;
            self.emit_sp(d, cat_s(s_of("Float "), t.text), t.start, t.end);
            return;
        }
        if (t.kind == "Str") {
            self.pos += 1;
            var tx = cat_b(s_of("Str raw=0 \""), dbg_escape_str(t.text));
            tx = cat_b(tx, "\"");
            self.emit_sp(d, tx, t.start, t.end);
            return;
        }
        if (t.kind == "Char") {
            self.pos += 1;
            self.emit_sp(d, cat_b(s_of("Char "), i2s(t.val)), t.start, t.end);
            return;
        }
        if (t.kind == "KwTrue") {
            self.pos += 1;
            self.emit_sp(d, s_of("Bool 1"), t.start, t.end);
            return;
        }
        if (t.kind == "KwFalse") {
            self.pos += 1;
            self.emit_sp(d, s_of("Bool 0"), t.start, t.end);
            return;
        }
        if (t.kind == "KwNull") {
            self.pos += 1;
            self.emit_sp(d, s_of("Null"), t.start, t.end);
            return;
        }
        if (t.kind == "Ident") {
            self.pos += 1;
            self.emit_sp(d, cat_s(s_of("Ident "), t.text), t.start, t.end);
            return;
        }
        return error.UnexpectedToken;
    }
}

// ---------- main ----------

fn main(args: o Vec(String)) !void {
    var path = args[0];
    if (args.len >= 2) { path = args[1]; }
    var src = try io.fs.read_file(path, alloc);
    var lx: o Lexer = alloc.init(Lexer{ src = src, n = @intCast(i32, src.len), pos = 0, out = Vec(Token).init(alloc) });
    lx.run();
    var p: o Parser = alloc.init(Parser{ toks = lx.out, pos = 0, lines = Vec(BufLine).init(alloc), field_buf = Vec(BufLine).init(alloc), method_buf = Vec(BufLine).init(alloc), buf_mode = 0, silent = 0, last_span_s = 0, last_span_e = 0 });
    try p.parse_program();
    p.flush();
}
