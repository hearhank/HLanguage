# 06 · 工具链

## 双后端（核心承诺）

- 同一份源码、两种执行方式：编译器产出原生二进制；解释器作为脚本执行。
- **共享前端 + 共享中间表示（IR）**：解释器执行 IR，编译器把 IR 编译为原生码。
- **语义一致性是硬承诺**：脚本里跑通，编译后行为必须一致（整数溢出、越界、错误路径等触发条件相同）。
- 双后端是"写脚本就是写正式代码"的基础；不允许解释器宽松、编译器严格。

## 命令

- `h run foo.h`——解释器执行（脚本模式）。
- `h build foo.h`——编译器产出原生二进制。
- 多文件项目 = 命名空间树目录整体构建。

## 运行时雏形（已交付）

- `src/`：lexer/parser/checker/evaluator 四模块 + `h.js` CLI。
- 命令：`node src/h.js run|check|parse|build <file>`（无文件读 stdin；`--trace` 输出执行轨迹；`build --exec` 编译后直接运行）。
- 示例：`examples/demo.h`（生命周期/字节化/并发演示）、`error.h`、`wrong.h`、`match.h`、`import.h`、`concurrency.h`、`calc.h`。
- 冒烟测试：`tests/smoke.js`（28 项断言通过）。

## 编译后端（已交付：C 原生 + JS 回退）

- `src/cgen.js`：AST → C 源码（纯块子集）——**"块 = 连续内存"在 C 中零运行时开销原样兑现**（struct → C struct、enum + 穷尽 match → typedef enum + switch、u64/f64/bool/Str → 原生类型）。
- `h build <file>`：探测编译器 `zig cc`（自带 clang）→ `gcc` → `cc` → `clang`，编译为原生二进制；无 C 编译器时回退 `jsgen`（JS 目标）。
- **最短往返浮点格式化**（`h_print_f64`）：%.*g 循环至往返一致——与解释器 JS 输出逐位对齐。
- **双后端一致性已验证（原生级）**：`h run calc.h` 与编译后的 `calc.exe` 输出逐行一致（smoke 断言，31 项全绿）。
- 入口语义：main 若定义且未被显式调用 → 自动调用（两后端一致）；枚举比较统一为 `类型.变体`。
- 不支持的结构（class/并发/error/ref/move/数组/顶层语句）编译时拒绝并提示用 `h run`。
- 环境注：Windows 下推荐 `zig cc`（Zig 自带 clang 21）；直接运行 exe 时中文输出需 UTF-8 代码页（`chcp 65001`）。

## C 目标映射（设计，待编译器环境）

- `struct` → C struct（块 = 连续内存的直接映射，C 天然值语义）
- `enum` + 穷尽 `match` → C enum + switch
- `u64`/`f64`/`bool` → `uint64_t`/`double`/`bool`；`Str` → `const char*`
- 函数/表达式 → C 函数/表达式；`print` → printf
- 树/并发/error：需运行时支持（双向引用通知/调度器），后续切片

## C 目标映射（已实现，见上）

- struct → C struct（typedef）；enum + match → typedef enum + switch（GNU 语句表达式）
- u64/f64/bool/Str → unsigned long long/double/bool/const char*
- 树/并发/error：需运行时支持，编译时拒绝（提示用 h run）

## 解释器定位

- 独立脚本执行为主（命令行直接运行源文件）。
- 预留宿主嵌入（语言作为库嵌入其他程序）。
- 数据层机制（双向引用通知、模式化共享、显式 allocator）在解释器中保持同语义（一致性承诺的一部分）。

## 编译目标与互操作

- **本机原生码为主**；wasm 作为未来分发目标（代码分发的预留通道）。
- **C ABI 互操作内建**：字节数组作为 FFI 边界协议——字节化与 C 结构内存布局天然衔接；双向引用只作用于语言内部，跨边界降级为纯数据传递。
- 函数字节化 = 代码引用 + 捕获环境：处理计划（函数引用 + 数据）可打包、存储、传输、在另一端恢复执行。

## OPEN

- 构建系统的细节（依赖管理、增量编译、测试入口）。
- IR 的具体形态与可分发性（解释器分发源码/IR 字节的通道）。
