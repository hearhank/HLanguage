// ============================================================
// token 流转储（K1 对照格式 = hc lex：{start} {end} {line} {col} {kind}）
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

fn dump_tokens(toks: Vec<Token>) void {
    var mut i: usize = 0;
    while (i < toks.len) {
        var t = toks[i];
        if (t.kind == "Char") {
            io.print("{} {} {} {} Char({})\n", t.start, t.end, t.line, t.col, @intCast(i32, t.text[0]));
        } else if (t.kind == "Str") {
            io.print("{} {} {} {} Str(\"", t.start, t.end, t.line, t.col);
            io.print("{}\")\n", dbg_escape(t.text).as_slice());
        } else if (t.kind == "Ident" or t.kind == "AtBuiltin" or t.kind == "Int" or t.kind == "Float") {
            io.print("{} {} {} {} {}(\"", t.start, t.end, t.line, t.col, t.kind);
            io.print("{}\")\n", t.text.as_slice());
        } else {
            io.print("{} {} {} {} {}\n", t.start, t.end, t.line, t.col, t.kind);
        }
        i += 1;
    }
}
