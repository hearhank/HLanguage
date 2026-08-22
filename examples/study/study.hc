// study.hc — 设计草图：import 符号选择（io as my / net.http,tcp）+ 新入口 main(args)
// 注：net.http/tcp 为第三块 E3 标准库扩展，此处仅演示 import 形态（未调用）。

import H.std.{io as my};
import H.std.net.{http, tcp};

fn main() !void {
    my.print("hello, world\n");
    io.print("x = {}, y = {}\n", 42, 3.14);
}
