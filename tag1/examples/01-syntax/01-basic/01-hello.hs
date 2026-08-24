import H.std.{io};

// hello.hs — H 语言脚本文件（B6-2：.hs 子集）
// hc run 直接执行，无 script 展开、无 comptime
fn main() {
    io.print("hello from .hs script\n");
    io.print("x = {}, y = {}\n", 42, 3.14);
}