// 闭包：值捕获 / move 捕获 / 函数引用混用 双后端一致性验证
// h run examples/closure.hc 与 h build examples/closure.hc --exec 输出必须完全一致

fun twice(x: u64) -> u64 {
    return x * 2
}

fun apply(f: fun(u64) -> u64, x: u64) -> u64 {
    return f(x)
}

class Counter {
    mut count: u64
}

fun make_adder(n: u64) -> fun(u64) -> u64 {
    return fun(x: u64) [n] -> u64 { return x + n }
}

fun main() -> void {
    // 值捕获：闭包字面量（环境复制块值）
    base = 10
    add = fun(x: u64) [base] -> u64 { return x + base }
    print("捕获:", add(5).to_str())

    // 闭包作为参数（与函数引用混用同一 fun 类型）
    print("apply闭包:", apply(add, 7).to_str())
    print("apply函数:", apply(twice, 7).to_str())

    // 函数名即值 + 再传递
    g = twice
    print("函数值:", g(3).to_str())
    print("再传递:", apply(g, 4).to_str())

    // 无捕获匿名函数 = 函数引用
    triple = fun(x: u64) -> u64 { return x * 3 }
    print("匿名:", triple(6).to_str())

    // move 捕获：树所有权入环境（闭包内可写）
    mut c = Counter{ count: 100 }
    step = fun(x: u64) [move c] -> u64 {
        c.count = c.count + x
        return c.count
    }
    print("move捕获:", step(1).to_str(), step(2).to_str())

    // 工厂：返回捕获闭包（环境随返回值传递）
    add5 = make_adder(5)
    print("工厂:", add5(3).to_str(), add5(10).to_str())
}
