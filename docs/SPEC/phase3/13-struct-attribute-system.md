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

### 任务 3: 特性解析改用字典查找（P1，~1h）

| 功能点 | 预估 | 验证方法 |
|--------|------|---------|
| 3.1 `TraitRegistry` 添加 `TraitBuilder` 回调注册 | 20min | 注册表可返回 `Trait` 枚举值 |
| 3.2 `parse_trait()` 改用字典查找构建 | 20min | 解析 `[pad]` `[align(8)]` `[test]` 行为不变 |
| 3.3 添加注册表测试 | 15min | 注册/查找/未知特性报错 |
| 3.4 清理旧硬编码分支 | 10min | 编译通过，测试不变 |

### 任务 4: `IAttribute` 接口（P2，~1h）

| 功能点 | 预估 | 验证方法 |
|--------|------|---------|
| 4.1 定义 `IAttribute` 系统接口 | 15min | 接口定义编译通过 |
| 4.2 struct 实现 `IAttribute` 标记 | 15min | `MyAttr` 实现 `IAttribute` 可注册为特性 |
| 4.3 注册表识别 `IAttribute` struct | 30min | 注册表可注册用户 struct 为特性 |

### 任务 5: 扩展方法（P3，~2h）

| 功能点 | 预估 | 验证方法 |
|--------|------|---------|
| 5.1 解析 `[Extension(StructType)] fn method(...)` | 30min | 解析正确 |
| 5.2 语义分析：扩展方法绑定 | 30min | 扩展方法可通过实例调用 |
| 5.3 运行时方法分派 | 45min | 调用正确的扩展方法 |
| 5.4 测试 + 边界检查 | 30min | 私有字段不可访问 |

### 任务 6: Struct 化 test 特性（P3，~1h）

| 功能点 | 预估 | 验证方法 |
|--------|------|---------|
| 6.1 `[test{name="x", mode=async, timeout=5}]` 语法 | 30min | 解析正确 |
| 6.2 兼容旧 `[test("x")]` / `[test(async)]` 语法 | 15min | 旧语法继续工作 |
| 6.3 测试 | 15min | 新旧语法均通过 |

## 执行顺序

按序号顺序执行，每个功能点完成后必须：
1. 编译验证（`cargo build`）
2. 运行相关测试（`cargo test -p hc && cargo test -p hc-rt`）
3. 上传到 git
4. 再继续下一个功能点