# 异步模型：async/await 关键字（逆转）

2026-08-13 项目所有者逆转「无 async/await 关键字」决策（此前按 Zig 第一——Zig 0.15 移除关键字、放标准库）：

- **async/await 是语言关键字**；`async fn f(...) T` 返回 `Future(T)`，`await` 等待完成（函数级标记）
- **Future = 线程任务的结果句柄**（复杂类型）：提交 async 任务即生成线程（`Io.Threaded` 默认），`await` 与 `t.join()` 语义同源
- 事件循环（Evented）为可选运行时实现，接口不变
- **偏离 Zig 第一**，取 Rust/TS 式（第二/第三参考）

理由：async/await 是异步代码的主流可读语法（Rust/TS 生态已验证）；与「线程是数据对象」模型兼容（Future 即结果句柄，A2 定案）；`await` 语义显式（等待数据对象的结果），不引入隐藏阻塞（「没有隐藏控制」）。

2026-08-13 评审补充（B2）：异步执行模型考虑 **Go 式协程 + 通道**（M:N 调度，缓解「每个 async 任务一个 OS 线程」的线程爆炸风险）；通道与四模式类型（12.21）衔接。方向待细化。

2026-08-13 Q20 定案（Future 类型参数统一）：`async fn f(...) R` 返回 `Future(R)`，R = 完整返回类型（含 error union，如 `Future(!String)`）；`await` 返回 R（`try await` 解包）。此前「async fn 返回 Future(T)」的公式表述修正为「Future(完整返回类型)」。
