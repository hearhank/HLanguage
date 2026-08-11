// H 语言词法器（从原型 #3-#5 提取并修复）
// 输出 token 流：{kind, value, line, col}

const KEYWORDS = new Set([
  "struct", "class", "enum", "interface", "fun", "mut", "ref", "move", "error",
  "return", "if", "else", "global", "spawn", "import", "hide", "alias", "use",
  "pub", "yield", "match", "try", "catch", "true", "false",
]);

const OPERATORS = [
  "=>", "->", "+=", "-=", "*=", "/=", "==", "!=", "<=", ">=", "&&", "||", "::", "..",
  "=", "<", ">", "+", "-", "*", "/", "%", "!", "{", "}", "(", ")", "[", "]",
  ";", ",", ":", ".", "?",
];

function lex(src) {
  const toks = [];
  let i = 0, line = 1, col = 1;
  const push = (kind, value, ln, cl) => toks.push({ kind, value, line: ln, col: cl });

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
      if (src[i] === "." && /\d/.test(src[i + 1] || "")) { s += "."; i++; while (i < src.length && /\d/.test(src[i])) s += src[i++]; }   // 小数只允许一个点（避免 1..3 被吞）
      push("NUMBER", parseFloat(s), line, col); col += s.length; continue;
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
