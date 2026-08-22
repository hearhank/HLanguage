# 第一阶段存档：语言系统（已实现）

> 对应第三块划分中的**第一块**（`07-bootstrap-plan.md` §二：M0 地基 / M1 前端 / M2 语义 / M3 双后端 / M4 运行时与语言内建）。本文件夹为**已实现文档的存档副本**，权威规范仍以 `docs/SPEC/` 为准（交叉引用不破）。
>
> 存档时间：2026-08-22（阶段重组：第三阶段 = 标准库 + 前两阶段未实现功能；第四阶段 = 自举）。

## 交付摘要

第一阶段（第一块语言系统）已达成 T1–T3（`07-bootstrap-plan.md` §六）：

- **T1（前端 + 语义完整）**：lexer / parser / AST / 诊断 / 名称解析 / 完整类型检查 / 所有权 / 错误集 / 函数（重载 / 闭包 / 捕获精确化）
- **T2（双后端可运行）**：共享 IR（`ir.rs`）+ 字节码 VM（HBC2）+ LLVM 原生（emit-.ll + zig cc），interp == IR 一致性
- **T3（语言系统完整）**：语言包可用——语法 / 语义 / 双后端 / 运行时 / 内建全部就绪

## 文档索引（存档副本）

| 文件 | 内容 |
|---|---|
| `01-language-design.md` | 语言设计总纲（§12 二十六项语言特性定义） |
| `02-milestones.md` | 1.0 里程碑（M0–M10）与特性映射 |
| `06-language-spec.md` | 语言规范总表 |
| `06-01-syntax.md` | 词法 / 声明 / 运算符 / 语句与控制流 / 测试 |
| `06-02-types.md` | 基础类型（标量、切片、可选、错误联合、指针） |
| `06-03-extended-types.md` | 扩展类型（class、枚举、元组、Table、String、tree） |
| `06-04-functions.md` | 函数与闭包、内建函数（box / copy / @） |
| `06-05-interfaces.md` | 接口、标量接口族、迭代契约、序列化内建 |
| `06-06-ownership.md` | 所有权与内存模型 |
| `06-07-errors.md` | 错误处理（error union、错误码、@panic） |
| `06-13-project-structure.md` | 项目结构 |
| `07-bootstrap-plan.md` | 三块实现计划（第一/二/三块 + 自举） |
| `08-mem-allocator-design.md` | 内存分配器设计 |

## 关联决策记录（ADR）

ADR 统一存放于 `docs/adr/`（本阶段相关：0001–0009）：

- 0001 引用策略 / 0002 编译器以 Rust 实现 / 0003 内存模型 / 0004 双模式架构（共享 IR 唯一语义源）/ 0005 所有权语法 / 0006 类型系统与脚本生成 / 0007 线程模型 / 0008 async-await / 0009 函数重载

## 实施落点（代码）

- `tag1/hc/`：前端（lexer / parser / ast / semantic / comptime）+ 后端（ir / bytecode / llvm）+ 错误码表
- `tag1/hc-rt/`：运行时（interp / value / io / net / fs / collections / async / errors / 一致性套件）
- `tag1/hc-tools/`：CLI（`hc run/test/check/errors/build/init/pkg/doc/fmt/lex`）+ 工具（scriptgen / comptimegen / fmtgen / buildzon）
