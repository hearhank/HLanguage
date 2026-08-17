import H.std.{io};

// 42-pricing.hc — 多文件项目（命名空间跨文件，Q21）
//
//   - namespace 块可跨文件、一文件多组（C# 式）
//   - pub 管包边界（同包 using 即达；跨包需 pub + build.zon 依赖）

namespace Pricing {
    pub fn with_tax(price: f64, rate: f64) f64 {
        return price * (1.0 + rate);
    }
}

[test] fn pricing_with_tax() !void {
    var total = Pricing.with_tax(100.0, 0.1);
    try expect(total > 109.99 and total < 110.01);
}
