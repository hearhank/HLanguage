// class（树）：C 后端验证（双后端一致性）
// h run examples/tree.hc 与 h build examples/tree.hc --exec 输出必须完全一致
// 覆盖：树构造/打印（Type{...} 逐字一致）/方法（self 字段读写 + 嵌套块）/
//       move 转移 / -> move T 返回（return 逃逸）/ 树参数 = 视图 / 块作用域销毁
// 注意：C 后端暂不支持 ref 字段、ref/move 参数、error、to_bytes（demo.hc 走 h run）

struct Item {
    name: Str
    price: f64
}

class Account {
    mut balance: f64
    id: u64
    label: Str
    items: [Item]

    fun deposit(amount: f64) -> f64 {
        balance += amount
        {
            bonus = 1.0
            balance += bonus
        }
        return balance
    }

    fun describe() -> Str {
        return label
    }
}

// -> move Account：树必须通过 move 返回（所有权随返回值转移）
fun make_account(id: u64) -> move Account {
    acc = Account{ balance: 0.0, id: id, label: "主账户", items: [Item{ name: "咖啡", price: 3.5 }] }
    return acc          // 无 move 关键字：返回值逃逸，函数退出不销毁
}

// 树参数 = 视图：不拥有，函数退出不销毁
fun peek(acc: Account) -> f64 {
    return acc.balance
}

fun main() -> void {
    mut a = make_account(7)   // mut：方法内写 receiver 字段需要可写源
    print("账户:", a)
    print("id:", a.id.to_str(), "label:", a.label.to_str())
    print("存入后:", a.deposit(100.0))
    print("视图余额:", peek(a))
    {
        tmp = Account{ balance: 1.0, id: 2, label: "临时", items: [] }
        print("块内:", tmp.balance.to_str())
    }                       // tmp 随块退出销毁
    b = move a              // 所有权转移：a 失效
    print("转移后余额:", b.balance.to_str())
    print("完成")
}
