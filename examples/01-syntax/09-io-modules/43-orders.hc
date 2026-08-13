// 43-orders.hc — 多文件项目（using 引入同包命名空间）

using Pricing;

namespace Orders {
    pub struct Line {
        item: String,
        price: f64,
    }

    pub fn total(lines: &Vec(Line)) f64 {
        var sum = 0.0;
        for (lines) |line| {
            sum += with_tax(line.price, 0.1);   // using 后直接使用（Q21）
        }
        return sum;
    }
}
