# 0032 — S8 闭环链路分级：宿主链为日常门禁，全 H 链为里程碑验证

> **Status: 部分被 ADR-0033 superseded（2026-08-30）**——oracle/resume 模式（全 H 链解释执行）已废弃，由产物链取代；fast 模式（宿主链日常门禁）继续有效。

bootstrap.bat 的 V1 自举闭环曾以全 H 链（stage1 interp 解释执行 stage2 编译器，嵌套解释，数小时量级）为唯一路径，闭环无法例行运行。决定将链路分级：**默认 fast 模式用宿主链**——Rust hc 以包模式直接解释执行 stage2 编译器（实测 21s 产出 304166 字节的 A.hbc），随后 Phase B + V1，作为日常门禁；**oracle/resume 模式保留全 H 链**，作为一次性里程碑验证（登记耗时基线，并用真实编译负载终极验证 stage1 interp 与 Rust hc 对 H 语义实现的一致性），产物独立命名为 A_oracle.hbc/B_oracle.hbc，resume 支持阶段级断点续跑（文件级标记 progress.txt 仅作诊断，不做检查点）。

理由：V1 断言（A == B）检验的是 stage2 编译器自身的确定性与自举等价，与执行宿主无关——stage2 编译器无非确定性来源（平行 Vec、手写稳定排序、无时间/随机），且两条链路喂给它的 argv 逐元素一致，跨宿主产物一致是构造上的必然；宿主间语义一致性另由 K4 parity 门禁（13 语料 stdout 逐字节一致）背书，残余分歧（如 `@intCast` 越界时 stage1 静默 void vs Rust 抛 `IntCastOverflow`）恰是 `fc /b` 要抓的对象。

## Considered Options

- **V1 只认全 H 链**：证明最强，但数小时/次使闭环沦为摆设——拒绝。
- **优化 Rust hc-rt 解释器压缩全 H 链耗时**：热点明确（pick_fn 每调用深克隆函数体 AST、变量读取深拷贝 String、每调用新建 HashMap），预期 2–5x；但 hc-rt 是全部执行模式的底座，为一次性工作承担回归风险不值——登记为后续独立立项。
- **`hc build` 原生 / `hc run --ir` 替代宿主**：LLVM 内建面缺 Map.init/put/get、io.fs.write_file、Vec.as_slice（运行时响亮 abort）；`--ir` 的 run_file_ir 不装载同目录兄弟文件——均确定性不可行。

## Consequences

- fast 产物（A.hbc/B.hbc）与 oracle 产物（A_oracle.hbc/B_oracle.hbc）并存，V1 各自配对断言；两模式可独立重跑互不覆盖。
- 全 H 链首跑基线登记（progress.txt + 文档）后，日常不再重复该链路；K6 可复现构建与 1.x 原生路线引用本分级。
