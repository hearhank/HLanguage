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

- 第三阶段工作集：[`docs/phase3/README.md`](../phase3/README.md)
- 原始自举计划：[`docs/phase1/07-bootstrap-plan.md`](../phase1/07-bootstrap-plan.md) §五 E7
- 第三块执行细表（自举原始 K 组）：[`docs/phase3/10-part3-execution.md`](../phase3/10-part3-execution.md)

## 当前状态（2026-08-22）

- `stage1/lexer.hc`（K1 H 版 lexer）**已完成**（git `12e8406`，自身源码 6621 token 零 diff + 对照语料全绿）
- K2–K6：未动工
- 1.x 项：全部未动工