# K5 执行计划：自举闭环 stage2（最小自展 + 全 H 链）

> 来源：2026-08-29 grilling 会话定案（Hank 全部确认），决策记录见 `docs/adr/0029-k5-minimal-bootstrap.md`。
> **交接快照：P 组（P1–P7）已完成，S 组待启动——实施细节/陷阱/限制见 `07-k5-handoff.md`。**
> 执行规则：沿用 K4 惯例——完成即测试验证 → 更新本文档进度 → 提交 → `node .gitnexus/run.cjs analyze --index-only`；单任务预估超 1h 即执行时细拆。
> 目录约定：stage2 源码写 `stage2/`（多文件，`import .{sym}` 同目录互导）；测试/对照脚本/探针写 `stage2/test/`；K5-pre 求值面修复改 `stage1/interp.hc`。

## 目标与验收

- **目标**：H 编译器（H 程序）用 stage1 工具链编译自身；产物再编译产物（二次自举验证）。
- **全 H 链**：`hc run stage1/interp.hc stage2/main.hc stage2/main.hc` → 产出 `A.hbc`；`hc run A.hbc stage2/main.hc` → 产出 `B.hbc`；**断言 A.hbc == B.hbc（逐字节）**。
- **产物可执行**：`A.hbc` 作为程序执行的行为与 stage2 源码经 Rust 编译的行为一致。
- **性能不进验收**：只登记自举一次的耗时基线。

## 前置事实（2026-08-29 调查，证据见 grilling 会话记录）

- 狗粮 10,491 行（lexer 718 / parser 2522 / checker 3499 / interp 3752）已实证：标量 u8/i32/i64/usize/f64/bool、`&[u8]` 切片、Vec/Map/?T、class+方法、位运算、`@intCast`、复合赋值、可选捕获、error/try/catch/orelse/.?。
- **interp.hc 静默错误类**（求值面洞，不修则全 H 链产出错误结果不报错）：
  1. `@intCast/@floatCast` 求值缺失（eval_call 只查用户 fn 表，结果 void）——interp.hc:3525-3563
  2. 位运算 binop 缺失（Amp/Pipe/Caret/Shl/Shr/Tilde 无分支）——interp.hc:3261-3297
  3. `if (opt)|v|` 捕获恒走 else（truthy 只认 bool/int，载荷不绑定）——interp.hc:2972-3026
  4. 纯枚举 `==` 恒真（EnumLit 无求值分支降级 void，0==0）——interp.hc:3264-3267
- switch 求值缺失（解析器已就绪 parse_switch_stmt:1801）；枚举声明解析已就绪（L1335）。
- **.hbc 是唯一 Rust 侧零改动的产物格式**：`hc run file.hbc` → decode 49/49 指令（decode.rs:185-453）→ execute_ir；`ir/json.rs` 是 json.parse 值级解析器，非 IrModule 序列化器。
- Rust 参考编译器 37,261 行；H 版为简化重写（stage1 四件套 vs Rust 前端约 1:1）。
- 已知规避模式 R1–R10（class+kind 分发、Map 只存标量、手动写回、单遍编译顺序等）全部有狗粮先例。

## stage2 编码纪律（违反即自举等价破坏或静默损坏）

1. 禁 Map 迭代（确定性要求）；需要顺序时维护平行 key Vec。
2. 禁 Map 存 class 实例（重定位损坏）；存标量索引（R2）。
3. 容器方法调用改变内容后显式写回变量（R5）。
4. 无闭包/接口/comptime/泛型用户代码/defer/元组（C 档绕过）。
5. 单遍编译顺序：被依赖的自由函数先定义（R6）。
6. switch/枚举负载用 if-else 链 + kind 字符串分发（R1/R7）——K5-pre 修完 switch 求值后可选使用 switch。
7. 对象字段写用重建 + 写回（R10）。

## 任务分解

### 组 P：K5-pre 求值面修复（硬前提）

| # | 任务 | 验收 | 预估 |
|---|---|---|---|
| P1 | `@intCast/@floatCast` 求值：eval_call 内建分支（Ident head 命中内建名表），宽度收窄/提升语义对齐 Rust 参考实测 | 探针：i64→u8 收窄、usize↔i64 往返与 `hc run` 一致 | ≤1h |
| P2 | 位运算 binop：Amp/Pipe/Caret/Shl/Shr/Tilde（int 族），语义对齐 Rust（含 usize 移位） | 探针：lexer.hc 式 UTF-8 解码逻辑在 interp 下输出与 Rust 一致 | ≤1h |
| P3 | `if (opt)\|v\|` 捕获：truthy 认 opt + 载荷绑定进块作用域；`while (opt)\|v\|` 同步评估（可选） | 探针：some 走 then 且载荷可用、none 走 else | ≤1h |
| P4 | 纯枚举求值：Enum 声明登记（变体→序数）、EnumLit、`Enum.Variant` Field 访问、`==` 按序数 | 探针：`AllocSource.None == AllocSource.None` 真、不同变体假（对齐 Rust） | ≤1h |
| P5 | switch 语句求值：字面量/枚举分支 + 或有守卫，执行语义对齐 Rust（不做穷举检查——checker 侧已有） | 探针：多分支命中/默认分支与 Rust 一致 | ≤1h |
| P6 | K5-pre 对照语料：`exec-corpus/11-switch-enum.hc`、`12-cast-bits.hc`、`13-opt-capture.hc` + k4_interp.rs 摘 ignore + 全量回归 | cargo k4 全绿（13 passed） | ≤1h |
| P7 | 最小多文件 import：checker + interp 支持 `import .{sym}` 同目录加载（parser 语法已就绪），顶层符号合并；环依赖检测报错 | 探针：两文件互导符号、循环 import 响亮报错 | ≤1h |

### 组 S：stage2 编译器（`stage2/`，多文件）

| # | 任务 | 验收 | 预估 |
|---|---|---|---|
| S1 | 源码骨架：`main.hc`（读源文件参数 + 阶段调度）+ `import .{lexer,parser,...}` 结构 + 纪律自查清单入库 | interp 检查通过、空编译跑通（产出空 .hbc 不要求） | ≤1h |
| S2 | lexer 提取移植：从内嵌副本提取为 `lexer.hc`，适配多文件（去 self-contained 假设） | 对 6621 token 自身源码与 Rust lex 零 diff（复用 K1 对照法） | 1–3h |
| S3 | parser 提取移植：`parser.hc` 多文件化，AST 节点模型对齐 stage2 子集 | 对 stage2 自身源码 parse 成功 + AST dump 对照抽查 | 1–3h |
| S4 | semantic 裁剪：从 checker.hc 裁剪名称解析 + 签名/调用点类型检查（砍所有权/错误集推断） | 对 stage2 自身源码 0 误报 0 漏报（对照 Rust check） | 1–3h |
| S5 | IR 模型：`IrModule/IrFunc/IrInst` class + kind 分发（按 stage2 子集圈定指令集，对照 ir_inst.rs 49 变体圈定） | 指令集清单入档（预计 ≤20 变体） | ≤1h |
| S6 | lower：AST → IrInst（变量/算术/控制流→跳转/调用/常量池） | 手写探针程序 lower 产物人工抽查 + run_ir 可加载（经 .hbc） | 1–3h |
| S7 | HBC2 encoder：魔数 HBC2/v7/opcode 表（对照 encode.rs 子集）+ func_index 表 | S6 产物编码后 `hc run` 能 decode 回读（round-trip 抽查） | 1–3h |
| S8 | 闭环脚本：`stage2/test/bootstrap.bat`（interp 跑 stage2 → A.hbc；hc run A.hbc → B.hbc；fc /b diff）+ 编码纪律检查清单 | 脚本一键运行 | ≤1h |
| S9 | 产物行为验证：A.hbc 执行输出与 stage2 源码经 Rust 编译执行输出对照 | 对照 MATCH | ≤1h |

### 组 V：验收

| # | 任务 | 验收 | 预估 |
|---|---|---|---|
| V1 | 字节级二次自举：A.hbc == B.hbc；耗时基线登记；全量回归（cargo workspace + k4） | diff 零差异；基线入档 | ≤1h |
| V2 | 文档同步：01-bootstrap-plan Z4、phase4/README、stage1/README（或新增 stage2/README）、本计划状态表 | 文档一致；提交 | ≤1h |

### 依赖顺序

P1→P2→P3→P4→P5→P6→P7 → S1 → S2 → S3 → S4 → S5 → S6 → S7 → S8 → S9 → V1 → V2（P 组内部可并行；S2 依赖 P7 多文件）

## 执行状态

| 任务 | 状态 | 提交 | 备注 |
|---|---|---|---|
| P1 @intCast/@floatCast 求值 | ✅ | 见 P1/P2 提交 | 范围检查+透传（对齐 hc-rt int_width_bounds）；越界 stage1 静默 void（Rust 抛 IntCastOverflow），语料规避；@floatCast Rust interp 本无，stage1 做 int→float/透传最小实现 |
| P2 位运算 binop | ✅ | 见 P1/P2 提交 | BitAnd/BitOr/BitXor/Shl/Shr + Unary BitNot；探针含 UTF-8 掩码组合与 usize 移位，双向一致 |
| P3 if/while 可选捕获 | ✅ | 见 P3 提交 | some→then 绑定载荷、none/err→else（payload_err 有则绑定）；非 opt/err 按 truthy；while 同步支持；守卫型 if 不在 stage1 求值面 |
| P4 纯枚举求值 | ✅ | 见 P4 提交 | Enum 登记（变体→序数）、Enum.Variant Field 访问、==/!= 同类型比序数；带负载变体不在 stage1 求值面 |
| P5 switch 语句求值 | ✅ | 见 P5 提交 | 字面量/枚举/else 分支 + 多模式臂（修内嵌 parser 逗号 break bug，对齐 Rust）；守卫 parser 已丢弃不支持；枚举模式用 Enum.Variant 全限定形（Rust parser 不接受 .Variant） |
| P6 对照语料 | ✅ | 见 P6 提交 | exec-corpus 11/12/13 + k4_interp.rs 3 测试 = 13 passed；对照脚本 13 MATCH；12 号语料踩纪律 5（utf8_len 须先于 main 定义）已修正 |
| P7 多文件 import | ✅ | 见 P7 提交 | interp+checker：`import .{sym}` 同目录 sym.hc 加载（递归/环检测/菱形去重），顶层符号平铺合并，run_main 两遍化；模块路径导入（H.std.{io}）不触发文件加载；环/缺文件响亮报错；模块限定访问（a.fn()）不在本轮 |
| S1 源码骨架 | ✅ | 见 S1 提交 | stage2/{main,lexer,parser}.hc + README 纪律清单 + test/smoke.hc；**含两个 K5-pre 漏项补齐（interp.hc）**：① run_main 绑定 main 形参（bootstrap 链硬前提；args[0]=自身路径+余参透传对齐 Rust hc）；② io.fs.read_file 宿主透传（NotFound/Io→目标 Try/Catch 通道）；③ main 返回 err → stdout 响亮（flow=="return" 且 retv 为 err 才判定——retv 是残留寄存器；err 名经 Vec 拷贝避开 AST 子切片的数组格式化）。验收四连：checker OK / smoke 贯通 / usage+Usage / 缺文件 NotFound |
| S2–S9 | 🔴 | — | |
| V1–V2 | 🔴 | — | |

## 风险登记

- **所有权规则已定案（ADR-0030，2026-08-29）**：转移改为指针形态（`owned *T`/`move &t`），move 后原变量冻结；对 K5 无阻塞——stage2 编码纪律本就规避所有权构造；Rust semantic 同步在 K6 前完成，checker.hc 同步在 K6。

- **checker.hc 裁剪回归**：S4 动 checker 副本时不得破坏 K3 对照门禁（15 项）——裁剪在 stage2/ 副本上进行，stage1/checker.hc 冻结。
- **HBC2 编码器正确性**：decode 丢 `ret_ty`/`type_implements`（decode.rs:94,136）对 execute_ir 无碍（只要求 func_index 含 main），但 encoder 需避免依赖被丢字段。
- **确定性漏洞**：任何 Map 迭代/哈希顺序依赖会破坏 V1 字节等价——纪律 1 执行中用探针盯防。
- **工作量**：S2–S7 标 1–3h，执行时按 K4 惯例细拆为 ≤1h 步骤；stage2 语言子集若失控（贪心支持更多构造）立即回切纪律 1–7。
