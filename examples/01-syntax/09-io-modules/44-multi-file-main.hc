import H.std.{io};

// 44-multi-file-main.hc — 多文件项目入口
//
//   - build.zon 声明包内文件（Q26）
//   - using 引入命名空间；限定访问 Orders.Line

using Orders;
using Pricing;

fn main(args: o Vec(String)) !void {
    var lines = Vec(Orders.Line).init(alloc);
    lines.append(Orders.Line{ item = String.from("apple", alloc), price = 3.0 });
    lines.append(Orders.Line{ item = String.from("banana", alloc), price = 2.0 });

    var total = Orders.total(&lines);
    io.print("total = {}\n", total);
}

[test] fn multi_file_project() !void {
    var lines = Vec(Orders.Line).init(alloc);
    lines.append(Orders.Line{ item = String.from("apple", alloc), price = 3.0 });
    lines.append(Orders.Line{ item = String.from("banana", alloc), price = 2.0 });
    var total = Orders.total(&lines);
    try expect(total > 5.49 and total < 5.51);
}
