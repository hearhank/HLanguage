# 后端降级必须响亮失败：未覆盖构造一律 error.Unsupported / error.NotBuiltin

**覆盖范围以 ADR-0004 分层明示为准**（2026-08-18 修正）：

- **`hc run --ir` / 字节码 VM（`hc run <file.hbc>`）**：全语言（含 G1-G5 标准库），与 tree-walking 同面；不在此列。
- **`hc build`（LLVM 原生）**：未全标准库——`compile mismatch ≤ 60` 边界（ADR-0004）。对原生后端未覆盖的构造，降级**不静默丢弃**：
  - 编译期（`ir::lower`）：一律返回 `error.Unsupported`，带行列 + 「请用默认 tree-walking 模式」提示，进程非零退出；
  - 运行期：未实现的原生内建/方法以 `error.NotBuiltin` / `error.NoMethod` / `error.Unsupported` 响亮中止。

IR 运行期仅剩的 `error.Unsupported` 为**个别需要类型布局表的内建**（Phase 7 取舍）：`Class.to_bytes`、`@offsetOf`（堆类型请用 `to_json` / `@sizeOf` 已在位）。

背景：早期存在 P0 缺陷——子集外构造被静默降级为 `void` 占位 / 丢语句，产生「看似成功实则错」的产物。本决策规定降级**不静默丢弃**：宁可报错，不可误编译。tree-walking 默认路径零改动。

> 历史注记：IR 参考解释器曾只覆盖 M3.1–Phase 6 子集（for/switch/break/continue/defer/errdefer、闭包/集合/指针/字段/索引/解构/取地址/函数引用/块表达式、实例方法调用、区间糖、全局/常量声明等均在子集外）。2026-08-18 IR 已随 Phase 1-8 + G1-G5 升格全语言，本策略的适用面随之收窄到原生后端。
