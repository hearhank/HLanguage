// 06-string.hc — 字符串
// 覆盖：&[u8] 字面量、len、索引、切片 [a..b]、concat、String.compare、String.fromInt、==（C7）
// 预期 stdout：
// 5
// 101
// ell
// hello world
// true
// 0
// 1
// 42
fn main() !void {
    var s: &[u8] = "hello";
    io.print("{}\n", s.len);
    io.print("{}\n", s[1]);
    io.print("{}\n", s[1..4]);
    var a: &[u8] = "hello ";
    var b: &[u8] = "world";
    io.print("{}\n", a.concat(b));
    io.print("{}\n", "abc" == "abc");
    io.print("{}\n", String.compare("abc", "abc"));
    io.print("{}\n", String.compare("xyz", "abc"));
    io.print("{}\n", String.fromInt(42));
}
