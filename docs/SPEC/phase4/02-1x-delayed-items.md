# 第四阶段延迟项清单（1.x + 保留项）

> 来源：`docs/SPEC/phase3/01-unimplemented-features.md` 附录二及各 ⏳ 条目 + 用户裁定迁移项。2026-08-22 迁移至第四阶段。
>
> 这些功能在第三阶段（标准库扩展 + 前两阶段未实现功能）中明确标记为 1.x 或用户裁定移至第四阶段，不做实施。

## 端到端示例（第四阶段保留项）

| 编号 | 功能 | 原出处 | 备注 |
|---|---|---|---|
| A8 | 端到端示例程序（四大支柱同时使用） | `01-unimplemented-features.md` A8 | 设计已定案（TCP 聊天室，行文本协议，异步事件循环，双模式验证）；用户裁定移至第四阶段 |

## LLVM 原生内建（第四阶段保留项）

| 编号 | 功能 | 原出处 | 备注 |
|---|---|---|---|
| C8 | LLVM 原生内建子集扩展（mismatch 归零） | `01-unimplemented-features.md` C8 | 52–57 mismatch（21 Unsupported + 31 运行时）；用户裁定 P11d 收束，移至第四阶段；重开需授权 |

## 标准库 1.x 项

| 编号 | 功能 | 原出处 | 备注 |
|---|---|---|---|
| A2 | 数据库连接抽象 | `01-unimplemented-features.md` A2 | 依赖真实 DB 驱动；`io.storage` KV 已落地 |
| A3 | 通用压缩算法（gzip / zip） | `01-unimplemented-features.md` A3 | RLE 已落地 |
| A4 | 时区完整（tz 库） | `01-unimplemented-features.md` A4 | `io.time.tick/elapsed` 已落地 |
| A5 | 真 OS 进程 / 共享内存 | `01-unimplemented-features.md` A5 | 进程内 ipc 已落地；与 A1（FFI）联动 |
| A6 | 标准库缺口：bitmap / 侵入式链表 / 环形缓冲 / 树 / 页内存 | `01-unimplemented-features.md` A6 | 底层机器前提 K1/K2/K4/K5 已就绪 |
| A7 | 惰性 / 组合子迭代器 | `01-unimplemented-features.md` A7 | ✅ 2026-08-23 落地：`iter()` 返回 LazyIter，`filter`/`map` 链式延迟，`next()` 按需求值，`to_array()` 全量解析；filter+map 按链式顺序交错应用；`for` 循环兼容 |

## 工具链 1.x 项

| 编号 | 功能 | 原出处 | 备注 |
|---|---|---|---|
| B4 | 包管理器正式版 + 官方注册中心 | `01-unimplemented-features.md` B4 | M10 冻结前正式版；B3 为基础 |

## 语言扩展 1.x 项

| 编号 | 功能 | 原出处 | 备注 |
|---|---|---|---|
| C4 | 绑定级只读（默认只读，Rust 式） | `01-unimplemented-features.md` C4 | 语义层大迁移；密封表 `init_with` 已覆盖最需只读场景 |
| C6 | comptime_int 超大常量 bignum | `01-unimplemented-features.md` C6 | `Value::Int(i128)` 无 bignum |

## 系统编程 1.x 项

| 编号 | 功能 | 原出处 | 备注 |
|---|---|---|---|
| E1 | K3 内联汇编 asm | `01-unimplemented-features.md` E1 | 特权指令非本块范围 |
| E2 | K6 freestanding（裸机模式，H core） | `01-unimplemented-features.md` E2 | 无 OS / 无 libc / 无默认分配器 |
| E3 | K7–K11：裸 fn 指针 / 位域 / 指针算术 / `@byteSwap` / `Atomic<T>` | `01-unimplemented-features.md` E3 | 裸函数指针与 C7 联动 |
| E4 | 真 OS 并行 + `mutex` + 单写者无锁快路径（F1/F5） | `01-unimplemented-features.md` E4 | 协作式透明实现已落地 |

## Table 1.x 项

以下为 Table 设计会话（2026-08-22 Q35–Q57）中明确延迟到 1.x 的功能：

| 编号 | 功能 | 出处 |
|---|---|---|
| T1 | 行范围切片 `t[i..j]` | 设计会话 Q39 |
| T2 | 列视图 `t[:, j]` | 设计会话 Q40 |
| T3 | 行/列操作（插入/删除/交换） | 设计会话 Q25 |
| T4 | 栈分配 `Table<T>.init_stack()` | 设计会话 Q32 |
| T5 | `init_uninit` 跳过初始化 | 设计会话 Q38 |
| T6 | 格式化输出 `t.format()` | 设计会话 Q43 |
| T7 | `init_with` 回调返回错误 | 设计会话 Q31 |
| T8 | 子表切片 `t[i..j, k..l]` | 设计会话 Q14 |