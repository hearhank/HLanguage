# A1 FFI 设计定案（`extern fn` + `@cImport` + `hc cc`）

> 2026-08-22 定案（A1 = 10-part3 组 G6 + 07 E3.5 + 04-stdlib-scope ffi 模块，grill-with-docs 访谈，Round 1 八子项 + Round 2 七子项全推荐）。关联：SPEC [01-language-design.md](../SPEC/01-language-design.md)（§12.26）、[04-stdlib-scope.md](../SPEC/04-stdlib-scope.md)（ffi 模块）、[06-04-functions.md](../SPEC/06-04-functions.md)（函数 / @ 内建）、[10-part3-execution.md](../SPEC/10-part3-execution.md)（组 G6）、[01-unimplemented-features.md](../phase3/01-unimplemented-features.md)（A1/B5 条目）、[02-syntax-rules.md](../phase3/02-syntax-rules.md)（§2.2 函数）。

## 背景

- Q-S4（2026-08-13）已定骨架：`extern fn c_func(...) ret;`（C ABI 外部声明，链接期解析）；`@cImport("header.h")`（内建 C 解析器，范围基本类型/struct/enum/函数指针）；**C 指针 = `*T` 但不参与双向登记**（外部内存用户负责，进入引用体系需显式 `box` 包装）；**C 错误码 → error union 手动映射**（无隐式转换）；C struct ↔ H Continuous POD 直映射 + `@offsetOf`/`@alignOf` 布局验证
- 组 G（2026-08-18）G6 ffi 按用户决定跳过——第三阶段标准库首要缺口
- 现状核实：`hc cc` 子命令**不存在**（CLI：run/test/check/errors/build/init/pkg/doc/fmt/lex）；`extern` 未入 lexer/token（LSP 关键字表有但无 `KwExtern` token）；`export fn`（K5，反向 H→C 符号）已落地（ADR-0014）；`hc build` 链接已用 `zig cc`
- FFI 本质 = 链接期特性，与 interp（tree-walking/IR）双后端不一致——需明确边界

## 决策

### A1-1 `extern fn` 语义：纯声明

1. `extern fn c_func(args...) 返回类型;` = **纯声明**（无 body，链接期解析外部 C 符号，Zig 式）；反向（H 侧实现 C ABI 函数）由既有 `export fn`（K5，ADR-0014）覆盖，不新增机制
2. 声明出现在 `@cImport` 生成或用户手写；`extern` 关键字需入 lexer/token（新增 `KwExtern`）+ parser（`parse_decl` 扩展）

### A1-2 `extern fn` 类型范围（MVP）

1. 参数/返回允许：标量（iN/uN/fN/bool/usize）+ `*T`/`*mut T` + `[continuous] class` POD
2. 不允许：错误联合 / 切片 / 接口 / 泛型 / 闭包——严格 C ABI 可表示

### A1-3 `@cImport` 前端（MVP 范围 + 位置/作用域）

1. **解析范围（MVP）**：头文件中直接可见的声明体——struct / enum / typedef / 函数声明；**不展开 `#include`、不处理宏**（`#define`/`#ifdef` 等）；解析失败 = 编译错误带位置
2. **位置/作用域**：顶层 `const c = @cImport("header.h");`——`c` 为编译期「导入对象」，成员经命名空间限定引用 `c.printf(...)` / `c.StructName`（复用既有命名空间/限定名机制，与 Zig 同构）
3. `@cImport` 仅编译期求值（comptime 内建），原生链接时才真实存在

### A1-4 双模式边界：FFI 原生-only

1. FFI = 链接期特性，**原生-only**；tree-walking/IR（interp）对 `extern fn` 调用响亮拒绝（`error.NotCallable` 风格，承 G4b 定案 A 哲学——不静默误编译）
2. FFI 测试走 `hc test --mode=compile`；**不进 interp 一致性套件**（interp==IR 双后端一致承诺不适用外部链接）

### A1-5 `hc cc` 工具链（zig cc 薄封装 + build.zon 集成；B5 并入）

1. `hc cc` = **薄封装 `zig cc`**——零新依赖，与既有 `hc build` 链接路径一致；`hc cc file.c` 产出目标 / 直接链接
2. **build.zon 集成**：增 C 源文件声明字段，`hc build` 自动 zig cc 编译 + 链接（与 `--dll` 联动）
3. **B5（`hc cc`，M8）并入 A1 统一设计**——同一子命令 + 集成，A1 完成即 B5 完成

### A1-6 C 指针外置与 `box` 进入

1. **外部标记 = 上下文推导**：凡出现在 `extern fn` 签名或 `@cImport` 生成声明中的指针类型自动视为外部（不参与 Debug 悬垂登记）；复用 `*T` 语法、无新类型/新标注
2. **`box(c_ptr, alloc)` = 复制进托管堆**：分配托管内存 + 按类型大小/布局复制 C 数据 → 返回  `owned *mut T` **参与登记**（所有权归 H，悬垂检测适用）；与既有 `box` 语义一致（08-mem-allocator G3：box = 分配 + 值写入堆 + 带所有权指针）；「包裹外部指针」不给所有权、悬垂检测仍不适用，不采用
3. 外部指针 deref 照常穿透（读/写），只是不注册

### A1-7 C struct/enum 生成

1. **`@cImport` 自动生成 `[continuous] class`**（字段 + 对齐，尊重 C packed/align），MVP 主路径；用户也可手写 H struct 并靠 `@offsetOf`/`@alignOf` 布局验证
2. C enum → H 纯常量 enum，经 `@enumFromInt`/`@intFromEnum` 转换
3. `@cImport` 中 C union → H 无标签 union（K1 已落地，ADR-0014）直映射（05 系统编程缺口 K1 注记「`@cImport` 无法映射 union」因此解除）

### A1-8 错误码映射：纯手动

1. Q-S4 已定：无隐式转换，`if (ret != 0) return error.X;`
2. **不加辅助内建**（如 `@errorFromInt`）——保持最小面，符合「没有隐藏控制」

### A1-9 FFI 回调与 C 字符串（1.x 边界）

1. **回调（C 函数指针参数）**：`@cImport`/`extern fn` 可**声明**函数指针类型/参数（Q-S4 范围含函数指针）；**传 H 函数作 C 回调 = 1.x**（依赖裸函数指针 K7 或闭包 ABI C7/Phase 8）；MVP 回调场景用 C 侧 wrapper 或跳过
2. **C 字符串（`const char*`）**：统一按指针处理——`const char*` → `*const u8`，H 侧用既有指针操作手动读；MVP 不提供专用 CStr 类型/辅助（1.x 可加 std 层）

### A1-10 验收形态

1. 自写小型 C 文件（如 `add(i32, i32) i32` + 一个简单 POD struct + 错误码函数）→ `extern fn` 声明（或 `@cImport` 生成）→ `hc build` / `hc cc` 原生链接 → 测试绿
2. 不进 interp 一致性套件（A1-4）

## 理由

- A1-1 纯声明：反向导出已有 `export fn`，不重复机制；Zig 式 `extern` 无 body 是成熟惯例
- A1-2 MVP 类型范围：严格 C ABI 可表示集，避免跨 ABI 边界语义歧义（错误联合/切片/接口为 H 特有表示，无 C ABI 对应）
- A1-3 `@cImport` 最小解析 + 命名空间式引用：不展开 include/宏避免引入完整 C 预处理器（重大工程量）；`const c = @cImport(...)` + `c.xxx` 复用既有限定名机制零新语法
- A1-4 原生-only：FFI 本质链接期特性，interp 无法真实链接；响亮拒绝承 G4b 哲学——不静默误编译；一致性套件（interp==IR）仅覆盖解释侧，FFI 属编译侧
- A1-5 zig cc 薄封装：零新依赖、与既有 build 链接路径一致（Zig 自带 clang 编译 C）；B5 并入避免两套 `cc` 机制
- A1-6 上下文推导外部 + box 复制进托管堆：推导免新类型语法；box 复制是唯一真正「进入引用体系」（注册 + 悬垂检测适用）的途径，与既有 box 语义一致
- A1-7 自动生成 continuous class：Q-S4 POD 直映射落地；K1 union 落地使 `@cImport` union 映射成为可能
- A1-8 纯手动错误码：保持「没有隐藏控制」最小面，不引隐式转换
- A1-9 回调/C 字符串 1.x：两者均依赖裸函数指针/闭包 ABI（K7/C7）或专用类型，MVP 不扩面
- A1-10 自写 C 验收：可控、无外部库依赖、端到端验证链接链路
