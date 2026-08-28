# 0028-tag1-rust-refactor-split.md

# tag1 Rust 代码整理：超长函数与超长文件拆分

tag1 的 Rust 实现（4 个 crate：hc / hc-rt / hc-tools / hc-lsp）中，多个核心函数超过 300 行、多个文件超过 1000 行（如 `hc/src/ir/builtin.rs` 4288 行、`hc/src/ir/mod.rs` 2028 行），单一函数/文件承载过多职责，难以维护与 AI 导航。决定按三条规则整理：

1. **超长函数拆分**：函数内有分支且行数 >400 → 按逻辑块拆成同文件私有 helper，主函数保留 dispatch 骨架（每个 helper 尽量 <100 行、单职责）。（修订 2026-08-29：原阈值 >300，调整为 300~400 行不拆、单职责优先；`encode_inst`（343 行）、`decode_inst`（269 行）据此未拆。）
2. **超长文件拆分**：文件含数据类型与函数、可分多类型、且总行数 >1000 → 按功能模块拆分；调用层级低（fan-in 高、被依赖）的为主文件（保持文件名不变），调用层级深（依赖主文件）的为子文件。
3. **类型/函数分离**：类型与函数混合时，类型移到 `<crate>/src/models/`（snake_case 文件名 = 类型名），函数按功能命名文件。

**选择原因**：保持 `pub` 符号名与路径不变（经 `pub use` 重导出），下游（hc-lsp、hc-tools）与自举对照（stage1 K1–K3 为行为对照）不受影响；拆分以 `cargo test --workspace` 全绿为验收门槛。范围限于 4 个主 crate 的 `src/`，不含 `tests/` 集成测试与 `hc-tools/hc-rt/` 实验副本。

**考虑过的替代**：不拆分（放弃——文件过大已不可维护）；按后端/模块横向重组（放弃——破坏现有模块边界，迁移成本高）；规则 2 的"主文件名+类型名"子文件命名 vs 规则 3 的"models/类型名"——统一采用规则 3 的组织，规则 2 的命名仅用于纯函数超长文件。

## 落地记录

| 日期 | 对象 | 结果 |
|---|---|---|
| 2026-08-28 | `hc-rt/src/value.rs`（1028 行 → 65 行） | 类型 → `value/models/`：19 个类型文件（一类型一文件，snake_case 命名）+ `models.rs` 声明子模块并 `pub use` 重导出；函数 → `value/display.rs`（显示/格式化）、`value/compare.rs`（比较：value_eq/value_lt）、`value/query.rs`（类型查询：as_bool/type_name）、`value/bytes.rs`（字节转换：extract_bytes）；`value.rs` 保留模块声明、`pub use models::*` 重导出与最高层构造入口（int/bool/str_bytes/str/arr/vec/map/class）。低层辅助跟随调用方：`align_up` 随 `ArenaState`、`copy_alloc_block`（可见性收窄为 `pub(super)`）随 `AllocatorTrait`。`pub` 符号路径经 `pub use models::*` 全量保持不变；验收：`cargo build -p hc-rt` 0 错误（19 warnings = 基线），`cargo test -p hc-rt` 34 个二进制 476 passed / 0 failed（= 基线） |
| 2026-08-29 | `hc-tools/src/lint/mod.rs`（1579 行 → 65 行） | 类型 → `lint/models/`：2 个类型文件（LintRule→lint_rule.rs、LintDiag→lint_diag.rs）+ `models.rs` 声明子模块并 `pub use models::*` 重导出；函数 → 12 个功能文件：rules.rs（RULES/all_rules/find_rule）、disable_comments.rs、collect_decls.rs、collect_refs.rs、unused_var.rs、unused_import.rs、simplifiable_construct.rs、upper_case_abbr.rs、simplifiable_if_else.rs、redundant_eq_false.rs、json.rs（diags_to_json）；`mod.rs` 保留模块声明、重导出与最高层入口 `lint_source`。`pub` 路径全量保持（all_rules/find_rule 经 `pub use` 重导出，加 `#[allow(unused_imports)]` 保持 bin target 基线）；验收：`cargo build -p hc-tools` 0 错误（13 warnings ≤ 基线 14），`cargo test -p hc-tools` 全绿 |
| 2026-08-29 | `hc-tools/src/cli/mod.rs`（1428 行 → 273 行） | 类型 → `cli/models/`：2 个类型文件（TestMode→test_mode.rs、DangleMode→dangle_mode.rs）+ `models.rs` 声明子模块并 `pub(crate) use models::*` 重导出（cli 符号原为 pub(crate)，用 `pub use` 重导出会触发 unused_imports）；函数 → 11 个功能文件：args.rs（extract_dangle/parse_dangle_mode/parse_test_mode）、colors.rs、usage.rs（USAGE）、read_source.rs、dump.rs（parse_command+dump_ast/decl/param/method/block/stmt/expr）、lex.rs、doc.rs、check.rs（check_file/errors_file）、fmt.rs、lint.rs、cc.rs；`mod.rs` 保留模块声明、`pub(crate) use` 重导出与最高层入口 `run_cli`。原私有 fn `read_source` 的未用重导出删除（私有符号无路径契约）；`crate::cli::{color_test_line, err_color, out_color, paint, DangleMode, TestMode}` 等外部引用路径经重导出保持不变；验收：`cargo build -p hc-tools` 0 错误（13 warnings ≤ 基线 14），`cargo test -p hc-tools` 11 个二进制 139 passed / 0 failed 全绿 |
| 2026-08-29 | `hc/src/ir/builtin.rs` 函数拆分 | `call_builtin` 839→49：抽 10 个功能组 helper（内省/诊断/指针转换/volatile/atomic/数值/格式/算法/解析/断言）+ 先期 box/unbox/copy/spawn/channel；`call_builtin_method` 648→166：string/array/array_bytes/iter/sync 5 组 helper，Class 分派表保留主函数；`call_intrlist_method_ir` 524→62：push_front/pop_front/push_back/pop_back/remove 5 方法 helper + 字段访问自由函数（替代捕获 class_cell 的闭包）；验收：`cargo test -p hc` 13 套件全绿 |
| 2026-08-29 | `hc/src/ir/runtime.rs` `exec_body`（623 行 → 96 行） | 拆 11 个指令组 helper：exec_data/exec_jumps/exec_call/exec_return/exec_ptr/exec_aggregate/exec_make/exec_pattern_iter/exec_closure/exec_global/exec_call_dynamic；Jump 系 `continue` 语义经返回 `Some(target)` 保持，Call 早退 `return Ok(())` + 主循环 `pc += 1` 补齐；验收：`cargo test -p hc` 全绿 |
| 2026-08-29 | `hc/src/ir/mod.rs`（2028 行 → 61 行） | 类型 → `ir/models/`：31 个类型文件（一类型一文件）+ `models.rs` 声明并 `pub use` 重导出（goroutine 组私有符号不入 re-export 以保 warning 基线 35）；函数 → 5 个功能文件：access.rs（字段/下标/切片）、iter.rs、value_ops.rs、eq.rs、pattern.rs；`mod.rs` 保留模块声明、重导出、`MAX_CALL_DEPTH`、入口 `run_ir`；Ctx 7 个私有方法调整为 `pub(in crate::ir)` 供跨模块调用；验收：`cargo test -p hc` 13 套件 458 tests 全绿 |
| 2026-08-29 | 全部完成验收 | `cargo test --workspace` 61 套件全部 ok、0 failed |
