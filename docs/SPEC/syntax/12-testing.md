# 12 测试

> 大模块：测试 | 对齐状态：**✅ 对齐完成（2026-08-30，无待裁决项；两处缺口移交 backlog）** | 初稿：2026-08-30
>
> 事实基础：定案 Q8/Q-R11/Q-T1–T6（原载 `06-01` §2，已废弃）、ADR-0010 决策 5（test_io 取消）、ADR-0026 规则 6（tests/ 目录）、tag1 实现（`parser/test_attribute.rs`、`ir/builtin.rs` 断言内建、`codegen/llvm/emit.rs` 测试 runner、`hc-tools` CLI）。
> 关联：错误传播机制 `08` §8.4（`try expect_*` 传播即失败）；`09` §9.6（tests/ 目录）。

## 12.1 `[test]` 特性标注（全形态）

- 规则（Q8/Q-R11 定案：测试 = 标记为测试的函数）：

```hc
[test] fn add_basic() !void { ... }                              // 默认串行
[test("加法基本用例")] fn t2() !void { ... }                       // 显示名
[test(async)] fn t3() !void { ... }                               // 异步模式
[test(thread)] fn t4() !void { ... }                              // OS 线程模式
[test(timeout=5)] fn t5() !void { ... }                           // 超时秒数
[test("组合", async, timeout=10)] fn t6() !void { ... }            // 组合
```

  - 显示名 = 标注名 ?? 函数名；执行模式三态 `Serial`（默认）/`Async`/`Thread`；`timeout` 为秒数（u64）。
  - 测试函数**可被普通代码调用/复用**；**不入函数重载池**（`05` §5.5）。
  - ⚠️ Q-T3「并发测试留 1.x」表述**过时**——async/thread 模式已实现（`TestMode` 三态）。
- 状态：✅ 已实现
- 证据：`parser/test_attribute.rs`（括号形态逐项解析 + 单元测试 7 项）；`parser/decl.rs` `finish_fn_decl`（Trait::Test → is_test/test_name/test_mode/test_timeout）；`semantic/collect.rs`（不入重载池）

## 12.2 断言 API（五件套，Q-T1 定案）

- 规则：
  - `expect(cond)` / `expect_eq(a, b)` / `expect_neq(a, b)` / `expect_error(error.e, expr)` / `expect_eq_slices(a, b)`——**测试函数内隐式可用**（编译器内建，归 std.debug 面）。
  - 失败**不抛错**——写入当前测试的 fail 记录（消息含 expected/got 或 display 差异），测试收尾判 FAIL；用 `try` 传播 = 失败路径（`08` §8.4）。
  - 全部返回 `anyerror!void`；`expect_eq` 支持 `==` 可比较类型（含 String 内容比较，H3）；`expect_eq_slices` 失败输出长度 + 首个差异位置；`expect_error` 失败输出 expected error.X / got。
  - 归属：API 详细面归标准库文档（`04-stdlib-scope.md`）；本规范收口语法形态与内建身份。
- 状态：✅ 已实现
- 证据：`ir/builtin.rs` L4407-4541（断言五件套 + 「失败不抛错，写入 fail」注释 + 消息文案）；`codegen/llvm/helpers.rs`（hc_expect* 原生 helper）

## 12.3 `hc test` 行为（CLI + 收集 + 输出）

- 规则：
  - CLI：`hc test [--mode=interpret|compile] [--dangle=on|off|auto] [目标路径]`——默认 `interpret`。
  - **收集**：`[test]` 标注函数（源码内）+ **`tests/` 目录**测试文件（项目根，ADR-0026 规则 6——不参与命名空间，`import` 引入被测模块）。
  - **输出**（Q-T2）：逐测试 `[PASS]/[FAIL]/[SKIP] 文件名::测试名`；FAIL 附错误类型 + 断言位置；汇总 `N passed, M failed, K skipped (总耗时)`；**失败数 > 0 → 退出码非零**。
  - **SKIP**：`return error.SkipTest;` → 计 SKIP（原生侧按错误码载荷识别续跑，F1）。
- 状态：✅ 已实现（--release ⏳ 见 §12.5）
- 证据：`hc-tools/cli/run_cli.rs` L125+（test 命令 + --mode 解析）；`codegen/llvm/emit.rs` `emit_test_runner` L658-740（is_test 收集、[PASS]/[FAIL]/[SKIP]、SkipTest 码特判）

## 12.4 失败不中止与原生边界

- 规则（Q-T2：**失败不中止**——逐测试续跑）：
  - 解释/IR 侧：✅ 逐测试续跑。
  - **原生（LLVM）侧边界**：断言失败即 `abort(exit 1)`——逐测试续跑需重做引导（实现注释明示）→ **backlog #18**（目标 = Q-T2 口径：记录失败继续）。
- 状态：⚠️ 部分实现（原生续跑 → backlog #18）
- 证据：`emit_test_runner` L658-659 注释（「因断言失败即 abort，逐测试续跑需重做」）

## 12.5 双模式矩阵（Q-T5 定案）

- 规则：
  - `--mode=interpret`（默认）：脚本 Debug——全检测（越界/溢出/悬垂 panic）。
  - `--mode=compile`：编译 **Debug**——双模式一致性验证（**含错误路径用例**）。
  - **`--release`（编译 Release，零开销验证——仅正常路径子集，不含越界/溢出/悬垂错误路径用例）：⏳ 未实现**（CLI 仅 interpret|compile）→ backlog #19。
- 状态：⚠️ 部分实现（--release ⏳ → backlog #19）
- 证据：`hc-tools/cli/args.rs` `parse_test_mode`（仅 interpret|compile）；历史 06-01 Q-T5

## 12.6 隔离与环境注入

- 规则：
  - 每个 test = **独立块作用域**（Q-T3）；测试环境由框架管理（隐式 `alloc` 预导入环境逐测试提供——`09` §9.7）。
  - ❌ **`test_io` 取消**（ADR-0010 决策 5，取代 Q-T4 的 test_io 注入）：测试直接调 `main()`；需要 io 的测试经 `import H.std.{io}` 使用环境。
  - IO 测试默认真实执行（Q-T4 后半保留）。
- 状态：✅ 已实现
- 证据：ADR-0010 决策 5；`implicit_env_value`（alloc 预导入）

## 12.7 测试文件位置（矛盾收敛）

- 规则：**双轨并存**——① 源码内 `[test]` 标注函数（Q8 原始定案）；② **`tests/` 项目根目录**测试文件（ADR-0026 规则 6：不参与命名空间，`import` 被测模块接口，仅 `hc test` 发现执行）。旧 06-13「无独立 tests/ 目录」与 06-08「tests/ 目录」矛盾由 ADR-0026 收敛。
- 状态：✅ 已实现（收集证据 ⚠️ tests/ 目录扫描归 hc-tools 核对）
- 证据：ADR-0026 规则 6 + 物理结构；`emit_test_runner`（is_test 收集）

## 12.8 示例形态四层（Q-T6 定案）

- 规则：示例测试四层分类——S1 纯逻辑断言 / S2 main smoke / S3 局部逻辑 / S4 演示标注（输出不捕获）。`--release` 仅运行正常路径子集（S1/S2 类）。
- 状态：✅ 定案（示例库落位归 examples 校订）

## 12.9 变更记录（相对旧 06-01 §2 测试段）

| 变更 | 依据 |
|---|---|
| `test_io` ❌（旧 Q-T4 注入形态废除） | ADR-0010 决策 5 |
| 「并发测试留 1.x」过时——`[test(async)]`/`[test(thread)]` 已实现 | `TestMode` 三态 + test_attribute.rs |
| `hc test --release` 第三档标 ⏳ → backlog #19（CLI 仅 interpret\|compile） | `parse_test_mode` |
| 原生 runner 失败即 abort ≠ Q-T2 失败不中止 → backlog #18 | `emit_test_runner` L658 注释 |
| 测试文件位置矛盾收敛（源内 `[test]` + `tests/` 双轨） | ADR-0026 规则 6 |
| 断言 API「失败不抛错、写 fail 记录」语义补录（旧文档「try 传播即失败」为其一途） | `call_expect_builtin` 注释 |
| 其余 hc CLI（run/build/cc/fmt/lint/doc）不在语法规范范围 → 工具链文档（hc-tools） | 规范范围界定（ADR-0034） |

## 12.10 待裁决清单

无——`test_io` 废除由 ADR-0010 收口（裁决规则 1），其余按实现证据直接对齐。
