# 入口与导入定案：main(args) + import 语句取代 using

> 2026-08-17 定案（第二部分评审 Q9/Q13/Q14）。关联：[ADR-0002 初始编译器](0002-initial-compiler-in-rust.md)、[06-08-modules.md](../SPEC/06-08-modules.md)、[04-stdlib-scope.md](../SPEC/04-stdlib-scope.md)、执行细表 [09-part2-execution.md](../SPEC/09-part2-execution.md)。
>
> **2026-08-25 更新**：本 ADR 中关于 `[module]` 特性标记的条款已被 ADR-0026 取代。模块系统改为 `src/Modules/` 目录驱动 + `IContext` IoC 容器架构。详见 ADR-0026。

## 背景

- 原定案（12.18 / Q22c）：**IO 是类型，必须显式传递**——入口 `fn main(io: Io) !void`，编译器注入句柄；标准库 `io` 以接口对象形态存在（`io: *T where T: Io`）
- 原定案（06-08）：**无「文件级 import」**（文件 = 物理单元，C# 式）；跨包访问走 `using pkg.xxx` + build.zon
- tag1 实践：136 个示例全部 `main(io: Io)`；`io`/`alloc`/`test_io` 已作为隐式环境全局存在（`IMPLICIT_ENV`），零参 main 已兼容；原生 `emit_main_wrapper` 对单参 main 注入 `hc_make_io()`
- 矛盾：入口传参与「io 是程序环境」的直觉不符（每个 main 都写样板参数）；`using` 与文件级导入需求（`import H.std.{io as my}`）重复且无别名层级路径

## 决策

1. **入口改为 `fn main() !void`**——main 不再接收 io 参数；`args` 由运行时注入（0 号 = 程序名，后续为命令行参数）；**命令行参数仅经入口注入，`io.args()` 取消（2026-08-17 F1 定案）**。io/alloc 作为**标准库模块与预导入环境**提供，不再经入口注入
2. **`import` 语句取代 `using`**（推翻「无文件级 import」定案）：

   ```hc
   import H.std.{io as my};        // 符号选择 + as 别名（重名重命名）
   import H.std.net.{http, tcp};   // 多符号
   import pkg.mod;                 // 整模块导入
   ```

   - `H.std` = 内置标准库根路径；用户库经 build.zon 声明后按包名引用；**导入对象 = 模块（`[module]` 标注的命名空间或包，2026-08-17 F2 定案）**
   - 冲突规则沿 06-08 既有定案：显式导入优先通配；重名用 `as 别名` 显式改名
3. **io = 标准库模块**：函数直接调用（`my.print(...)`）；环境状态（env/exit/fs/net/time，模块内管理；**args 经入口 `main(args)` 注入，`io.args()` 取消**）。**库符号访问规则**：库函数可直接调用；库类型需创建（`alloc.init(T)` 堆上 / 值字面量栈上）；**值类型栈上分配，经 `alloc` 堆上分配**
4. **`Io` 接口保留**（并发 E2 的 `Io.threaded()/evented()` 显式切换机制仍是设计的一部分），仅取消入口注入
5. **`test_io` 取消**：测试直接调 `main()`；需要 io 的测试经 `import H.std.{io}` 使用环境
6. **模块 = `[module]` 标注的命名空间**（F2 定案）：内容与其它命名空间隔离；需要其它库的数据经**上下文（init 参数列表）**初始化注入——模块概念承接 Q11，init 上下文语法在实现组 A2/H3 细化

## 影响

- **示例全量迁移**：136 个示例 `main(io: Io)` → `main(args)` + `using`→`import`；示例回归基线重设（执行组 A6）
- **三后端接线**：interp `run_main`、IR、原生 `emit_main_wrapper`/`@__init__` 前置、字节码 VM 入口
- **语义层**：入口签名校验（args 参数形态）、import 符号登记/可见性/别名解析、using 迁移
- **文档**：06-08（import 定案）、04（io 模块形态）、CONTEXT（术语）、02/07（引用与差异标注）
- **未变**：`io.*` API 面（print/fs/net/time/args/env/exit 的方法与签名不变，仅形态从对象方法改为模块函数）；四大支柱范围；双模式一致性承诺

## 取舍

- 选择「模块函数 + 环境状态」而非「接口对象注入」：入口零样板、库符号直接可用、环境状态模块内聚；代价是 `Io` 接口的动态切换能力退居 E2 场景（并发）
- 选择「import 统一取代 using」而非并存：单一导入机制，避免两套语法；代价是同包 namespace 访问也走 import（表达式略长）
- 推翻「无文件级 import」：文件级导入是库引用（H.std/外部包）的自然形态；namespace 块保留（模块内组织、跨文件）
