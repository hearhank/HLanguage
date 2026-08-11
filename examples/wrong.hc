// 静态违规演示：只读变量写入（R4）
// 运行：node src/h.js check examples/wrong.hc（应报 R4，拒绝执行）

fun f() {
    x = 1          // val 声明：只读
    x = 2          // R4：只读变量不可写
}

f()
