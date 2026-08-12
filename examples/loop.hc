// 整数除法 + 循环（for/while/break/continue）：双后端一致性验证
// h run examples/loop.hc 与 h build examples/loop.hc --exec 输出必须完全一致

fun main() -> void {
    // 整数除法：u64 / u64 = 整除（向零截断）；f64 参与 = 浮点
    print("整除:", (7 / 3).to_str(), "取余:", (7 % 3).to_str())
    print("浮点:", (7.0 / 3).to_str())
    print("混合:", (7.0 / 2).to_str())

    // for 区间求和（半开 0..5）
    mut sum = 0
    for i in 0..5 {
        sum = sum + i
    }
    print("求和:", sum.to_str())

    // 嵌套循环
    mut total = 0
    for a in 0..3 {
        for b in 0..3 {
            total = total + a * b
        }
    }
    print("嵌套:", total.to_str())

    // break：找到第一个 3 的倍数
    mut found = 0
    for i in 1..20 {
        if i % 3 == 0 {
            found = i
            break
        }
    }
    print("首个3倍数:", found.to_str())

    // continue：跳过偶数求和
    mut oddSum = 0
    for i in 0..10 {
        if i % 2 == 0 {
            continue
        }
        oddSum = oddSum + i
    }
    print("奇数求和:", oddSum.to_str())

    // while 循环
    mut n = 1
    mut steps = 0
    while n < 100 {
        n = n * 2
        steps = steps + 1
    }
    print("倍增步数:", steps.to_str())
}
