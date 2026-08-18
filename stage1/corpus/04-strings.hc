// 字符串：常规转义、\xHH（>=0x80 → 2 字节 UTF-8）、\u{...}、原始多行、中文内容
const A = "hello world";
const B = "a\nb\rc\td\"e\\f";
const C = "\x00\x01\x1f\x7f\x80\x9f\xa0\xc2\xff";
const D = "\u{48}\u{65}\u{4f}\u{1F600}\u{10FFFF}\u{0}";
const E = "PascalCase，如 中文内容 空格";
const R = """raw
multi
line content""";
const Q = "\\u{41}";
const M = "null\x00term";
