# 第四阶段工作集：自举闭环 + 1.x 延迟项

> **阶段定义（2026-08-22 用户定）**：第四阶段 = **自举闭环**（E7 / K5–K6）+ **1.x 延迟项**。**不属第三阶段**。
>
> 本文件夹为第四阶段的工作集。自举是 H 语言从「宿主语言（Rust）实现」过渡到「自托管（H 编译 H）」的里程碑。

## 范围

- **第四阶段核心**：自举闭环（`stage1/` H 版 lexer 已就绪，parser / 语义 / 后端 / stage2 / 可复现构建）
- **1.x 延迟项**：真 OS 并行 / mutex / freestanding / 位域 / 指针算术 / 数据库 / 压缩 / 时区 / 惰性迭代 / 绑定级只读 / bignum / asm / Atomic 类型等
- **端到端示例（A8）**：四大支柱合一验收示例（TCP 聊天室），设计已定案
- **LLVM 原生内建（C8）**：mismatch 归零（52–57 mismatch），用户裁定 P11d 收束，重开需授权

## 工作文档（本文件夹核心）

| 文件 | 内容 |
|---|---|
| `01-bootstrap-plan.md` | **自举计划**（E7 渐进路线：K1–K6 任务分解 + 验收标准 + 当前状态） |
| `02-1x-delayed-items.md` | **1.x 延迟项清单**（第三阶段不做的功能，逐个登记出处与条件） |

## 关联文档

- 第三阶段工作集：[`docs/SPEC/phase3/README.md`](../phase3/README.md)
- 原始自举计划：[`docs/SPEC/phase1/07-bootstrap-plan.md`](../phase1/07-bootstrap-plan.md) §五 E7
- 第三块执行细表（自举原始 K 组）：[`docs/SPEC/phase3/10-part3-execution.md`](../phase3/10-part3-execution.md)

## 当前状态（2026-08-24）

### 已完成（从 1.x 升级）
- ✅ **E4 真 OS 并行 + mutex + 单写者无锁快路径**：spawn 使用 OS 线程 + `Mutex` 类型 + `Pipe` 使用 mpsc 无锁 SPSC
- ✅ **E2 协程+通道模型**：M:N 协程调度器 + `chan<T>` 类型（send/recv/try_send/try_recv/close），替代四模式容器
- ✅ **A3 通用压缩算法**：LZ77 滑动窗口压缩
- ✅ **A4 时区完整**：`io.time.components`/`format`/`local_offset`
- ✅ **A6 标准库数据结构**：bitmap / ringbuf / pagemem / intrlist / treemap
- ✅ **A7 惰性迭代器**：`iter()` 返回 LazyIter，`filter`/`map` 链式延迟

### 自举进度（2026-08-29 更新）
- **K1 H 版 lexer** ✅：`stage1/lexer.hc`，6621 token 零 diff
- **K2 H 版 parser** ✅：性能已优化（解析自身 ~1s，较原 60s+ 提升 ~60x，8 项语料对照通过）
- **K3 H 版语义** ✅：`stage1/checker.hc`（11/11 任务，13 项对照测试全部通过）；自检 stage1 三源不崩溃 ✅（2026-08-29 修复 NoField 崩溃，详见 `04-execution-status.md`）
- **K3.5 误报收敛** 🟡：checker 类/方法/字段建模不全导致的自检误报待收敛（见 `05-k4-execution-plan.md`）
- **K4–K6** 🔴：待实现（K4 详细任务分解见 `05-k4-execution-plan.md`）

### 1.x 延迟项（剩余）
- ⏳ 包注册中心正式版
- ⏳ 真 OS 进程/共享内存（跨进程）
- ⏳ 数据库连接抽象
- ⏳ 绑定级只读（C4）
- ⏳ LLVM 原生内建子集扩展（C8，剩余 14 mismatch 收束待授权）
- ⏳ 自举（K4–K6）