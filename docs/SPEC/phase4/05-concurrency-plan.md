# 真 OS 并行 + Mutex + 单写者无锁快路径 实现计划

> 对应 `02-1x-delayed-items.md` E4 项。当前协作式透明实现已落地（单线程延迟执行），
> 此计划将其升级为真 OS 级并行。

## 背景

当前 `spawn` 使用协作式延迟执行：`spawn` 立即返回句柄，`join` 才在同一线程同步执行。
四模式容器（OneToOne/OneToMany/ManyToOne/ManyToMany）使用 `Vec<Rc<RefCell<...>>>` 队列，
空读返回错误而非阻塞。

## 实现策略

`Value` 使用 `Rc<RefCell<...>>` 非线程安全。使用 `unsafe impl Send` 配合深复制确保
线程安全：spawn 时深复制参数到新线程，每个线程独立操作自己的值副本。Mutex 使用
`Arc<std::sync::Mutex<Value>>`（Send+Sync）提供跨线程共享。

---

## ✅ Task 1: 添加 `unsafe impl Send` 使跨线程传值合法

| 属性 | 值 |
|------|-----|
| 状态 | ✅ 已完成 |
| 文件 | `tag1/hc-rt/src/value.rs` + `tag1/hc-tools/hc-rt/src/value.rs` |
| 验证 | 编译通过，`cargo test --workspace` 全绿 |

**改动**：为 `Value`、`ClassData`、`ClosureData`、`LazyOp`、`LazyIterData`、`ArenaState`、
`BoxedData`、`VecData`、`MapData`、`AllocatorImpl`、`PoolState` 添加 `unsafe impl Send`。

**安全论证**：每个值实例在任一时刻只被一个线程访问。spawn 时深复制值到新线程，
原始线程和子线程操作各自副本，无数据竞争。

---

## ✅ Task 2: Interpreter `spawn` 使用 OS 线程

| 属性 | 值 |
|------|-----|
| 状态 | ✅ 已完成 |
| 文件 | `tag1/hc-rt/src/interp/call.rs` + `mod.rs` + `loader.rs` + `io.rs` |
| 验证 | `hc-rt/tests/thread.rs` 7 测试全绿，`cargo test --workspace` 全绿 |

**改动**：
- `spawn` 立即用 `std::thread::spawn` 启动 OS 线程
- 线程函数：创建新 `Interp` → 加载程序 → 调用函数 → 结果通过 `Arc<Mutex<Option<ThreadResult>>>` 传回
- `Thread` 值存储 `_tid`，通过 `thread_handles: HashMap<i64, ThreadState>` 管理
- `join()` 等待线程结束，返回结果
- `is_done()` 检查 `ThreadState.done` 或 Thread 类字段
- `cancel()` 设置 `AtomicBool` 取消标志，线程启动时检查
- `detach()` 标记分离（程序结束时不等待）

**注意**：每个线程创建独立 `Interp` 实例，全局变量不跨线程共享。

---

## ✅ Task 3: IR 后端 `spawn` 使用 OS 线程

| 属性 | 值 |
|------|-----|
| 状态 | ✅ 已完成 |
| 文件 | `tag1/hc/src/ir/mod.rs` + `tag1/hc/src/ir/builtin.rs` + `tag1/hc/src/ir/method.rs` + `tag1/hc/tests/ir.rs` |
| 验证 | `cargo test --workspace` 全绿 |

**改动**：
- `Ctx` 添加 `thread_handles: HashMap<i64, ThreadStateIr>`、`next_tid: i64`、`module: Option<Arc<IrModule>>`
- `spawn` 立即用 `std::thread::spawn` 启动 OS 线程
- 线程函数：创建新 `IrRuntime` → `init(&module)` → 调用函数 → 结果通过 `Arc<Mutex<Option<ThreadResultIr>>>` 传回
- `join()` 等待线程结束，返回结果
- `is_done()` 检查 `ThreadStateIr.done` AtomicBool
- `cancel()` 设置 `AtomicBool` 取消标志，线程启动时检查
- `detach()` 丢弃 join 句柄（线程继续运行），标记分离
- 每线程独立 Arena 实例（在线程的 Ctx 内创建，避免 cell 索引跨 Ctx 传递）

---

## Task 4: Mutex 类型 🔴

| 属性 | 值 |
|------|-----|
| 预估 | 50min |
| 文件 | `tag1/hc-rt/src/value.rs` + `tag1/hc-rt/src/interp/call.rs` + `tag1/hc-rt/src/interp/io.rs` + `tag1/hc/src/ir/builtin.rs` |
| 验证 | 新增 `hc-rt/tests/mutex.rs` 测试，核心行为覆盖 |

**API 设计**：
```hc
// 创建 Mutex
var m = Mutex.init(42);
// 加锁获取值（阻塞直到获得锁）
var v = try m.lock();
// 尝试加锁（非阻塞）
var v = try m.try_lock();  // ?T，None 表示已被锁定
// 解锁：超出作用域自动释放（Mutex 内部管理）
```

**实现**：
- `Value::Mutex(Arc<std::sync::Mutex<Value>>)` 新变体（Send+Sync）
- `Mutex.init(v)` 构造
- `.lock() -> !T` 阻塞获取锁，返回内部值的克隆
- `.try_lock() -> ?T` 非阻塞尝试
- `Mutex` 作为内建命名空间函数注册

---

## Task 5: 四模式容器单写者无锁快路径 🔴

| 属性 | 值 |
|------|-----|
| 预估 | 50min |
| 文件 | `tag1/hc-rt/src/interp/call.rs` + `tag1/hc/src/ir/builtin.rs` |
| 验证 | 新增 `hc-rt/tests/mutex.rs` 测试含通道场景 |

**改动**：
- `OneToOne`：使用 `std::sync::mpsc::channel()`（无锁 SPSC 队列）
- `OneToMany`：每个消费者独立 channel，写者广播
- `ManyToOne` / `ManyToMany`：使用 `Arc<Mutex<VecDeque<Value>>>` + `Condvar` 阻塞读
- `read()` 空时阻塞等待（而非返回错误）
- `try_read()` 空时返回 None（非阻塞）

---

## ✅ Task 6: 更新示例 + 文档

| 属性 | 值 |
|------|-----|
| 状态 | ✅ 已完成 |
| 文件 | `examples/04-concurrency/90-thread-lifecycle.hc` + `docs/` |
| 验证 | 示例回归测试通过 |

**改动**：
- 更新 `90-thread-lifecycle.hc` 注释和测试反映真并行语义
- 更新 `docs/SPEC/phase4/05-concurrency-plan.md` 状态

---

## 测试策略

- 每个 Task 完成后运行 `cargo test --workspace` 确保不破坏既有功能
- Task 2 完成后：`cargo test --package hc-rt --test thread` 7 测试全绿
- Task 4 完成后：`cargo test --package hc-rt --test mutex` 新增测试全绿
- 最终：`cargo test --workspace` 全绿 + 示例回归 147/0/1