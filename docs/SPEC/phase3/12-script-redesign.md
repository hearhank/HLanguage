# 脚本功能重新设计

> 2026-08-23 定案（grill-with-docs 访谈）。关联：ADR-0013（script 块语义，**已废弃**）、[`00-feature-inventory.md`](../00-feature-inventory.md)（功能清单）、[`01-unimplemented-features.md`](01-unimplemented-features.md)（Phase 3 Backlog）。

## 背景

原设计（ADR-0013）中，`.hc` 文件包含 `script { }` 块，在编译期展开生成代码字符串。实践中发现这种设计有两个问题：

1. **职责不清**：`.hc` 文件既是编译代码又是脚本容器，`script { }` 块的双阶段执行（装载期求值 + 替换重解析）增加了编译器的复杂性
2. **工具链割裂**：`hc run` 需要区分 "普通 .hc" 和 "含 script 的 .hc"，执行路径不统一

新设计将脚本功能从 `.hc` 中剥离，`.hs` 成为唯一的脚本文件格式。

## 设计决策

### D1 — 脚本文件与编译代码分离

**决策**：`.hs` 后缀 = 脚本文件，`.hc` 后缀 = 编译代码文件。解释器只执行 `.hs` 文件。`.hc` 文件中的 `script { }` 块**删除**。

**理由**：职责分离——`.hc` 是编译代码，`.hs` 是脚本代码，两者执行路径不同，不互相混合。

### D2 — 脚本文件引用机制

**决策**：`.hs` 文件使用 `import "path"` 文件引用（`Decl::Include`），不通过命名空间组织。引用搜索路径：

1. **当前文件所在目录**（相对路径优先）
2. **SDK 目录**（`~/.hc/sdk/`）
3. **当前项目目录**（入口 `.hs` 文件所在目录）

**搜索规则**：先按字面路径搜索，找不到则依次尝试附加 `.hs` 后缀。

**理由**：脚本文件不需要命名空间和包管理，文件引用更直接。

### D3 — 脚本引用限制

**决策**：`.hs` 文件只能引用其它 `.hs` 文件（`.hs` 后缀）。引用 `.hc` 文件 → 编译错误。

**理由**：保持脚本系统和编译系统的清晰边界。

### D4 — 标准库访问

**决策**：`.hs` 文件保留通过 `import H.std.{io}` 访问标准库的能力。

**理由**：脚本需要 IO、分配器等基础功能，与编译代码共享标准库。

### D5 — Comptime 保留

**决策**：`comptime { }` 块在 `.hc` 文件中保留（与 `script { }` 独立）。`comptime` 是编译期求值（结果丢弃），不生成代码字符串。

**理由**：`comptime` 是类型函数、泛型具体化等编译期功能的必需机制，与脚本功能无关。

## 实现计划

### 1. 删除 `script { }` 块

| 文件 | 变更 |
|------|------|
| `hc/src/ast.rs` | 保留 `Decl::Script` 变体但标记废弃（或删除），移除 `KwScript` 关键字 |
| `hc/src/parser/decl.rs` | 删除 `KwScript` → `Decl::Script` 解析分支 |
| `hc/src/lexer.rs` | 移除 `script` 关键字（或保留但解析器忽略） |
| `hc-tools/src/scriptgen.rs` | 删除 `expand_scripts()`、`find_first_script()`、`eval_script()`、`ScriptSite`；简化 `parse_with_scripts()` |
| `hc-tools/src/run.rs` | 更新 `run_file_dangle_bench()` 直接解析 + comptime，不再调用 script 展开 |
| `hc-tools/src/scriptgen.rs` | 保留缓存函数（`source_cache_key`、`cache_dir`、`try_read_cache`、`write_cache`、`hs_cache_dir`、`hs_cache_key`）供 `.hs` 缓存使用 |

### 2. 扩展 `.hs` 文件引用搜索路径

| 文件 | 变更 |
|------|------|
| `hc-tools/src/run.rs` | 更新 `resolve_hs_includes()` 搜索路径：① 当前文件目录 → ② SDK 目录 → ③ 项目目录 |
| `hc-tools/src/scriptgen.rs` | 新增 `sdk_dir()` 函数返回 `~/.hc/sdk/` |

### 3. `.hs` 引用验证

| 文件 | 变更 |
|------|------|
| `hc-tools/src/run.rs` | `resolve_hs_includes()` 验证引用的文件后缀为 `.hs`，否则报错 |

### 4. 测试更新

| 文件 | 变更 |
|------|------|
| `hc-tools/tests/scriptgen.rs` | 更新测试：移除 `expand_scripts` 相关测试，保留 comptime 测试 |
| 示例文件 | 移除含 `script { }` 的示例或迁移到 `.hs` |

## 文件结构

### 脚本文件搜索路径

```
~/.hc/
├── cache/
│   ├── script/       # script 展开缓存（废弃）
│   └── hs/           # .hs 脚本文件缓存 (B6-2)
├── sdk/              # SDK 标准脚本库
│   └── *.hs
└── registry/         # 包注册中心
```

### SDK 目录

`~/.hc/sdk/` 存放系统级的 `.hs` 脚本库，安装 H 编译器时一并安装。搜索路径中最优先的是当前文件目录，SDK 是第二优先级。