# 共享 IR 为唯一语义源：四后端共用一个语义

延续主项目 [ADR-0004](../docs/adr/0004-dual-mode-architecture.md)（共享前端 → 共享 IR → 双后端）的定案，tag1 把「唯一语义源」落到 `IrModule` + `run_ir` 上，并扩展为**四后端**：

| 后端 | 入口 | 覆盖 |
|---|---|---|
| tree-walking 解释器 | `hc run <file.hc>`（默认） | 全语言 |
| IR 参考解释器 | `hc run --ir <file.hc>` | 全语言（含 G1-G5 标准库；唯一语义源） |
| 字节码 VM | `hc run <file.hbc>`（HBC2） | 全语言（同 IR，复用 `run_ir`） |
| LLVM 原生 | `hc build <file.hc>` | 未全标准库（`compile mismatch ≤ 60` 边界，见 ADR-0004） |

> 2026-08-18 修正：IR/字节码 由「M3.1–Phase 6 子集」升格为**全语言**——G1-G5 模块
>（net/ipc/storage/archive/text/time/rng + io 差异项 + 线程生命周期）已同步进 IR 后端
>（Q20 双语），interp == IR 由 `hc-rt/tests/consistency.rs` 保障。原生后端仍受 ADR-0004
> 52 mismatch 边界约束。

关键实现约束：**字节码 VM 不写第二个 dispatch 循环**——`run_bytecode` = `decode` + 复用 `run_ir`，HBC2 只是 `IrModule` 的确定性序列化。任何后端不得携带私语义，「双模式一致」才有保证。

代价：字节码 VM 未做紧凑运行时 dispatch / 寄存器式优化（性能提升留后续，须先由一致性套件证明等价）。
