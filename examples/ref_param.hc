// ref/move 参数：C 后端验证（双后端一致性）
// h run examples/ref_param.hc 与 h build examples/ref_param.hc --exec 输出必须完全一致
// 覆盖：ref 参数写透（树/块）、ref 参数调用方法、move 参数（函数退出销毁）、move 实参后源失效

class Account {
    mut balance: f64

    fun deposit(amount: f64) -> f64 {
        balance += amount
        return balance
    }
}

// ref 参数：写透别名（改的就是调用者变量）
fun bump(acc: ref Account, amount: f64) {
    acc.balance += amount
}

// 块类型 ref 参数
fun inc(x: ref u64) {
    x += 1
}

// move 参数：函数退出时销毁（所有权随调用转移）
fun consume(a: move Account) -> f64 {
    return a.balance
}

fun main() -> void {
    mut a = Account{ balance: 100.0 }
    bump(a, 50.0)
    print("写透后:", a.balance.to_str())        // 150
    print("方法:", a.deposit(10.0).to_str())    // 160
    mut n = 10
    inc(n)
    print("块 ref 写透:", n.to_str())           // 11
    total = consume(move a)                     // a 失效；数据随 consume 退出销毁
    print("move 参数:", total.to_str())         // 160
    print("完成")
}
