import H.std.{io};

// 91-orders-domain.hc — 模块(domain)约定（组 H3，第二部分代码管理）
//
// `[module]` 特性标注的命名空间 = 领域模块（domain）。本示例演示三项约定：
//
//   1. 边界：模块 owns 数据（模块内 class 类型），对外只暴露 `pub` API
//   2. 上下文（init 参数列表）：依赖经 `init(...)` 显式注入并返回上下文，
//      模块 API 首参携带上下文——数据/依赖注入，与 `import` 符号引用正交（Q24）
//   3. 隔离：`[module]` 成员仅限定名（`Orders.xxx`），不参与包扁平命名空间；
//      同包其它代码/测试须限定访问（扁平 `total(...)` 会 UndefinedName）
//
// 跨包形态（06-08-modules.md）：库包内 `pub [module] namespace Orders`，
// 应用 `import orders.Orders;` 后 `Orders.total(...)` 限定访问。本示例为单文件
// 同包形态，聚焦边界与上下文；import 机制另见 41-namespaces / 02-packages。

// [module] namespace Orders {
    // —— owns 数据（模块内私有类型：无 pub = 模块私有，扁平/跨包不可见）——
    class LineItem {
        sku: String,
        qty: i32,
        price_cents: i32
    }

    // —— 上下文：依赖注入载体（init 参数列表构造）——
    class OrderCtx {
        tax_pct: i32,
        min_qty: i32
    }

    // 上下文工厂：显式接收依赖（税率、最小起订量），返回模块上下文；
    // 调用方持有 ctx 并传入各 API（数据/依赖经上下文进入模块）
    pub fn init(tax_pct: i32, min_qty: i32) OrderCtx {
        return move OrderCtx{tax_pct = tax_pct, min_qty = min_qty};
    }

    // —— 对外 pub API（边界）——
    // 订单合计（整数分，精确）：低于最小起订量的行剔除，末尾按上下文税率加税
    pub fn total(ctx: Orders.OrderCtx, lines: [2]Orders.LineItem) i32 {
        var sum: i32 = 0;
        for (lines) |line| {
            if (line.qty >= ctx.min_qty) {
                sum += line.qty * line.price_cents;
            }
        }
        return sum + (sum * ctx.tax_pct) / 100;
    }
// }

[Test] fn orders_domain_boundary_and_total() !void {
    var ctx = Orders.init(10, 2);   // 税率 10%、最小起订 2 件
    var lines = [
        Orders.LineItem{sku = "A", qty = 2, price_cents = 300},   // 600 分
        Orders.LineItem{sku = "B", qty = 1, price_cents = 400},   // 不足 min_qty，剔除
    ];
    // (2×300) + 10% = 660 分
    try expect_eq(Orders.total(ctx, lines), 660);
}

[Test] fn orders_domain_context_parameterizes_behavior() !void {
    // 同一批行，注入不同上下文 → 结果不同（行为由上下文参数化）
    var low_tax = Orders.init(0, 1);     // 无税、最小 1 件：两行全部计入
    var high_tax = Orders.init(20, 2);   // 20% 税、最小 2 件：仅 A 行计入
    var lines = [
        Orders.LineItem{sku = "A", qty = 2, price_cents = 300},   // 600 分
        Orders.LineItem{sku = "B", qty = 1, price_cents = 400},   // 400 分
    ];
    try expect_eq(Orders.total(low_tax, lines), 1000);   // 600 + 400
    try expect_eq(Orders.total(high_tax, lines), 720);   // 600 + 20% = 720（B 剔除）
}
