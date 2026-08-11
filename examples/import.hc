// class 导入机制：import / 深度传递 / hide / alias / 同名冲突
// 运行：node src/h.js run examples/import.hc

// 基础类：可支付（amount 访问 receiver 的 balance 字段）
class PayableBase {
    fun amount() -> f64 {
        return balance
    }
}

// 另一个基础类：记账
class Bookkeeping {
    fun ledger_name() -> Str {
        return "总账"
    }
}

// 深度传递：Sub 导入 PayableBase（amount）+ Bookkeeping（ledger_name）
class Sub {
    import PayableBase
    import Bookkeeping
}

// 同名冲突源：Bookkeeping2 也有 name()（与 Account 自己的 name 冲突 → 自己优先，自动隐藏）
class Bookkeeping2 {
    fun name() -> Str {
        return "账本二"
    }
}

// 完整类：导入两个基础类；hide 隐藏 ledger_name；alias 暴露 Bookkeeping2::name
class Account {
    mut balance: f64
    import Sub              // 深度传递：amount + ledger_name（来自 Sub 的导入）
    import Bookkeeping2

    fun name() -> Str {
        return "账户"
    }
    hide Sub::ledger_name   // 隐藏导入的方法（外部不可调用）
    alias book = Bookkeeping2::name  // 别名：暴露指定方法
}

fun main() -> error void {
    mut acc = Account{ balance: 100 }
    print("amount():", acc.amount().to_str())       // 导入的方法：receiver 字段访问 balance
    print("name():", acc.name().to_str())           // 自己的方法优先（Bookkeeping2::name 自动隐藏）
    print("alias book():", acc.book().to_str())     // alias：Bookkeeping2::name
    acc.balance += 50
    print("写入后 balance:", acc.balance.to_str())
}

main()
