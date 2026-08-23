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

## C8 LLVM 原生内建扩展

| 子任务 | 预估 | 状态 |
|--------|------|------|
| 1a 分析 mismatch | 20min | 🔴 |
| 1b String 原生内建 | 40min | 🔴 |
| 1c 序列化原生内建 | 40min | 🔴 |
| 1d 验证+更新基线 | 20min | 🔴 |