# 共享 IR 为唯一语义源：四后端共用一个语义

延续主项目 [ADR-0004](../docs/adr/0004-dual-mode-architecture.md)（共享前端 → 共享 IR → 双后端）的定案，tag1 把「唯一语义源」落到 `IrModule` + `run_ir` 上，并扩展为**四后端**：

| 后端 | 入口 | 覆盖 |
|---|---|---|
| tree-walking 解释器 | `hc run <file.hc>`（默认） | 全语言 |
| IR 参考解释器 | `hc run --ir <file.hc>` | M3.1–Phase 6 子集（唯一语义源） |
| 字节码 VM | `hc run <file.hbc>`（HBC2） | M3.1–Phase 6 子集 |
| LLVM 原生 | `hc build <file.hc>` | M3.1–Phase 6 子集（emit-.ll + zig cc） |

关键实现约束：**字节码 VM 不写第二个 dispatch 循环**——`run_bytecode` = `decode` + 复用 `run_ir`，HBC2 只是 `IrModule` 的确定性序列化。任何后端不得携带私语义，「双模式一致」才有保证。

代价：字节码 VM 未做紧凑运行时 dispatch / 寄存器式优化（性能提升留后续，须先由一致性套件证明等价）。
