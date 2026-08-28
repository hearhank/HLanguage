# 第四阶段执行状态

> 跟踪 Phase 4 功能实现进度。按优先级+难易程度排序。

## ②-3 IntrList 侵入式链表 ✅

| 子任务 | 文件 | 状态 | 验证 |
|--------|------|------|------|
| 1a-core | `hc/src/ds_intrlist.rs` | ✅ | 11 单元测试通过 |
| 1b-io | `hc-rt/src/interp/io.rs` | ✅ | ✅ |
| 1b-ir | `hc/src/ir/builtin.rs` | ✅ | ✅ |
| 1b-dispatch | `hc-rt/src/interp/call.rs`, `eval.rs`, `hc/src/ir/runtime.rs` | ✅ | ✅ |
| 1b-tests | `hc-rt/tests/intrlist.rs`, `consistency.rs` | ✅ | 11 解释器测试 + 1 一致性测试通过 |
| 提交 | `584767d` | ✅ | `cargo test --workspace` 全绿 |

## ②-4 TreeMap（BST 有序映射）✅

| 子任务 | 文件 | 状态 | 验证 |
|--------|------|------|------|
| 1a-core | `hc/src/ds_treemap.rs` | ✅ | 16 单元测试通过 |
| 1a-dispatch | 注册 namespace + dispatch 分派 | ✅ | ✅ |
| 1b-io | `hc-rt/src/interp/io.rs` | ✅ | ✅ |
| 1b-ir | `hc/src/ir/builtin.rs` | ✅ | ✅ |
| 1b-tests | `hc-rt/tests/treemap.rs` + consistency | ✅ | 8 解释器测试 + 1 一致性测试通过 |
| 提交 | `TBD` | ✅ | `cargo test --workspace` 全绿 |

## A8 TCP 聊天室示例

| 子任务 | 预估 | 状态 |
|--------|------|------|
| 1a 实现服务器+客户端 | 50min | 🔴 |
| 1b 集成测试+双模式验证 | 30min | 🔴 |

## C8 LLVM 原生内建扩展 ✅（2026-08-26）

| 子任务 | 预估 | 状态 |
|--------|------|------|
| 1a 分析 mismatch | 20min | ✅ |
| 1b @ 内建缺失 | 40min | ✅ 已提交 cd45c82 |
| 1c 修复 `bitcast i128 to double` 无效指令（`bin` 浮点运算/比较 + `call_builtin` min/max 浮点路径） | 20min | ✅ |
| 1d 修复 `CallIndirect` T_FN/T_CLOSURE 路径传递槽指针而非值 | 20min | ✅ |
| 1e 验证 LLVM 单元测试 | 10min | ✅ 60 全绿 |
| 1f 验证交叉验证 | 20min | ✅ 40→16 mismatch |

**结果：`zig cc 编译失败` 24 例全部修复（0 例）。**

### 剩余 16 mismatch 分析（非 LLVM 后端问题）

| 类别 | 数量 | 原因 | 归属 |
|------|:----:|------|------|
| `defer try f()` 体控制流 | 11 | 设计内硬错误，`defer` 体不允许控制流 | IR 降级器 |
| `Vec` 字面量构造 | 3 | `Vec<T>{}` 非 class/enum，IR 降级器不支持 | IR 降级器 |
| 解释器失败 vs 原生退出 0 | 2 | 解释器 `error.NoField` 但原生正确 | 解释器 tree-walking |

> 注：`back`/`get`/`put`/`remove` 集合方法、全部字符串方法（concat/split/find/substring/replace/to_upper/to_lower）、`front`/`back`/`get`/`put` 等方法早已在 LLVM 后端实现。现状表已过时，现更新。

## P1.5 自检崩溃修复 ✅（2026-08-29）

**症状**：`hc run stage1/checker.hc stage1/checker.hc`（及 lexer/parser 自身）→ 解释器 `error.NoField at 0:0` 响亮中止（吃狗粮暴露，K3 对照仅覆盖语料文件未覆盖自身）。

**根因与修复**（`stage1/checker.hc`）：

| # | 根因 | 修复 |
|---|------|------|
| 1 | `ty_of` / `type_of_expr`（Ident/Call 分支）对 `Map.get` 返回的 `?SType`/`?FnSig` 未解包直接取字段（`sig.ret_type`）——违反 `?T` 显式解包规范（`06-02-types.md`） | 补 `if (t) \|tt\| { return tt; }` 解包（3 处） |
| 2 | `parse_if_stmt`/`parse_while_stmt` 解析 `\|payload\|` 后丢弃绑定名 → 自检时载荷变量全部误报 undefined | 载荷名经 `node_add_prop` 存入 `payload`/`payload_err` 属性，`check_if`/`check_while` 注册到作用域 |

**归属说明**：解释器 NoField 为符合规范的行为（可选值无字段），非解释器 bug；Rust 语义检查器未在编译期拒绝 `.field` 作用于 `?T`（语义检查缺口，登记待办）。

**验证**：`cargo test --workspace` 59 套件全绿（K3 对照 13→14 项，新增 `self_check_completes_on_stage1_sources`）；示例回归 interpret 161 passed / 0 failed / 1 skipped 不漂移。

**余量**：checker 类/方法/字段建模不全（误报 690/1616/2387 行，非崩溃）→ K4 计划前置任务 K3.5（`05-k4-execution-plan.md`）。