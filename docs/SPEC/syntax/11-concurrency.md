# 11 并发

> 大模块：并发 | 对齐状态：**✅ 对齐完成（2026-08-30，无待裁决项；核对注随文）** | 初稿：2026-08-30
>
> 事实基础：**ADR-0024**（M:N 协程 + 单一 chan\<T\>，2026-08-24，现行权威）、ADR-0011（协作式模型，**已被取代**）、ADR-0017 C3-3（Send/Sync）、ADR-0019（C7 原生闭包 ABI）、ADR-0028（通道状态拆分）、定案 Q8/Q14/Q18/Q19/Q32/Q-S3/Q-R12、历史 `06-10-concurrency.md`（已废弃，其「协作式延迟执行」口径过时）、tag1 实现（`ir/builtin.rs` spawn/sync、`ir/method.rs` 线程与四模式、`ir/models/channel_state_ir.rs`）。
> 证据总库：`tag1/hc/tests/async_future.rs`、`tag1/hc/tests/send_sync.rs`、`tag1/hc/tests/thread_capture.rs`。

## 11.1 并发模型：M:N 协程（ADR-0024 定案，已实现）

- 规则：
  - **M:N 调度**：N 个协程（G）多路复用到 M 个 OS 线程（worker 线程池）——`spawn` 提交协程任务到调度器（`ctx.scheduler.submit`，启动 `scheduler.start()`）；**真并行执行**。
  - 调度器在 Rust 运行时（hc-rt）实现，解释器与 LLVM 后端共用；初始**协作式调度**（通道操作为调度点，无抢占；work-stealing 后续可加）；`GOMAXPROCS` 控制 M（默认 = CPU 核数）——⚠️ 环境变量接线待核对。
  - ❌ **ADR-0011「协作式延迟执行（单线程确定性）」模型整体被取代**——历史 06-10 头部口径（spawn 不并发运行、join 才执行）不再成立；冻结窗口等借用期规则仍有效（§11.3）。
- 状态：✅ 已实现（GOMAXPROCS ⚠️ 核对注）
- 证据：`ir/builtin.rs` `call_spawn_builtin` L3715-3763（scheduler.start + submit 协程任务）；ADR-0024 决策表；`ir/method.rs` join 轮询注释

## 11.2 spawn 与线程生命周期

- 规则：
  - `spawn(f, args...)` → **`owned Thread<T>`**（T = f 返回类型）——f 为函数引用；普通调用形态解析（`02` §2.1 前缀层）。
  - 方法集（2026-08-14 定案 + 组 G 落地）：`join() !T`（消耗所有权，等待完成取结果；错误以 error union 跨线程传播）/ `cancel() !void`（置取消标志 → 任务内 `error.Cancelled`）/ `is_done() bool` / `detach()`。
  - **线程所有权**：spawn 归当前作用域；作用域退出时已完成 → 销毁、运行中 → **移交根作用域**（无隐式阻塞）；根作用域 = 程序最后退出场所，负责最终回收（评审 A7）；`io.poll()` 排空提升队列（threaded 恒 0）。
  - 每线程 alloc（Q8）。
- 状态：✅ 已实现
- 证据：`ir/method.rs` `call_thread_method_ir`（join/cancel/detach/is_done）；`ir/builtin.rs` L3695-3699（共享状态）；历史 06-10 组 G 条目

## 11.3 线程捕获规则（Q18 + Q19）

- 规则：
  - **捕获**：值类型复制值；引用类型 move 或 global；**作用域例外（Q18）**——句柄在声明作用域内被 join/await（绑定）时可捕获引用；**逃逸线程引用捕获禁用**（detach 或退出未 join → 编译错误）。
  - **绑定/逃逸判定（数据流）**：句柄被 join/await → 绑定；被 detach 或作用域退出未 join → 逃逸。
  - **冻结窗口（Q19）**：绑定场景下，被捕获引用的目标从 spawn 到 await/join 之间主线程**不可写**（不可 `var mut` 写入、不可取 `&mut`）——编译期检查，await/join 后恢复；并发写共享数据 → 显式用 chan/Mutex。
  - **与 Send/Sync 正交**：Q19 管借用期，Send/Sync 管类型层可传递性。
- 状态：✅ 已实现
- 证据：`tag1/hc/tests/thread_capture.rs`；历史 06-10 组 G3 条目

## 11.4 Send / Sync（内建标记接口，ADR-0017 C3-3）

- 规则：
  - `Send`/`Sync` = **内建标记接口**（编译器内建实现，不可用户自定义）；**可推导**：标量/值类型自动 Send+Sync；指针/切片看指向类型；内建容器看元素/负载；用户 `class Foo: Send` 由编译器验证字段全满足（含 `*mut` 或可变共享 → 非 Sync）。
  - **诊断**：`spawn`/`await` 边界捕获非 `Send` 引用 → 编译错误带位置（`captured value of type X is not Send at spawn boundary`）。
  - 真并行运行时检查 1.x（当前零运行时开销）。
- 状态：✅ 已实现
- 证据：`tag1/hc/tests/send_sync.rs`；ADR-0017 C3-3

## 11.5 chan\<T\> 通道（ADR-0024：单一通道类型）

- 规则：
  - **`chan<T>` = 唯一通道类型**（替代四模式，§11.6）：`chan<T>.init(alloc)`（无缓冲）/ `chan<T>.init(alloc, cap)`（有界）。
  - API：`send(v)`（缓冲满阻塞）/ `recv() T`（空阻塞）/ `try_send(v) bool`（非阻塞）/ `try_recv() ?T`（非阻塞）/ `close()`。
  - 运行时 = Mutex + Condvar 有界队列（真阻塞；closed 标志）；close 后 send → 错误、recv 语义按实现。
- 状态：✅ 已实现
- 证据：`ir/builtin.rs` L3325-3333（`ChanStateIr`：Mutex + send_cond/recv_cond + capacity）；`call_sync_method_ir` L3007-3024（send/recv/try_send/try_recv/close）；ADR-0024 API 表

```hc
var ch: owned *mut Chan<i32> = Chan<i32>.init(alloc, 8);
ch.send(42);
var v: i32 = ch.recv();
ch.close();
```

## 11.6 四模式容器（❌ 弃用，过渡期可用）

- 规则：
  - `Pipe<T>`（单读单写）/ `Tee<T>`（单读多写）/ `Funnel<T>`（多读单写）/ `Hub<T>`（多读多写）——**已弃用，推荐迁移 `chan<T>`**（ADR-0024）；过渡期仍可解析运行。
  - 方法集（历史 Q14/Q-R12）：`init(alloc[, cap])` / `write(v)` / `read() T` / `try_read() ?T` / `close()` / `send(v)` / `recv() T`；close 后 write/send → `error.Closed`、send 满 → `error.ChannelFull`。
  - 实现现状：Pipe 一对一路径；Tee/Funnel/Hub 已升级为 **Mutex+Condvar 真并行队列**（E4——历史「协作式四变体一致」口径过时）。
  - 方法取 `*Self`（Q32 内建共享特例：并发安全由类型保证，用户类型不可模拟）。
- 状态：⚠️ 弃用保留（迁移目标 chan\<T\>；删除时间随 ADR-0024 迁移计划）
- 证据：`ir/method.rs` `call_four_mode_method_ir` L635-659（Tee/Funnel/Hub → Mutex+Condvar）；ADR-0024 旧类型映射表；`00-feature-inventory` §6.3

## 11.7 Mutex

- 规则：`Mutex.init(v)` 包装初始值（`Arc<std::sync::Mutex>` 真锁）；方法 `lock() T`（毒化 → `MutexPoisoned`）/ `try_lock() ?T`（非阻塞）。
- 状态：✅ 已实现
- 证据：`ir/builtin.rs` L3298-3311（Mutex.init）；`call_sync_method_ir` L3014-3024（lock/try_lock）

## 11.8 @atomic 原子操作（Q-S3 定案）

- 规则：`@atomicLoad(T, p, order)` / `@atomicStore(T, p, v, order)` / `@atomicRmw(T, p, op, v, order)`——无锁原语；内存序 `relaxed`/`acquire`/`release`/`acq_rel`/`seq_cst`（默认 seq_cst）；首参 T = 编译期类型名（不对运行时求值）。详见 `13-builtins.md`（全集归属）。
- 状态：✅ 已实现
- 证据：`ir/builtin.rs` `is_type_arg_pos`（@atomic* 首参类型名注释）；历史 06-10 组 F 条目

## 11.9 async / await 与 Future（组 E）

- 规则：
  - `async fn` 声明（`05` §5.9）——调用点返回 `Future<R>`（R = 完整返回类型，含错误联合 `Future<!R>`）。
  - **`await f` ≡ `f.join()`**（组 E 落地）且**任何函数可用**（Q19——无 async 传染）；await 幂等缓存；协作式取消（cancel → `error.Cancelled`）、is_done 状态转移。
  - Future 复用线程机制（`make_future`/`future_run`）——在 M:N 调度器下的执行形态 ⚠️ 核对注（历史协作式 Future → 现调度器承接）。
- 状态：✅ 已实现（执行形态 ⚠️ 核对注）
- 证据：`parser/expr.rs` L199-204（await 前缀）；`tag1/hc/tests/async_future.rs`；历史 06-10 组 E 条目

## 11.10 Io 执行模型（组 E E3，R-4 工厂）

- 规则：`Io.threaded()`（阻塞 IO + 每操作线程，默认）/ `Io.evented()`（单线程事件循环，select/epoll 式非阻塞 IO + async/await 协作调度）——同一 `Io` 接口的两个具体实现（接口工厂 R-4，`06` §6.3）；构造器写 runtime 字段（默认 threaded）；`io.poll()` 排空根回收队列。
- 状态：⚠️ interp ✅；**原生构造器未实现，编译模式响亮拒绝**（C7/Phase 8 边界，G4b 定案 A——不静默误编译）
- 证据：历史 06-10 组 E3 条目；ADR-0019（原生 ABI 定案）

## 11.11 变更记录（相对旧 06-10-concurrency.md）

| 变更 | 依据 |
|---|---|
| **并发模型权威切换：ADR-0024 M:N 协程 + 真并行**（spawn = 调度器协程任务）；ADR-0011 协作式延迟执行 ❌ 整体取代；旧文档「spawn 不并发运行、join 才执行」口径废除 | ADR-0024 + `call_spawn_builtin` L3715-3763 |
| **chan\<T\> 单一通道类型 ✅**（send/recv/try_send/try_recv/close；Mutex+Condvar 真阻塞） | ADR-0024 + `ChanStateIr` |
| 四模式容器 ❌ 弃用（过渡期可用；Tee/Funnel/Hub 已升级真并行队列） | ADR-0024 + `call_four_mode_method_ir` L635-659 |
| Mutex ✅ 收口（真锁 Arc\<std::sync::Mutex\>；旧「mutex 延迟 1.x」口径废除） | `Mutex.init` + `call_sync_method_ir` |
| M:N 调度器 ✅（worker 线程池 + submit；GOMAXPROCS ⚠️ 核对注） | `scheduler.start/submit` |
| `spawn(f, args...) owned Thread<T>` 签名维持；cancel 语义 = 协作取消标志 | 组 G + `call_thread_method_ir` |
| Io.threaded/evented 原生侧边界维持响亮拒绝（Phase 8） | ADR-0019 + G4b 定案 A |

## 11.12 待裁决清单

无——ADR-0024 决策与 E4 实现一致，直接对齐。
