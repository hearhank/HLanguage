# compile mismatch 52 为本阶段原生交叉验证边界（P11d 收束，用户裁定）

`hc test --mode=compile`（原生交叉验证：同一套 `[test]` 在编译模式跑，与解释模式比对）当前基线为 **60 项 mismatch**，经项目所有者裁定（2026-08-17，P11d 收束，当时为 52）作为本阶段验收边界。

> 计数演进（P11d 后按已申报副作用逐步上调，见 `tag1/scripts/check-examples.sh` 注释）：52→53（D1 `fmt_int`：63-template-render）、53→54（G1 `spawn` 解析：77-producer-consumer）、54→55（G5：90-thread-lifecycle）、55→58（组 E E1 async/await：37/38/39/76/80）、58→60（捕获语法解析：78/79 由双解析失败转为同组 04-concurrency 包级原生 MISMATCH）。计数上升均系「interp 侧功能落地使示例可解析、原生侧相应构造未覆盖」的**可见化**，非原生能力回归。

含义：

- mismatch 全部是**已申报的缺口**，非语义漂移：未实现的原生内建 / 方法 / 降级点（`error.NotBuiltin` / `error.NoMethod` / `error.Unsupported` 响亮运行时中止），原生 ABI 留后续阶段全标准库。
- CI 基线：`compile mismatch ≤ 60`，低于基线即失败（`tag1/scripts/check-examples.sh`）。
- 逐文件正确标记，只降级、不静默：如 30-interface 显式环境全局播种、连续类值语义（`DeepCopy` 指令 + 运行时门）、`alloc.init` / `Type.new` 构造降级、原生集合方法 `Vec.append` 族等已在前置提交中转 MATCH。

本边界是「双模式语义一致」在原生后端尚未全标准库阶段的**阶段性定义**：不把未完成当作失败，也不把缺口当作未知；后续阶段全标准库落地后，边界应下移直至归零。

## IR 后端覆盖范围明示（2026-08-18 修正）

**52 mismatch 边界仅约束原生（LLVM compile）后端**，与 IR 后端无关。「双模式语义一致」按后端分层明确覆盖范围：

| 后端 | 入口 | 覆盖 | 一致性保障 |
|---|---|---|---|
| tree-walking 解释器 | `hc run <file.hc>`（默认） | 全语言 | — |
| IR 参考解释器 | `hc run --ir <file.hc>` | 全语言（含 G1-G5 标准库：net/ipc/storage/archive/text/time/rng + io 差异项 + 线程生命周期 + 全核心标准库） | `hc-rt/tests/consistency.rs`（interp == IR，当前 77 用例） |
| 字节码 VM | `hc run <file.hbc>` | 同 IR（`run_bytecode` = `decode` + 复用 `run_ir`，ADR-0004 唯一语义源） | 同上 |
| LLVM 原生 | `hc build <file.hc>` | 未全标准库（**60 mismatch 边界所在**） | `hc test --mode=compile` 交叉验证 |

**修正缘由**：G1-G5 模块此前为 tree-walking 独有（interp-only 私语义），IR 后端命中 `error.Unsupported`，与「双模式一致」承诺相悖。2026-08-18 IR 后端同步 G1-G5（Q20 双语，类名分派对齐 oracle），双跑验证 interp 与 `--ir` 对全部 G 模块输出逐字节一致；此后 IR/字节码 与 tree-walking 同面，consistency 套件持续守护。

任一后端不得携带私语义——新增语言构造须先进一致性套件（interp == IR），再谈原生覆盖。
