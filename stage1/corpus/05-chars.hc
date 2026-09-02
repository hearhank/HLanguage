// 字符字面量（ADR-0037 D11：Unicode 标量码点，comptime_int 定型）：ASCII、转义、多字节、十六进制、Unicode、引号/反斜杠
const A = 'a';
const B = '\n';
const C = '中';
const D = '\x41';
const E = '\u{42}';
const F = '\u{4f60}';
const G = '\'';
const H = '\';
const I = ' ';
const J = '\u{1F600}';
