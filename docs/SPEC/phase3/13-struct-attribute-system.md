# Struct 类型与特性系统 — 实施计划

> 设计定案：ADR-0022（2026-08-24 grilling 会话）
> 优先级排序：P0（必须）→ P1（重要）→ P2（锦上添花）→ P3（未来）

## 当前状态：2026-08-24

Phase 1-4 基础功能已全部实现（Phase 4 自举推迟）。
以下为剩余待办任务，按优先级排序。

## 剩余任务列表

### 任务 1: 字段默认值 ✅（2026-08-24 完成）

| 功能点 | 状态 | 文件 |
|--------|------|------|
| 1.1 `FieldDecl` 添加 `default: Option<Expr>` | ✅ | `ast.rs` |
| 1.2 `parse_struct()` 存储默认值 | ✅ | `parser/type_decl.rs` |
| 1.3 `alloc.init(T)` 使用默认值 | ✅ | `hc-rt/src/interp/call.rs` |
| 1.4 `arena.init(T)` 使用默认值 | ✅ | `hc-rt/src/interp/call.rs` |
| 1.5 测试验证 | ✅ | `hc-rt/tests/semantics.rs` |

### 任务 2: 字段级 `[Align(n)]` ✅（2026-08-24 完成）

| 功能点 | 状态 | 文件 |
|--------|------|------|
| 2.1 `continuous_layout()` 读取字段级 Align 特性 | ✅ | `hc-rt/src/interp/layout.rs` |
| 2.2 `continuous_align()` 优先使用字段级对齐 | ✅ | `hc-rt/src/interp/layout.rs` |
| 2.3 测试验证 | ✅ | `hc-rt/tests/semantics.rs` |

### 任务 3: 特性解析改用字典查找 ✅（2026-08-24 完成）

| 功能点 | 状态 | 文件 |
|--------|------|------|
| 3.1 `TraitRegistry` 添加 `TraitHandlerFn` 注册 | ✅ | `trait_registry.rs` |
| 3.2 `parse_trait()` 改用字典查找构建 | ✅ | `parser/decl.rs` |
| 3.3 注册表测试 | ✅ | `trait_registry.rs` |
| 3.4 清理旧硬编码分支 | ✅ | `parser/decl.rs` |

### 任务 4: `IAttribute` 接口 ✅（2026-08-24 完成）

| 功能点 | 状态 | 文件 |
|--------|------|------|
| 4.1 定义 `AttributeKind` 区分系统/用户特性 | ✅ | `trait_registry.rs` |
| 4.2 注册表支持注册用户特性（IAttribute struct） | ✅ | `trait_registry.rs` |
| 4.3 注册表识别 `IAttribute` struct | ✅ | `trait_registry.rs` |
| 4.4 测试验证 | ✅ | `trait_registry.rs` |

## 完成总结

### 已实现

| 任务 | 状态 | 描述 |
|------|------|------|
| 1. 字段默认值 | ✅ | `FieldDecl` 添加 `default` 字段，`alloc.init`/`arena.init` 使用默认值 |
| 2. 字段级 `[Align(n)]` | ✅ | 布局计算支持字段级对齐 |
| 3. 特性解析改用字典查找 | ✅ | `parse_trait()` 通过 `TraitRegistry` 字典查找 |
| 4. `IAttribute` 接口基础设施 | ✅ | `AttributeKind` 区分系统/用户特性 |
| 5. 扩展方法 | ✅ | `[Extension(Type)] fn method(...)` 语法 + 运行时方法分派 |
| 6. Struct 化 test 特性 | ✅ | 支持 `[test{name="x", mode=async}]` 和 `[align{value=8}]` 语法 |

### 未实现（推迟到 1.x）

- 第四阶段自举（Phase 4 Bootstrapping）

## 执行顺序

按序号顺序执行，每个功能点完成后必须：
1. 编译验证（`cargo build`）
2. 运行相关测试（`cargo test -p hc && cargo test -p hc-rt`）
3. 上传到 git
4. 再继续下一个功能点