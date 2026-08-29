# K5 最小自展：全 H 链 + .hbc 产物

**Status**: accepted（2026-08-29，grilling 会话定案）
**关联**: `docs/SPEC/phase4/06-k5-execution-plan.md`（任务分解）、`01-bootstrap-plan.md` Z4

## 决策

K5 自举闭环采用**最小自展**策略，六项裁决：

1. **范围**：stage2 = H 版编译器切片（lexer + parser + 裁剪 semantic + IR 生成 + HBC2 编码器），只编译 stage2 自身源码所用的语言子集；不完整复刻 Rust 编译器（37,261 行）。
2. **全 H 链**：`interp.hc`（H 版求值器）执行 stage2 → stage2 编译 stage2 源码产出 `.hbc` → `hc run stage2.hbc`（Rust 字节码门）执行 → 再编译同一源码。
3. **产物格式 = .hbc 字节码**（HBC2 v7）：Rust 侧 decode 49/49 指令全覆盖（`decode.rs:185-453`）、零新增；stage2 侧用 H 写 opcode 编码器（字节级发射，lexer.hc 的 UTF-8/数字编码是先例）。
4. **二次自举等价 = 字节级**：interp 执行 stage2 产 A.hbc；`hc run A.hbc` 再编译同一 stage2 源码产 B.hbc；断言 A.hbc == B.hbc（逐字节 diff）。
5. **前置修复（K5-pre）**：interp.hc 四个静默错误类（`@intCast` 求值缺失、位运算 binop 缺失、`if (opt)|v|` 捕获恒走 else、纯枚举 `==` 恒真）+ switch/枚举求值 + 同目录多文件 import（`import .{sym}`，checker/interp 补加载）。
6. **semantic 裁剪复用 checker.hc**：名称解析 + 签名/调用点类型检查；所有权/错误集推断不做（stage2 源码纪律规避）。性能不进 K5 验收（只登记基线）。

## Considered Options

- **完整复刻 Rust 编译器**：工作量 ~3x，自举意义不变 → 对齐留给 K6 一致性阶段。
- **新文本 IR / JSON IR 产物**：`ir/json.rs` 是 `json.parse` 值级解析器而非 IrModule 序列化器（调查纠正预设）；IrModule 级文本/JSON 设施双侧均无，IrModule 含 HashMap 需定序约定 → 弃。
- **半 H 链（stage2 由 Rust `hc run` 执行）**：绕过求值洞但背离「用 H 编译 H」的定义 → 静默洞前置修复后走全 H 链。
- **语义等价 vs 字节级等价**：字节级与 K4 逐字节对照纪律一致，自动化即一行 diff → 采字节级。

## Consequences

- interp.hc 求值洞从「已知余量」升级为 K5 硬前提：静默错误类不修，全 H 链会产出错误结果而不报错。
- stage2 源码受编码纪律约束（禁 Map 迭代/Map 存 class 实例、容器方法调用后显式写回、无闭包/接口/comptime/泛型用户代码、单遍编译顺序），登记于 K5 计划。
- stage2 破万行触发**最小多文件支持**（checker/interp 补 `import .{sym}` 同目录加载）——自举吃狗粮反馈设计（Z8）的首个正例。
- Rust 参考实现保留为 parity oracle，不删除（自举失败风险对策）。
