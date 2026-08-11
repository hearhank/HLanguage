// match 枚举匹配：穷尽性由编译期保证（缺变体在 h run / h check 时拒绝）
// 运行：node src/h.js run examples/match.hc

enum OrderStatus { Pending, Paid, Refunded }

fun status_text(s: OrderStatus) -> Str {
    return match s {
        Pending  => "待处理"
        Paid     => "已支付"
        Refunded => "已退款"
    }
}

fun main() -> error void {
    st = OrderStatus.Paid
    print("订单状态:", status_text(st))
    st2 = OrderStatus.Refunded
    print("另一单:", status_text(st2))
    // 错误值 + match 的对照：错误仍是显式数据
    if status_text(OrderStatus.Pending) == "待处理" {
        print("match 结果可用于比较")
    }
}

main()
