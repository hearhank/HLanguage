# 模块系统：基于 IoC 容器的模块化架构

2026-08-25 定案。本 ADR 定义 H 语言的模块系统——以 `src/Modules/` 目录为物理载体、`IContext` 接口为依赖注入容器的模块化架构，取代 ADR-0010 中关于 `[module]` 特性的条款。

## 背景

原模块设计（ADR-0010 / CONTEXT.md §10）采用 `[module]` 特性标记作用于文件级命名空间，配合 `context` 关键字实现 IoC 模式。实践中发现：

1. `[module]` 标记是声明式属性，与目录结构无关，模块边界不直观
2. 模块的 context 缺乏明确的接口契约，不同模块的 context 实现不一致
3. 模块间依赖注入缺乏层级委托机制，所有依赖必须在同一层注册
4. 模块内部符号可见性规则不够清晰

## 决策

模块系统改为**目录驱动 + IoC 容器**架构：

### 物理结构

```
src/
├── main.hc              # 入口，命名空间 = 项目名
├── Modules/
│   ├── Auth/             # 模块，命名空间 = project.Auth
│   │   ├── context.hc    # 模块 context（IContext 实现）
│   │   ├── services.hc   # 模块内部实现
│   │   └── ...
│   └── Storage/          # 模块，命名空间 = project.Storage
│       ├── context.hc
│       ├── storage.hc
│       └── ...
└── utils/                # 普通代码（非模块）
    └── helpers.hc
tests/                    # 项目根目录，测试文件
├── test_auth.hc
└── test_storage.hc
```

### 规则

1. **`src/Modules/` 目录下的每个子目录 = 一个模块**。子目录名即模块名，编译器自动发现，无需手动声明。
2. **模块目录仅支持扁平结构**，不支持嵌套子模块。嵌套应通过独立包实现。
3. **每个模块必须定义 context**（`src/Modules/X/context.hc`），实现 `IContext` 接口。纯工具函数应放在 `src/` 下的非 `Modules/` 目录中。
4. **`[module]` 特性标记移除**。模块完全由目录结构定义。
5. **命名空间规则**：`src/` 根目录 = 项目名命名空间，`src/Modules/X/` = `project.X`。`namespace` 关键字保留，可覆盖默认路径命名空间。
6. **`tests/` 目录位于项目根目录**，不参与命名空间系统，仅由 `hc test` 发现和执行。测试文件通过 `import` 引入被测模块的接口。

### IContext 接口

```h
// H.std.ioc 提供
interface IContext {
    // 注册单例实例（深拷贝到 Arena，调用者管理原实例）
    fn register<T>(self, impl: T);
    // 命名单例（深拷贝到 Arena）
    fn register<T>(self, name: &[u8], impl: T);
    // 注册工厂方法（首次调用结果缓存到 Arena）
    fn registerFactory<T>(self, name: &[u8], factory: fn(ctx: &IContext) -> T);
    // 获取 Arena 引用（无所有权，不需要 defer）
    fn get<T>(self) -> *T;
    // 按名获取 Arena 引用（无所有权，不需要 defer）
    fn get<T>(self, name: &[u8]) -> *T;
    // 创建新实例（调用者拥有所有权，必须 defer 或 move）
    fn make<T>(self, name: &[u8]) -> owned T;
}
```

**内存管理规则**：
- `get<T>()` 返回 `*T`（Arena 引用，无所有权，不需要 `defer`）
- `make<T>(name)` 返回 `owned T`（调用者拥有，必须 `defer` 或 `move`）
- `register<T>(impl)` 在 Arena 中深拷贝一份，原实例由调用者自己管理
- `registerFactory<T>(name, fn)` 工厂首次调用结果缓存到 Arena，后续 `get` 返回缓存引用；`make<T>(name)` 每次调用工厂创建新实例

### Context 层级委托

```
AppContext (应用域，H.std.ioc.AppContext)
 ├── 注册全局服务（Database, Logger, Config...）
 ├── AuthContext (模块子域，继承 AppContext)
 │    ├── 注册模块特有服务
 │    └── get<T>() 未找到 → 委托 AppContext 查找
 └── StorageContext (模块子域，继承 AppContext)
      ├── 注册模块特有服务
      └── get<T>() 未找到 → 委托 AppContext 查找
```

- 子 context 持有父 context 引用，解析不到时向上委托
- 每个 context 背靠 Arena 分配器，context 销毁时所有通过它创建的对象一并销毁
- `IContext` 接口和 `AppContext` 实现在 `H.std.ioc` 模块中提供

### 模块面向接口编程

- 模块只知接口，不知具体实现。注册什么就用什么。
- 接口定义在提供该接口的模块中（如 `src/Modules/Auth/interfaces.hc`），使用方通过 `import project.Auth.{IUserService}` 引入接口类型。
- 模块的公开 API = context 结构体 + 接口定义。模块内部非 `pub` 符号对外不可见。
- 模块与标准库外的对象交流必须通过 context。标准库（`H.std`）可直接 `import` 使用。

### 引导流程

```h
// src/main.hc
import H.std.{io};
import H.std.ioc.{IContext, AppContext};
import myapp.Auth.{AuthContext, IUserService};
import myapp.Storage.{StorageContext, IFileService};

fn main() !void {
    // 1. 创建应用级 context
    var app_ctx = AppContext.init(alloc);
    defer app_ctx.deinit();

    // 2. 注册全局服务
    const user_svc = UserService{};
    const file_svc = FileService{};
    app_ctx.register(IUserService, user_svc);
    app_ctx.register(IFileService, file_svc);

    // 3. 初始化模块（注册到父 context 的子域）
    var auth = AuthContext.init(app_ctx);
    var storage = StorageContext.init(app_ctx);

    // 4. 运行应用
    run(app_ctx);
}
```

### 初始化与生命周期

- 初始化即注册：`AuthContext.init(app_ctx)` 将模块注册到父 context
- 懒加载实例化：`get<T>()` 按需创建对象
- 命名注册：同一接口可注册多个实现，通过 `name` 区分
- 工厂方法接收 context 引用，可在工厂内部解析依赖

### 测试

```h
// tests/test_auth.hc
import myapp.Auth.{AuthContext, IUserService};

[Test] fn test_auth_service() !void {
    var ctx = AuthContext.init(alloc);
    defer ctx.deinit();

    // 注入 mock 实现
    ctx.register(IUserService, MockUserService{});
    // 测试模块逻辑
    // ...
}
```

## 影响

### 兼容性

- `[module]` 特性标记移除：现有使用 `[module]` 的代码需迁移到 `src/Modules/` 目录结构
- `context` 关键字保留，但语义精化为 `IContext` 接口实现
- `import` 机制不变，仍用于引入接口类型

### 与现有概念的关系

| 概念 | 原设计 | 新设计 |
|------|--------|--------|
| 模块定义 | `[module]` 文件属性 | `src/Modules/X/` 目录 |
| context | 语言级关键字 | `IContext` 接口实现 |
| 模块可见性 | 文件级公开/私有 | 目录级：模块内私有，`pub` 接口公开 |
| 模块发现 | 手动声明 | 自动扫描 `src/Modules/` |
| 依赖注入 | 单层 context | 层级委托（父→子） |
| 接口位置 | 未明确 | 定义在提供方模块 |

### 实现计划

| 任务 | 预估时间 | 验证方式 |
|------|---------|---------|
| 1. `IContext` 接口定义（H.std.ioc） | 1h | 编译含 `IContext` 的 .hc 文件 |
| 2. `AppContext` 实现（Arena 背靠） | 1h | `hc test` 验证注册/获取/销毁 |
| 3. 编译器自动扫描 `src/Modules/` | 1h | 模块目录自动识别为命名空间 |
| 4. 模块 context 文件约定识别 | 1h | `context.hc` 自动关联为模块入口 |
| 5. 层级委托实现 | 1h | 子 context 委托父 context 查找 |
| 6. 工厂方法 + 命名注册 | 1h | 多实现注册与按名获取 |
| 7. 测试框架适配 | 1h | 测试中注入 mock |
| 8. 移除 `[module]` 特性标记 | 0.5h | 旧代码报错提示迁移 |