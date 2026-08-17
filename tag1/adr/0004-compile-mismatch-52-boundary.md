# compile mismatch 52 为本阶段原生交叉验证边界（P11d 收束，用户裁定）

`hc test --mode=compile`（原生交叉验证：同一套 `[test]` 在编译模式跑，与解释模式比对）当前基线为 **52 项 mismatch**，经项目所有者裁定（2026-08-17，P11d 收束）作为本阶段验收边界。

含义：

- mismatch 全部是**已申报的缺口**，非语义漂移：未实现的原生内建 / 方法 / 降级点（`error.NotBuiltin` / `error.NoMethod` / `error.Unsupported` 响亮运行时中止），原生 ABI 留后续阶段全标准库。
- CI 基线：`compile mismatch ≤ 52`，低于基线即失败（`tag1/scripts/check-examples.sh`）。
- 逐文件正确标记，只降级、不静默：如 30-interface 显式环境全局播种、连续类值语义（`DeepCopy` 指令 + 运行时门）、`alloc.init` / `Type.new` 构造降级、原生集合方法 `Vec.append` 族等已在前置提交中转 MATCH。

本边界是「双模式语义一致」在原生后端尚未全标准库阶段的**阶段性定义**：不把未完成当作失败，也不把缺口当作未知；后续阶段全标准库落地后，边界应下移直至归零。
