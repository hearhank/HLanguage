# 06 · 工具链

## 双后端（核心承诺）

- 同一份源码、两种执行方式：编译器产出原生二进制；解释器作为脚本执行。
- **共享前端 + 共享中间表示（IR）**：解释器执行 IR，编译器把 IR 编译为原生码。
- **语义一致性是硬承诺**：脚本里跑通，编译后行为必须一致（整数溢出、越界、错误路径等触发条件相同）。
- 双后端是"写脚本就是写正式代码"的基础；不允许解释器宽松、编译器严格。

## 命令

- `h run foo.hc`——解释器执行（脚本模式）。
- `h build foo.hc`——编译器产出原生二进制。
- 多文件项目 = 命名空间树目录整体构建。

## 运行时雏形（已交付）

- `src/`：lexer/parser/checker/evaluator 四模块 + `h.js` CLI。
- 命令：`node src/h.js run|check|parse|build <file>`（无文件读 stdin；`--trace` 输出执行轨迹；`build --exec` 编译后直接运行）。
- 示例：`examples/demo.hc`（生命周期/字节化/并发演示）、`error.hc`、`wrong.hc`、`match.hc`、`import.hc`、`concurrency.hc`、`calc.hc`、`tree.hc`（class 双后端一致性）。
- 冒烟测试：`tests/smoke.js`（34 项断言通过）。

## 编译后端（已交付：C 原生 + JS 回退）

- `src/cgen.js`：AST → C 源码（块子集）——**"块 = 连续内存"在 C 中零运行时开销原样兑现**（struct → C struct、enum + 穷尽 match → typedef enum + switch、u64/f64/bool/Str → 原生类型）。
- **动态块 `[T]`**：连续数据区 + 长度（`T_Array`）——数组字面量 → 复合字面量、索引 → `.data[i]`、`.len` → `.len`。
- `h build <file>`：探测编译器 `zig cc`（自带 clang）→ `gcc` → `cc` → `clang`，编译为原生二进制；无 C 编译器时回退 `jsgen`（JS 目标）。
- **最短往返浮点格式化**（`h_print_f64`）：%.*g 循环至往返一致——与解释器 JS 输出逐位对齐。
- **双后端一致性已验证（原生级）**：`calc.hc`（enum/match/struct）、`array.hc`（动态块）、`tree.hc`（class 树 + move + 方法）、`ref.hc`（ref 字段双向引用通知）、`ref_param.hc`（ref/move 参数）、`error.hc`（error 未处理即终止）、`concurrency.hc`（并发 + Channel 交替，单线程逐字一致 / `--threads 2` M:N 关键行齐全）、`spawn_args.hc`（带参 spawn）、`bytes.hc`（字节化往返）编译后输出与 `h run` 逐行一致（smoke 断言，51 项全绿）。
- 入口语义：main 若定义且未被显式调用 → 自动调用（两后端一致）；枚举比较统一为 `类型.变体`。
- 文件后缀：H 语言源码统一使用 `.hc`（区别于 C 头文件）。
- 限制（编译时拒绝并提示用 `h run`）：非 Channel 的 global/访问模式、非 Windows 平台并发运行时。
- 环境注：Windows 下推荐 `zig cc`（Zig 自带 clang 21）；直接运行 exe 时中文输出需 UTF-8 代码页（`chcp 65001`）。

## C 目标映射（设计，待编译器环境）

- `struct` → C struct（块 = 连续内存的直接映射，C 天然值语义）
- `enum` + 穷尽 `match` → C enum + switch
- `u64`/`f64`/`bool` → `uint64_t`/`double`/`bool`；`Str` → `const char*`
- 函数/表达式 → C 函数/表达式；`print` → printf
- 树/并发/error：需运行时支持（双向引用通知/调度器），后续切片

## C 目标映射（已实现，见上）

- struct → C struct（typedef + 前向声明）；enum + match → typedef enum + switch（GNU 语句表达式）
- u64/f64/bool/Str → unsigned long long/double/bool/const char*
- 数组 → `T_Array`（len + 指针），构造函数内深拷贝数据区，析构释放
- **class（树）→ 堆指针 + helper**：
  - `Account` → `struct Account` + `typedef`；值 = `Account*`（指针即引用语义，零拷贝）
  - 构造 `Account{...}` → `h_new_Account(字段按声明序)`（malloc）；作用域退出 → `h_free_Account`（free）
  - **生命周期 = 作用域**：局部树变量登记到销毁表，块/函数退出自动 free；`move x` 把源移出销毁表（所有权转移，不销毁）；`return x`（`-> move T`）同样逃逸
  - **树参数 = 视图**（val 不拥有、不销毁）；`move` 参数才拥有所有权
  - 方法 → 静态派发 `Type_method(self, ...)`；方法体内裸字段名 → `self->field`（含嵌套块）
  - **打印一致性**：`print(树)` 生成 `h_print_Type`（递归输出 `Type{字段: 值, ...}`，数组 `[...]`、字符串带引号）——与解释器 valueToStr 逐字一致
  - **ref 字段（双向引用通知）**：`next: ref T` → `T*` + 对象内嵌被引用链表（`_refs` 头 + 每 ref 字段一个 `h_ref_link` 节点，零额外 malloc）；写字段经 setter（先注销旧目标、再注册新目标）；`h_free_Type` 先遍历链表把所有指向本对象的 ref 字段置 NULL（防悬垂），再注销自身持有的 ref 字段；访问已失效字段 = NULL 解引用（原型）；循环引用天然安全（先销毁者通知置空）
  - **ref 参数 = 写透别名**：`ref T` 参数 → `T**`（指向调用者变量），函数内使用自动解引用（`(*p)->f` / `(*p)`）；实参传 `&var`；checker R3 强制实参为可写变量（消灭求值器退化路径，两端统一严格）
  - **move 参数 = 所有权转移**：`move T` 参数拥有所有权，函数退出时销毁；`foo(move x)` 后源失效（已从销毁表移除）
  - **error union**：`-> error T` → `h_err_T{ ok, val, name }`；`return error.X` / `return 值` 分别生成 ok=false/true 包装；调用 error 函数自动解包（语句表达式），未处理时打印 `❌ error.X（未处理）` 到 stderr 并以退出码 1 终止（与解释器一致）；error 入口同样检查
  - **并发**：**跨平台**协程 + M:N worker 线程运行时——Windows 用 Fiber、POSIX（Linux/macOS）用 ucontext，锁/条件变量/原子/线程经宏与平台函数抽象（`h_lock`/`h_cond`/`h_atomic_*`/`h_thread_*`），调度核心平台无关；`h build --threads N` → `H_THREADS` 宏——`spawn f()` 请求投递到 worker（round-robin），**协程须在创建线程运行故由 worker 线程自建**；`yield` → 切回本 worker 调度器重入队；`Channel(n)` → 全局状态 + 锁/条件变量：send 满/recv 空挂起，成功路径即时唤醒跨线程等待者；`print` 经输出锁原子输出；执行体完成计数归零后广播 shutdown
  - **单线程模式（H_THREADS=1，默认）**：延迟唤醒（空转时 recv/send 等待者按序唤醒）——与求值器单线程调度**逐字一致**；多线程模式顺序不保证（SPEC 04），验证关键行集合
  - 带参 spawn 经打包结构体 `h_sp_ctx_f` 传参；全局 `Channel<T>` 在 `h_global_init` 构造（原型仅 u64 值）
  - **字节化（to_bytes/from_bytes）**：可逆自描述 JSON（与求值器 `JSON.stringify` 逐字节一致）——per-type 生成 `h_tb_T`（序列化，最短往返数字、`\x` 转义字符串）+ `h_jrev_T`（反序列化，递归重建）+ `h_to_bytes_T`/`h_from_bytes_T` 入口；**顶层带格式版本字段 `"__ver":1`**（未知版本报错、缺失视为 v1 兼容旧数据）；**类型标签注册机制**——生成 `h_type_registry`（类型名→元数据，运行时可见）+ `from_bytes` 校验 `__type` 匹配目标类型（两端一致）；`x.to_bytes()` → 字符串，`Type.from_bytes(s)` → 恢复实例（ref 字段经 setter 重新注册）；块=连续数据直接映射、树=序列化压平
- 限制：非 Channel 的 global（Exclusive/SharedRead）编译时拒绝（提示用 h run）。Windows 平台运行验证（Fiber）；POSIX 平台经 zig cc 交叉编译验证（x86_64/aarch64 Linux），运行验证待真实环境。
- 元组/切片（ADR 0007/0008）：设计已定，两后端实现待排期（parser/checker/evaluator/cgen 同步落地）。

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
