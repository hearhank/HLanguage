<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **HLanguage** (7490 symbols, 25346 relationships, 357 execution flows).

> Index stale? Run `node .gitnexus/run.cjs analyze --index-only` from the project root — it auto-selects an available runner. No `.gitnexus/run.cjs` yet? Bootstrap with `npx`, `bunx`, or `pnpm dlx` — e.g. `bunx gitnexus@latest analyze` (npm 11 npx crash; #1939).

## Always Do

- **MUST run impact analysis before editing.** Use `impact({target: "symbolName", direction: "upstream"})` (MCP) or `node .gitnexus/run.cjs impact "symbolName" --direction upstream --repo .` (CLI fallback); report callers, processes, and risk. Never substitute grep for graph analysis.
- **MUST analyze graph changes before committing.** Use `detect_changes({scope: "all"})` (MCP) or `node .gitnexus/run.cjs detect-changes --scope all --repo .` (CLI fallback). `partial: true` or `truncated: true` is not a clean check — a zero means unseen, not unaffected; re-run it. For regression review: `detect_changes({scope: "compare", base_ref: "master"})` or `node .gitnexus/run.cjs detect-changes --scope compare --base-ref "master" --repo .`.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- **MUST treat `risk: UNKNOWN` as unresolved, not as low.** An empty caller set is not evidence the symbol is unused — it can also mean the callers are not resolvable by the index (plain-object property access, dynamic dispatch, cross-language calls). `impact` pairs `UNKNOWN` with a `riskNote` saying so. Confirm with a text search before treating the symbol as safe to change or delete; do not proceed on the strength of a zero.
- When exploring unfamiliar code, use `query({search_query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `context({name: "symbolName"})`.
- For security review, `explain({target: "fileOrSymbol"})` lists taint findings (source→sink flows; needs `analyze --pdg`).

## Never Do

- NEVER edit a function, class, or method before MCP/CLI impact analysis.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis, and never read `UNKNOWN` as an all-clear — it means the walk could not answer, which is the one verdict that requires confirming by other means.
- NEVER rename symbols with find-and-replace — use `rename` which understands the call graph.
- NEVER commit before MCP/CLI graph change analysis.

## Resources

| Resource | Use for |
| --- | --- |
| `gitnexus://repo/HLanguage/context` | Codebase overview, check index freshness |
| `gitnexus://repo/HLanguage/clusters` | All functional areas |
| `gitnexus://repo/HLanguage/processes` | All execution flows |
| `gitnexus://repo/HLanguage/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
| --- | --- |
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->

# 语法规范权威来源（2026-08-30 定案）

一切语言功能实现与语法检查以 `docs/SPEC/syntax/`（H 语言语法功能说明，一模块一文件）为**唯一依据**：

1. **唯一依据**：功能实现、语法检查、诊断文案、示例编写均以 `docs/SPEC/syntax/` 为准。与其冲突的旧文档（`docs/SPEC/phase1/06-*` 系列、`docs/H Language.md`、`docs/native-types.md`）一律视为历史资料，不得作为实现依据。
2. **文档先行**：修改语法行为前，先修改对应模块文档（必要时先落 ADR），再改代码；未同步文档的语法改动视为未完成。
3. **禁止双写**：任何语法规则只在一处定义，其他位置以相对路径引用；发现重复描述立即收敛到单一来源。
4. **状态同步**：模块文档中每个功能点带实现状态标记（✅ 已实现 / ⚠️ 部分实现 / ⏳ 未实现·目标 / ❌ 已废弃）与证据路径（tag1 测试/源码）；代码行为变化时同步更新。
5. **对齐流程**：逐模块循环——盘点（ADR + 计划文档 + tag1 代码）→ 草案 + 待裁决清单 → 项目所有者裁决 → 定稿。自举链路（stage1/stage2）不在本规范覆盖范围。

<!-- gitnexus:end -->
