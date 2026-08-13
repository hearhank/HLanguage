# Zig 实现事实清单（截至 2026-08）

> 为 H 语言设计调研而做的核实。所有条目均通过 `fetch` 访问 ziglang.org 官方下载页 / 发布说明 / 首页 / Codeberg 官方仓库 README 核实（2026-08-12 抓取）。每条注明来源 URL。
>
> 未能获取的来源：语言参考（`documentation/master/` 单页体积过大未抓取，其中 Memory/Errors/comptime 章节内容通过与发布说明、官网首页交叉核实）；旧 FAQ 页与 GitHub wiki FAQ 已下线（404 / 不存在）。

## 1. 最新稳定版本与 1.0 状态

最新稳定版为 **0.16.0（2026-04-13 发布）**；master 为 0.17.0-dev（2026-08-11 构建）。**1.0 尚未发布，仍处于 0.x 预发布阶段**。
来源：https://ziglang.org/download/

## 2. 发布节奏与官方对 1.0 的态度

- 节奏：约半年一个 minor（0.14.0 2025-03-05 → 0.15.1 2025-08-19 → 0.15.2 2025-10-11 → 0.16.0 2026-04-13）；0.16 周期耗时 8 个月、1183 个 commit、244 位贡献者。0.17 规划为短周期（主要做 LLVM 22 升级与 build runner / build.zig 配置分离）。
- 1.0 官方态度：**没有时间表**。官方明确列出的"到达 1.0 的步骤"（0.10.0 Roadmap）：稳定语言（不再有语言变更）→ 完成语言规范初稿 → 实现官方包管理器 → 稳定标准库 → 完整一个发布周期无破坏性变更 → 才能打 1.0 标签。0.16 Roadmap 的首要大事仍是"完成并稳定语言"；0.16 发布说明称"当 Zig 达到 1.0.0 时，Tier 1 支持将新增 bug 政策"。0.15.1 发布说明把当时的大破坏（Writergate I/O 重设计）称为"守护语言稳定化的最后堡垒"——即官方认为冻结前仍可接受破坏性变更。
来源：https://ziglang.org/download/0.10.0/release-notes.html#Roadmap ；https://ziglang.org/download/0.16.0/release-notes.html#Roadmap ；https://ziglang.org/download/0.15.1/release-notes.html

## 3. 内存管理哲学

- **无 GC、无隐藏内存分配**（官网首页卖点："No hidden memory allocations"）。
- 所有堆内存分配都通过显式 `Allocator` 进行；类型为 `std.mem.Allocator`（stdlib 源码路径 `std/mem/Allocator.zig`，0.10.0 发布说明中出现 `allocator: std.mem.Allocator` 签名）。
- 官方表述：**"所有分配内存的代码都需要访问一个 `Allocator` 实例"**（0.15.1 Roadmap 中与 `Io` 相类比的原话）。
- 常用分配器：`std.heap.page_allocator`、`std.heap.ArenaAllocator`（0.16 起线程安全、无锁）、以及 main 默认的 gpa（通用分配器，Debug 模式开启泄漏检测，见 0.16 "Juicy Main"）。
- 释放惯用法：`defer` / `errdefer` 在作用域结束/出错时释放（首页示例 `defer list.deinit(gpa)`；0.10.0 文档以 `defer allocator.free(foo)` 修复泄漏检测示例）。
来源：https://ziglang.org/ ；https://ziglang.org/download/0.15.1/release-notes.html ；https://ziglang.org/download/0.16.0/release-notes.html ；https://ziglang.org/download/0.10.0/release-notes.html ；语言参考 Memory 章节 https://ziglang.org/documentation/0.16.0/#Memory（内容与上述一致，经交叉核实）

## 4. 类型系统

- **显式类型标注**：所有变量/参数/返回类型均显式标注，无隐式类型推断（官方示例贯穿一致）。
- **comptime 编译期求值**：官网首页——"可在编译期调用任意函数""把类型当值操作、零运行时开销""comptime 模拟目标架构"。
- **泛型方式**：无独立泛型语法，通过"以 `comptime` 参数接收类型并返回类型的普通函数"实现（0.15.1 示例 `CounterMixin(comptime T: type) type`）；`anytype` 用作泛型函数参数（0.15.1："forcing all functions to be generic as well with anytype"，示例 `fn foo(old_writer: anytype)`；注意 0.10.0 已移除 struct 字段上的 `anytype`，仅保留参数用法）。
- 惰性分析：未被引用的声明不分析（0.16 "Lazy Field Analysis"）。
来源：https://ziglang.org/ ；https://ziglang.org/download/0.15.1/release-notes.html ；https://ziglang.org/download/0.16.0/release-notes.html ；https://ziglang.org/download/0.10.0/release-notes.html

## 5. 错误处理

- 错误用 **error union** 类型表达：`E!T`（错误集 E 与负载 T 的并），常用简写 `!T`（如 `pub fn main(init: std.process.Init) !void`，遍布 0.16 发布说明示例）。
- **错误集 error set**：显式 `error{ ... }` 语法声明（0.16 起不允许反射重建错误集，"declare your error sets explicitly using `error{ ... }` syntax"）；错误值写作 `error.ErrorName`（如 `error.Canceled`）。
- 处理方式：`try` 向上传播；`catch` 就地处理（如 `_ = foo.cancel(io) catch {}`）；`else |err| switch (err)` 匹配错误集；另有错误返回追踪（error return traces，0.10.0 有大量改进）。
来源：https://ziglang.org/download/0.16.0/release-notes.html ；https://ziglang.org/download/0.10.0/release-notes.html

## 6. 并发现状

- **async/await 关键字已移除——你的记忆正确**。移除发生在 0.15 系列（0.15.1 发布说明章节 "async and await keywords removed"，同时移除 `@frameSize`）。官方明确表态：**语言中不会再引入 async/await 关键字，而是放进标准库，作为 Io 接口的一部分**（"it is settled that there will not be async/await keywords in the language. Instead, they will be in the Standard Library as part of the Io Interface"）。
- 当前推荐的并发原语：
  - 底层：`std.Thread`（OS 线程）。
  - 0.16.0 起：新的 `std.Io` 接口作为统一 I/O+并发抽象——`Io.Threaded`（基于线程，功能完备）、`Io.Evented`（实验性 M:N / 用户态栈切换 / green threads）、`Io.Uring` / `Io.Kqueue` / `Io.Dispatch`（原型）；`io.async` / `io.concurrent` 产生 `Future`，支持 `await` / `cancel`，另有 `Group` / `Batch` 批量任务。
  - 同步原语迁往 `std.Io`：`Thread.Mutex → Io.Mutex`、`Thread.Condition → Io.Condition`、`Thread.Semaphore → Io.Semaphore`、`Thread.ResetEvent → Io.Event` 等（0.16 迁移表）。
  - `std.Thread.Pool` 已在 0.16.0 移除，改用 `Io.async` / `Io.Group.async`。
来源：https://ziglang.org/download/0.15.1/release-notes.html ；https://ziglang.org/download/0.16.0/release-notes.html

## 7. 构建系统

- **`zig build` = 构建系统 + 官方包管理器，一体内置在编译器中**（0.10.0 Roadmap 原话："Having a package manager built into the Zig compiler is a long-anticipated feature"）。声明式 `build.zig` + 依赖清单 `build.zig.zon`；0.16 新增本地包覆盖（`--fork`）、将包 fetch 到项目内 `zig-pkg` 目录、指纹校验等。
- **`zig cc` / `zig c++`**：零依赖的 drop-in C/C++ 编译器（内置 Clang、跨编译开箱即用；0.16 基于 Clang 21.1.8，可静态链接目标平台 libc）。
来源：https://ziglang.org/download/0.16.0/release-notes.html#Build_System ；https://ziglang.org/ ；https://ziglang.org/download/0.10.0/release-notes.html

## 8. 自举历史

- 2015 年由 Andrew Kelley 创建；**最初编译器用 C++ 编写**（即 stage1，使用 LLVM，官方 enum 注释原话："The original Zig compiler created in 2015 by Andrew Kelley. Implemented in C++. Uses LLVM."）。
- **0.10.0（2022-10-31）起默认编译器切换为用 Zig 自写的"自托管编译器"**（self-hosted / stage2；0.10.0 发布说明："the main feature of this release cycle is the début of the self-hosted compiler. It is now enabled by default"）。
- **现在编译器主体全部用 Zig 编写**（stage3 即"由 Zig 自身构建的 Zig"：README "This produces `stage3/bin/zig` which is the Zig compiler built by itself"）。但引导仍需外部工具：①无 LLVM 路径——用 C 编译器编译 `bootstrap.c` 产出 stage2（`zig2`，缺 Release 优化等能力）；②CMake 路径——C/C++ 工具链 + LLVM/Clang/LLD 22.x 开发库；③预构建路径——旧版 Zig + 用 Zig 构建的 LLVM（zig-bootstrap 项目）。
- **后端**：默认仍是 LLVM（Release 模式）；自研后端在推进——**x86_64 自托管后端自 0.15.x 起成为 Debug 模式默认**（行为测试通过率反超 LLVM 后端，编译快约 5 倍）、aarch64 WIP（目标成为未来 Debug 默认）、WebAssembly（92%）、C 后端、arm/riscv64/sparc64 等。长期路线是**把 LLVM 从"库依赖"变为"进程依赖"（Clang）**（0.16 Roadmap）。
- 残留 C++：0.16.0 时编译器源码树仅剩约 3,763 行 C++（translate-c 已从 libclang 换成用 Zig 写的 arocc，"Goodbye and good riddance to 5,940 lines of our remaining C++ code"）。
来源：https://ziglang.org/download/0.10.0/release-notes.html ；https://ziglang.org/download/0.15.1/release-notes.html ；https://ziglang.org/download/0.16.0/release-notes.html ；https://codeberg.org/ziglang/zig/raw/branch/master/README.md

---

## 与你的提问逐条对照

| 你的问题 | 核实结果 |
| --- | --- |
| 最新稳定版 / 是否 1.0 | 0.16.0（2026-04-13），尚无 1.0，仍在 0.x |
| 0.15 左右移除 async/await | 正确，0.15.1 发布说明确认移除，改由标准库 Io 接口承担 |
| 用 std.Thread 等 | 线程仍在（std.Thread），但 0.16 起并发/I/O 抽象统一为 std.Io 接口，同步原语迁入 std.Io，Thread.Pool 已移除 |
| 引导最初语言 | C++（stage1，2015-2022）；现主体为 Zig 自举，但引导仍需 C 编译器（bootstrap.c）或旧版 Zig |
| 后端是否用 LLVM | 是，LLVM 仍为默认（Release）；x86_64 自托管后端自 0.15.x 起为 Debug 默认，aarch64/WASM/C 后端推进中；长期目标是把 LLVM 变为可选的进程级依赖 |
