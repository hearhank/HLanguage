import H.std.{io};

// 43-orders.hc — 多文件项目（using 引入同包命名空间）

using Pricing;

namespace Orders {
    pub struct Line {
        item: String,
        price: f64,
    }

    pub fn total(lines: &Vec<Line>) f64 {
        var sum = 0.0;
        for (lines) |line| {
            sum += with_tax(line.price, 0.1);   // using 后直接使用（Q21）
        }
        return sum;
    }
}

[test] fn orders_total() !void {
    var lines = Vec<Orders.Line>.init(alloc);
    lines.append(Orders.Line{item = String.from("apple", alloc), price = 3.0});
    lines.append(Orders.Line{item = String.from("banana", alloc), price = 2.0});
    var total = Orders.total(&lines);
    try expect(total > 5.49 and total < 5.51);   // (3+2) * 1.1 = 5.5
}
