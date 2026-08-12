// H 语言词法器（从原型 #3-#5 提取并修复）
// 输出 token 流：{kind, value, line, col}

const KEYWORDS = new Set([
  "struct", "class", "enum", "interface", "fun", "mut", "ref", "move", "error",
  "return", "if", "else", "global", "spawn", "import", "hide", "alias", "use",
  "pub", "yield", "match", "try", "catch", "true", "false",
  "for", "while", "break", "continue", "in", "null",
]);

const OPERATORS = [
  "=>", "->", "+=", "-=", "*=", "/=", "==", "!=", "<=", ">=", "&&", "||", "::", "..",
  "=", "<", ">", "+", "-", "*", "/", "%", "!", "{", "}", "(", ")", "[", "]",
  ";", ",", ":", ".", "?",
];

/* 数字字面量后缀：5u8 / -3i32 / 1.5f32（无后缀默认 u64/f64） */
const NUM_SUFFIXES = new Set(["u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize", "f32", "f64"]);

function lex(src) {
  const toks = [];
  let i = 0, line = 1, col = 1;
  const push = (kind, value, ln, cl, float, suffix) => toks.push({ kind, value, line: ln, col: cl, float, suffix });

  while (i < src.length) {
    const c = src[i];
    if (c === "\n") { push("NEWLINE", "\n", line, col); line++; col = 1; i++; continue; }
    if (c === " " || c === "\t" || c === "\r") { col++; i++; continue; }
    if (c === "/" && src[i + 1] === "/") { while (i < src.length && src[i] !== "\n") i++; continue; }
    if (c === '"') {
      let s = "", j = i + 1;
      while (j < src.length && src[j] !== '"') { s += src[j]; j++; }
      push("STRING", s, line, col); col += (j - i) + 1; i = j + 1; continue;
    }
    if (/\d/.test(c)) {
      let s = "";
      while (i < src.length && /\d/.test(src[i])) s += src[i++];
      if (src[i] === "." && /\d/.test(src[i + 1] || "")) {
        // 小数只允许一个点（避免 1..3 被吞）；紧跟成员访问 '.' 时按整数拆（元组 .0.1）
        const afterMemberDot = toks.length && toks[toks.length - 1].kind === "OP" && toks[toks.length - 1].value === ".";
        if (!afterMemberDot) { s += "."; i++; while (i < src.length && /\d/.test(src[i])) s += src[i++]; }
      }
      // 类型后缀：5u8 / 1.5f32（紧跟数字，无空格）
      let suffix = "";
      if (/[A-Za-z_]/.test(src[i] || "")) {
        let ss = "";
        while (i < src.length && /[A-Za-z0-9_]/.test(src[i])) ss += src[i++];
        if (NUM_SUFFIXES.has(ss)) suffix = ss;
        else throw { lex: true, msg: "非法数字后缀 '" + ss + "'", line, col };
      }
      push("NUMBER", parseFloat(s), line, col, s.includes("."), suffix);   // 第 5 参：浮点标志；第 6 参：类型后缀
      col += s.length + suffix.length; continue;
    }
    if (/[A-Za-z_]/.test(c)) {
      let s = "";
      while (i < src.length && /[A-Za-z0-9_]/.test(src[i])) s += src[i++];
      push(KEYWORDS.has(s) ? "KEYWORD" : "IDENT", s, line, col); col += s.length; continue;
    }
    let matched = null;
    for (const op of OPERATORS) { if (src.startsWith(op, i)) { matched = op; break; } }
    if (matched) { push("OP", matched, line, col); i += matched.length; col += matched.length; continue; }
    throw { lex: true, msg: "无法识别的字符 '" + c + "'", line, col };
  }
  push("EOF", "", line, col);
  return toks;
}

module.exports = { lex };
