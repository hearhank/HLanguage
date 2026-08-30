# 09 — 自举产物链计划（Binary Chain，Phase I）

> 定案日期：2026-08-30（grilling 会话，ADR-0033）。本计划重新定义自举闭环的操作方式，
> 取代 06-k5-execution-plan.md 中 S8 的「interp 全链解释执行」验收路径（06 保留为 K1–K5 历史）。
> 背景实测：A.hbc（HBC2 产物）在 VM 上执行为分钟级；旧 oracle 解释链为数小时级且中途无检查点。

## 约束（项目所有者定案，2026-08-30）

1. **放弃脚本（解释）执行逻辑**：自举链每一环都以编译产物（HBC2 字节码，Phase II 起含原生二进制）执行。
2. **分步完成**：每步一个最小测试用例验证本步功能；改动波及 stage1 时重跑 K1–K4 门禁（`cargo test -p hc-tools --test k1_lexer / k2_parser / k3_checker / k4_interp`）。
3. **每步与 Rust 对比**：stdout 逐行比较 + 执行时间比较。
4. **每命令超时 = Rust 实现同功能 ×10~20**。**策略：严格 ×20 起步以暴露问题；超时 FAIL 后可手动调长，调长时必须在本表记录新值与理由。**
5. 测试形态：沿用 k4 模板扩展（`CARGO_BIN_EXE_hc` 子进程 + stdout 逐行 `assert_eq!` + `Instant` 计时断言），门禁可 CI 化。

## 链路形态对比

| 链路 | 形态 | 耗时 | 状态 |
|---|---|---|---|
| 旧 oracle（已废弃） | tree-walking(Rust) → interp 解释 stage2 编译器 → 编译 | 数小时 | ADR-0033 废弃 |
| 宿主链（fast，保留） | Rust hc 包模式直接执行 stage2 编译器 | 26.7s | 日常门禁 |
| **产物链（本计划）** | 每环均为编译产物：A.hbc → interp.hbc → A2.hbc | 待实测（S3 决策门） | Phase I |

## Phase I 步骤表

> 字段：命令 / 功能描述 / Rust 基线 / H 实测（执行后填）/ 超时（严格 Rust×20，可手动调长）/ 通过状态 / 最小测试。

### S0 — stage2 浮点支持（常量 + binop 透传 + f64 位编码）

- **命令**：修改 `stage2/src/ir.hc`（IrConst 加 `f: f64` 字段 + 构造器）、`stage2/src/lower.hc`（FloatLit 臂，复用 parse_float，对齐 tag1 `lower_impl.rs` L1125-1131 的 `unwrap_or(0.0)` 语义）、`stage2/src/encode.hc`（`enc_const_tag` Float→`0x01` + 8 字节 f64 小端，对齐 tag1 `encode.rs` L554-557）；binop 零改动（op 透传 + 三层运行时已支持）。
- **功能描述**：stage2 编译器目前对 FloatLit 一律报「不在子集」（32 条诊断），无法编译 interp.hc（浮点字面量 ~40 处 + 浮点运算 ~30 处：Add/Sub/Mul/Div/比较/Neg/复合赋值/混合提升）。本步打通浮点编译。
- **硬问题**：H 无 `@bitCast`——f64→8 字节位编码用纯 H 算术做 IEEE-754 分解（特判 ±0；x<0 取符号；归一化到 [1,2) 计指数；尾数 (x−1)×2^52 经 `@intCast` 转 i128；位运算拼装 sign<<63 | (e+1023)<<52 | m）。
- **最小测试**：
  - T0a：浮点最小程序（字面量/四则/混合提升/比较/Neg/`+=` `*=`）→ stage2 编译 → `hc run` 执行 → stdout 逐行对照 `hc run`（Rust）；
  - T0c：产物 .hbc 中每个浮点常量的 8 字节 == Rust `f64::to_le_bytes`（cargo test 内构造期望值）；
  - 回归：`hc run stage1/checker.hc stage2/src/main.hc` = OK；K4 门禁重跑。
- **Rust 基线**：编译+执行 ~0.2s；**超时**：×20 = 4s；**通过状态**：✅（T0a 18 行逐行一致；T0c 10 值字节对拍 bit-exact；教训：hc-rt @intCast 不接受 float 源 → 尾数改纯 f64 比较/减法逐位提取）
- **S0 范围追加（checker 强化，2026-08-30）**：Call 被调名解析检查、Assign 目标/类型检查、Defer/Errdefer/ConstDecl body 检查（parser 修复节点丢失）、Catch/Orelse/Move/Default 遍历、type_of_expr Binary 臂修复（op 在 props 非 children）；`!optional` 求值坑确认（禁用，统一 if 解包）；lower 补 Move 指令（opcode 29，对齐 tag1）。验收：负例 3 诊断命中、stage2 自身 0 误报、K4 13 passed。

### S1 — 重产 A.hbc + fast 回归

- **命令**：`stage2\test\bootstrap.bat`（fast）。
- **功能描述**：含浮点支持的编译器重产 A.hbc；宿主链 V1 回归。
- **Rust 基线**：26.7s；**超时**：×20 ≈ 8.9min；**通过状态**：✅（V1 PASS，A==B 313,568 B）
- **最小测试**：fast 闭环 PASS（A==B）。

### S2 — 产 interp.hbc（编译器编译解释器）

- **命令**：`hc run stage2/test/A.hbc --emit-hbc stage2/test/interp.hbc stage1/interp.hc`。
- **功能描述**：A.hbc（VM 执行的 stage2 编译器）编译 stage1 解释器。前次实测 11.3s 到 lower 失败（32 条浮点诊断）；S0 后应全通（check 66 decls 已过）。
- **Rust 基线**：宿主链折算 26.7s；**超时**：×20 ≈ 8.9min；**通过状态**：✅（实测 11.7s，interp.hbc 243,240 B）
- **最小测试**：interp.hbc 执行 `stage1/exec-corpus/01-arith.hc`，stdout 逐行对照 `hc run`。
- **实施追加**：S0 后首次实测报 2 条「子集外表达式 Catch」（interp.hc 的 2 处 `catch |err| {…}`，host_read_file/host_write_file）→ 补 lower Catch 臂（JumpIfErr + res_slot + lo_bind，对齐 tag1 lower_expr Catch 臂；Bind body 需以 return 结尾——子集切片）后全通。

### S3 — 速度测定（决策门）

- **命令**：`hc run stage2/test/interp.hbc <语料>` vs `hc run stage1/interp.hc <语料>`（同语料计时）。
- **功能描述**：IR VM 执行 interp vs tree-walking 执行 interp 的耗时比值（interp.hc 在 Rust hc 上执行 01-arith = 0.126s / 04-fn-rec = 0.390s）。比值决定 interp.hbc 的定位：≥2x 推广为执行工具；<2x 降级为语义对照工具（S4/S5 照跑，仅时间预期不同）。
- **Rust 基线**：0.055–0.057s（语料 Rust 执行）；**超时**：记录项，无阈值；**通过状态**：✅（比值 2–2.6x：01-arith 0.068s vs 0.126s；04-fn-rec 151ms vs 390ms → interp.hbc 推广为执行工具）

### S4 — interp.hbc 全语料对照

- **命令**：对 `stage1/exec-corpus/01–13` 逐个：`hc run stage2/test/interp.hbc <语料>` vs `hc run <语料>`。
- **功能描述**：编译后的解释器执行全部语料，验证 IR VM 宿主语义；stdout 逐行比较 + 计时。
- **Rust 基线**：0.055s/个（已测）；**超时**：×20 = 1.1s/个（严格，FAIL 即记录——两层嵌套可能超，正是要暴露的问题）；**通过状态**：✅（13/13 逐行 MATCH，interp.hbc 90–151ms vs Rust 77–91ms，全部在阈值内）
- **最小测试**：k4 模板扩展的 cargo 测试（每语料一项）。

### S5 — 跨编译器等价（取代旧 oracle 的证明目标）

- **命令**：`hc run stage2/test/interp.hbc stage2/src/main.hc --emit-hbc stage2/test/A2.hbc <stage2 全部 7 源文件>`，然后 `fc /b stage2/test/A2.hbc stage2/test/A.hbc`。
- **挂机运行（推荐）**：外层 PowerShell 逐行加时间戳并 Tee 落盘（时间戳不进自举链内，保产物确定性；脚本内置 S5 断言）：

```bash
powershell -ExecutionPolicy Bypass -File stage2\test\s5-run.ps1
```

日志落盘 `stage2/test/s5.log`，每行含绝对时刻 + 相对秒；完成后自动断言并输出 `S5 PASS: A2 == A byte-identical (N bytes)` 或首个差异偏移。

- **功能描述**：interp.hbc（IR VM → interp）解释执行 stage2 编译器，编译 stage2 自身 → A2.hbc；断言与宿主链产物逐字节一致。三层等价证明（IR VM 执行 interp 解释执行编译器 ≡ Rust 直接执行编译器），证明力强于旧 oracle。
- **Rust 基线**：宿主链 26.7s；**超时**：严格 ×20 ≈ 8.9min（预期大概率超时 FAIL——超了记录实测值，按约束 4 策略手动调长并在此表登记新值与理由）；**通过状态**：🔴

### S6 — 二次自举实证

- **命令**：`hc run stage2/test/A2.hbc --emit-hbc stage2/test/B2.hbc <7 源文件>` + `fc /b A2 B2`。
- **功能描述**：A2（interp 链产物）在 HBC2 VM 上自编译 → B2；S5 成立则 B2==A2 理论自动成立，仍实证一遍。
- **Rust 基线**：26.7s；**超时**：×20 ≈ 8.9min；**通过状态**：🔴

### S7 — 文档/CI 收尾

- 新测试入库（建议 `tag1/hc-tools/tests/k5_binary_chain.rs`）；S3 比值与各步实测入本表；stage2/README、06 状态同步；CONTEXT/ADR 复核。

## Phase II — 原生二进制（独立立项，另行细化）

zig 0.16.0 已安装，`hc build` 工具链前提满足。阻塞项 = LLVM 原生内建面缺口（`Map.init/put/get/contains/len`、`io.fs.write_file/list_dir`、`Vec.as_slice`、`String.compare/fromInt` 等，运行时响亮 abort，见 tag1/hc/src/codegen/llvm/body.rs 分派表）。工作量 ≈ 把「全核心标准库」的内建面在 LLVM 侧重做（P11d 后续）。完成后 `hc build stage1/interp.hc` → 原生解释器，再上一个数量级。

## 超时调长记录

| 步 | 原值 | 新值 | 理由 | 日期 |
|---|---|---|---|---|
| （空——调长时登记） | | | | |
