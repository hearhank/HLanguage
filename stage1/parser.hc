// ============================================================
// stage1/parser.hc 鈥?H 鐗?parser锛圞2锛欵7 鑷妇娓愯繘璺嚎 路 璇硶娈碉級
//
// 鍙屽疄鐜板鐓э細鏈枃浠朵笌 Rust 鍙傝€?parser锛坱ag1/hc/src/parser/锛宍hc parse` 杞偍锛?
// 杈撳嚭鍚屼竴鏍戞牸寮忥紙娣卞害缂╄繘 + NodeType|key=val锛夛紝閫愯 diff锛屽樊寮傚嵆 bug銆?
// Rust 鍙傝€冨疄鐜伴暱鏈熶繚鐣欙紙鑷妇澶辫触椋庨櫓瀵圭瓥锛夈€?
//
// 鐢ㄦ硶锛歨c run stage1/parser.hc <file.hc>
//
// 杈撳嚭鏍煎紡锛堜笌 Rust `hc parse` 涓€鑷达級锛?
//   Program
//     Fn|name=main
//       Param|name=x|ty=i32
//       ret: !void
//       Block
//         Return
//           IntLit|text=0
// ============================================================

import H.std.{io};

// ============================================================
// 杈呭姪鍑芥暟锛堝鐢?lexer.hc 閫昏緫锛?
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
// Token 绫诲瀷
// ============================================================

// ============================================================
// Token 绫诲瀷
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
    if (name == "spawn") return "KwSpawn";
    if (name == "extern") return "KwExtern";
    if (name == "void") return "KwVoid";
    if (name == "null") return "KwNull";
    if (name == "true") return "KwTrue";
    if (name == "false") return "KwFalse";
    return null;
}

// ============================================================
// 璇嶆硶鍒嗘瀽鍣紙Token 娴侊級
// ============================================================

class Token {
    kind: Vec<u8>,
    text: Vec<u8>,
    start: i32,
    end: i32,
    line: i32,
    col: i32,
}

class Lexer {
    src: &[u8],
    n: i32,
    mut pos: i32,
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
        self.pos += utf8_width(self.src[self.pos]);
    }

    fn append_char(self: *mut Self, content: Vec<u8>) void {
        var w = utf8_width(self.src[self.pos]);
        var k: i32 = 0;
        while (k < w) {
            content.append(self.src[self.pos + k]);
            k += 1;
        }
        self.bump();
    }

    fn push_token(self: *mut Self, kind: &[u8], text: Vec<u8>, start: i32) void {
        var tok = Token{
            kind = vec_from_slice(kind),
            text = text,
            start = start,
            end = self.pos,
            line = self.line,
            col = self.col,
        };
        self.tokens.append(tok);
    }

    fn push_simple(self: *mut Self, kind: &[u8], start: i32) void {
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
            var c = self.src[self.pos];
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

    fn lex_ident(self: *mut Self, start: i32) void {
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

    fn lex_number(self: *mut Self, start: i32) void {
        var buf = Vec<u8>.init(alloc);
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

    fn finish_number(self: *mut Self, start: i32, kind: &[u8], buf: Vec<u8>) void {
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
        var txt = vec_from_slice(buf);
        self.push_token(kind, txt, start);
    }

    fn lex_string(self: *mut Self, start: i32) void {
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
                self.append_char(content);
            }
            self.push_token("RawStr", content, start);
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
                var ec = self.src[self.pos];
                self.bump();
                if (ec == 'n') { content.append('\n'); }
                else if (ec == 'r') { content.append('\r'); }
                else if (ec == 't') { content.append('\t'); }
                else if (ec == '\\') { content.append('\\'); }
                else if (ec == '"') { content.append('"'); }
                else if (ec == '\'') { content.append('\''); }
                else if (ec == 'x') {
                    var hi: i32 = -1; var lo: i32 = -1;
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
                    var brace = self.src[self.pos]; self.bump();
                    if (brace == '{') {
                        var v: i64 = 0;
                        while (true) {
                            if (self.pos >= self.n) { break; }
                            var ch = self.src[self.pos]; self.bump();
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
                self.append_char(content);
            }
        }
        self.push_token("Str", content, start);
    }

    fn lex_char(self: *mut Self, start: i32) void {
        self.bump();
        var val: i32 = -1;
        if (self.pos >= self.n) { return; }
        if (self.src[self.pos] == '\\') {
            self.bump();
            if (self.pos >= self.n) { return; }
            var c = self.src[self.pos]; self.bump();
            if (c == 'n') { val = 0x0A; }
            else if (c == 'r') { val = 0x0D; }
            else if (c == 't') { val = 0x09; }
            else if (c == '\\') { val = 0x5C; }
            else if (c == '\'') { val = 0x27; }
            else if (c == 'x') {
                var hi: i32 = -1; var lo: i32 = -1;
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
        var close = self.src[self.pos]; self.bump();
        if (close == '\'') {
            var txt = Vec<u8>.init(alloc);
            txt.append(@intCast(u8, val));
            self.push_token("Char", txt, start);
        }
    }

    fn lex_punct(self: *mut Self, start: i32) void {
        var c = self.src[self.pos];
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
    var i: i32 = 0;
    while (i < @intCast(i32, s.len)) {
        v.append(s[i]);
        i += 1;
    }
    return v;
}

fn kind_eq(k: Vec<u8>, s: &[u8]) bool {
    if (k.len != @intCast(i32, s.len)) return false;
    var i: i32 = 0;
    while (i < @intCast(i32, k.len)) {
        if (k[@intCast(usize, i)] != s[i]) return false;
        i += 1;
    }
    return true;
}

// ============================================================
// AST 绫诲瀷瀹氫箟
// ============================================================

// 绠€鍖栫殑 AST锛氱敤 Vec<u8> 瀛樺偍鑺傜偣绫诲瀷鍚嶏紝灞炴€х敤瀛楃涓查敭鍊煎
// 简单的 AST：用 Vec<u8> 存储节点类型名，属性用 Vec<Prop> 字符串键值对
// 输出格式：NodeType|key=val|key=val\n  children (indented)

// 简单的 AST：用 Vec<u8> 存储节点类型名，属性用 Vec<u8> 字符串键值对
// 输出格式：NodeType|key=val|key=val\n  children (indented)

class AstNode {
    kind: Vec<u8>,
    // props: flat Vec<u8> with key=value pairs separated by null
    props: Vec<u8>,
    // children
    children: Vec<AstNode>,
}

fn make_node(kind: &[u8]) AstNode {
    var n = AstNode{
        kind = Vec<u8>.init(alloc),
        props = Vec<u8>.init(alloc),
        children = Vec<AstNode>.init(alloc),
    };
    n.kind = vec_from_slice(kind);
    return n;
}

fn node_add_prop(node: *mut AstNode, key: &[u8], val: &[u8]) void {
    // encode: |key=value
    node.props.append('|');
    var i: i32 = 0;
    while (i < @intCast(i32, key.len)) {
        node.props.append(key[i]);
        i += 1;
    }
    node.props.append('=');
    i = 0;
    while (i < @intCast(i32, val.len)) {
        node.props.append(val[i]);
        i += 1;
    }
}

fn node_add_child(node: *mut AstNode, child: AstNode) void {
    node.children.append(child);
}

fn quoted(s: &[u8]) Vec<u8> {
    var buf = Vec<u8>.init(alloc);
    buf.append('"');
    var i: i32 = 0;
    while (i < @intCast(i32, s.len)) {
        buf.append(s[i]);
        i += 1;
    }
    buf.append('"');
    return buf;
}

// ============================================================
// 瑙ｆ瀽鍣紙Parser锛?
// ============================================================

class Parser {
    tokens: Vec<Token>,
    mut pos: i32,
    n: i32,

    fn peek(self: *mut Self) Vec<u8> {
        var tok = self.tokens[@intCast(usize, self.pos)];
        return tok.kind;
    }

    fn peek_n(self: *mut Self, n: i32) Vec<u8> {
        var idx = self.pos + n;
        if (idx >= self.n) { idx = self.n - 1; }
        var tok = self.tokens[@intCast(usize, idx)];
        return tok.kind;
    }

    fn peek_text(self: *mut Self) Vec<u8> {
        var tok = self.tokens[@intCast(usize, self.pos)];
        return tok.text;
    }

    fn at(self: *mut Self, kind: &[u8]) bool {
        var k = self.peek();
        if (k.len != @intCast(i32, kind.len)) return false;
        var i: i32 = 0;
        while (i < @intCast(i32, k.len)) {
            if (k[@intCast(usize, i)] != kind[i]) return false;
            i += 1;
        }
        return true;
    }

    fn text_eq(self: *mut Self, s: &[u8]) bool {
        var t = self.peek_text();
        if (t.len != @intCast(i32, s.len)) return false;
        var i: i32 = 0;
        while (i < @intCast(i32, t.len)) {
            if (t[@intCast(usize, i)] != s[i]) return false;
            i += 1;
        }
        return true;
    }

    fn at_any(self: *mut Self, kinds: &[&[u8]]) bool {
        var k = self.peek();
        var i: i32 = 0;
        while (i < @intCast(i32, kinds.len)) {
            if (self.at(kinds[i])) return true;
            i += 1;
        }
        return false;
    }

    fn advance(self: *mut Self) Token {
        var t = self.tokens[@intCast(usize, self.pos)];
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

    fn expect_ident(self: *mut Self) Vec<u8> {
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

    fn expect_name_or_keyword(self: *mut Self) Vec<u8> {
        var k = self.peek();
        if (kind_eq(k, "Ident")) {
            var txt = self.peek_text();
            self.advance();
            return txt;
        }
        // 鍏抽敭瀛楀彲浣滅偣鍙峰瓧娈靛悕
        var txt = self.peek_text();
        if (kind_eq(k, "KwVar")) { self.advance(); return vec_from_slice("var"); }
        if (kind_eq(k, "KwConst")) { self.advance(); return vec_from_slice("const"); }
        if (kind_eq(k, "KwFn")) { self.advance(); return vec_from_slice("fn"); }
        if (kind_eq(k, "KwIf")) { self.advance(); return vec_from_slice("if"); }
        if (kind_eq(k, "KwElse")) { self.advance(); return vec_from_slice("else"); }
        if (kind_eq(k, "KwWhile")) { self.advance(); return vec_from_slice("while"); }
        if (kind_eq(k, "KwFor")) { self.advance(); return vec_from_slice("for"); }
        if (kind_eq(k, "KwBreak")) { self.advance(); return vec_from_slice("break"); }
        if (kind_eq(k, "KwContinue")) { self.advance(); return vec_from_slice("continue"); }
        if (kind_eq(k, "KwReturn")) { self.advance(); return vec_from_slice("return"); }
        if (kind_eq(k, "KwSwitch")) { self.advance(); return vec_from_slice("switch"); }
        if (kind_eq(k, "KwDefer")) { self.advance(); return vec_from_slice("defer"); }
        if (kind_eq(k, "KwErrdefer")) { self.advance(); return vec_from_slice("errdefer"); }
        if (kind_eq(k, "KwClass")) { self.advance(); return vec_from_slice("class"); }
        if (kind_eq(k, "KwEnum")) { self.advance(); return vec_from_slice("enum"); }
        if (kind_eq(k, "KwUnion")) { self.advance(); return vec_from_slice("union"); }
        if (kind_eq(k, "KwTree")) { self.advance(); return vec_from_slice("tree"); }
        if (kind_eq(k, "KwInterface")) { self.advance(); return vec_from_slice("interface"); }
        if (kind_eq(k, "KwWhere")) { self.advance(); return vec_from_slice("where"); }
        if (kind_eq(k, "KwNamespace")) { self.advance(); return vec_from_slice("namespace"); }
        if (kind_eq(k, "KwUsing")) { self.advance(); return vec_from_slice("using"); }
        if (kind_eq(k, "KwImport")) { self.advance(); return vec_from_slice("import"); }
        if (kind_eq(k, "KwPub")) { self.advance(); return vec_from_slice("pub"); }
        if (kind_eq(k, "KwExport")) { self.advance(); return vec_from_slice("export"); }
        if (kind_eq(k, "KwOwned")) { self.advance(); return vec_from_slice("owned"); }
        if (kind_eq(k, "KwMove")) { self.advance(); return vec_from_slice("move"); }
        if (kind_eq(k, "KwMut")) { self.advance(); return vec_from_slice("mut"); }
        if (kind_eq(k, "KwAnd")) { self.advance(); return vec_from_slice("and"); }
        if (kind_eq(k, "KwOr")) { self.advance(); return vec_from_slice("or"); }
        if (kind_eq(k, "KwTry")) { self.advance(); return vec_from_slice("try"); }
        if (kind_eq(k, "KwCatch")) { self.advance(); return vec_from_slice("catch"); }
        if (kind_eq(k, "KwOrelse")) { self.advance(); return vec_from_slice("orelse"); }
        if (kind_eq(k, "KwScript")) { self.advance(); return vec_from_slice("script"); }
        if (kind_eq(k, "KwComptime")) { self.advance(); return vec_from_slice("comptime"); }
        if (kind_eq(k, "KwAnytype")) { self.advance(); return vec_from_slice("anytype"); }
        if (kind_eq(k, "KwType")) { self.advance(); return vec_from_slice("type"); }
        if (kind_eq(k, "KwAsync")) { self.advance(); return vec_from_slice("async"); }
        if (kind_eq(k, "KwAwait")) { self.advance(); return vec_from_slice("await"); }
        if (kind_eq(k, "KwSpawn")) { self.advance(); return vec_from_slice("spawn"); }
        if (kind_eq(k, "KwExtern")) { self.advance(); return vec_from_slice("extern"); }
        if (kind_eq(k, "KwVoid")) { self.advance(); return vec_from_slice("void"); }
        if (kind_eq(k, "KwNull")) { self.advance(); return vec_from_slice("null"); }
        if (kind_eq(k, "KwTrue")) { self.advance(); return vec_from_slice("true"); }
        if (kind_eq(k, "KwFalse")) { self.advance(); return vec_from_slice("false"); }
        if (kind_eq(k, "KwGlobal")) { self.advance(); return vec_from_slice("global"); }
        return txt;
    }

    // ---------- 绋嬪簭鍏ュ彛 ----------

    fn parse_program(self: *mut Self) AstNode {
        var prog = make_node("Program");
        while (!self.at("Eof")) {
            var decl = self.parse_decl();
            node_add_child(&prog, decl);
        }
        return prog;
    }

    // ---------- 澹版槑瑙ｆ瀽 ----------

    fn parse_decl(self: *mut Self) AstNode {
        // pub
        var is_pub = false;
        if (self.at("KwPub")) { is_pub = true; self.advance(); }
        // export
        var is_export = false;
        if (self.at("KwExport")) { is_export = true; self.advance(); }
        // [pad] [align(T)] [Test]
        var traits = Vec<&[u8]>.init(alloc);
        while (self.at("LBracket")) {
            var t = self.parse_trait();
            if (t) |tr| { traits.append(tr); }
        }

        var k = self.peek();
        if (kind_eq(k, "KwGlobal")) {
            self.advance();
            return self.parse_global(is_pub);
        }
        if (kind_eq(k, "KwConst")) {
            self.advance();
            return self.parse_const(is_pub);
        }
        if (kind_eq(k, "KwAsync")) {
            self.advance();
            self.expect("KwFn");
            return self.finish_fn_decl(traits, is_pub, true, is_export);
        }
        if (kind_eq(k, "KwExtern")) {
            self.advance();
            return self.parse_extern_fn(is_pub);
        }
        if (kind_eq(k, "KwFn")) {
            self.advance();
            return self.finish_fn_decl(traits, is_pub, false, is_export);
        }
        if (kind_eq(k, "KwClass") or kind_eq(k, "KwTree")) {
            self.advance();
            return self.parse_class(is_pub);
        }
        if (kind_eq(k, "KwEnum")) {
            self.advance();
            return self.parse_enum(is_pub);
        }
        if (kind_eq(k, "KwUnion")) {
            self.advance();
            return self.parse_union(is_pub);
        }
        if (kind_eq(k, "KwInterface")) {
            self.advance();
            return self.parse_interface(is_pub);
        }
        if (kind_eq(k, "KwNamespace")) {
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
        if (kind_eq(k, "KwUsing")) {
            self.advance();
            var path = self.parse_path();
            var alias: ?Vec<u8> = null;
            if (self.at("Ident") and kind_eq(self.peek_text(), "as")) {
                self.advance();
                alias = self.expect_ident();
            }
            self.expect("Semi");
            var u = make_node("Using");
            node_add_prop(&u, "path", path);
            if (alias) |a| { node_add_prop(&u, "alias", a); }
            return u;
        }
        if (kind_eq(k, "KwImport")) {
            self.advance();
            var path = self.parse_import_path();
            var select: ?Vec<AstNode> = null;
            if (self.at("Dot") and kind_eq(self.peek_n(1), "LBrace")) {
                self.advance(); // .
                self.advance(); // {
                var syms = Vec<AstNode>.init(alloc);
                while (true) {
                    var name = self.expect_ident();
                    var alias: ?Vec<u8> = null;
                    if (self.at("Ident") and kind_eq(self.peek_text(), "as")) {
                        self.advance();
                        alias = self.expect_ident();
                    }
                    var s = make_node("ImportSelect");
                    node_add_prop(&s, "name", name);
                    if (alias) |a| { node_add_prop(&s, "alias", a); }
                    syms.append(s);
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
                self.expect("RBrace");
                select = syms;
            }
            var alias: ?Vec<u8> = null;
            if (select == null and self.at("Ident") and kind_eq(self.peek_text(), "as")) {
                self.advance();
                alias = self.expect_ident();
            }
            self.expect("Semi");
            var imp = make_node("Import");
            node_add_prop(&imp, "path", path);
            if (alias) |a| { node_add_prop(&imp, "alias", a); }
            return imp;
        }
        if (kind_eq(k, "KwScript")) {
            self.advance();
            self.parse_block();
            var sc = make_node("Script");
            return sc;
        }
        if (kind_eq(k, "KwComptime")) {
            self.advance();
            self.parse_block();
            var cp = make_node("Comptime");
            return cp;
        }
        // 鏈煡澹版槑 鈫?绌鸿妭鐐?骞朵笖鎺ㄨ繘褰撳墠 token 闃叉鏃犻檺寰幆
        self.advance();
        return make_node("UnknownDecl");
    }

    fn parse_trait(self: *mut Self) ?&[u8] {
        self.expect("LBracket");
        var name = self.expect_ident();
        if (kind_eq(name, "continuous")) { self.expect("RBracket"); return "continuous"; }
        if (kind_eq(name, "pad")) { self.expect("RBracket"); return "pad"; }
        if (kind_eq(name, "module")) { self.expect("RBracket"); return "module"; }
        if (kind_eq(name, "test")) {
            if (self.at("LParen")) {
                self.advance();
                if (self.at("Str")) { self.advance(); }
                self.expect("RParen");
            }
            self.expect("RBracket");
            return "test";
        }
        if (kind_eq(name, "align")) {
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
        if (self.at("Ident") and kind_eq(self.peek_text(), "error") and kind_eq(self.peek_n(1), "LBrace")) {
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
        // 妫€鏌?test 鐗规€?
        var i: i32 = 0;
        while (i < @intCast(i32, traits.len)) {
            if (traits[@intCast(usize, i)] == "test") {
                node_add_prop(&f, "test", "true");
            }
            i += 1;
        }
        // 娉涘瀷鍙傛暟 <T>
        if (self.at("Lt")) {
            self.advance();
            while (!self.at("Gt") and !self.at("Eof")) {
                self.expect_ident();
                if (self.at("Comma")) { self.advance(); }
            }
            self.expect("Gt");
        }
        // 鍙傛暟 (params)
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
        // 杩斿洖绫诲瀷
        if (self.at("Bang")) {
            self.advance();
            if (self.at("Ident") or self.at("KwVoid")) {
                var ret_ty = self.peek_text();
                self.advance();
                var r = make_node("ret:");
                var k: i32 = 0;
                while (k < @intCast(i32, ret_ty.len)) {
                    r.props.append(ret_ty[@intCast(usize, k)]);
                    k += 1;
                }
                node_add_child(&f, r);
            } else {
                self.parse_type();
            }
        } else if (self.at("KwVoid") or self.at("Ident")) {
            var ret_ty = self.peek_text();
            // 关键字（如 void）的 text 为空，直接用关键字名
            if (ret_ty.len == 0) {
                if (self.at("KwVoid")) { ret_ty = vec_from_slice("void"); }
            }
            self.advance();
            var r = make_node("ret:");
            var k: i32 = 0;
            while (k < @intCast(i32, ret_ty.len)) {
                r.props.append(ret_ty[@intCast(usize, k)]);
                k += 1;
            }
            node_add_child(&f, r);
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
                var k: i32 = 0;
                while (k < @intCast(i32, ret_ty.len)) {
                    r.props.append(ret_ty[@intCast(usize, k)]);
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
            var k: i32 = 0;
            while (k < @intCast(i32, ret_ty.len)) {
                r.props.append(ret_ty[@intCast(usize, k)]);
                k += 1;
            }
            node_add_child(&f, r);
        }
        self.expect("Semi");
        return f;
    }

    fn parse_param(self: *mut Self) AstNode {
        var name = self.expect_ident();
        self.expect("Colon");
        var p = make_node("Param");
        node_add_prop(&p, "name", name);
        if (self.at("Ident") or self.at("KwVoid")) {
            var ty = self.peek_text();
            self.advance();
            if (ty.len > 0) {
                node_add_prop(&p, "ty", quoted(ty[0..ty.len]));
            } else {
                node_add_prop(&p, "ty", quoted("void"));
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
        // 鎺ュ彛
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
        // 瀛楁鍜屾柟娉?
        while (!self.at("RBrace") and !self.at("Eof")) {
            if (kind_eq(self.peek(), "LBracket") or kind_eq(self.peek(), "KwPub") or kind_eq(self.peek(), "KwFn")) {
                // 鏂规硶
                self.parse_method(cls);
            } else {
                // 瀛楁
                self.parse_field(cls);
            }
        }
        self.expect("RBrace");
        return cls;
    }

    fn parse_field(self: *mut Self, cls: AstNode) void {
        var is_fpub = false;
        if (self.at("KwPub")) { is_fpub = true; self.advance(); }
        var name = self.expect_ident();
        self.expect("Colon");
        self.parse_type();
        self.expect("Semi");
    }

    fn parse_method(self: *mut Self, cls: AstNode) void {
        // traits
        while (self.at("LBracket")) { self.parse_trait(); }
        var is_pub = false;
        if (self.at("KwPub")) { is_pub = true; self.advance(); }
        self.expect("KwFn");
        var name = self.expect_ident();
        // 娉涘瀷 <T>
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
        var body = self.parse_block();
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
        var i: i32 = 0;
        while (i < @intCast(i32, first.len)) {
            parts.append(first[@intCast(usize, i)]);
            i += 1;
        }
        while (self.at("Dot")) {
            self.advance();
            parts.append('.');
            var seg = self.expect_name_or_keyword();
            var j: i32 = 0;
            while (j < @intCast(i32, seg.len)) {
                parts.append(seg[@intCast(usize, j)]);
                j += 1;
            }
        }
        return parts;
    }

    fn parse_import_path(self: *mut Self) Vec<u8> {
        return self.parse_path();
    }

    // ============================================================
    // 绫诲瀷瑙ｆ瀽
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
        // &[T] / &mut [T] 鎴?&T
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
        // !T锛坅nyerror锛?
        if (self.at("Bang")) {
            self.advance();
            self.parse_type();
            return;
        }
        // 鍩虹绫诲瀷
        self.parse_type_base();
        // E!T锛堝懡鍚嶉敊璇泦锛?
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
            // [N]T 瀹氶暱鏁扮粍
            self.advance();
            self.parse_expr();
            self.expect("RBracket");
            self.parse_type();
        } else if (self.at("LParen")) {
            // 鍏冪粍
            self.advance();
            while (!self.at("RParen") and !self.at("Eof")) {
                self.parse_type();
                if (self.at("Comma")) { self.advance(); }
                else { break; }
            }
            self.expect("RParen");
        } else if (self.at("KwClass")) {
            // struct { ... } 绫诲瀷瀛楅潰閲?
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
            // 鍏抽敭瀛椾綔绫诲瀷鍚嶏紙濡?void, type 绛夛級
            self.advance();
        }
    }

    // ============================================================
    // 璇彞瑙ｆ瀽
    // ============================================================

    fn parse_block(self: *mut Self) AstNode {
        var b = make_node("Block");
        self.expect("LBrace");
        while (!self.at("RBrace") and !self.at("Eof")) {
            var stmt = self.parse_stmt();
            node_add_child(&b, stmt);
        }
        self.expect("RBrace");
        return b;
    }

    fn parse_stmt(self: *mut Self) AstNode {
        // 寰幆鏍囩
        if (self.at("Colon")) {
            self.advance();
            if (self.at("Ident")) { self.advance(); }
            if (self.at("KwWhile") or self.at("KwFor")) {
                // 鏍囩鍚庤窡 while/for
            }
        }
        var k = self.peek();
        if (kind_eq(k, "LBrace")) {
            return self.parse_block();
        }
        if (kind_eq(k, "Semi")) {
            self.advance();
            return make_node("Empty");
        }
        if (kind_eq(k, "KwVar")) {
            self.advance();
            return self.parse_var_decl();
        }
        if (kind_eq(k, "KwConst")) {
            self.advance();
            var name = self.expect_ident();
            self.expect("Eq");
            self.parse_expr();
            self.expect("Semi");
            var c = make_node("ConstDecl");
            node_add_prop(&c, "name", name);
            return c;
        }
        if (kind_eq(k, "KwIf")) {
            return self.parse_if_stmt();
        }
        if (kind_eq(k, "KwWhile")) {
            return self.parse_while_stmt();
        }
        if (kind_eq(k, "KwFor")) {
            return self.parse_for_stmt();
        }
        if (kind_eq(k, "KwSwitch")) {
            return self.parse_switch_stmt();
        }
        if (kind_eq(k, "KwReturn")) {
            self.advance();
            var r = make_node("Return");
            if (!self.at("Semi")) {
                var val = self.parse_expr();
                node_add_child(&r, val);
            }
            self.expect("Semi");
            return r;
        }
        if (kind_eq(k, "KwBreak")) {
            self.advance();
            var b = make_node("Break");
            self.expect("Semi");
            return b;
        }
        if (kind_eq(k, "KwContinue")) {
            self.advance();
            var c = make_node("Continue");
            self.expect("Semi");
            return c;
        }
        if (kind_eq(k, "KwDefer")) {
            self.advance();
            self.parse_expr();
            self.expect("Semi");
            return make_node("Defer");
        }
        if (kind_eq(k, "KwErrdefer")) {
            self.advance();
            self.parse_expr();
            self.expect("Semi");
            return make_node("Errdefer");
        }
        // 榛樿锛氳〃杈惧紡璇彞
        var e = self.parse_expr();
        self.expect("Semi");
        var es = make_node("ExprStmt");
        node_add_child(&es, e);
        return es;
    }

    fn parse_var_decl(self: *mut Self) AstNode {
        var is_mut = false;
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
                    node_add_prop(&v, "ty", quoted(ty[0..ty.len]));
                } else {
                    node_add_prop(&v, "ty", quoted("void"));
                }
            } else {
                self.parse_type();
            }
        }
        if (self.at("Eq")) {
            self.advance();
            self.parse_expr();
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
        // 鎹曡幏 |v|
        if (self.at("Pipe")) {
            self.advance();
            var cap = self.expect_ident();
            self.expect("Pipe");
        }
        // 閿欒鎹曡幏 |err|
        if (self.at("Pipe")) {
            self.advance();
            var err = self.expect_ident();
            self.expect("Pipe");
        }
        self.expect("RParen");
        var then_b = self.parse_block();
        node_add_child(&ifn, then_b);
        if (self.at("KwElse")) {
            self.advance();
            if (self.at("KwIf")) {
                var else_if = self.parse_if_stmt();
                node_add_child(&ifn, else_if);
            } else {
                var else_b = self.parse_block();
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
        if (self.at("Pipe")) {
            self.advance();
            var cap = self.expect_ident();
            self.expect("Pipe");
        }
        self.expect("RParen");
        // step 瀛愬彞
        if (self.at("Colon") and kind_eq(self.peek_n(1), "LParen")) {
            self.advance();
            self.expect("LParen");
            self.parse_expr();
            self.expect("RParen");
        }
        var body = self.parse_block();
        node_add_child(&wn, body);
        return wn;
    }

    fn parse_for_stmt(self: *mut Self) AstNode {
        self.advance();
        var for_node = make_node("For");
        self.expect("LParen");
        if (self.at("KwMut")) { self.advance(); }
        var cap = self.expect_ident();
        self.expect("Pipe");
        var iter = self.parse_expr();
        node_add_child(&for_node, iter);
        self.expect("RParen");
        var body = self.parse_block();
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
        // 妯″紡鍒楄〃
        while (!self.at("FatArrow") and !self.at("RBrace") and !self.at("Eof")) {
            var pat = self.parse_switch_pattern();
            node_add_child(&arm, pat);
            if (self.at("Comma")) { self.advance(); break; }
        }
        self.expect("FatArrow");
        // 瀹堝崼
        if (self.at("KwIf")) {
            self.advance();
            self.parse_expr();
        }
        // 浣擄紙鍧楁垨琛ㄨ揪寮忥級
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
    // 琛ㄨ揪寮忚В鏋愶紙閫掑綊涓嬮檷 + 浼樺厛绾ц〃锛?
    // ============================================================

    fn parse_expr(self: *mut Self) AstNode {
        return self.parse_or();
    }

    fn parse_or(self: *mut Self) AstNode {
        var l = self.parse_and();
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
        var l = self.parse_range();
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
        var l = self.parse_comparison();
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
        var l = self.parse_bitor();
        var cmp_op = self.peek();
        if (kind_eq(cmp_op, "EqEq") or kind_eq(cmp_op, "Ne") or kind_eq(cmp_op, "Lt") or kind_eq(cmp_op, "Le") or kind_eq(cmp_op, "Gt") or kind_eq(cmp_op, "Ge")) {
            self.advance();
            var r = self.parse_bitor();
            var b = make_node("Binary");
            if (kind_eq(cmp_op, "EqEq")) { node_add_prop(&b, "op", "Eq"); }
            else { node_add_prop(&b, "op", cmp_op); }
            node_add_child(&b, l);
            node_add_child(&b, r);
            l = b;
        }
        return l;
    }

    fn parse_bitor(self: *mut Self) AstNode {
        var l = self.parse_bitxor();
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
        var l = self.parse_bitand();
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
        var l = self.parse_shift();
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
        var l = self.parse_addsub();
        while (true) {
            var opname = self.peek();
            if (kind_eq(opname, "Shl") or kind_eq(opname, "Shr")) {
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
        var l = self.parse_muldiv();
        while (true) {
            var opname = self.peek();
            if (kind_eq(opname, "Plus") or kind_eq(opname, "Minus")) {
                self.advance();
                var r = self.parse_muldiv();
                var b = make_node("Binary");
                if (kind_eq(opname, "Plus")) { node_add_prop(&b, "op", "Add"); }
                else { node_add_prop(&b, "op", "Sub"); }
                node_add_child(&b, l);
                node_add_child(&b, r);
                l = b;
            } else { break; }
        }
        return l;
    }

    fn parse_muldiv(self: *mut Self) AstNode {
        var l = self.parse_unary();
        while (true) {
            var opname = self.peek();
            if (kind_eq(opname, "Star") or kind_eq(opname, "Slash") or kind_eq(opname, "Percent") or kind_eq(opname, "PercentPercent")) {
                self.advance();
                var r = self.parse_unary();
                var b = make_node("Binary");
                if (kind_eq(opname, "Star")) { node_add_prop(&b, "op", "Mul"); }
                else if (kind_eq(opname, "Slash")) { node_add_prop(&b, "op", "Div"); }
                else if (kind_eq(opname, "Percent")) { node_add_prop(&b, "op", "Mod"); }
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
        if (kind_eq(k, "Minus")) {
            self.advance();
            var e = self.parse_unary();
            var u = make_node("Unary");
            node_add_prop(&u, "op", "Neg");
            node_add_child(&u, e);
            return u;
        }
        if (kind_eq(k, "Bang")) {
            self.advance();
            var e = self.parse_unary();
            var u = make_node("Unary");
            node_add_prop(&u, "op", "Not");
            node_add_child(&u, e);
            return u;
        }
        if (kind_eq(k, "Tilde")) {
            self.advance();
            var e = self.parse_unary();
            var u = make_node("Unary");
            node_add_prop(&u, "op", "BitNot");
            node_add_child(&u, e);
            return u;
        }
        if (kind_eq(k, "Amp")) {
            self.advance();
            var is_mut = false;
            if (self.at("KwMut")) { is_mut = true; self.advance(); }
            var e = self.parse_unary();
            var a = make_node("AddrOf");
            if (is_mut) { node_add_prop(&a, "mut", "true"); }
            node_add_child(&a, e);
            return a;
        }
        if (kind_eq(k, "KwTry")) {
            self.advance();
            var e = self.parse_unary();
            var t = make_node("Try");
            node_add_child(&t, e);
            return t;
        }
        if (kind_eq(k, "KwAwait")) {
            self.advance();
            var e = self.parse_unary();
            var a = make_node("Await");
            node_add_child(&a, e);
            return a;
        }
        if (kind_eq(k, "KwSpawn")) {
            self.advance();
            var args = self.parse_call_args();
            var c = make_node("Call");
            var callee = make_node("Ident");
            node_add_prop(&callee, "name", "spawn");
            node_add_child(&c, callee);
            var i: i32 = 0;
            while (i < @intCast(i32, args.len)) {
                node_add_child(&c, args[@intCast(usize, i)]);
                i += 1;
            }
            return c;
        }
        if (kind_eq(k, "KwMove")) {
            self.advance();
            // 闂寘
            if (self.at("Pipe") or (self.at("KwMut") and kind_eq(self.peek_n(1), "Pipe"))) {
                return self.parse_closure();
            }
            var e = self.parse_unary();
            var m = make_node("Move");
            node_add_child(&m, e);
            return m;
        }
        return self.parse_postfix();
    }

    fn parse_closure(self: *mut Self) AstNode {
        var c = make_node("Closure");
        var is_mut = false;
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
        // 浣撻儴
        if (self.at("LBrace")) {
            var body = self.parse_block();
            node_add_child(&c, body);
        } else {
            var e = self.parse_expr();
            var es = make_node("ExprStmt");
            node_add_child(&es, e);
            node_add_child(&c, es);
        }
        return c;
    }

    fn parse_postfix(self: *mut Self) AstNode {
        var e = self.parse_primary();
        while (true) {
            var kk = self.peek();
            if (kind_eq(kk, "Dot")) {
                self.advance();
                if (self.at("Question")) {
                    // .? 閾惧紡瑙ｅ寘
                    self.advance();
                    var u = make_node("Unwrap");
                    node_add_child(&u, e);
                    e = u;
                } else {
                    var field = self.expect_name_or_keyword();
                    if (self.at("LParen")) {
                        // 鏂规硶璋冪敤
                        var args = self.parse_call_args();
                        var call = make_node("Call");
                        var fe = make_node("Field");
                        node_add_prop(&fe, "field", field);
                        node_add_child(&fe, e);
                        node_add_child(&call, fe);
                        var i: i32 = 0;
                        while (i < @intCast(i32, args.len)) {
                            node_add_child(&call, args[@intCast(usize, i)]);
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
            } else if (kind_eq(kk, "LBracket")) {
                self.advance();
                var idx = self.parse_expr();
                self.expect("RBracket");
                var ie = make_node("Index");
                node_add_child(&ie, e);
                node_add_child(&ie, idx);
                e = ie;
            } else if (kind_eq(kk, "DotStar")) {
                self.advance();
                var d = make_node("Deref");
                node_add_child(&d, e);
                e = d;
            } else if (kind_eq(kk, "Question")) {
                // 鍚庣紑 ? 瑙ｅ寘
                self.advance();
                var u = make_node("Unwrap");
                node_add_child(&u, e);
                e = u;
            } else if (kind_eq(kk, "LParen")) {
                var args = self.parse_call_args();
                var call = make_node("Call");
                node_add_child(&call, e);
                var i: i32 = 0;
                while (i < @intCast(i32, args.len)) {
                    node_add_child(&call, args[@intCast(usize, i)]);
                    i += 1;
                }
                e = call;
                // 娉涘瀷瀛楅潰閲忥細Pair<i32>{...}
                if (self.at("LBrace")) {
                    // 绠€鍗曞鐞嗭細璺宠繃瀛楅潰閲忓瓧娈?
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
            } else if (kind_eq(kk, "KwOrelse")) {
                self.advance();
                var r = self.parse_expr();
                var orelse_node = make_node("Orelse");
                node_add_child(&orelse_node, e);
                node_add_child(&orelse_node, r);
                e = orelse_node;
            } else if (kind_eq(kk, "KwCatch")) {
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
        // 闂寘
        if (kind_eq(k, "Pipe") or (kind_eq(k, "KwMut") and kind_eq(self.peek_n(1), "Pipe"))) {
            return self.parse_closure();
        }
        // 鎺ㄦ柇鏋氫妇鍊?.variant
        if (kind_eq(k, "Dot")) {
            self.advance();
            var variant = self.expect_name_or_keyword();
            var d = make_node("Dot");
            node_add_prop(&d, "field", variant);
            return d;
        }
        // @鍐呭缓
        if (kind_eq(k, "AtBuiltin")) {
            var txt = self.peek_text();
            self.advance();
            var args = self.parse_call_args();
            var call = make_node("Call");
            var callee = make_node("Ident");
            node_add_prop(&callee, "name", txt[0..txt.len]);
            node_add_child(&call, callee);
            var i: i32 = 0;
            while (i < @intCast(i32, args.len)) {
                node_add_child(&call, args[@intCast(usize, i)]);
                i += 1;
            }
            return call;
        }
        // struct { ... } 绫诲瀷瀛楅潰閲?
        if (kind_eq(k, "KwClass")) {
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
        // 瀛楅潰閲?
        if (kind_eq(k, "Int")) {
            var txt = self.peek_text();
            self.advance();
            var n = make_node("IntLit");
            node_add_prop(&n, "text", txt[0..txt.len]);
            return n;
        }
        if (kind_eq(k, "Float")) {
            var txt = self.peek_text();
            self.advance();
            var n = make_node("FloatLit");
            node_add_prop(&n, "text", txt[0..txt.len]);
            return n;
        }
        if (kind_eq(k, "Str")) {
            var txt = self.peek_text();
            self.advance();
            var s = make_node("StrLit");
            node_add_prop(&s, "value", txt[0..txt.len]);
            return s;
        }
        if (kind_eq(k, "RawStr")) {
            var txt = self.peek_text();
            self.advance();
            var s = make_node("StrLit");
            node_add_prop(&s, "value", txt[0..txt.len]);
            node_add_prop(&s, "raw", "true");
            return s;
        }
        if (kind_eq(k, "Char")) {
            var txt = self.peek_text();
            self.advance();
            var c = make_node("CharLit");
            node_add_prop(&c, "value", txt[0..txt.len]);
            return c;
        }
        if (kind_eq(k, "KwTrue")) {
            self.advance();
            var b = make_node("BoolLit");
            node_add_prop(&b, "value", "true");
            return b;
        }
        if (kind_eq(k, "KwFalse")) {
            self.advance();
            var b = make_node("BoolLit");
            node_add_prop(&b, "value", "false");
            return b;
        }
        if (kind_eq(k, "KwNull")) {
            self.advance();
            return make_node("NullLit");
        }
        if (kind_eq(k, "KwVoid")) {
            self.advance();
            return make_node("VoidLit");
        }
        // 鏍囪瘑绗?
        if (kind_eq(k, "Ident")) {
            var name = self.peek_text();
            self.advance();
            // 鏋氫妇甯搁噺 error.NotFound
            if (kind_eq(name, "error") and self.at("Dot")) {
                self.advance();
                var err = self.expect_ident();
                var e = make_node("ErrorLit");
                node_add_prop(&e, "name", err);
                return e;
            }
            var id = make_node("Ident");
            node_add_prop(&id, "name", name[0..name.len]);
            // 瀹冨悗闈㈠彲鑳借窡娉涘瀷瀹炲弬锛歍ype(T1)
            if (self.at("LParen") and !kind_eq(self.peek_n(1), "RParen") and !kind_eq(self.peek_n(1), "Star") and !kind_eq(self.peek_n(1), "Slash") and !kind_eq(self.peek_n(1), "Plus") and !kind_eq(self.peek_n(1), "Minus")) {
                // 鍙兘鏄被鍨嬫瀯閫犳垨鍑芥暟璋冪敤锛岀敱 parse_postfix 澶勭悊
                // 浣嗚繖閲屼笉鍋氳秴鍓嶅垽鏂紝浜ょ粰璋冪敤鑰?
            }
            return id;
        }
        // 閿欒瀛楅潰閲?
        if (kind_eq(k, "KwScript")) {
            self.advance();
            self.parse_block();
            return make_node("Script");
        }
        // 鍧楄〃杈惧紡
        if (kind_eq(k, "LBrace")) {
            return self.parse_block();
        }
        // 鍏冪粍/鎷彿琛ㄨ揪寮?
        if (kind_eq(k, "LParen")) {
            self.advance();
            var e = self.parse_expr();
            if (self.at("Comma")) {
                // 鍏冪粍
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
        // 鏁扮粍瀛楅潰閲?
        if (kind_eq(k, "LBracket")) {
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
        // 閿欒锛氳烦杩?
        self.advance();
        return make_node("Unknown");
    }
}

// ============================================================
// AST 输出（dump 函数，与 Rust `hc parse` 格式一致）
// ============================================================

// 打印属性：props 是 |key=value 格式的扁平 Vec<u8>
fn dump_props(props: Vec<u8>) void {
    if (@intCast(i32, props.len) > 0) {
        var s = String.from_slice(props, alloc);
        io.print("{}", s);
    }
}

fn dump_ast(node: AstNode, depth: i32) void {
    var i = 0;
    while (i < depth * 2) {
        io.print(" ");
        i += 1;
    }
    var kind_str = String.from_slice(node.kind, alloc);
    // Handle ret: nodes specially
    if (kind_str == "ret:") {
        io.print("ret: ");
        if (node.props.len > 0) {
            var s = String.from_slice(node.props, alloc);
            io.print("\"{}\"", s);
        }
        io.print("\n");
        return;
    }
    io.print("{}", kind_str);
    dump_props(node.props);
    io.print("\n");
    var ci = 0;
    while (ci < @intCast(i32, node.children.len)) {
        dump_ast(node.children[ci], depth + 1);
        ci += 1;
    }
}

// ============================================================
// 鍏ュ彛
// ============================================================

fn main(args: Vec<String>) !void {
    var path = args[0];
    if (args.len >= 2) { path = args[1]; }
    var src = try io.fs.read_file(path, alloc);
    // 璇嶆硶鍒嗘瀽
    var lx: Lexer = alloc.init(Lexer{
        src = src, n = @intCast(i32, src.len),
        pos = 0, line = 1, col = 1,
        tokens = Vec<Token>.init(alloc)
    });
    lx.run();
    // 璇硶鍒嗘瀽
    var parser: Parser = alloc.init(Parser{
        tokens = lx.tokens, pos = 0,
        n = @intCast(i32, lx.tokens.len)
    });
    var ast = parser.parse_program();
    // 杈撳嚭 AST
    dump_ast(ast, 0);
}
