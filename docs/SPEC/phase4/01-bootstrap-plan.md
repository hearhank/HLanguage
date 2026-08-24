# 自举计划（K1–K6：用 H 编译 H）

> 来源：`docs/SPEC/phase3/10-part3-execution.md` 组 K + `docs/SPEC/phase1/07-bootstrap-plan.md` §五 E7。
>
> 当前阶段：**第四阶段**（从 `docs/SPEC/phase3/` 迁移至此，2026-08-22）。

## 渐进路线

E7 渐进路线：H lexer → parser/AST → 语义（类型/所有权/错误集）→ 后端（IR/VM/LLVM）。**双实现对照**：与 Rust 版 token/AST/执行结果对比，差异即 bug。**Rust 参考实现长期保留**（自举失败风险对策，见 `docs/SPEC/phase3/05-open-questions-and-risks.md` 风险登记）。

## 任务分解

| # | 任务（行为面） | 验收 | 依赖 | 预估 |
|---|---|---|---|---|
| **K1** | ✅ **H 版 lexer**（.hc 源码 → token 流）+ 对照测试（Rust lexer 输出 diff） | H lexer 可跑 + 对照绿 | D（comptime 支撑编译期工具） | 2h |
| **K2** | H 版 parser/AST（token → 声明树）+ 对照测试 | H parser 可跑 + 对照绿 | K1 | 2h |
| **K3** | H 版语义（名称解析/类型检查/所有权/错误集）+ 对照测试 | H 语义可跑 + 对照绿 | K2 | 2h |
| **K4** | H 版后端：IR 参考解释器（跑 H 自身测试）+ 对照 | H 后端可跑 + 执行结果对照绿 | K3 | 2h |
| **K5** | 自举闭环 stage2：H 编译器（H 程序）用 stage1 编译自身；产物再编译产物（二次自举验证） | **用 H 编译 H 达成** | K4 | 2h |
| **K6** | 可复现构建 + 规范一致性（Rust/H 双实现交叉验证全语法/语义/内存/并发） | 一致性套件扩展绿 | K5 | 2h |

## 当前状态

### ✅ K1（H 版 lexer）已完成（2026-08-18）

`stage1/lexer.hc` 用 H 语言重写词法分析，与 Rust 参考（`tag1/hc/src/lexer.rs`，`hc lex` 转储）逐 token 对照，格式 `{start} {end} {line} {col} {kind:?}`。

**对照绿**：自身源码 6621 token 零 diff、对照语料 `stage1/corpus/*.hc` 9 文件全绿（关键字/数字前缀归一化/惰性宽度后缀/转义全套/错误路径/未知字符双 Error/未闭合注释/UTF-8 计列）、92 示例全绿、全部 61 个 Rust 源文件全绿。

**对照测试**：`hc-tools/tests/k1_lexer.rs`（语料 + 自身源码两个用例）。

**复刻的隐藏保真细节**：span=END 位置；consume-then-check（闭引号/转义/`\u{` 先消费再判定）；数字前缀字母与浮点指数归一化小写但数字位大小写保留（0XFF→"0xff"、1.5E2→"1.5e2"）；后缀含 CJK 时 Rust `suffix.len()`（字节）× bump（每字符）导致的过度消费（42i32中文 吞后续）；Debug 转义 `\0` + is_printable 近似表（探针实证 U+115F/3164/FFA0/FFFC/FFFD 可打印）；ident 延续 `is_alphanumeric`（CJK 表意文字 E4–E9 收、全角标点不收）。

**已知近似**：Unicode 空白仅 ASCII 六种、is_printable 与 CJK 扩展区为近似表。

**门禁**：`cargo test --workspace` 全绿 + `check-examples.sh` 基线不漂移。

### 🔴 K2–K6：未动工

## 详细功能清单

| 编号 | 功能 | 状态 | 出处 |
|---|---|---|---|
| Z1 | K2 H 版 parser / AST（token → 声明树）+ 对照测试 | 🔴 | 原 `10-part3-execution.md` 组 K |
| Z2 | K3 H 版语义（名称解析 / 类型 / 所有权 / 错误集）+ 对照 | 🔴 | 组 K |
| Z3 | K4 H 版后端（IR 参考解释器，跑 H 自身测试）+ 对照 | 🔴 | 组 K |
| Z4 | K5 自举闭环 stage2（H 编译 H；产物再编译产物） | 🔴 | 组 K；`07` E7.2 |
| Z5 | K6 可复现构建 + 规范一致性（Rust/H 双实现交叉验证） | 🔴 | 组 K；`07` E7.3 |
| Z6 | M9 语言规范定稿 + 一致性测试套件（规范 ↔ 实现） | 🔴 | `02-milestones.md` M9 |
| Z7 | M10 冻结与 1.0（语言冻结 / 规范定稿 / 包管理器正式版 / stdlib 冻结 / edition 演进） | 🔴 | `02-milestones.md` M10 |
| Z8 | 自举吃狗粮反馈（J5：E7 暴露缺口反馈回设计） | 🔴 | 组 J5 |

## 验收标准

- **第三块总验收**（`docs/phase1/07-bootstrap-plan.md` §五）：**`用 H 编译 H` 达成（stage2）**
- 可复现构建（同源码同结果）
- 规范一致性（Rust/H 双实现交叉验证）