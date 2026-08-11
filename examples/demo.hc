// H 语言演示程序：数据模型 + 生命周期 + 字节化
// 运行：node src/h.js run examples/demo.hc

// 块（连续内存，复制语义）
struct Order {
    id: u64
    amount: f64
    items: [Str]
}

// 枚举（块的一种）
enum OrderStatus { Pending, Paid, Refunded }

// 接口（纯静态契约）
interface Payable {
    fun amount() -> f64
}

// class = 树（值字段 + 引用字段 + 方法集）
class Account : Payable {
    mut balance: f64
    orders: [Order]
    next: ref Account

    fun amount() -> f64 {
        return balance
    }
}

// 函数契约：块值复制 / ref 可写指针 / error 显式
fun process(order: Order, account: ref Account) -> error bool {
    if order.amount > account.balance {
        return error.InsufficientFunds
    }
    account.balance -= order.amount
    return true
}

fun describe(acc: Account) -> Str {
    return "余额=" + acc.balance.to_str()
}

fun build_account(initial: f64) -> move Account {
    mut acc = Account{ balance: initial }
    return move acc
}

// 作用域 / 字节化 / move
fun demo() -> error void {
    mut acc = build_account(1000.0)
    {
        ref view = acc          // 可写指针（双向引用）
        view.balance += 50
        print("子作用域写 view.balance →", view.balance)
    }                           // view 随作用域销毁，acc 数据不受影响
    print("describe:", describe(acc))
    bytes = acc.to_bytes()      // 树 → 字节（序列化压平）
    restored = Account.from_bytes(bytes)  // 字节 → 树（恢复）
    print("字节化往返:", restored.balance.to_str())
    store("account.bin", bytes)
    other = move acc            // 所有权转移：acc 失效
    print("demo 完成（acc 已 move 给 other）")
}

// 并发：全局必须声明访问模式
global ledger: Exclusive<[Str]> = []

fun worker() {
    print("worker 运行")
    yield
}

spawn worker()
demo()
