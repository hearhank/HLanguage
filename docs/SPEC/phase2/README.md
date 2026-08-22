# 第二阶段存档：最小外围（已实现）

> 对应第三块划分中的**第二块**（`07-bootstrap-plan.md` §三：M5 最小标准库 / M6 测试基建 / M7 工具链最小；执行细表 `09-part2-execution.md` 组 A–H 全完成）。本文件夹为**已实现文档的存档副本**，权威规范仍以 `docs/SPEC/` 为准。
>
> 存档时间：2026-08-22（阶段重组）。

## 交付摘要

第二阶段（第二块最小外围）已达成 T4（`07-bootstrap-plan.md` §六）：

- **`hc build` / `hc run` / `hc test` 完整**（最小功能集可用，不自举）
- io 入口调整（`main(args)`）、`import` 导入取代 `using`（ADR-0010）
- io.print 格式串补全（B 组）
- 包形态：目录运行 + 库产出（静态库 / dll，C 组）
- serialize 库（D 组）、`arena.init(T)` typed 构造（E 组）
- 测试空白补全（F 组）
- **线程生命周期提前**（spawn / join / cancel / is_done / detach，G 组）
- 代码管理四项（项目结构 / 引用库 / 模块 / 文档生成，H 组）

## 文档索引（存档副本）

| 文件 | 内容 |
|---|---|
| `04-stdlib-scope.md` | 标准库范围（四大支柱 + 系统编程扩展） |
| `06-08-modules.md` | 模块与包（namespace / import / pub / build.zon） |
| `09-part2-execution.md` | 第二部分执行细表（A–H 全完成 + 完成注记） |

## 关联决策记录（ADR）

- 0010 入口与导入（`main(args)` + `import` 取代 `using`）

## 实施落点（代码）

- `tag1/hc-tools/`：CLI 扩展（`hc run <目录>` / `hc build --dll` / `hc pkg add` / `hc doc` / `hc init`）
- `tag1/hc-rt/`：线程生命周期（spawn / join / cancel / detach，协作式）
