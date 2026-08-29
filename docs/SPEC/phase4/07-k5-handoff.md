# K5 交接快照：P 组完成，S 组待启动

> 写于 2026-08-29，P1–P7 全部完成的时点。新 Thread 从本文 + `06-k5-execution-plan.md` 接续，直接开工 **S1**。

## 会话状态

- **分支**：`feature/self-host-k5`；P 组提交序列（旧→新）：`01476ee`(P1/P2) → `ca85827`(P3) → `02d4ba8`(P4) → `392967c`(P5) → `c4eb58f`(P6) → `f45ae70`(P7) → `d38699a`(快照)
- **工作树干净**（仅 `docs/k5_anysly.md` 未跟踪，属 Hank，勿动勿提交）
- **门禁基线**：k3_checker 15 ✅ / k4_interp 13 ✅ / run-compare 13 MATCH ✅ / workspace 1120（Rust 语义层未动，仅 k4_interp.rs +3 测试）
- GitNexus 已重索引：7,684 nodes / 25,612 edges

## P 组落地内容（新代码位置）

| 面 | 位置 | 内容 |
|---|---|---|
| 求值 | `stage1/interp.hc` exec_stmt | P3 If/While 可选捕获（payload prop；some 绑定载荷/none→else；非 opt 值按 truthy） |
| 求值 | 同上 Switch 分支 + `pattern_match` 方法 | P5 字面量/枚举变体/else/多模式臂；守卫 parser 丢弃→运行时不可支持 |
| 求值 | 同上 `binop` | P2 BitAnd/BitOr/BitXor/Shl/Shr + eval_expr Unary `BitNot` |
| 求值 | 同上 `eval_field`/`binop Eq/Ne`/`run_main` | P4 Enum 登记（`enums` 表）、`Enum.Variant` 求值（kind="enum"，name=枚举名，s=变体名，i=序数）、==/!= 同类型比序数 |
| 求值 | 同上 `eval_call` Ident 分支 + `builtin_cast` 方法 | P1 `@intCast`（范围检查+透传，对齐 hc-rt `int_width_bounds`）、`@floatCast` 最小实现 |
| parser | `stage1/interp.hc` 内嵌 parser | P5 多模式臂修复（逗号后 break→continue）；P7 parse_import 存 `syms` prop |
| 多文件 | interp.hc + checker.hc 双侧 | P7 `import .{sym}` → 同目录 sym.hc：递归加载/环检测(error.ImportCycle)/菱形去重/顶层符号平铺合并；interp 的 `run_main` 两遍化（先全量注册再定位 main）；checker 的 `load_imports` 为 Checker 方法（`imports` 字段） |
| 语料 | `stage1/exec-corpus/11~13` + `tag1/hc-tools/tests/k4_interp.rs` | P6 三个永久语料 + 3 个 parity 测试 |

## P 组定案的限制（S 组按此编码，勿假设更多能力）

1. **@floatCast**：Rust interp 本就没有该内建；stage1 仅 int→float/float 透传，float→int 截断未做。语料勿用。
2. **switch 守卫**：parser 丢弃守卫表达式，不可用。switch 模式须 `Color.Red` 全限定形（Rust parser 不接受 `.Red` 点模式）。
3. **import 语义**：仅 `import .{sym}`（path prop 为**空串**）触发同目录文件加载；`import H.std.{io}` 等模块路径导入是 no-op（四件套头部都有，别误改）。符号**平铺合并**——被导文件符号直接按名引用，无 `a.fn()` 模块限定访问。
4. **越界 @intCast**：Rust 抛 IntCastOverflow，stage1 静默 void——语料规避越界用例。
5. **纯枚举**：仅无负载变体；枚举 == 按序数。带负载变体不在求值面。

## 已踩陷阱（勿重复）

1. **`pi` 是 H 内置常量（π）**：同名变量声明被静默遮蔽跳过，`pi < n` 变 `3.14 < n` 恒假。循环变量避开内置名（eval_call 内 C5.1 注释有警告先例）。
2. **局部到局部 class 拷贝**：`var pv = cv` 被 Rust checker 拒绝（`cannot assign reference type by value`）——在调用点内联取值或用 `copy(&x)`。
3. **`Map.put` 要求 `var mut` 绑定**（Vec.append 不要求——checker 的变异方法分类）；跨函数传 Map 用类方法走 `self` 字段（`self.fns.put` 先例），指针参数 `.put` 不行。
4. **单遍编译纪律 5**：被依赖的自由函数必须先于 main 定义（run_main 两遍化前，main 后定义的 fn 不会被注册——两遍化后求值器已无关，但 stage2 编译器输出顺序仍要守纪律）。
5. **bin/hc.exe 过期陷阱**：改 Rust 侧后必须 `cp tag1/target/release/hc.exe bin/hc.exe` 再跑 CLI 验证。
6. **vendored `tag1/hc-tools/hc-rt` 与 `tag1/hc-rt` 有历史分叉**——同步改动必须两份都改、按锚点逐处改，禁整文件拷贝。
7. 临时探针一律写 `stage1/k4test/`（用完即删）；`.gitignore` 已挡 `/stage1/k4test/{diff,tmp}-*`。

## S 组开工指引

- **S1**：`stage2/main.hc`（读源文件参数 + 阶段调度）+ `import .{lexer,parser,...}`；纪律自查清单入库。验收：interp 检查通过、空编译跑通。
- **S2**：lexer 从 **interp.hc 内嵌副本**提取为 `stage2/lexer.hc`（去 self-contained 假设），多文件化。验收：对 6621 token 自身源码与 Rust lex 零 diff（K1 对照法）。注意：**以 interp.hc 内嵌版为准**（含 P5/P7 修复），不是独立 `stage1/parser.hc`/`stage1/lexer.hc`（有历史分叉）。
- **S3–S4**：parser/semantic 同法提取裁剪；S4 动 checker 副本时 **stage1/checker.hc 冻结**（K3 门禁 15 项不可破）。
- **S5–S7**：IrModule/IrFunc/IrInst class + kind 分发（对照 `tag1/hc/src/ir/ir_inst.rs` 49 变体圈定 ≤20）→ lower → HBC2 encoder（魔数 HBC2/v7，对照 `tag1/hc/src/codegen/bytecode/encode.rs` 子集；decode 丢 `ret_ty`/`type_implements` 对 execute_ir 无碍）。
- **S8**：`stage2/test/bootstrap.bat`：`hc run stage1/interp.hc stage2/main.hc` → A.hbc；`hc run A.hbc stage2/main.hc` → B.hbc；`fc /b` 断言 A==B。
- **S9/V1/V2**：产物行为对照 → 字节级二次自举 + 耗时基线 → 文档同步（01-bootstrap-plan Z4、phase4/README、stage2/README、计划状态表）。

## 每任务惯例（K4/K5-pre 沿用）

验证（探针双向 diff + 门禁）→ 更新 `06-k5-execution-plan.md` 状态表 → 提交（subject ≤50 字符祈使句）→ `node .gitnexus/run.cjs analyze --index-only`。S2–S7 标 1–3h 的任务执行时细拆为 ≤1h 步骤。stage2 子集若失控立即回切纪律 1–7。
