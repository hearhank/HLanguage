# H 语言规范：模块与包

> 2026-08-25 重构（ADR-0026）：移除 `[module]` 特性标记，改用 `src/Modules/` 目录结构 + `IContext` IoC 容器。

## 命名空间

**文件路径即命名空间**——不再使用 C# 式块式 `namespace { }` 声明。规则：
- 根入口文件 `src/main.hc` 的命名空间 = **项目名称**（build.zon 的 `name` 字段）
- 标准库的根命名空间 = `H.std`
- 子目录文件的命名空间 = `{上级命名空间}.{当前文件夹名称}`
- 示例：`src/main.hc` → `{项目名}`；`src/Modules/Auth/interfaces.hc` → `{项目名}.Auth`
- 文件内不写 `namespace` 关键字时，自动归属上述路径命名空间
- 文件内写 `namespace abc { }` 时，**覆盖**默认路径命名空间，指定为 `abc`
- `namespace` 关键字仍可用于**显式覆盖**默认路径命名空间
- **命名规范**：命名空间名 `PascalCase`
_Avoid_: 一文件多命名空间

## 模块系统（`src/Modules/` 目录驱动）

`[module]` 特性标记已移除，模块由 `src/Modules/` 目录结构定义（ADR-0026）。

### 物理结构

```
src/
├── main.hc              # 入口，命名空间 = 项目名
├── Modules/
│   ├── Auth/             # 模块，命名空间 = project.Auth
│   │   ├── context.hc    # 模块 context（IContext 实现）
│   │   ├── interfaces.hc # 公开接口定义
│   │   └── services.hc   # 内部实现
│   └── Storage/          # 模块，命名空间 = project.Storage
│       ├── context.hc
│       ├── interfaces.hc
│       └── storage.hc
└── utils/                # 普通代码（非模块）
    └── helpers.hc
tests/                    # 项目根目录，测试文件
├── test_auth.hc
└── test_storage.hc
```

### 模块定义规则

1. **`src/Modules/` 下的每个子目录 = 一个模块**。子目录名即模块名，编译器自动发现，无需手动声明。
2. **模块目录仅支持扁平结构**，不支持嵌套子模块。嵌套应通过独立包实现。
3. **每个模块必须定义 context**（`src/Modules/X/context.hc`），实现 `IContext` 接口。纯工具函数应放在 `src/` 下的非 `Modules/` 目录中。
4. **模块内非 `pub` 符号对外不可见**。模块的公开 API = context 结构体 + 接口定义。
5. **模块与标准库外的对象交流必须通过 context**。标准库（`H.std`）可直接 `import` 使用。

### IContext 接口与 IoC 容器

`IContext` 接口定义在 `H.std.ioc` 中，提供 IoC 容器能力：

```h
interface IContext {
    fn register<T>(self, impl: T);                             // 注册单例（深拷贝到 Arena）
    fn register<T>(self, name: &[u8], impl: T);                // 命名单例
    fn registerFactory<T>(self, name: &[u8], factory: fn(ctx: &IContext) -> T);
    fn get<T>(self) -> *T;                                     // 获取 Arena 引用（无所有权，不 defer）
    fn get<T>(self, name: &[u8]) -> *T;                        // 按名获取 Arena 引用
    fn make<T>(self, name: &[u8]) -> owned T;                  // 创建新实例（调用者拥有，必须 defer）
}

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
      └── get<T>() 未找到 → 委托 AppContext 查找
```

- 子 context 持有父 context 引用，解析不到时向上委托
- 每个 context 背靠 Arena 分配器，context 销毁时所有通过它创建的对象一并销毁
- `IContext` 接口和 `AppContext` 实现在 `H.std.ioc` 模块中提供

### 模块面向接口编程

- 模块只知接口，不知具体实现。注册什么就用什么。
- 接口定义在提供该接口的模块中（如 `src/Modules/Auth/interfaces.hc`），使用方通过 `import project.Auth.{IUserService}` 引入接口类型。
- 模块内类型直接创建外部类型 → 编译错误。
- 模块间连接：`import` = 符号引用（类型/函数，API 面）；`context` = 数据/依赖注入——两者正交。

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
    app_ctx.register(IUserService, UserService{});
    app_ctx.register(IFileService, FileService{});

    // 3. 初始化模块（注册到父 context 的子域）
    var auth = AuthContext.init(app_ctx);
    var storage = StorageContext.init(app_ctx);

    // 4. 运行应用
    run(app_ctx);
}
```

### 初始化与生命周期

- 初始化即注册：`AuthContext.init(app_ctx)` 将模块注册到父 context
- 懒加载实例化：`get<T>()` 按需创建对象，对象随 context 销毁
- 同一接口可注册多个实现，通过 `name` 区分
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
}
```

## 导入语句 import

文件级导入语句（2026-08-17 定案，ADR-0010——取代 using）：

```hc
import H.std.{io as my};        // 符号选择 + as 别名（重名重命名）
import H.std.net.{http, tcp};   // 多符号选择
import pkg.mod;                 // 整模块导入
import pkg.mod as m;            // 整模块 + 别名
```

- **`import` 是文件级导入语句**（同包跨命名空间限定访问或符号选择；跨包需 build.zon 依赖 + pub）
- **`H.std`** = 内置标准库根路径，用户库经 build.zon 声明后按依赖名引用
- **依赖解析顺序**：(1) 系统 SDK 目录（`$H_HOME/sdk/<name>/`，未设置则回退 `~/.hc/sdk/<name>/`），(2) 当前项目目录
- **重名冲突规则**：同名导入符号冲突 → 编译错误，用户必须用 `as` 显式消歧
- **库符号访问规则**：库函数可直接调用；库类型需创建（`alloc.init(T)` 堆上 / 值字面量栈上）
- **import 与上下文分工**：`import` = **符号引用**（类型/函数，API 面）；模块间**数据连接走 context**（依赖注入式）——两者正交

## 可见性

- **默认私有**（`pub` 显式导出）；同包内 `pub` 项可见
- 跨包只暴露 `pub` 项
- 模块内非 `pub` 符号对外不可见（模块边界）

## 编译单元 / 文件模型

- **目录 = 包（package）**——包内全部 `.hc` 文件共享命名空间
- 跨包访问：`import pkg.mod` + `build.zon` 依赖声明
- `hc build` 编译包内全部文件；`hc run file.hc` 单文件脚本运行（隐式单文件包）；`hc run <目录>` 目录包运行
- **入口**：`fn main() !void`——`io`/`alloc` 为标准库模块与预导入环境（`import H.std.{io}` 显式引用）

## 包管理

- 包管理器内置编译器
- **包形态**：应用（`Kind::exe`，含 `main`）/ 库（`Kind::lib`，无 `main`，1+ 模块）
- 库产出：**lib 静态库**（编译时链接进 exe）或 **dll 动态库**（exe 运行时加载）
- **依赖清单** = H 数据字面量（`const build = Build{ ... }`，build.zon 式）
- 官方注册中心；`hc build` / `hc cc`（系统库自带、静态链接默认）

## `.hs` 脚本导入

`.hs` 文件使用 `import "path/to/file.hs"` 引用其他 `.hs` 文件。Parser 扩展：`import` 后跟字符串字面量 → 文件引用（AST 新增 `Decl::ImportFile` 变体）；跟标识符路径 → 模块引用（既有 `Decl::Import`）。脚本项目不需要 `build.zon`。
_Avoid_: 混用 `.hs` 文件引用与 `.hc` 模块引用