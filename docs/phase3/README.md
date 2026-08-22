# 第三阶段工作集：标准库扩展 + 前两阶段未实现功能

> **阶段定义（2026-08-22 用户定）**：第三阶段 = **标准库扩展** + **前两个阶段未实现的功能**的集合。**自举（E7 / K5–K6）移到第四阶段**，不属本阶段。
>
> 本文件夹为第三阶段的**工作集**：新工作文档（未实现功能清单 + 通用语法规则）为实施基础；规范副本为参考（权威规范仍以 `docs/SPEC/` 为准）。
>
> 实施规则：所有修改同步更新到 SPEC；每功能点 ≤2h，超出即分解；每子任务完成即提交（「先提交再继续」，沿用 `10-part3-execution.md` §0 规则）。

## 范围

- **第三阶段**：标准库扩展（net / ipc / storage / text / ffi 剩余项）+ 前两阶段未实现功能（工具链：lint / lsp 完整 / 注册中心；语言扩展：惰性迭代 / switch 守卫 / Send/Sync / Table 多索引 / 绑定级只读等；测试：并发测试 runner；系统编程：K3 asm 等）
- **第四阶段（不属本阶段）**：自举闭环（`stage1/` H 版 lexer 已就绪，parser / 语义 / 后端 / stage2 / 可复现构建）
- **1.x 延迟项**：真 OS 并行 / mutex / freestanding / 位域 / 指针算术等（见 `01-unimplemented-features.md` 附录）

## 工作文档（本文件夹核心）

| 文件 | 内容 |
|---|---|
| `01-unimplemented-features.md` | **未实现功能清单**（按条目排列，含状态 / 出处 / 落点）——第三阶段实施 backlog |
| `02-syntax-rules.md` | **通用语法规则**（基本语法功能整合成统一语法规则）——实施与对照参考 |

## 参考文档（存档副本）

| 文件 | 内容 |
|---|---|
| `04-stdlib-scope.md` | 标准库扩展明细（net / ipc / storage / text / ffi + 系统编程扩展） |
| `05-open-questions-and-risks.md` | 开放问题 / 系统编程缺口（J1 裁决依据） |
| `06-09-meta.md` | 元编程规范（script / comptime，已实现部分） |
| `06-10-concurrency.md` | 并发规范（异步 / 四模式 / 原子，已实现部分） |
| `10-part3-execution.md` | 第三块执行细表（组 A–G 已完成记录 + I/J/K 待办 + 完成注记） |

## 关联决策记录（ADR）

- 0011 并发模型衔接（协作式，四模式逆转落地）/ 0012 comptime 类型即值 / 0013 script 块语义 / 0014 系统编程范围（K1–K5 纳入、K6 延迟）
- 0015 Table 密封表 + 绑定级只读未实现（[ADR-0015](../adr/0015-table-sealed-init_with.md)，2026-08-22 设计会话产出，见 `01-unimplemented-features.md` C1 / C4 条目）

## 当前状态基线（2026-08-22）

- 示例回归：interpret **147/0/1**；compile **52–57 mismatch**（原生内建子集边界）
- 一致性套件：104 用例（interp == IR）
- 已实现（第三块组 A–G / H1–H4 / I1 / K1）：script 块、comptime 完整、异步、四模式 / @atomic、标准库 G1–G5、系统编程 H1–H4、`hc fmt`、H 版 lexer
