import H.std.{io as my};
import H.std.net.{http,tcp};

fn main(args:o vec<String>) !void {
    my.print("hello, world\n");
    io.print("x = {}, y = {}\n", 42, 3.14);
    http.print();
}

[test]
fn hello_entry_runs() !void {
    try main(test_io);   // S2：smoke test（入口 !void 错误自动捕获）
}
