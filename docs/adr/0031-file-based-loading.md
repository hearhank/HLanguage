# 按文件加载与命名空间模型：追认 ADR-0026，同目录扁平共享

**Status**: accepted（2026-08-29，grilling 会话定案）
**关联**: CONTEXT.md §10（ADR-0026 文件路径即命名空间）、`docs/SPEC/phase1/06-13-project-structure.md`、`docs/SPEC/phase2/06-08-modules.md`、`docs/SPEC/phase4/06-k5-execution-plan.md`（S2/S3）

## 决策

stage2 多文件化暴露了三方矛盾：Q21「包内共享命名空间」（06-13 文件模型节）、可见性节「默认私有 pub 导出」、M1.4 loader 实现「兄弟文件顶层 fn 不登记（文件私有）」——`hc run stage2` 下 main.hc 调用 `lex_source` 报 `NoFunction` 即此矛盾的爆发。裁决：

1. **追认 ADR-0026（2026-08-25，晚于全部矛盾文本）**：文件路径即命名空间——`src/*.hc` 同目录文件同命名空间，**扁平共享、无需 import**；跨命名空间（子目录/Modules）走既有 `import NS.{sym}` / 限定调用（ADR-0010）。
2. **Rust loader 修复**：`register_fn_decl_prefixed_filter` 的兄弟扁平压制（`skip_entry && prefix.is_empty()` 与 `!skip_entry && !skip_flat` 组合）对「推断命名空间 == 入口命名空间」的兄弟文件放行——同命名空间按自有文件登记扁平名；同名冲突 → 编译错误（列两处来源，对齐 ADR-0010 重名规则），不做静默覆盖。
3. **stage1 interp 包模式**：入口文件名为 `main.hc` 时自动加载同目录兄弟 `.hc`（字典序合并，确定性），实现 ADR-0026 包语义；单文件保持 P7 `import .{sym}` 装载（stage1 interp 专有扩展，**不入语言规范**——Rust parser 按 ADR-0010 拒绝该形态是正确行为）。
4. **`import .{sym}` 不入规范**；`.hs` 的 `import "path"`（ImportFile）保持既有 spec 独立实现。M7 模块（src/Modules/）不受影响——文件导入/包模式均为其前置垫层。

## Considered Options

- **Q21 字面真共享（改 loader 全量扁平注册）**：被否——同名重载池污染需引入冲突规则，且隐式共享使「程序由哪些文件组成」不可见。
- **显式按文件导入为唯一通道（废除包模式自动加载）**：被否——与 ADR-0026 文件路径命名空间模型冲突（同目录同命名空间本应扁平可见），且自举编译器需额外实现目录扫描之外的导入图解析。
- **`import .{sym}` 入规范**：被否——与 ADR-0010 的标识符路径模块引用形态重叠，双轨导入机制违背「_Avoid_: 多套导入机制并存」。

## Consequences

- stage2 采用**自包含单文件**形态（对齐 stage1 四件套惯例），多文件拆分在「同命名空间扁平共享」落地后回归（S3 parser 提取即为首个用户，无需任何 import 语句）。
- stage1 interp 的 P7 `import .{sym}` 保留为工具链扩展（单文件链路显式加载），不参与规范。
- 同命名空间同名声明 → 编译错误（此前被 loader 静默跳过，现为响亮诊断）。
- 前置阻塞：stage1 interp 类实例缺陷（probe-tok6：类实例经函数返回 + Vec 存储后引用型字段丢失）——不修则 stage1 链路无法端到端验证多文件。
