# H 语言类型实现 Checklist

用途：追踪 H 语言数据类型（对照 Rust / Zig）的实现进度。完整对照与设计背景见 `docs/spec/07-comparison.md`（01 数据模型见 `docs/spec/01-data.md`）；本清单是可勾选的追踪版。

图例：☑ 已实现（双后端一致，smoke 验证）｜◐ 部分（语法/检查器有，运行时受限）｜△ 设计定（ADR/SPEC，未实现）｜☐ 未实现

验证口径：一项类型"已实现"= 解释器（`h run`）与 C 编译（`h build --exec`）输出逐字一致，并有 smoke 断言。

## 标量

| ☑ | Rust | Zig | H | 状态 | 说明 |
|---|---|---|---|---|---|
| ☑ | `u64` | `u64` | `u64` | 已实现 | 默认整数（无后缀） |
| ☑ | `u8`/`u16`/`u32`/`u128`/`usize` | `u8`/`u16`/`u32`/`u128`/`usize` 等 | `u8`/`u16`/`u32`/`u128`/`usize` | 已实现 | 字面量后缀 `5u8` |
| ☑ | `i8`/`i16`/`i32`/`i64`/`i128`/`isize` | `i8`-`i128` 等 | `i8`/`i16`/`i32`/`i64`/`i128`/`isize` | 已实现 | 字面量后缀 `-3i32` |
| ☑ | `f64` | `f64` | `f64` | 已实现 | 默认浮点 |
| ☑ | `f32` | `f32` | `f32` | 已实现 | 单精度逐运算截断（`Math.fround`/C `float`） |
| ☐ | `f16`/`f80`/`f128` | `f16`/`f80`/`f128` | — | 未实现 | 无 C 原生对应 |
| ☑ | `bool` | `bool` | `bool` | 已实现 | — |
| ☐ | `char` | `u21` + 字符串 | — | 未实现 | 无单字符类型，用 `Str` |
| ☑ | `()` | `void` | `void` / `()` | 已实现 | 空元组 |

## 复合

| ☑ | Rust | Zig | H | 状态 | 说明 |
|---|---|---|---|---|---|
| ☑ | 元组 `(T1, T2)` | 匿名 struct | 元组 `(T1, T2)`/`(x: T1, y: T2)` | 已实现 | 位置/命名严格分离 + 解构 + 字节化 |
| ☐ | 定长数组 `[T; N]` | 定长数组 `[N]T` | — | 未实现 | 只有动态块 `[T]` |
| ☑ | `Vec<T>` | `ArrayList(T)` | `[T]` 动态块 | 已实现 | 连续数据区+长度 |
| ☑ | `&[T]`/`&mut [T]` | `[]T`/`[]const T` | `[]T` 切片 | 已实现 | 借用视图；`mut` 写透；R12 借入不借出 |
| ☑ | `&str`/`String` | `[]const u8`/`[]u8` | `Str` | 已实现 | 数据区+长度；`+` 拼接 |

## 引用 / 指针

| ☑ | Rust | Zig | H | 状态 | 说明 |
|---|---|---|---|---|---|
| ☑ | `&T` | `*const T` | 树参数默认只读指针 | 已实现 | 只读引用是默认语义 |
| ☑ | `&mut T` | `*T` | `ref T`（字段/参数） | 已实现 | 双向引用通知；不跨执行体 |
| ☐ | `*const T`/`*mut T` 裸指针 | `[*]T` 多指针 | — | 未实现 | 设计取舍：高级指针替代 |
| ☐ | `Option<&T>` | `?*T` | — | 未实现 | 见可选类型 |

## 函数

| ☑ | Rust | Zig | H | 状态 | 说明 |
|---|---|---|---|---|---|
| ☑ | `fn(T) -> R` | `fn` 类型 | `fun(T1, T2) -> R`（函数引用） | 已实现 | 无捕获引用可赋值/传参/调用；闭包未实现 |
| ☐ | 闭包 `Fn`/`FnMut`/`FnOnce` | 闭包（有限） | — | 未实现 | 捕获设计 `[x]`/`[move y]`/`[ref z]` OPEN |
| ☐ | — | `anytype` | — | 未实现 | 用类型推断 + 组合替代 |
| △ | — | — | 函数字节化 | 设计定 | 代码引用+捕获环境，可打包/存储/传输 |

## 自定义类型

| ☑ | Rust | Zig | H | 状态 | 说明 |
|---|---|---|---|---|---|
| ☑ | `struct` | `struct` | `struct`（块） | 已实现 | 值语义、连续内存；块只含块 |
| ☑ | `enum`（无 payload 子集） | `enum` | `enum` + 穷尽 `match` | 已实现 | payload 变体 H/Zig 均无 |
| ☐ | `union` | `union`/tagged union | — | 未实现 | 用 class/枚举+组合表达 |
| ☑ | `dyn Trait`（静态子集） | 无 | `interface` | 已实现 | 纯静态，无运行期接口值 |
| ☑ | `struct` + `Box`/`Rc`/`Arc` | `struct` + 手动堆分配 | `class`（树） | 已实现 | 树 = 一等内存形状 |
| ☐ | `!`（never） | `noreturn` | — | 未实现 | — |

## 可选 / 错误

| ☑ | Rust | Zig | H | 状态 | 说明 |
|---|---|---|---|---|---|
| ☑ | `Option<T>` | `?T` 可选 | `?T` + `null` + `x.?` | 已实现 | 自动提升；仅块 T；字节化 |
| ☑ | `Result<T, E>` | error union `E!T` | `error T` | 已实现 | 值 = 枚举；未处理即终止 |
| ☐ | — | 错误集 `error{...}`/`anyerror` | — | 未实现 | 错误是枚举（块），无全局错误集 |

## 并发 / 共享

| ☑ | Rust | Zig | H | 状态 | 说明 |
|---|---|---|---|---|---|
| ◐ | `Mutex<T>`/`RwLock<T>` | `std.Thread.Mutex` 等 | `Exclusive<T>`/`SharedRead<T>` | 部分 | 模式已进类型（R8）；运行时仅 Channel |
| ◐ | `Arc<T>`/`Atomic*` | `std.atomic.Value` | 模式类型（设计） | 部分 | 共享数据走访问模式 |
| ☑ | mpsc / tokio | 自建 | `Channel<T>` | 已实现 | 内建容量 + 等待者队列 |

## 泛型 / 元编程

| ☑ | Rust | Zig | H | 状态 | 说明 |
|---|---|---|---|---|---|
| ◐ | 泛型 + trait 约束 | `comptime` 参数 | `T<...>`（GenericType） | 部分 | 仅内建模式包装；用户泛型未实现 |
| △ | 宏 | `comptime` | 无宏 | 设计定 | 抽象靠函数 + class 组合 |
| ☑ | `const fn` | `comptime` 全量 | 编译期方法表/类型注册表 | 已实现 | 编译期替代物 |

## 标准库容器

| ☑ | Rust | Zig | H | 状态 | 说明 |
|---|---|---|---|---|---|
| ☐ | `HashMap`/`HashSet`/`BTreeMap`/`BTreeSet` | `HashMap`/`StringHashMap`/`AutoHashMap` | — | 未实现 | 字典按数据模型判定为树 |
| ☐ | `VecDeque`/`LinkedList`/`BinaryHeap` | `PriorityQueue`/`Stack`/`RingBuffer`/`Bitset` | — | 未实现 | — |
| ☑ | `String` | `std.mem` 等 | `Str` | 已实现 | 仅 `Str` |

## 生命周期机制

| ☑ | Rust | Zig | H | 状态 | 说明 |
|---|---|---|---|---|---|
| ☑ | `Box<T>` | `allocator.alloc` | class 构造 + 作用域销毁 | 已实现 | 分配内建但可见 |
| ☑ | `Copy`/`Clone` | 赋值即拷贝 | 块 = memcpy；树 = 引用；`clone()` | 已实现 | 语义按内存形状自动定 |
| ☑ | `Drop` | `defer` | 作用域退出自动销毁 | 已实现 | 无显式 `defer`（自动即显式可观测） |
| ☑ | `Cell`/`RefCell` | — | ref 字段（双向引用通知） | 已实现 | 运行时通知替代借用检查 |

## 待办清单（可勾选追踪）

### 建议优先
- [x] 整数除法（u64 整除，f64 浮点，混合提升）——`examples/loop.hc`
- [x] 循环（`for i in 0..n` / `while` / `break` / `continue`）——`examples/loop.hc`
- [x] 元组 + 切片双后端——`examples/tuple_slice.hc`
- [x] 嵌套元组字节化（struct 字段/数组元素）——`examples/tuple_slice.hc`
- [x] 切片 clone 复合元素（递归深拷贝）——`examples/tuple_slice.hc`
- [x] 可选类型 `?T`（`null`/自动提升/`x.?` 解包/字节化）——`examples/optional_fun.hc`
- [x] 函数作为参数（`fun(T) -> R` 类型、函数名即值、函数值调用）——`examples/optional_fun.hc`
- [x] 全标量类型（`u8`-`u128`/`i8`-`i128`/`usize`/`isize`/`f32`，字面量后缀 + 提升 + 整除扩展 + f32 单精度）——`examples/scalars.hc`
- [ ] 数组 `push` / `pop`（BUILTIN_METHODS 已声明，运行时未实现）
- [ ] `alloc` / `free`（显式分配，BUILTIN_METHODS 已声明）

### 能力补全
- [ ] 闭包（捕获标注 `[x]`/`[move y]`/`[ref z]`）——函数引用（无捕获）已实现，捕获环境是下一步
- [ ] 用户自定义泛型 `T<...>`（当前仅内建模式包装）
- [ ] `Exclusive<T>` / `SharedRead<T>` 运行时实现（C 端目前仅 Channel）
- [ ] `try` / `catch` 错误展开（当前未处理即终止）

### 设计待定（OPEN）
- [ ] 函数字节化（代码引用 + 捕获环境，可打包/存储/传输/恢复执行）
- [ ] 显式 allocator 传参写法（`[T]` 分配函数签名形态）
- [ ] `transmit` 传输内建
- [ ] 标准库容器（HashMap/Set/队列/堆……）——字典按树建模

### 明确不做（设计取舍）
- [x] 裸指针 / 多指针（`*T`/`[*]T`）——高级指针（ref）替代
- [x] 定长数组 `[T; N]`——只有动态块 `[T]`
- [x] `char` 单字符类型——用 `Str`
- [x] 宏 / `comptime`——编译期方法表/类型注册表替代
- [x] `dyn` 动态分派——接口纯静态
- [x] Zig 任意位宽整数（`u24` 等）与 `f16`/`f80`/`f128`——无 C 原生对应，C 映射需结构包装
