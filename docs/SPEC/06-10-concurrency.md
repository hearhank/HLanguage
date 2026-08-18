# H 语言规范：并发与异步

> 对应实现模块：07 **第三块 E2**（并发与异步）。**第一部分最小功能集明确不实现**——最小例子不必实现多线程。
>
> **组 G 已提前落地（2026-08-17，第二部分）**：E2.2 **线程生命周期**子集 = `spawn(f, args...) o Thread(T)` / `join() !T` / `cancel() !void` / `is_done() bool` / `detach()` + 每线程 alloc（Q8）+ 捕获规则（Q18 绑定/逃逸 + Q19 冻结窗口静态检查）。并发模型为**协作式延迟执行**（确定性、单线程）：spawn 立即返回句柄但不并发运行，join/detach/程序结束时才执行到完成。实现覆盖三后端（interp / IR / 字节码一致）；**原生 LLVM 为子集边界**——spawn 需函数引用（FnRef），原生 ABI 未支持（Phase 8），编译模式响亮拒绝（`error.NotCallable`），不静默误编译（G4b 定案 A）。
>
> **状态（2026-08-18 组 F 落地后更新）**：`async`/`await`/`Future(T)`/`Io.threaded()/evented()` 已随组 E（E2.3 异步，协作式 Future）落地（标注 ✅）；**四模式类型（OneToOne/OneToMany/ManyToOne/ManyToMany）与 `@atomic*` 已随组 F（E2.1/E2.4）落地（2026-08-18，ADR-0011 逆转）**——协作式透明实现：单线程确定性下四变体运行时行为一致（读者/写者数量为类型层契约），原子操作无竞争 → 透明（标注 ✅）。**真 OS 并行与 `mutex` 仍按 ADR-0011 延迟 1.x**（需真并发硬件语义，本块不实现）。

```hc
var shared: o ManyToMany(i32) = ...;   // 四模式共享容器
var t: o Thread(i32) = spawn(af, ...); // spawn = 函数 + 显式参数（Q18）
var r = try t.join();                  // 消耗所有权（await 同源）
t.cancel() / t.is_done() / t.detach()
var f: Future(R) = af(...); var v = await f;  // await 任何函数可用（Q19）；R = 完整返回类型（Q20）
```

- ✅ **四模式类型（组 F，2026-08-18 落地）**：`OneToOne(T)`（单读单写）/ `OneToMany(T)`（单读多写）/ `ManyToOne(T)`（多读单写）/ `ManyToMany(T)`（多读多写）——内建泛型共享内存容器，写者数量由类型名保证（单写者无锁、多写者互斥）；用泛型（脚本生成）插入数据类型。**协作式透明实现**：单线程确定性模型下四变体运行时行为一致（读者/写者数量为类型层契约，不引入真锁/真并发；真 OS 并行归 1.x）；运行时 = `Value::Class`/`IrValue::Class` + 类名分派，fields `queue`（FIFO）/`closed`/`alloc`/`cap`
- ✅ **四模式类型方法集（组 F，Q14 定案 + Q-R12 通道方法）**：`init(alloc)`（构造）/ `init(alloc, cap)`（通道有界）/ `write(v)`（队尾追加；close 后 → `error.Closed`）/ `read() T`（队首弹出；空 → `error.Empty`）/ `try_read() ?T`（队首弹出或 null）/ `close()`（置结束标志）/ **`send(v)` / `recv() T`**（通道方法：send = 有界写，满 → `error.ChannelFull`；recv ≡ read）；全部方法取 `*Self`（Q32 内建共享特例：并发安全由类型保证，用户类型不可模拟）
- ✅ **缓冲与阻塞（组 F，2026-08-14 设计按协作式映射）**：共享内存容器（write/read）**无容量概念**——write 不阻塞（队尾追加）、read 空 → `error.Empty`；通道（send/recv）为**有界队列**——容量构造时指定（`init(alloc, cap)`），send 满 → `error.ChannelFull`（协作式无真阻塞）、recv 空 ≡ read；close 后 write/send 报 `error.Closed`、try_read 返回 null
- ✅ **线程所有权（组 G）**：spawn 归当前作用域；退出时已完成→销毁、运行中→移交根作用域（**无隐式阻塞**）；**根作用域 = 程序最后退出场所**，负责最终资源回收（评审 A7）；显式 `join() !T` 消耗所有权，错误以 error union 跨线程传播（G2 已落地：join 透传 / cancel→`error.Cancelled` / detach 立即运行）
- ✅ **线程捕获（组 G3）**：值类型复制值；引用类型 move 或 global；**作用域例外（Q18）**——作用域绑定的执行（join 后回到当前作用域）可捕获引用；逃逸线程引用捕获禁用（编译期检查）
- ✅ **线程捕获静态检查（组 G3，Q19 定案）**：
  - **绑定/逃逸判定（数据流）**：句柄（Thread/Future）在声明作用域内被 join/await → 绑定（引用捕获合法）；被 detach 或作用域退出未 join → 逃逸（引用捕获编译错误）
  - **冻结窗口（借用期）**：绑定场景下，被捕获引用的目标从 spawn 到 await/join 之间主线程不可写（不可 `var mut` 写入、不可取 `&mut`）——编译期检查，await/join 后恢复；并发写共享数据 → 显式用四模式类型
- ✅ **async/await（组 E，2026-08-18 落地）**：`async fn` 返回 `Future(R)`（R = 完整返回类型含错误联合 `Future(!R)`）；**await ≡ join()** 且**任何函数可用**（Q19，无 async 传染）；执行模型 = **协作式延迟执行**（非 Go 式协程/M:N，ADR-0011 定案）——async fn 调用点返回**惰性** `Future` 值，体延迟到 await 才执行（复用组 G 线程机制 `make_future`/`future_run`），协作式取消（cancel → `error.Cancelled`）、is_done 状态转移、await 幂等缓存
- ✅ **Thread 方法集一致（2026-08-14 定案；组 G 落地 Thread 侧）**：`join() !T` / `cancel() !void`（**协作式取消**——延迟模型下运行点 = join/detach/程序结束，cancel 置协作标志）/ `is_done() bool` / `detach()`；`await f` ≡ `f.join()`（组 E 已落地，2026-08-18）
- ✅ **Io 执行模型（组 E E3，2026-08-18 落地）**：`Io.threaded()` / `Io.evented()` 构造器写 runtime 字段（默认 io = threaded）；`io.poll()` 排空根回收队列（作用域退出提升的未 join 线程运行到完成并返回计数；threaded 恒 0）——interp-only（原生构造器未实现，编译模式响亮拒绝）；设计保留：`Io.threaded()` = 阻塞 IO + 每操作线程（简单，默认），`Io.evented()` = **单线程事件循环**（select/epoll 式非阻塞 IO + async/await 协作调度，配合协作取消），两者同一 `Io` 接口（接口工厂 R-4）
- ✅ **原子操作（组 F，2026-08-18 落地；Q-S3 定案）**：`@atomicLoad(T, p, order)` / `@atomicStore(T, p, v, order)` / `@atomicRmw(T, p, op, v, order)`——无锁原语；内存序 `relaxed`/`acquire`/`release`/`acq_rel`/`seq_cst`（默认 seq_cst）。**协作式透明实现**：单线程无竞争 → load = deref、store = 写穿指针、Rmw op = `.add/.sub/.exchange`（返回旧值），内存序五值求值后丢弃；类型参数 `T` 为编译期类型名（不对运行时求值）。四模式类型内部实现基于这些原语（单写者路径可免原子）；真并发硬件语义归 1.x
