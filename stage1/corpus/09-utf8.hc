// 多字节 UTF-8 的 span/col 计数（col 按字符计，非字节）
var 中 = "文";                // 中/文 在代码位置 → 各 2 个 Error
var a = "中文字符串内容";
var b = 中文标识;
const c = '文';
var d = "中文"; var e = "x";  // var e 的 col 需按字符计数
