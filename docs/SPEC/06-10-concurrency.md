# H 语言规范：并发与异步

> 对应实现模块：07 **第三块 E2**（并发与异步）。**第一部分最小功能集明确不实现**——最小例子不必实现多线程。
>
> **组 G 已提前落地（2026-08-17，第二部分）**：E2.2 **线程生命周期**子集 = `spawn(f, args...) o Thread(T)` / `join() !T` / `cancel() !void` / `is_done() bool` / `detach()` + 每线程 alloc（Q8）+ 捕获规则（Q18 绑定/逃逸 + Q19 冻结窗口静态检查）。并发模型为**协作式延迟执行**（确定性、单线程）：spawn 立即返回句柄但不并发运行，join/detach/程序结束时才执行到完成。实现覆盖三后端（interp / IR / 字节码一致）；**原生 LLVM 为子集边界**——spawn 需函数引用（FnRef），原生 ABI 未支持（Phase 8），编译模式响亮拒绝（`error.NotCallable`），不静默误编译（G4b 定案 A）。
>
> **以下内容仍属第三块（不在本阶段实现）**：四模式类型（OneToOne/OneToMany/ManyToOne/ManyToMany）、真 OS 并行、`async`/`await` 关键字、`Future(T)`、`@atomic*`、`mutex`、`Io.threaded()/evented()`。下面标注 ✅ 的为本阶段已落地语义。

```hc
var shared: o ManyToMany(i32) = ...;   // 四模式共享容器
var t: o Thread(i32) = spawn(af, ...); // spawn = 函数 + 显式参数（Q18）
var r = try t.join();                  // 消耗所有权（await 同源）
t.cancel() / t.is_done() / t.detach()
var f: Future(R) = af(...); var v = await f;  // await 任何函数可用（Q19）；R = 完整返回类型（Q20）
```

- **四模式类型**：`OneToOne(T)`（单读单写）/ `OneToMany(T)`（单读多写）/ `ManyToOne(T)`（多读单写）/ `ManyToMany(T)`（多读多写）——内建泛型共享内存容器，写者数量由类型名保证（单写者无锁、多写者互斥）；用泛型（脚本生成）插入数据类型
- **四模式类型方法集（Q14 定案；Q-R12 补充通道方法）**：`init(alloc)`（构造）/ `write(v)`（写）/ `read() T`（**阻塞读**：有值即返回，close 后空则运行时错误）/ `try_read() ?T`（**非阻塞读**）/ `close()`（**结束标志**：此后 `write` 报错、`try_read` 返回 `null`）/ **`send(v)` / `recv() T`——通道方法（Q-R12 定案：线程间数据传输的通道语义，send = 阻塞写、recv = 阻塞读，与 write/read 同源）**；全部方法取 `*Self`（Q32 内建共享特例：内部同步、并发安全由类型保证，用户类型不可模拟）
- **缓冲与阻塞（2026-08-14 定案）**：共享内存容器（write/read）**无容量概念**——write 不阻塞（直接写共享槽 + 内部同步）、read 阻塞到有值；通道（send/recv）为**有界队列**——容量构造时指定（`init(alloc, cap)`），send 满时阻塞、recv 空时阻塞；close 后 write/send 报错、try_read/try_recv 返回 null
- ✅ **线程所有权（组 G）**：spawn 归当前作用域；退出时已完成→销毁、运行中→移交根作用域（**无隐式阻塞**）；**根作用域 = 程序最后退出场所**，负责最终资源回收（评审 A7）；显式 `join() !T` 消耗所有权，错误以 error union 跨线程传播（G2 已落地：join 透传 / cancel→`error.Cancelled` / detach 立即运行）
- ✅ **线程捕获（组 G3）**：值类型复制值；引用类型 move 或 global；**作用域例外（Q18）**——作用域绑定的执行（join 后回到当前作用域）可捕获引用；逃逸线程引用捕获禁用（编译期检查）
- ✅ **线程捕获静态检查（组 G3，Q19 定案）**：
  - **绑定/逃逸判定（数据流）**：句柄（Thread/Future）在声明作用域内被 join/await → 绑定（引用捕获合法）；被 detach 或作用域退出未 join → 逃逸（引用捕获编译错误）
  - **冻结窗口（借用期）**：绑定场景下，被捕获引用的目标从 spawn 到 await/join 之间主线程不可写（不可 `var mut` 写入、不可取 `&mut`）——编译期检查，await/join 后恢复；并发写共享数据 → 显式用四模式类型
- **async/await（第三块）**：语言关键字（逆转，见 ADR-0008）；`Future(T)` = 线程任务结果句柄；await 与 join 同源且**任何函数可用**（Q19，无 async 传染）；执行模型考虑 **Go 式协程 + 通道**（M:N，评审 B2 方向）
- ✅ **Thread 方法集一致（2026-08-14 定案；组 G 落地 Thread 侧）**：`join() !T` / `cancel() !void`（**协作式取消**——延迟模型下运行点 = join/detach/程序结束，cancel 置协作标志）/ `is_done() bool` / `detach()`；`await f` ≡ `f.join()`（await 第三块，join 已落地）
- **Io 执行模型**：`Io.threaded()` = 阻塞 IO + 每操作线程（简单，默认）；`Io.evented()` = **单线程事件循环**（select/epoll 式非阻塞 IO + async/await 协作调度，配合协作取消）；两者同一 `Io` 接口（接口工厂 R-4），双模式一致
- **原子操作（Q-S3 定案）**：`@atomicLoad(T, p, order)` / `@atomicStore(T, p, v, order)` / `@atomicRmw(T, p, op, v, order)`——无锁原语；内存序 `relaxed`/`acquire`/`release`/`acq_rel`/`seq_cst`（默认 seq_cst）；四模式类型内部实现基于这些原语（单写者路径可免原子）
