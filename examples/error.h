// error 传播演示：error union 显式返回，未处理时传播并终止
// 运行：node src/h.js run examples/error.h

fun risky(amount: f64) -> error f64 {
    if amount < 0 {
        return error.NegativeAmount
    }
    return amount * 2
}

fun main() -> error void {
    print("先算一个合法的：", risky(10).to_str())
    // 触发错误 —— error.NegativeAmount 传播，程序在此终止
    bad = risky(-5)
    print("这行不会执行")
}

main()
