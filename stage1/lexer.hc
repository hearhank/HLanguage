import H.std.{io};

// ============================================================
// stage1/lexer.hc — H 版 lexer（K1：E7 自举渐进路线 · 词法段）
//
// 双实现对照：本文件与 Rust 参考 lexer（tag1/hc/src/lexer.rs，`hc lex` 转储）
// 输出同一格式 `{start} {end} {line} {col} {kind:?}`，逐行 diff，差异即 bug。
// Rust 参考实现长期保留（自举失败风险对策）。
//
// 用法：hc run stage1/lexer.hc <file.hc>
//
// 已复刻的保真细节：
//   - span line/col = token 消费后的 END 位置（下一 token 起点），非 token 起点
//   - 列/行按 Unicode 字符计数（utf8_width，非字节）
//   - 45 关键字（struct → KwClass 合并；`&&` 亦 KwAnd；and → KwAnd）
//   - 数字前缀 0x/0b/0o 与浮点指数归一化为小写（0XFF → "0xff"、3E5 → "3e5"）；
//     前缀后无数字也成 token（0x）、0o8 拆两个、1.5.6 整体 Float、0x1.8p3 拆解
//   - 惰性宽度后缀：整体形如 iN/uN/fN/isize/usize 才消费，否则数字止步、余下另成 ident；
//     后缀含 CJK 时复刻 Rust 的 `suffix.len()` 字节数 × bump（每 bump 一字符）导致的
//     过度消费（42i32中文 会吞掉后续空白 + 下一个 token 开头，与 Rust 一致）
//   - 字符串转义全套；`\xHH`(>=0x80) 产出 2 字节 UTF-8；`\u{...}` 越界/代理对 → 报错
//   - 未知字符产出两个相同 Error token；未闭合块注释 → Error + Eof 双 token
//   - Debug 输出：字符串内容转义（\t\r\n\"\\\0 + is_printable 表，探针实证对齐 Rust），
//     错误消息反斜杠双写（源串已预双写）
//
// 已知近似（语料约束在覆盖范围内，非 ASCII 边界行为可能不同）：
//   - 空白仅 ASCII 六种（Rust 含 Unicode 空白）
//   - is_ident_cont 非 ASCII 仅收 CJK 表意文字（U+4E00–U+9FFF = E4–E9 首字节），
//     不收全角标点/扩展 A/B/全角字母（Rust is_alphanumeric 覆盖更广）
//   - is_printable 为探针实证的近似表，未覆盖全部 Unicode 格式符
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
    // 非 ASCII 首字节：仅 CJK 表意文字（U+4E00–U+9FFF = E4–E9 首字节）视为字母数字，
    // 与 Rust `is_alphanumeric()` 一致；全角标点（U+FF00 区 = EF 首字节）不算。
    // 近似：CJK 扩展 A（U+3400–U+4DBF）与扩展 B+（F0+）未纳入，语料不涉及。
    if (b >= 0xE4 and b <= 0xE9) return true;
    return false;
}
fn is_ws(b: u8) bool {
    return b == 0x20 or b == 0x09 or b == 0x0A or b == 0x0D or b == 0x0B or b == 0x0C;
}
// UTF-8 字符宽度（按首字节）；与 Rust `char.len_utf8()` 对齐，col 按字符计数
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

// 关键字表（46 项）
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
    if (name == "tree") return "KwTree";
    if (name == "interface") return "KwInterface";
    if (name == "where") return "KwWhere";
    if (name == "namespace") return "KwNamespace";
    if (name == "import") return "KwImport";
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
    if (name == "void") return "KwVoid";
    if (name == "null") return "KwNull";
    if (name == "true") return "KwTrue";
    if (name == "false") return "KwFalse";
    return null;
}

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

// Rust `char::is_printable()` 近似：C0/C1 控制、DEL、NBSP、软连字符、
// 格式符（ZWSP/ZWJ/bidi/LS/PS/WJ 等）、变体选择符、BOM、非字符、私用区。
// 探针实证：U+115F/1160、U+3164、U+FFA0、U+FFFC/FFFD 可打印（不在排除表）。
fn is_printable(cp: i32) bool {
    if (cp >= 0x20 and cp <= 0x7E) return true;
    if (cp <= 0x1F) return false;                          // C0（\t\r\n 已在主循环特判）
    if (cp >= 0x7F and cp <= 0xA0) return false;           // DEL + C1 + NBSP
    if (cp == 0x00AD) return false;                        // 软连字符
    if (cp == 0x034F) return false;                        // 组合字素连接符
    if (cp == 0x061C) return false;                        // 阿拉伯字母标记
    if (cp == 0x17B4 or cp == 0x17B5) return false;
    if (cp == 0x180E) return false;                        // 蒙古元音分隔符
    if (cp >= 0x200B and cp <= 0x200F) return false;       // ZWSP/ZWNJ/ZWJ/LRM/RLM
    if (cp >= 0x2028 and cp <= 0x202E) return false;       // LS/PS/双向控制
    if (cp >= 0x2060 and cp <= 0x2064) return false;       // WJ/隐形运算符
    if (cp >= 0x206A and cp <= 0x206F) return false;
    if (cp >= 0xFE00 and cp <= 0xFE0F) return false;       // 变体选择符
    if (cp == 0xFEFF) return false;                        // BOM
    if (cp >= 0xFFF0 and cp <= 0xFFFB) return false;       // 特殊（FFFC/FFFD 可打印）
    if (cp >= 0xFDD0 and cp <= 0xFDEF) return false;       // 非字符
    if (cp >= 0xE000 and cp <= 0xF8FF) return false;       // BMP 私用区
    if (cp >= 0x1BCA0 and cp <= 0x1BCA3) return false;
    if (cp >= 0x1D173 and cp <= 0x1D17A) return false;
    if (cp >= 0xE0000 and cp <= 0xE0FFF) return false;     // 标签区
    if (cp >= 0xF0000 and cp <= 0x10FFFF) return false;    // 15/16 平面私用区
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

fn main(args:Vec<String>) !void {
    var mut path = args[0];
    if (args.len >= 2) { path = args[1]; }
    var mut src = try io.fs.read_file(path, alloc);
    var lx: Lexer = alloc.init(Lexer{ src = src, n = @intCast(i32, src.len), pos = 0, line = 1, col = 1 });
    lx.run();
}
