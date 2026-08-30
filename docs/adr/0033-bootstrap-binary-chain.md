# 0033 — 自举产物链取代解释执行链

自举闭环的操作方式重新定案（项目所有者 2026-08-30 五条约束）：每一环都以编译产物执行（HBC2 字节码，后续原生二进制），不再采用「stage1 interp 解释执行 stage2 编译器」的数小时嵌套解释路径。Phase I 步骤：补 stage2 浮点支持（S0）→ 重产 A.hbc（S1）→ 编译 interp.hc 产 interp.hbc（S2）→ 速度测定决策门（S3）→ interp.hbc 全语料逐行对照（S4）→ 跨编译器等价断言 A2 == A.hbc（S5，取代旧 oracle 的证明目标且更强：两个 H 编译器栈的产物逐字节一致）→ 二次自举实证（S6）。每步带最小测试、Rust 基线（stdout 逐行 + 耗时）与超时阈值（严格 Rust×20 起步，FAIL 后手动调长并登记），详见 `docs/SPEC/phase4/09-bootstrap-binary-chain-plan.md`。

理由：跨宿主确定性已在 S7 验证（宿主链产物 == A.hbc 自编译产物逐字节相等），V1 检验的是 stage2 编译器自身的确定性，嵌套解释宿主不提供额外证明力却耗数小时且无检查点；产物链把等价证明压缩到分钟-小时级且每步可测可超时可对比。原生二进制（LLVM 内建面补齐）为 Phase II 独立立项，避免以大工程阻塞产物链闭环。

## Considered Options

- 保留 oracle 解释链为可选里程碑：数小时、无检查点、证明力弱于 S5 的字节级断言——拒绝。
- 直接补 LLVM 内建面一步到位原生：工程量大且阻塞全部后续步骤——降级为 Phase II。

## Consequences

本 ADR **supersede ADR-0032 的 oracle/resume 部分**：bootstrap.bat 收缩为 fast（宿主链日常门禁保留）；A_oracle.hbc/B_oracle.hbc 产物命名废弃，改用 A2.hbc/B2.hbc（interp 链产物）。约束「每命令超时 = Rust×10~20」对无 Rust 对应物的环节以宿主链耗时为基线对象，严格 FAIL 优先于放宽——超时是发现问题的手段，不是要绕过的障碍。
