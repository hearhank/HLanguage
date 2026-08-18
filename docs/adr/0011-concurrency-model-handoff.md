# 并发模型衔接定案：async/Future 走协作式；四模式与 @atomic 延迟至 1.x

> **逆转注记（2026-08-18，本块内）**：本 ADR 原定「四模式类型与 `@atomic*` 延迟 1.x」。用户指令「完成并发和异步」**主动逆转**该裁决——**组 F（E2.1 四模式 + E2.4 原子）已在第三块内落地**，无需真 OS 线程：协作式单线程确定性模型下，四模式容器四变体（OneToOne/OneToMany/ManyToOne/ManyToMany）**运行时行为相同**（读者/写者数量为类型层契约，不引入真锁/真并发）；`@atomic*` 的 C11 内存序无竞争 → **透明实现**（load = deref、store = 写穿、Rmw = add/sub/exchange 返回旧值，内存序求值后丢弃）。示例 37/76/77/78 转绿（interpret 147/0/1）。**真 OS 并行与 `mutex` 仍归 1.x**。本 ADR 的 async/Future 协作式决策与「确定性承诺不变」不受影响。

> 2026-08-18 定案（第三块前置裁决 A1）。关联：[ADR-0007 多线程模型](0007-threading-model.md)、[ADR-0008 async/await](0008-async-await.md)、[ADR-0004 双模式架构](0004-dual-mode-architecture.md)、[06-10-concurrency.md](../SPEC/06-10-concurrency.md)、[07-bootstrap-plan.md](../SPEC/07-bootstrap-plan.md)、执行细表 [10-part3-execution.md](../SPEC/10-part3-execution.md)。

## 背景

- **ADR-0007/0008（2026-08-13）**：四模式共享容器（`OneToOne/OneToMany/ManyToOne/ManyToMany`）以**真 OS 线程**为预设——「单写者无锁路径」「send 满时阻塞」；async/await 提交任务「即生成线程」（`Io.Threaded` 默认）；`@atomic*`（Q-S3）= C11 五内存序无锁原语，且是四模式内部实现基础
- **组 G（2026-08-17，第二部分）**：落地 E2.2 线程生命周期时经用户裁决采用**协作式延迟执行**（确定性、单线程）——spawn 不并发运行，join/detach/程序结束时才执行到完成；一致性套件（ADR-0004 唯一语义源）要求 interp == IR 对同一程序 PASS/FAIL 完全一致
- **张力**：四模式/@atomic 的「无锁路径」「阻塞 send」「C11 内存序」在协作式单线程模型下**无实际并发对象**——语义无法落地为真并发行为；而引入真 OS 线程将破坏协作式的确定性，与「没有隐藏控制」（ADR-0007 选择理由）和一致性套件的可比性冲突

## 决策

1. **`async`/`await`（E2.3）在协作式模型上落地（本块内）**：
   - `async fn f(...) R` 返回 `Future(R)`（R = 完整返回类型含错误 union，Q20）；`await` ≡ `join()`，任何函数可用（Q19，无 async 传染）
   - **执行模型 = 协作式延迟任务**：`Future` 复用组 G 的 `Thread` 机制（`{fn, args, alloc, cancel, done, detached, result}` 类名分派 + 每线程 alloc），不引入 OS 线程；await 时运行到完成（确定性）
   - 协作式取消沿用组 G：cancel 置协作标志，await/detach/程序结束为运行点
   - `Io.evented()`（单线程事件循环 + 非阻塞 IO + 协作调度）本块内实现；`Io.threaded()`（真线程）随四模式延迟
2. **四模式类型与 `@atomic*`（E2.1/E2.4）——原延迟 1.x，已于 2026-08-18 逆转落地（组 F）**：
   - ~~四模式容器的「单写者无锁路径」「send 满阻塞」「多写者互斥」与 `@atomicLoad/Store/Rmw` 的 C11 内存序**需要真并发硬件**才有语义；协作式模型下实现它们 = 摆设（无并发对象）或推翻协作式（破确定性）~~ → **逆转（2026-08-18）**：协作式模型下**透明落地**——单线程确定性下四变体运行时行为一致（读者/写者数量 = 类型层契约），`send 满 → error.ChannelFull`（有界通道）、`read 空 → error.Empty`、`close 后 write → error.Closed`、`try_read 空 → null`；`@atomic*` load/store/rmw 透明（deref/写穿/add·sub·exchange），内存序求值后丢弃。不引入真锁/真 OS 线程，**确定性承诺与一致性套件可比性保持**
   - 延迟理由（原裁决，保留供 1.x 真并发评估）：① 一致性套件可比性——真线程使 interp/IR 同一程序的执行非确定、不可比，直接违反 ADR-0004；② C11 语义需真实内存模型；③ 本块重点（script/comptime/异步/标准库/自举前奏）不依赖真并发；④ 与用户划定的「H 编译 H 前」范围边界一致
   - 06-10 中四模式/@atomic/`Io.threaded()` 内容已从「1.x」改为「组 F 已落地」；**真 OS 并行与 `mutex` 仍标注 1.x**（ADR-0007 不被推翻，真并发排期后移）
3. **确定性承诺不变**：本块所有并发构造（Thread/Future/async）继续协作式延迟执行，三后端一致；真 OS 并行归 1.x（届时再评估与一致性套件的关系）

## 影响

- **执行细表**：组 E（E2.3 异步）按协作式排程实施；**组 F（E2.1 四模式 + E2.4 原子）于 2026-08-18 逆转落地（见上方注记）**，10-part3 §3 标注已完成；示例 37/76/77/78 转绿（interpret 147/0/1）
- **06-10-concurrency.md**：async/Future 标注「本块协作式落地」；四模式/@atomic 标注「组 F 已落地（透明实现）」；真 OS 并行与 `mutex` 标注「1.x」
- **07-bootstrap-plan.md**：E2.1/E2.4 状态表标注本块落地；E2.3 标注本块
- **语义层（组 F 已实施）**：四模式容器 = `Value::Class`/`IrValue::Class` + 类名分派（`queue`/`closed`/`alloc`/`cap` fields）；`@atomic*` 三内建（interp + IR + 语义层 `is_builtin_type`/`call_at_builtin`）
- **未变**：组 G 已落地的 Thread 生命周期与捕获规则；ADR-0007 的捕获/所有权设计本身；双模式一致性承诺；**真 OS 并行归 1.x**

## 取舍

> 本节为 2026-08-18 定案时的取舍记录；**「四模式延迟」一项已于同日被组 F 落地逆转**（协作式透明实现，见顶部逆转注记）——「本块即上真线程」仍不采纳（真并行归 1.x），四模式/@atomic 改为协作式透明语义落地，代价从「能力不可用」变为「语义为类型层契约、无真并发行为」。

- 选择「协作式 async + 四模式延迟」而非「本块即上真线程」：保住确定性模型与一致性套件可比性（ADR-0004 承诺的根基），把真并发放到有真实内存模型需求的 1.x；~~代价是 76–80 示例与四模式/@atomic 能力在 1.x 前不可用~~ → 组 F 已以透明语义落地（示例 37/76/77/78 转绿），真 OS 并行仍 1.x
- 选择「async 复用 Thread 机制」而非另起协程调度器：Go 式 M:N 协程（评审 B2 方向）是 1.x 真并发时的可选演进，本块不预支——协作式下无线程爆炸问题，无需 M:N
- 未推翻 ADR-0007/0008：四模式、捕获规则、async 关键字逆转均为既定设计，仅执行排期调整（本块做协作式子集、真并发归 1.x）
