import H.std.{io};

// 44-multi-file-main.hc — 多文件项目入口
//
//   - build.zon 声明包内文件（Q26）
//   - import 引入命名空间；限定访问 Orders.Line

import Orders;
import Pricing;

fn main() !void {
    var lines = Vec<Orders.Line>.init(alloc);
    lines.append(Orders.Line{item_id = 1, price = 3.0});
    lines.append(Orders.Line{item_id = 2, price = 2.0});

    var total = Orders.total(&lines);
    io.print("total = {}\n", total);
}
