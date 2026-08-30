# 09 模块与程序结构

> 大模块：模块与程序结构 | 对齐状态：**✅ 对齐完成（2026-08-30，K1 裁决 + ADR-0036）** | 初稿：2026-08-30
>
> 事实基础：**ADR-0010**（入口与导入，2026-08-17）、**ADR-0026**（模块系统 IoC，2026-08-25）、**ADR-0031**（按文件加载，2026-08-29，已落地）、历史 `06-08-modules.md`/`06-13-project-structure.md`（已废弃）、tag1 实现（`parser/decl.rs` import/namespace、`ir/runtime.rs` main 分派、`semantic/collect.rs` M1.4、`ir/lower_impl.rs` collect_imports）。
> 证据总库：`tag1/hc/tests/frontend.rs`、`tag1/examples/`（136 示例基线）。

## 9.1 程序入口（矛盾收敛：4 版本 → 2 形态）

- 规则（ADR-0010 定案 + 运行时实现核对 + K1 裁决 2026-08-30）：
  - **形态一（规范默认）**：`fn main() !void { ... }`——零参入口。
  - **形态二（命令行参数注入）**：`fn main(owned args: *mut Vec<String>) !void { ... }`——`owned` 修饰参数名（K1/ADR-0036 名称前缀模型）；单参数 = 命令行参数（**0 号 = 程序名**）；运行时注入（argc/argv → 可写 Vec 句柄，`*mut` 借用形态 + owned 拥有标注）。
  - 返回类型 `!void` **必写**（H1：返回类型必须标记）；main 返回 error → 退出映射（`08` §8.8：Error/1，正常 Exit/0）。
  - ❌ `fn main(io: Io) !void`（编译器注入 io 句柄）——**废除**（ADR-0010 决策 1）；io 经预导入环境/`import H.std.{io}` 获取（§9.7）。
  - ❌ `io.args()`——取消（2026-08-17 F1：命令行参数仅经入口注入）。
  - 入口分派（实现）：同名 main 多候选时**零参优先**，否则首个（`emit_main_wrapper`/`IrRuntime::call`）——重载启用前的确定性规则。
- 状态：⚠️ 形态二签名待改（实现现为 `owned Vec(String)` → backlog #16）
- 证据：裁决 K1 + ADR-0036；`ir/runtime.rs` `call`（单参/零参分派）；`codegen/llvm/emit.rs` `emit_main_wrapper` L590-625（零参优先 + argc 注入）；ADR-0010 决策 1

```hc
fn main() !void { ... }                                  // 形态一
fn main(owned args: *mut Vec<String>) !void {            // 形态二
    var first: String = args.*.get(0) orelse "";          // 0 号 = 程序名
}
```

## 9.2 namespace（命名空间）

- 规则：
  - `namespace 名称 { 声明... }`——声明级块，内容隔离；可与文件路径命名空间叠加（ADR-0026 规则 5：`namespace` 可覆盖默认路径命名空间）。
  - **文件路径即命名空间**（ADR-0031，2026-08-29 落地）：`src/*.hc` **同目录文件同命名空间，扁平共享、无需 import**；跨命名空间（子目录/Modules）走 `import NS.{sym}` / 限定调用。
  - **同命名空间同名声明 = 编译错误**（不静默覆盖，列两处来源；v1 由调用期歧义响亮报错替代，编译期跨文件检测 ⏸ K6 细化项）。
  - 命名空间内声明继承统一前缀（`01` §1.8：pub/export/特性标注）。
- 状态：✅ 已实现（[module] ❌ 报错引导）
- 证据：`parser/decl.rs` L164-193（namespace 块）；`[module]` 硬错误 L24-26（ADR-0026）；ADR-0031 实施状态（`load_siblings`/`same_ns` 落地 + stage2 三链路贯通）

```hc
// src/main.hc 与 src/utils.hc 同命名空间（扁平共享，无需 import）
// src/Modules/Auth/ 内文件 = project.Auth 命名空间
```

## 9.3 import（唯一导入机制，ADR-0010）

- 规则：
  - `import H.std.{io as my};`——**符号选择 + as 别名**（重名重命名）。
  - `import H.std.net.{http, tcp};`——多符号。
  - `import pkg.mod;` / `import pkg.mod as m;`——整模块导入。
  - `H.std` = 内置标准库根路径（**虚拟根**：`io.print` 等直接路由内建调用，不展开文件）；用户库经 build.zon 声明后按包名引用。
  - `import "path/file.hc";`——**文件路径导入**（`.hs` 脚本配套，B6-2）；`.hs` 系统实现 ⏸ 自举后（排除列表），文法保留。
  - ❌ **`import .{sym}` 不入规范**（ADR-0031 决策 3：stage1 interp 工具链扩展，双轨导入违背单一机制原则）。
  - 冲突规则：显式导入优先通配；重名 `as` 别名显式改名。
- 状态：✅ 已实现（符号选择/别名/限定名展开）
- 证据：`parser/decl.rs` L194-263（文件路径/路径选择/别名三分支）；`ir/lower_impl.rs` `collect_imports` L124-125（H.std 虚拟根注释）+ C3 限定名展开 L1462-1472

## 9.4 可见性

- 规则：
  - **默认私有，`pub` 显式导出**（Q3/M7.2）；`export` = 原生符号级导出（仅 fn/async fn，K5，`01` §1.2.1）。
  - 顶层函数**文件私有**在「loose 单文件模式」（无 build.zon）下保持；**同命名空间**（`src/` 包模式）扁平登记（ADR-0031 实施状态：loose 行为不变）。
  - namespace 成员可见性：模块内私有、`pub` 接口公开（ADR-0026 可见性表，随模块系统 ⏳）。
- 状态：✅ 已实现（pub/export/文件私有/同命名空间登记）
- 证据：`semantic/collect.rs` L8-18 + L274-284（M1.4 兄弟文件规则）；ADR-0031 Consequences

## 9.5 模块系统（ADR-0026：目录驱动 + IoC 容器）

- 规则（ADR-0026 定案，**实现 ⏳ 未开始**）：
  - **`src/Modules/` 下每个子目录 = 一个模块**（编译器自动发现，扁平结构、无嵌套子模块）；命名空间 = `project.X`。
  - 每模块必须定义 **`context.hc`**（实现 `IContext` 接口，`H.std.ioc` 提供）。
  - **IContext 容器**：`register<T>`（深拷贝到 Arena）/ `registerFactory<T>`（首次缓存）/ `get<T>`（Arena 引用，无所有权）/ `make<T>`（返回 `owned T`，defer/move 义务）；**层级委托**（子 context 解析不到 → 向上委托父 context）；每 context 背靠 Arena，销毁时一并销毁。
  - **模块面向接口编程**：接口定义在提供方模块，使用方 `import project.Auth.{IUserService}`；模块公开 API = context 结构体 + 接口定义。
  - ❌ `[module]` 特性标记已移除（实现 ✅ 报错引导迁移）。
  - `context` 关键字：词法层无 `context` 关键字（`01` §1.2.1 关键字表无此项）——ADR-0026「context 关键字语义精化」表述按现状修正为 **IContext 接口名**（非语言关键字）。
- 状态：⏳ 未实现·目标（编译器自动扫描/context.hc 约定/IContext/AppContext/层级委托——ADR-0026 实现计划 8 任务）
- 证据：ADR-0026 全文；tag1 全文 **无 IContext/IoC 命中**（grep 证实）；`[module]` 硬错误 ✅

## 9.6 项目结构与构建

- 规则：
  - **标准布局**（ADR-0026 物理结构）：`src/main.hc`（入口，命名空间 = 项目名）、`src/Modules/X/`（模块，⏳）、`src/` 其余（普通代码）、`tests/`（项目根，测试文件，不参与命名空间，仅 `hc test` 发现执行）。
  - **build.zon**：包名/依赖声明——用户库经 build.zon 声明后按包名 `import` 引用（ADR-0010 决策 2）。
  - `hc init` 脚手架：生成标准布局（历史脚手架用 `main(args: owned Vec(String))` 形态——与 §9.1 形态二一致）。
  - loose 单文件模式：无 build.zon 时单文件直跑（兄弟文件各归各自文件命名空间，文件私有，ADR-0031）。
- 状态：⚠️ 部分——布局约定 ✅ 定案；build.zon 解析/hc init 脚手架/包名引用的证据归工具链核对（hc-tools）
- 证据：ADR-0026 物理结构节；ADR-0031 loose 模式；`06-13` 历史（脚手架树）

## 9.7 预导入环境（implicit env）

- 规则：
  - **`alloc`/`io` 作为预导入环境全局可用**（无需入口注入，ADR-0010 决策 1/3）：`io.print(...)` 等 std 函数直接调用；环境状态（env/exit/fs/net/time）模块内管理。
  - `Io` 接口保留（并发 E2 的 `Io.threaded()/evented()` 显式切换机制，R-4 工厂返回具体类型）——归 `11-concurrency.md`。
  - 集合缺省分配器回退：`Vec(T).init()` 缺省捕获全局 alloc（实现 G4）。
- 状态：✅ 已实现
- 证据：`ir/lower_impl.rs` LoadGlobal "alloc" / `implicit_env_value`；`ir/builtin.rs` L2896-2906（alloc 缺省回退注释）；`ir/runtime.rs` main 注释（io 经 `import H.std.{io}` 引入）

## 9.8 变更记录（入口 4 版本矛盾收敛）

| 变更 | 依据 |
|---|---|
| 入口收敛为双形态：`fn main() !void` / `fn main(args: owned Vec(String)) !void`；`main(io: Io)` ❌ 废除 | ADR-0010 决策 1 + 运行时实现核对（`06-总纲`/`06-04`/`06-08`/`06-13` 四版本归一） |
| `io.args()` ❌（参数仅经入口注入） | ADR-0010 F1 |
| `test_io` ❌（测试直接调 main / 经 import 用 io） | ADR-0010 决策 5 |
| 文件路径即命名空间（同目录扁平共享、同名冲突 = 编译错误）成文 | ADR-0031（2026-08-29 落地） |
| `import .{sym}` ❌ 不入规范（工具链扩展） | ADR-0031 决策 3 |
| 模块系统 = src/Modules/ + IContext（⏳）；`[module]` ❌ | ADR-0026 |
| 「context 关键字」表述修正：IContext 是接口名，非语言关键字（关键字表无 context） | `01` §1.2.1 + ADR-0026 |
| 测试文件位置矛盾收敛：`tests/` 项目根（ADR-0026）+ 源内 `[test]` 标注函数并存 | ADR-0026 规则 6 + `06-01` Q8（核对归 `12-testing.md`） |
| 预导入环境（alloc/io 隐式全局 + 缺省分配器回退）补录 | G4 + `implicit_env_value` |

## 9.9 裁决记录（2026-08-30，项目所有者）

| # | 条目 | 裁决 | 影响 |
|---|---|---|---|
| K1 | 入口参数 + 参数/字段所有权标注模型 | **入口参数 = `owned args: *mut Vec<String>`**（`owned` 名称前缀）；**参数/字段所有权标注模型**（`mut T` 必定拥有、`*mut T` 不一定、值类型 + owned = 编译错误）→ **ADR-0036** | §9.1、`07` §7.1.1、`05` §5.2、`04` §4.1、backlog #16 |
