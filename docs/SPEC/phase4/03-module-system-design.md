# 模块系统 + Context/IOC 设计

> 2026-08-23 定案（grill-with-docs 访谈）。关联：`00-feature-inventory.md`（功能清单）、`01-unimplemented-features.md`（Phase 3 Backlog）、`02-1x-delayed-items.md`（1.x 延迟项）。

## 术语表

| 术语 | 定义 |
|------|------|
| **命名空间 (Namespace)** | 符号组织单元，默认 = 文件相对目录路径。一个文件只能有一个命名空间。 |
| **模块 (Module)** | 带有 `[module]` 标记的命名空间，是高度集中的功能集，有明确边界和依赖。 |
| **Context** | 模块的全局连接点，负责与外部类型库通信。模块内的类型不直接创建外部类型，必须通过 Context。 |
| **IOC (Inversion of Control)** | 控制反转模式，通过 Context 注册接口 → 模块通过 Context 获取实现。 |
| **项目 (Project)** | 由 `build.zon` 描述的代码集合，有根命名空间（= 项目名称）。 |
| **SDK 目录** | `~/.hc/sdk/`，系统级库搜索路径。 |
| **包 (Package)** | 有 `build.zon` 的可复用代码单元，通过 `import pkg.name` 引用。 |

---

## 一、命名空间系统

### 1.1 自动命名空间解析

**决策**：一个文件只能有一个命名空间。默认命名空间 = 文件相对目录路径。

```
project/
├── build.zon           # 项目名 = "myapp" → 根命名空间 = myapp
├── main.hc             # 命名空间 = myapp (根)
├── math/
│   ├── vec.hc          # 命名空间 = myapp.math (目录继承)
│   └── matrix.hc       # 命名空间 = myapp.math
└── net/
    └── http/
        └── client.hc   # 命名空间 = myapp.net.http
```

**规则**：
1. 如果文件写了 `namespace myapp.xxx {}`，则显式指定命名空间
2. 如果文件没有写 `namespace {}`，则命名空间 = 项目根命名空间 + 文件相对目录路径
3. 编译时按命名空间组织在一起编译（同命名空间的内容合并）

### 1.2 根命名空间

**决策**：根命名空间 = 项目名称（从 `build.zon` 的 `name` 字段读取）。

```zon
// build.zon
const build = Build{
    name = "myapp",
    version = "0.1.0",
    kind = Kind.exe,
    files = [ "main.hc", "math/vec.hc" ],
};
```

标准库的固定命名空间为 `H.Std`，不可更改。

### 1.3 显式命名空间

**决策**：文件可以显式写 `namespace 名称 { ... }` 来指定命名空间。编译时，该文件的内容按指定命名空间编译。

```hc
// file: math/extra.hc
namespace myapp.math.extra {
    fn distance(a: f64, b: f64) f64 { ... }
}
```

如果没有写 `namespace`，则文件内容属于它的目录路径对应的命名空间（见 1.1）。

---

## 二、模块系统

### 2.1 [module] 标记

**决策**：`[module]` 是一个特性标记（Trait），作用于命名空间。

```hc
[module] namespace Orders {
    // 模块内容：类型、函数等
}
```

`[module]` 标记的含义：
1. 该命名空间是一个**模块**——高度集中的功能集，有明确的边界和依赖
2. 在 `main` 中注册，在使用时加载
3. 模块内容与其它命名空间隔离

### 2.2 模块边界规则

**D1 — 无子命名空间**：`[module]` 标记的命名空间不存在子命名空间。其子文件夹的内容编译为**私有**的，对外部调用者不可见。

```
myapp.modA/            # [module] 标记
├── modA.hc            # 命名空间 = myapp.modA, 公开内容
├── internal/
│   ├── helper.hc      # 编译为私有，外部不可见
│   └── impl.hc        # 编译为私有，外部不可见
```

**D2 — Visibility 规则**：
- `pub` 标记的类型和函数——包外（模块外）可见
- 非 `pub` 标记——仅在模块内可见，同一包（可能有多个模块）也不行
- 模块的子文件夹内容默认私有（对外不可见）

### 2.3 Context 要求

**D3 — 模块必须定义 Context**：每个 `[module]` 标记的命名空间必须定义一个 `Context` 类型。这是编译时检查项。

```hc
[module] namespace Orders {
    // Context 类型：模块与外部世界的连接点
    pub class Context {
        // 存储外部接口的注册表
        // ...
    }
    
    // 模块内的类型通过 Context 创建外部类型
    pub fn process(ctx: *Context, data: i32) !void {
        // 不直接创建外部类型，通过 ctx 获取
    }
}
```

如果 `[module]` 命名空间没有定义 `Context` 类型 → 编译错误。

### 2.4 模块注册与加载

**决策**：模块在 `main` 中注册，在使用时加载。

```hc
// main.hc
import myapp.Orders as Orders;

fn main() !void {
    // 创建模块的 Context，注入依赖
    var ctx = Orders.Context{
        db = myapp.Db.init(alloc),
        cache = myapp.Cache.init(alloc),
    };
    // 注册 Context 到模块
    Orders.register(&ctx);
    
    // 使用模块功能
    try Orders.process(42);
}
```

---

## 三、Context / IOC 系统

### 3.1 Context 接口

**决策**：Context 是模块的 global 存储，模块内的所有类型对象默认可以访问。Context 需要一个接口来定义具体功能，本质上是一个字典，可以添加不同的对象。

```hc
// Context 接口定义
pub interface IContext {
    fn get(ty: type) ?*T;
    fn set(ty: type, obj: *T) void;
}
```

**关键设计原则**：
1. Context 作为模块的 global 存储，模块内所有类型/函数可访问
2. Context 通过接口注册对象（类似 Windows IOC 模式）
3. 接口可以继承，可以有默认实现，但不能被实例化

### 3.2 IOC 模式

**决策**：采用 IOC（控制反转）模式，通过 Context 注册和获取依赖。

```
Context 注册接口实现 → 模块内部通过 Context 获取实现 → 模块使用实现
```

**实现方式**：
1. Context 在初始化时注册真实类型
2. 模块内部通过 Context 实例化对象然后使用
3. 模块内的类型不直接创建外部类型，必须通过 Context 来创建

```hc
// 接口定义
pub interface IRepository {
    fn find(id: i32) !Order;
    fn save(order: *Order) !void;
}

// Context 定义
pub class Context {
    // 内部字典：类型 → 实现
    var registry: Map<type, *T>;
    
    pub fn register<T>(ty: type, impl: *T) void {
        registry.set(ty, impl);
    }
    
    pub fn get<T>(ty: type) ?*T {
        return registry.get(ty).as(*T);
    }
}

// 模块内部使用
fn process(ctx: *Context, data: i32) !void {
    var repo = ctx.get(IRepository).?;
    var order = try repo.find(data);
    // ...
}
```

### 3.3 接口继承

**决策**：接口可以继承，可以有默认实现，但不能被实例化。

```hc
pub interface IBase {
    fn base_method() void;
}

pub interface IExtended: IBase {
    fn extended_method() void;
    fn default_method() void {
        // 默认实现
        base_method();
        extended_method();
    }
}
```

---

## 四、项目结构标准化

### 4.1 标准项目目录

**决策**：采用方案B，项目结构包含：

```
myapp/
├── build.zon           # 项目描述（名称、版本、依赖等）
├── main.hc             # 入口文件
├── version.hc          # 版本信息（编译时自动更新 build number）
├── docs/
│   └── README.md       # 项目说明
├── src/                # 源码目录
│   ├── modA/           # 模块 A
│   └── modB/           # 模块 B
└── tests/              # 测试目录
```

### 4.2 version.hc

**决策**：`version.hc` 包含版本号段，编译时自动修改 build number 和时间。

```hc
// version.hc — 自动生成，编译时更新
pub const VERSION = "0.1.0";
pub const BUILD = 42;         // 每次编译自动递增
pub const BUILD_TIME = "2026-08-23 12:00:00";
```

### 4.3 build.zon 自动发现

**决策**：`hc run <dir>` 和 `hc build <dir>` 自动查找 `build.zon` 和 `main.hc`，找不到则报错。

```
hc run .            # 查找 ./build.zon + ./main.hc
hc run ./myapp/     # 查找 ./myapp/build.zon + ./myapp/main.hc
hc build .          # 查找 ./build.zon + ./main.hc
```

---

## 五、依赖解析

### 5.1 Name-only 依赖

**决策**：依赖可以只写 `name`，不提供具体路径时在系统目录（SDK 目录）下搜索。如果搜索不到，则在当前项目目录下面搜索。

```zon
// build.zon
const build = Build{
    name = "myapp",
    version = "0.1.0",
    kind = Kind.exe,
    deps = [
        Pkg{ name = "jsonlib", version = "0.1.0" },          // name-only: 搜索 SDK → 项目目录
        Pkg{ name = "logger", version = "0.2.0", path = "../libs/logger" },  // 显式路径
    ],
};
```

**搜索顺序**：
1. SDK 目录：`~/.hc/sdk/<name>/<version>/`
2. 项目目录：`<project_root>/<name>/`

### 5.2 SDK 目录

**决策**：SDK 目录是系统级的库搜索路径，安装 H 编译器时一并安装。

```
~/.hc/
├── sdk/
│   ├── std/               # 标准库
│   ├── jsonlib/           # 官方库
│   └── ...
├── cache/
│   └── hs/                # .hs 脚本缓存
└── registry/              # 包注册中心
```

---

## 六、标准库命名空间

### 6.1 命名空间前缀

**决策**：标准库的默认命名空间是 `H.Std`。后面文件夹的命名空间是前缀 + 当前文件夹名称。

```
H.Std                    # 标准库根命名空间
H.Std.io                 # IO 模块
H.Std.collections        # 集合模块
H.Std.time               # 时间模块
H.Std.rng                # 随机数模块
H.Std.text               # 文本处理模块
```

### 6.2 标准库内容

**决策**：内建接口、分配器模块、多线程模块、扩展类型都在标准库中。

- **内建接口**：`IIterable`、`IComparable`、`ICloneable` 等
- **分配器**：`Allocator` 接口 + `Page`/`Arena`/`Pool` 实现
- **多线程**：`Thread`、`Mutex`、`Channel` 等
- **扩展类型**：`Result`、`Option`、`Either` 等

---

## 七、实施计划

### 第一阶段：命名空间自动解析

| 任务 | 预估 | 说明 |
|------|------|------|
| M1-1 | ≤1h | 文件级命名空间自动推断：无 `namespace {}` 时，命名空间 = 文件相对目录 |
| M1-2 | ≤1h | 根命名空间 = 项目名称（从 build.zon 读取），标准库命名空间 = `H.Std` |

### 第二阶段：[module] 语义

| 任务 | 预估 | 说明 |
|------|------|------|
| M2-1 | ≤1h | [module] 命名空间禁止子命名空间，编译检查 |
| M2-2 | ≤1h | [module] visibility 规则：`pub` 对外可见，非 pub 仅 module 内可见 |
| M2-3 | ≤1h | [module] 必须定义 Context 类型，编译检查 |

### 第三阶段：Context/IOC

| 任务 | 预估 | 说明 |
|------|------|------|
| M3-1 | ≤1h | Context 接口定义（接口注册 + 对象创建） |
| M3-2 | ≤1h | Context 作为 module 的 global 存储 |
| M3-3 | ≤1h | IOC 风格：Context 注册接口 → module 通过 Context 创建对象 |

### 第四阶段：项目结构标准化

| 任务 | 预估 | 说明 |
|------|------|------|
| M4-1 | ≤1h | version.hc 支持（编译时自动递增 build number） |
| M4-2 | ≤1h | hc run/build 自动查找 build.zon + main.hc |
| M4-3 | ≤1h | 依赖 name-only 解析（SDK 目录 → 项目目录搜索） |
| M4-4 | ≤1h | 分配器：Arena 通过接口实现，移除 `with_arena` |