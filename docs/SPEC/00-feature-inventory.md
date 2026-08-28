# H 语言功能清单

> 按功能领域分类，仅保留必要描述与完成状态标记。已实现功能标记 ✅，部分实现 🟡，延迟 ⏳。

---

## 一、语言核心（M1 前端）

### 1.1 词法分析（Lexer）
| 功能 | 描述 | 状态 |
|------|------|------|
| 关键字全集 | `fn / var / const / global / if / else / while / for / break / continue / return / switch / defer / errdefer / class / enum / union / tree / interface / where / namespace / import / pub / export / owned / o / move / mut / and / or / try / catch / orelse / script / comptime / anytype / type / async / await / spawn / extern / void / null / true / false` | ✅ |
| 字面量 | 整数（进制前缀+后缀）、浮点、字符串（含转义）、原生字符串 `"""..."""`、字符 | ✅ |
| 运算符/标点 | 完整运算符集（算术/比较/逻辑/位/赋值/范围/`||` 错误集联合） | ✅ |
| 注释 | 行注释 `//` + 块注释 `/* */` | ✅ |
| `@` 内建标识 | `@sizeOf`、`@intCast` 等内建函数名 | ✅ |
| 位置追踪 | 行/列+字节偏移，全诊断使用 | ✅ |

### 1.2 语法分析（Parser + AST）
| 功能 | 描述 | 状态 |
|------|------|------|
| 声明解析 | 函数/变量/常量/全局/class/enum/union/interface/namespace/import/comptime（`script` 已移除，见 12-script-redesign.md） | ✅ |
| 语句解析 | 表达式/变量声明/if/while/for/switch（含守卫）/return/break/continue/defer/errdefer/块 | ✅ |
| 表达式解析 | 完整优先级：字面量/标识符/一元/二元/调用/索引/字段/取址/解引用/if/switch/闭包/构造 | ✅ |
| 类型解析 | 命名类型/指针/切片/可选/错误联合/元组/数组/推断类型 | ✅ |
| 函数参数 | 类型标注/默认值/anytype/T: type 类型参数 | ✅ |
| 重载解析 | 同名函数多候选（含可选参数） | ✅ |
| 测试标记 | `[test]` / `[test("name")]` 声明级属性 | ✅ |
| 类属性 | `[Continuous]` / `[Pad]` / `[Align]` | ✅ |
| 跨文件解析 | 兄弟文件符号登记、目录 = 包 | ✅ |

### 1.3 诊断基础设施
| 功能 | 描述 | 状态 |
|------|------|------|
| 多错误收集 | 全量收集，非首个即停 | ✅ |
| 错误级别 | Error / Warning / Note 三级 | ✅ |
| 精确位置 | 行列号+源行指示符 | ✅ |
| 渲染输出 | 可读文本 | ✅ |

---

## 二、语义分析（M2）

### 2.1 名称解析
| 功能 | 描述 | 状态 |
|------|------|------|
| 符号表 | 声明级名称登记与查找 | ✅ |
| 重载池 | 同名函数多候选管理 + 歧义检测 | ✅ |
| 接口三用途 | 类型约束 / 方法表 / 多态分发 | ✅ |
| 跨文件符号 | 兄弟文件全可见、依赖包仅 pub | ✅ |

### 2.2 类型检查
| 功能 | 描述 | 状态 |
|------|------|------|
| 基本类型 | i8–i128 / u8–u128 / f16–f64 / bool / void / null / type | ✅ |
| 复合类型 | struct / enum / union / 指针 / 切片 / 数组 / 元组 / 可选 / 错误联合 | ✅ |
| 表达式级检查 | 类型一致性、运算符约束 | ✅ |
| 期望类型传播 | 上下文类型向下传播 | ✅ |
| 字段/索引校验 | struct 字段存在性、数组索引合法性 | ✅ |
| 存储形态验证 | `[Continuous]` 字段值类型验证 | ✅ |
| 泛型 where 约束 | 调用点 where 子句验证 | ✅ |
| 无限大小类型拒绝 | 值内嵌自引用/互递归无间接层 → 编译错误 | ✅ |
| 标量宽度检查 | `var g: u8 = 256` 编译期报错 | ✅ |
| 引用赋值禁止 | `var w: Vec<i32> = v` 报错（要求 `copy(&v)` 或指针） | ✅ |
| 错误集成员检查 | `return error.X` 必须属于函数错误集 | ✅ |

### 2.3 类型推断
| 功能 | 描述 | 状态 |
|------|------|------|
| 局部推断 | 变量绑定处类型推断 | ✅ |
| 泛型 T | 泛型类型参数推断 | ✅ |
| 指针形态 | `*T` / `*mut T` 自动判定 | ✅ |
| 多路径返回 | 条件分支类型统一 | ✅ |
| 重载歧义 | 候选参数匹配与歧义报告 | ✅ |

### 2.4 所有权分析
| 功能 | 描述 | 状态 |
|------|------|------|
| 分配来源跟踪 | 编译时追踪值的作用域来源（Arena/global/栈） | ✅ |
| `move` 合法性 | 禁止 `move` 出 Arena/global/值类型 | ✅ |
| 引用逃逸 | 引用赋值禁止、局部引用逃逸检测 | ✅ |
| 多指针合法 | 多 `*mut`/`*T` 合法，无唯一写者约束 | ✅ |
| 作用域销毁 | LIFO 销毁代码生成 | ✅ |
| definite assignment (C7) | `alloc.init(T)` 无参构造后字段未全赋值即 return → 编译错误 | ✅ |
| `owned` 变量检查 | `owned` 标注变量必须匹配 `defer` 或 `move`，否则编译错误 | ✅ |

### 2.5 错误集分析
| 功能 | 描述 | 状态 |
|------|------|------|
| 显式错误集 | `error{ NotFound, ... }` 声明 | ✅ |
| `!T` 推断 | 函数返回错误集自动推断 | ✅ |
| `anyerror` | 不约束的通用错误类型 | ✅ |
| 错误码表 | 名↔码全局唯一映射（包 ID 高位 + 包内码低位） | ✅ |
| 首次出现位置 | 每个错误记录首次出现位置 | ✅ |
| 内建标准错误 | `OutOfMemory` 等内建错误 | ✅ |

### 2.6 函数语义
| 功能 | 描述 | 状态 |
|------|------|------|
| 函数重载 | 同名函数多签名（参数类型/数量区分） | ✅ |
| 可选参数 | 带默认值的可选参数 | ✅ |
| 闭包捕获 | 精确化捕获集合 + `move` 语义 | ✅ |
| 闭包类型推断 | 参数/返回类型推断 | ✅ |

---

## 三、双后端（M3）

### 3.1 共享 IR
| 功能 | 描述 | 状态 |
|------|------|------|
| 线性指令 | 标量运算 / 控制流 / 函数调用 / 错误值通道 | ✅ |
| 指针操作 | 取址 / 解引用 / 写穿别名 | ✅ |
| 聚合操作 | 数组/元组/struct/enum 字面量、字段/索引/切片读写、解构、`move` | ✅ |
| switch + range + for | 模式匹配线性链、`0..n` 糖、迭代含 mut 写回、无标签 break/continue | ✅ |
| 闭包/方法/重载 | 闭包表、方法表、重载候选索引 | ✅ |
| 全局/常量 | 全局表、`@__init__` 初始化 | ✅ |
| 错误码运行时 | 错误值通道 + 码运行时表示 | ✅ |
| 子集外拒绝 | 不支持特性以 `error.Unsupported` 硬错误 | ✅ |
| 类型表 | ClassInfo / EnumInfo / UnionInfo 构建 | ✅ |
| 枚举变体表 | 枚举变体名 → 索引映射 | ✅ |

### 3.2 字节码 VM（HBC2）
| 功能 | 描述 | 状态 |
|------|------|------|
| 序列化格式 | HBC2：magic + 版本 + 函数表 + 闭包表 + 全局表 + 枚举表 + continuous 表 + union 表 | ✅ |
| 全精度载荷 | i128 16 字节、f64 8 字节、字符串长度前缀 | ✅ |
| 解码执行 | decode → 复用 `run_ir`（同一语义源） | ✅ |
| 确定性编码 | 编码→解码→再编码 字节级一致 | ✅ |

### 3.3 LLVM 原生后端
| 功能 | 描述 | 状态 |
|------|------|------|
| LLVM IR 文本发射 | 生成 `.ll` 文本 | ✅ |
| `zig cc` 编译链接 | 外部 `zig cc` 编译为可执行文件 | ✅ |
| 库构建 | 静态归档（`.a`）+ dll（`.dll`，`--dll` 标志） | ✅ |
| 依赖库链接 | 本地依赖先构建为库，收集符号表链接 | ✅ |
| 符号表 | 限定名 → 导出符号映射（`.sym`） | ✅ |
| 测试跑器 | `codegen_tests` 生成测试驱动 IR | ✅ |
| 字节码回退 | `zig cc` 缺失时回退 .hbc + 启动器 | ✅ |

### 3.4 双模式一致性
| 功能 | 描述 | 状态 |
|------|------|------|
| 一致性测试 | tree-walking ↔ IR 参考解释器 90 测试 | ✅ |
| 交叉验证 | `hc test --mode=compile` 解释器 vs 原生 | ✅ |
| 四个后端同语义源 | 共享 `IrModule` + `run_ir`（ADR-0004） | ✅ |

---

## 四、运行时与内建（M4）

### 4.1 值模型
| 功能 | 描述 | 状态 |
|------|------|------|
| 值类型 | Int / Float / Bool / Str / Arr / Slice / Class / Enum / Opt / Err / Ptr / Boxed / Vec / Map / Fn / Closure / Allocator / Bytes / Alloc / Arena / Void / Dangling | ✅（Allocator/Bytes ⏳ Phase 1；Alloc/Arena 🟡 Phase 3 移除） |
| 值比较 | 相等比较（`value_eq`）+ 排序（`value_lt`） | ✅ |
| 类型名 | 运行时类型名查询 | ✅ |
| 显示 | 值格式化输出 | ✅ |

### 4.2 内存模型
| 功能 | 描述 | 状态 |
|------|------|------|
| 作用域 LIFO | 作用域退出时自动销毁局部值 | ✅ |
| Arena 分配器 | bump 分配 + 块链表 + 批量释放 | ✅ |
| Allocator 抽象 | 全局回退 + 显式参数传递 | ✅ |
| 内存泄漏检测 | Debug 退出时报告泄漏清单 | ✅ |

### 4.3 `@` 内建函数
| 功能 | 描述 | 状态 |
|------|------|------|
| 类型查询 | `@sizeOf` / `@alignOf` / `@offsetOf` / `@typeOf` | ✅ |
| 类型转换 | `@intCast` / `@ptrCast` / `@truncate` | ✅ |
| 编译错误 | `@compileError` | ✅ |
| 算术 | `@addWithOverflow` / `@subWithOverflow` / `@mulWithOverflow` | ✅ |
| 内存 | `@ptrFromInt` / `@intFromPtr`（K4） | ✅ |
| 原子操作 | `@atomicLoad` / `@atomicStore` / `@atomicRmw` | ✅ |
| 暂停 | `@panic`（中止/回卷） | ✅ |
| 正则 | `@regex` / `@regexMatch` | ✅ |
| 枚举转换 | `@intFromEnum` / `@enumFromInt` | ✅ |
| 对齐转换 | `@alignCast` | ✅ |
| 易变访问 | `@volatileLoad` / `@volatileStore` | ✅ |

### 4.4 序列化内建
| 功能 | 描述 | 状态 |
|------|------|------|
| 二进制 | `@toBytes` / `@fromBytes` | ✅ |
| JSON | `@toJson` / `@fromJson` | ✅ |
| 装箱 | `box` / `unbox` | ✅ |
| `copy` 浅复制 | `copy(&x, .shallow)` 浅层复制，默认深复制不变 | ✅ |

### 4.5 标量接口族
| 功能 | 描述 | 状态 |
|------|------|------|
| ICompare | `<` `<=` `>` `>=` `==` `!=` | ✅ |
| INumber | 算术运算符绑定 | ✅ |
| IInt / IUint | 整数接口 | ✅ |
| IFloat | 浮点接口 | ✅ |

### 4.6 迭代内建
| 功能 | 描述 | 状态 |
|------|------|------|
| IIterable | 可迭代接口三态 | ✅ |
| `iter()` | 容器迭代统一入口 | ✅ |

### 4.7 Debug 悬垂检测
| 功能 | 描述 | 状态 |
|------|------|------|
| 悬垂标记 | 目标销毁时标记指向它的指针 | ✅ |
| 访问检测 | 访问已标记指针 → 带位置错误提示 | ✅ |
| 开关 | `--dangle=on|off|auto` 运行时控制 | ✅ |
| Release 裸读 | Release 无标记开销 | ✅ |

### 4.8 语义辅助
| 功能 | 描述 | 状态 |
|------|------|------|
| `.name` 枚举字面量推断 | `copy(&x, .shallow)` ≡ `copy(&x, CopyMode.shallow)` | ✅ |
| `owned` 所有权前缀 |  `owned T` 标注所有权形态，仅记录不改变语义 | ✅ |
| `FnN<i32> i32` 函数类型 | `Fn1<i32> i32` 等函数类型语法，参数+返回类型 | ✅ |
| definite assignment (C7) | `alloc.init(T)` 无参构造跟踪字段初始化，return 前未全赋值 → 编译错误 | ✅ |

---

## 五、元编程（E1）

### 5.1 脚本系统（12-script-redesign.md，2026-08-23 定案）

脚本功能重新设计：`.hs` 文件为唯一脚本文件格式，`.hc` 中的 `script { }` 块已移除。

| 功能 | 描述 | 状态 |
|------|------|------|
| `.hs` 文件解析 | `.hs` 后缀脚本文件直接解析执行 | ✅ |
| 文件引用 `import "path"` | 脚本文件通过 `import "path"` 引用其他 `.hs` 文件 | ✅ |
| 三路径搜索 | ① 当前文件目录 → ② SDK 目录 → ③ 项目目录 | ✅ |
| 引用验证 | `.hs` 只能引用 `.hs` 文件，引用 `.hc` 报错 | ✅ |
| 标准库访问 | 保留 `import H.std.{io}` 标准库访问 | ✅ |
| 脚本缓存 (B6-2) | `~/.hc/cache/hs/<hash>` 基于文件路径 + mtime 缓存 | ✅ |
| Comptime 保留 | `comptime { }` 块在 `.hc` 中保留，与脚本功能独立 | ✅ |

### 5.2 Comptime（ADR-0012）
| 功能 | 描述 | 状态 |
|------|------|------|
| `comptime { }` 块 | 装载期求值，结果丢弃 | ✅ |
| 类型函数 | `fn List(T: type) type` 返回 `type` 的编译期函数 | ✅ |
| 具体化引擎 | 名字 + 实参的 monomorphization + 惰性缓存 | ✅ |
| 类型替换 | `T` → 实参类型深度遍历替换 | ✅ |
| 返回值类型 | `return struct { ... }` / `return T;` / `return [n]T;` | ✅ |
| `anytype` | 参数按实参具体类型实例化 | ✅ |
| comptime 值函数 | 参数含 `T: type` 但非返回 `type` 的编译期函数 | ✅ |
| `comptime_int` | 惰性宽度字面量参数 | ✅ |
| 错误机制 | comptime 块返回 `error.X` = 编译错误 | ✅ |

### 5.3 泛型
| 功能 | 描述 | 状态 |
|------|------|------|
| 泛型函数 | 通过类型函数 + anytype 实现 | ✅ |
| 泛型容器 | `List<i32>` / `Pair(i32, f64)` 等 | ✅ |
| 嵌套实例化 | `List(Pair<i32>)` 内层先登记 | ✅ |
| where 子句 | 泛型约束验证 | ✅ |
| 内建泛型嵌套具体化 | `Vec<List<i32>>` 具体化后类型正确 | ✅ |
| 无限大小类型拒绝 | 值内嵌自引用/互递归无间接层 → 编译错误 | ✅ |

---

## 六、并发与异步（E2）

### 6.1 协程与通道（新增，替代四模式容器）
| 功能 | 描述 | 状态 |
|------|------|------|
| `chan<T>` | 通道类型（Mutex+Condvar 实现） | ✅ |
| `chan.init(alloc[, cap])` | 通道构造（有缓冲/无缓冲） | ✅ |
| `ch.send(v)` | 阻塞发送 | ✅ |
| `ch.recv() T` | 阻塞接收 | ✅ |
| `ch.try_send(v) bool` | 非阻塞发送 | ✅ |
| `ch.try_recv() ?T` | 非阻塞接收 | ✅ |
| `ch.close()` | 关闭通道 | ✅ |
| `spawn(f, args...)` | 启动协程（M:N 调度） | ✅ |
| `join()` | 等待协程完成 | ✅ |
| `cancel()` | 取消协程 | ✅ |
| `is_done()` | 查询协程完成状态 | ✅ |
| `detach()` | 分离协程 | ✅ |
| M:N 调度器 | G+P+M 模型，worker 线程池 | ✅ |

### 6.2 异步
| 功能 | 描述 | 状态 |
|------|------|------|
| `async fn` | 异步函数定义 | ✅ |
| `await` | 异步等待表达式 | ✅ |
| 事件驱动 | `Io.evented` 事件模式 | ✅ |

### 6.3 四模式容器（已弃用，推荐使用 chan<T>）
| 功能 | 描述 | 状态 |
|------|------|------|
| `Pipe` | 单生产者-单消费者 | ⚠️ 弃用 |
| `Tee` | 单生产者-多消费者 | ⚠️ 弃用 |
| `Funnel` | 多生产者-单消费者 | ⚠️ 弃用 |
| `Hub` | 多生产者-多消费者 | ⚠️ 弃用 |

### 6.4 原子操作
| 功能 | 描述 | 状态 |
|------|------|------|
| `@atomicLoad` | 原子加载 | ✅ |
| `@atomicStore` | 原子存储 | ✅ |
| `@atomicRmw` | 原子读-改-写 | ✅ |

### 6.5 Send/Sync 静态诊断
| 功能 | 描述 | 状态 |
|------|------|------|
| `type_is_send`/`type_is_sync` | 递归类型检查 | ✅ |
| class 接口声明 | `class Foo: Send/Sync` 字段验证 | ✅ |
| spawn 边界 | 非 Send 捕获 → 编译错误 | ✅ |

### 6.6 Mutex
| 功能 | 描述 | 状态 |
|------|------|------|
| `Mutex.init(v)` | 互斥锁构造 | ✅ |
| `m.lock() !T` | 阻塞获取锁 | ✅ |
| `m.try_lock() ?T` | 非阻塞尝试 | ✅ |

---

## 七、标准库（M5 + E3）

### 7.1 mem 内存分配
| 功能 | 描述 | 状态 |
|------|------|------|
| `Allocator` 接口 | 分配器抽象接口（`alloc`/`realloc`/`free`） | ✅ |
| `page_allocator` | 全局无状态分配器（每 `alloc` 创建独立 Vec） | ✅ |
| `Arena` | bump 分配器，接收后备分配器 `Arena.init(backing)` | ✅ |
| `Pool(T)` | 固定大小对象池，空闲链表 + 后备分配器 | ✅ |
| 全局回退 | 默认全局分配器（`alloc` 环境变量） | ✅ |
| `AllocatorImpl` 枚举 | Rust 侧分配器枚举（Page/Arena/Pool/Custom） | ✅ |
| `Value::Allocator` | 统一分配器值，替代 `Value::Alloc`/`Value::Arena` | ✅ |
| `Value::Bytes` | 原始内存块值类型 | ✅ |
| `AllocBlock` | Rust 侧分配器返回的内存块（data + offset + len） | ✅ |
| `AllocErr` | 分配失败错误（OutOfMemory/InvalidSize） | ✅ |
| `with_arena(fn)` | 创建临时 Arena，调用函数后自动释放（**Phase 3 已移除**，使用 `Arena.init(alloc)` 替代） | ✅ |
| 自定义分配器（H 侧） | 用户实现 `Allocator` 接口的自定义分配器 | ⏳ 1.x |

### 7.2 collections 集合
| 功能 | 描述 | 状态 |
|------|------|------|
| `Vec` | 动态数组 | ✅ |
| `String` | 字符串类型 | ✅ |
| `Map` | 哈希表 | ✅ |
| `Deque` | 双端队列 | ✅ |
| `Table` | 多索引二维表（`t[i, j]` 语法） | ✅ |
| `Pool(T)` | 固定大小对象池，空闲链表 + 后备分配器 | ✅ |
| `sort` | 数组/切片排序（含比较器闭包） | ✅ |
| `binary_search` | 二分查找 | ✅ |
| `sqrt` | 平方根 | ✅ |
| `math` 命名空间 | 数学函数命名空间 | ✅ |

### 7.3 io 输入输出
| 功能 | 描述 | 状态 |
|------|------|------|
| print | 标准输出（格式说明符 `{d}/{x}/{X}/{b}/{e}/{s}` + 宽度/对齐/精度） | ✅ |
| 格式串 comptime 校验 | 说明符与参数类型不匹配编译报错 | ✅ |
| 文件系统 | 文件读写（seek/pos/read_at/write_at、read_int/write_int、open_dir/Dir、list_dir → `Vec<DirEntry>`） | ✅ |
| `io.stdout`/`io.stderr` | 独立字节流句柄 | ✅ |
| net TCP | TCP 网络 | ✅ |
| net UDP | UDP 网络 | ✅ |
| net HTTP | HTTP 客户端 | ✅ |
| net 帧读写 | `read_u32_le`/`write_u32_le` 帧读写 | ✅ |
| IPC 管道 | 进程间通信管道（PipeReader/PipeWriter，匿名管道） | ✅ |
| 共享内存 | Shm 命名共享内存 | ✅ |
| 程序环境 | 命令行参数、环境变量 | ✅ |

### 7.4 time 时间
| 功能 | 描述 | 状态 |
|------|------|------|
| 时间查询 | 当前时间、时间计算 | ✅ |
| 单调测量 | `tick()`/`elapsed(tick)` 纳秒计数单调测量 | ✅ |

### 7.5 rng 随机数
| 功能 | 描述 | 状态 |
|------|------|------|
| xorshift64* | 伪随机数生成器 | ✅ |

### 7.6 text 文本
| 功能 | 描述 | 状态 |
|------|------|------|
| 文本处理 | 字符串操作（含 `String.to_upper`/`to_lower`），正则表达式 `matches`/`find`/`replace`/`split` | ✅ |

### 7.7 序列化
| 功能 | 描述 | 状态 |
|------|------|------|
| `fmt_int` / `fmt_float` | 整数/浮点格式化输出 | ✅ |
| `parse_int` / `parse_float` | 数字文本解析 | ✅ |
| `json.parse` / `csv.parse` | JSON/CSV 解析 | ✅ |
| parse 辅助组 | `parse_number` / `skip_space` / `peek` / `advance` / `is_digit` / `expect` | ✅ |
| 序列化内建 | `@toBytes` / `@fromBytes` / `@toJson` / `@fromJson` | ✅ |

### 7.8 归档
| 功能 | 描述 | 状态 |
|------|------|------|
| RLE 压缩 | `io.archive` RLE 编解码 | ✅ |

### 7.9 storage 存储
| 功能 | 描述 | 状态 |
|------|------|------|
| `io.storage` KV 存储 | 文件持久化键值存储（put/get/contains/remove/len/close） | ✅ |

### 已实现（2026-08 更新）
| 功能 | 描述 | 状态 |
|------|------|------|
| 惰性/组合子迭代器 | `iter()` 返回 LazyIter，`filter`/`map` 链式延迟 | ✅ A7 |
| 通用压缩算法 | LZ77 滑动窗口压缩（token 0x00/0x01/0x02），RLE 保留 | ✅ A3 |
| 时区完整 | `io.time.components`/`format`/`local_offset` | ✅ A4 |
| 标准库数据结构 | bitmap / 环形缓冲 / 页内存 / 侵入式链表 / 有序映射 | ✅ A6 |

### 未实现
| 功能 | 描述 | 状态 |
|------|------|------|
| 包注册中心正式版 | 官方包注册中心 + 供应链校验 | ⏳ 1.x |
| 真 OS 进程/共享内存 | 跨进程共享内存完整 | ⏳ 1.x |
| 数据库连接抽象 | 数据库连接抽象 | ⏳ 1.x |

---

## 八、系统编程（E4）

| 功能 | 描述 | 状态 |
|------|------|------|
| K1 无标签 union | 裸 union 类型（ADR-0014 H1） | ✅ |
| K2 `volatile` | 易变变量标记（ADR-0014 H2） | ✅ |
| K4 `@ptrFromInt` / `@intFromPtr` | 整数↔指针转换（ADR-0014 H3） | ✅ |
| K5 `export fn` | 导出函数 + 链接脚本（ADR-0014 H4） | ✅ |
| `extern fn` | 外部函数声明（FFI 基础） | ✅ |
| K3 内联汇编 `asm` | 内联汇编 | ⏳ 1.x |
| K6 freestanding | 裸机环境 | ⏳ 1.x |
| K7–K11 | 裸 fn 指针/位域/指针算术/端序/原子类型 | ⏳ 1.x |

---

## 九、错误处理（M6）

| 功能 | 描述 | 状态 |
|------|------|------|
| `error union` 运行时 | `Value::Err { name, code }` 运行时表示 | ✅ |
| `try` 传播 | `try` 表达式沿值通道传播错误 | ✅ |
| `catch` 全链拦截 | `catch` 可捕获错误并提供默认值或处理体 | ✅ |
| `orelse` | 可选值解包 + 默认值 | ✅ |
| `errdefer` | 错误路径延迟执行 | ✅ |
| 不可恢复错误 | `@panic` abort（可配置中止/回卷，无 unwind） | ✅ |
| 错误集联合 `||` | 错误集合并运算符 | ✅ |
| 根作用域未处理 | 未处理错误到根 → panic 中止 | ✅ |
| 悬垂访问错误 | 带触发位置的悬垂错误报告 | ✅ |

---

## 十、测试基础设施（M6）

| 功能 | 描述 | 状态 |
|------|------|------|
| `[test]` 标记 | 声明级测试标记 | ✅ |
| `[test("name")]` | 带显示名的测试函数 | ✅ |
| `[test(async)]` | 测试模式属性解析 + 异步 runner（evented IO + Future 执行） | ✅ |
| `[test(thread)]` | 测试模式属性解析 + OS 线程 runner（独立 Interp 实例 + recv_timeout 硬超时） | ✅ |
| `[test(timeout=N)]` | 测试超时属性解析 + 运行时超时检测（含 async/thread 模式） | ✅ |
| 断言五件套 | `expect` / `expect_eq` / `expect_neq` / `expect_error` / `expect_eq_slices` | ✅ |
| 输出统计 | `[PASS]` / `[FAIL]` / `[SKIP]` + 汇总 | ✅ |
| 失败非零退出 | 测试失败非零退出码 | ✅ |
| 跳过机制 | `return error.SkipTest` | ✅ |
| 测试隔离 | 独立函数作用域、默认串行 | ✅ |
| 隐式环境 | 测试函数内隐式 `test_io` + `alloc` | ✅ |
| 编译模式交叉验证 | `hc test --mode=compile` 解释器 vs 原生 | ✅ |
| 测试基建自测 | 独立测试文件：收集/退出码/注入/汇总 | ✅ |
| `io.exit`/`ExitType` 测试 | Exit 静默/Error 打印/退出码 | ✅ |
| fs 余项直测 | append/rename/remove/list_dir/read_int/write_int | ✅ |

---

## 十一、模块与包系统（M1 + M7）

| 功能 | 描述 | 状态 |
|------|------|------|
| `namespace` | 命名空间声明 | ✅ |
| `import` | 包导入（含 `H.std.{...}` 限定选择） | ✅ |
| `pub` | 可见性控制 | ✅ |
| `export` | 导出标记 | ✅ |
| 目录 = 包 | 同目录 `.hc` 文件自动组包 | ✅ |
| 兄弟文件符号 | 同包文件间全可见 | ✅ |
| `src/Modules/` 目录模块 | 子目录自动识别为模块，命名空间计算自动剥离 `Modules/` 前缀 | ✅ |
| `context.hc` 文件约定 | 模块的 context 定义文件，编译器自动识别 | ✅ |
| 模块 context.hc 验证 | `hc run` 时检查 src/Modules/ 下各子目录是否有 context.hc | ✅ |
| `IContext` 接口 | `H.std.ioc` 提供 IoC 容器接口（register/get/registerFactory） | ✅ 运行时内建 |
| `AppContext` 实现 | 应用级 context，背靠 Arena，支持层级委托 | ✅ |
| Context 层级委托 | 子 context 持有父 context 引用，解析不到时向上委托 | ✅ |
| 模块面向接口编程 | 模块只知接口，不知具体实现，通过 context 获取实例 | ⏳ 待实现 |
| 命名注册 | 同一接口可注册多个实现，通过 name 区分 | ✅ |
| 工厂方法注册 | `registerFactory<T>(name, factory)` 接收 context 参数，`make` 调用 | ✅ |
| `tests/` 目录 | 项目根目录，不参与命名空间，`hc test` 发现执行 | ✅ |
| 依赖包 pub 边界 | 跨包仅 pub 符号可见 | ✅ |
| `build.zon` 清单 | 包名/版本/类型/文件/依赖声明 | ✅ |
| 本地依赖 | 基于 path 的本地包依赖 | ✅ |
| 注册中心依赖 | 远程包解析（`~/.hc/registry/`） | ✅ |
| 依赖递归装载 | 依赖的依赖递归解析 | ✅ |
| 版本声明检查 | 依赖版本不符告警 | ✅ |
| 指纹校验 | SHA-256 / 整数指纹 | ✅ |
| `[module]` 领域约定 | 已移除，由 `src/Modules/` 目录结构替代（ADR-0026） | 🟡 移除 |

---

## 十二、工具链（M7 + E5）

### 12.1 CLI 命令
| 功能 | 描述 | 状态 |
|------|------|------|
| `hc run <file>` | 脚本模式（tree-walking 解释器，全语言） | ✅ |
| `hc run <file.hs>` | `.hs` 脚本文件直接执行（B6-2：无 script 展开、无 comptime） | ✅ |
| `.hs` 文件引用 | `import "path/to/file.hc"` 文件路径引用（B6-2：补充命名空间导入） | ✅ |
| `Decl::Include` 解析 | 解析器支持 `import "path"` 语法（AST 新变体） | ✅ |
| `hc run --bench <file>` | 分阶段计时输出（B6-1） | ✅ |
| `hc run <dir>` | 目录包模式（入口 = main.hc 或首个 .hc） | ✅ |
| `hc run --ir <file>` | IR 参考解释器 | ✅ |
| `hc run <file.hbc>` | 字节码 VM（HBC2） | ✅ |
| `hc run --dangle=on|off|auto` | 悬垂检测模式控制 | ✅ |
| `hc test [file|dir]` | 测试运行器 | ✅ |
| `hc test --mode=compile` | 编译模式交叉验证 | ✅ |
| `hc test --dangle=...` | 测试时悬垂模式控制 | ✅ |
| `hc build <file|dir>` | 原生编译（LLVM + zig cc） | ✅ |
| `hc build --dll` | dll 动态库构建 | ✅ |
| `hc check <file>` | 仅词法/语法/装载检查 | ✅ |
| `hc errors <file>` | 错误码表导出 | ✅ |
| `hc lex <file>` | Token 流转储 | ✅ |
| `hc init <name>` | 项目骨架初始化 | ✅ |
| `hc fmt <file>` | 代码格式化（token 级重排 + AST 保真） | ✅ |
| `hc fmt --check` | 格式化幂等门 | ✅ |
| `hc lint <file>` | 静态检查 | ✅ |
| `hc lint --json` | JSON 格式输出 | ✅ |
| `hc doc <file>` | 文档生成 | ✅ |
| `hc doc --project` | 项目级文档生成 | ✅ |
| `hc doc --stdlib` | 标准库文档生成 | ✅ |
| `hc cc` | C 互操作编译 | ✅ |
| `hc pkg add` | 包依赖添加 | ✅ |
| `hc pkg publish` | 包发布到注册中心 | ✅ |

### 12.2 LSP 语言服务器
| 功能 | 描述 | 状态 |
|------|------|------|
| 诊断推送 | 实时语法/语义诊断 | ✅ |
| 符号表 | 函数/类/枚举/接口/变量/常量/命名空间/字段/方法 | ✅ |
| 文档管理 | 打开/关闭/变更/保存 | ✅ |
| 项目上下文 | 项目根目录 + build.zon 解析 | ✅ |
| 自动补全 | 关键字/类型/符号自动补全 + dot-qualified 补全 | ✅ |
| 跳转到定义 | 跨文件符号定义跳转 | ✅ |
| 悬停提示 | 类型信息/签名/文档注释 | ✅ |

### 12.3 Linter 规则
| 功能 | 描述 | 状态 |
|------|------|------|
| `unused_var` | 未使用变量 | ✅ |
| `unused_import` | 未使用导入 | ✅ |
| `redundant_eq_false` | `x == false` 可简化为 `!x` | ✅ |
| `redundant_eq_true` | `x == true` 可简化为 `x` | ✅ |
| `redundant_ne_false` | `x != false` 可简化为 `x` | ✅ |
| `redundant_ne_true` | `x != true` 可简化为 `!x` | ✅ |
| `simplifiable_if_else` | `if x { true } else { false }` 可简化为 `x` | ✅ |
| `simplifiable_construct` | 可简化类型构造 | ✅ |
| `upper_case_abbr` | 全大写缩写命名警告 | ✅ |
| `// lint-off` | 行内 lint 禁用 | ✅ |

### 12.4 文档生成器
| 功能 | 描述 | 状态 |
|------|------|------|
| `///` 文档注释 | 文档注释解析 | ✅ |
| 单文件文档 | 从 `.hc` 生成 Markdown 文档 | ✅ |
| 项目文档 | 多文件项目文档生成 + 索引页 + 导航回链 | ✅ |
| 标准库文档 | 标准库 API 文档生成（内置目录化摘要） | ✅ |
| 输出目录约定 | 默认 `<target>/docs/api/`，`--out` 可覆盖 | ✅ |

### 12.5 格式化器
| 功能 | 描述 | 状态 |
|------|------|------|
| Token 级重排 | 基于 token 流的格式化 | ✅ |
| AST 保真 | 格式化后保持 AST 语义不变 | ✅ |
| `--check` | 幂等性检查 | ✅ |

---

## 十三、项目基建（M0）

| 功能 | 描述 | 状态 |
|------|------|------|
| Cargo 工作区 | 四 crate 工作区（hc / hc-rt / hc-tools / hc-lsp） | ✅ |
| 零外部依赖 | 编译器核心零外部依赖（hc / hc-rt） | ✅ |
| CI | 每次 push/PR 运行完整示例套件回归 | ✅ |
| 示例套件 | 90 例（语法/惯用法/模式/并发/工具），全部附带 `[test]` | ✅ |
| 决策记录 | ADR 0001–0020 | ✅ |
| 设计文档 | SPEC 01–11 + phase1–4 + review | ✅ |

---

## 十四、双后端覆盖矩阵

| 后端 | 入口 | 覆盖范围 | 状态 |
|------|------|----------|------|
| tree-walking 解释器 | `hc run <file.hc>` | 全语言 | ✅ |
| IR 参考解释器 | `hc run --ir <file.hc>` | 全语言（含 G1–G5 标准库） | ✅ |
| 字节码 VM | `hc run <file.hbc>` | 全语言（复用 `run_ir`） | ✅ |
| LLVM 原生 | `hc build <file.hc>` | 已实现内建子集（compile mismatch = 16，全为设计内硬错误/IR 降级器限制/解释器 bug，非 LLVM 后端问题） | 🟡 |

四个后端共享同一语义源（`IrModule` + `run_ir`，ADR-0004），禁止后端私语义。

---

## 十五、测试与验证

| 指标 | 结果 | 时间 |
|------|------|------|
| `cargo test --workspace` | 900+ 项全绿（含新增 chan/mutex/scheduler 测试） | 2026-08-24 |
| 解释模式示例回归 | 147 通过 + 0 失败 + 1 跳过（全绿） | 2026-08-23 |
| 原生模式交叉验证 | 16 项 mismatch（11 defer-try-f 设计内硬错误 + 3 Vec 字面量构造 IR 限制 + 2 解释器 vs 原生行为差异） | 2026-08-26 |
| 一致性测试 tree-walking ↔ IR | 100+ 测试全绿 | 2026-08-23 |
| 通道测试（chan<T>） | 9 测试全绿（含 3 spawn+通道集成测试） | 2026-08-24 |
| 调度器单元测试 | 4 测试全绿（submit+complete/state transitions/multi/unknown） | 2026-08-24 |
| Mutex 测试 | 11 测试全绿（含 spawn 共享访问） | 2026-08-24 |
| LLVM 后端单元测试 | 60 测试全绿（含 C8-1 类型槽表） | 2026-08-26 |

---

## 十六、里程碑达成状态

| 节点 | 内容 | 状态 |
|------|------|------|
| T1（M0–M2） | 前端 + 语义完整 | ✅ |
| T2（M3） | 双后端可运行、双模式一致 | ✅ |
| T3（M4） | 语言系统完整 | ✅ |
| **T4（M5–M7）** | **第一部分最小功能集可用** | ✅ **2026-08-17** |
| T5（E1–E2） | 元编程 + 并发完整 | ✅ 已达成（元编程 E1 完整 + 并发 E2 完整：协程+通道+async/await+Mutex+@atomic+Send·Sync，四模式容器已弃用） |
| T6（E3–E5） | 标准库 + 工具链完整 | 🟡 部分（标准库 E3/A6 扩展 + 系统编程 E4 + 工具链 E5 大部分落地，C8 LLVM 原生内建 mismatch 归零 1.x） |
| T7（E7） | 自举闭环 | ⏳ 计划中（K1 lexer ✅，K2 parser 🔴，K3–K6 待实现） |
| T8（E6 + 冻结） | 1.0 冻结 | ⏳ |

---

## 十七、自举（E7）

| 功能 | 描述 | 状态 |
|------|------|------|
| K1 H 版 lexer | `stage1/lexer.hc` 用 H 重写词法分析，6621 token 零 diff | ✅ |
| K2 H 版 parser/AST | token → 声明树 + 对照测试（K2 性能已优化：解析自身约 1s，较原 60s+ 提升 ~60x，8 项语料对照通过） | 🟢 已优化 |
| K3 H 版语义 | 名称解析/类型检查/所有权/错误集 + 对照 | ✅ 已完成（11/11 任务：Checker 骨架/类型系统/符号表/收集阶段/未定义名称检测/类型解析+变量声明类型检查/表达式类型检查+二元运算符/if-while-for 语句类型检查/所有权分析含引用逃逸/错误集分析/集成验证；13 项对照测试全部通过，覆盖全部语义语料文件） |
| K4 H 版后端 | IR 参考解释器（跑 H 自身测试）+ 对照 | 🔴 |
| K5 自举闭环 stage2 | 用 H 编译 H；产物再编译产物 | 🔴 |
| K6 可复现构建 | Rust/H 双实现交叉验证全语法/语义/内存/并发 | 🔴 |
| M9 语言规范定稿 | 规范 ↔ 实现一致性测试套件 | 🔴 |
| M10 冻结与 1.0 | 语言冻结/规范定稿/包管理器正式版/stdlib 冻结 | 🔴 |

---

## 文档索引

| 文档 | 内容 |
|------|------|
| `docs/SPEC/phase1/` | 第一阶段存档：语言系统（M0–M4）已实现文档 |
| `docs/SPEC/phase2/` | 第二阶段存档：最小外围（M5–M7）已实现文档 |
| `docs/SPEC/phase3/` | 第三阶段工作集：标准库扩展 + 未实现功能 |
| `docs/SPEC/phase4/` | 第四阶段工作集：自举闭环 + 1.x 延迟项 |
| `docs/SPEC/README.md` | 1.0 实现计划总纲 |
| `docs/SPEC/phase1/07-bootstrap-plan.md` | 三块实现计划 + 实现状态表 |
| `docs/SPEC/phase1/02-milestones.md` | 阶段里程碑与验收标准（M0–M10） |
| `docs/SPEC/phase1/06-language-spec.md` | 语言规范总纲 |
| `docs/SPEC/phase3/01-unimplemented-features.md` | 未实现功能清单（第三阶段 backlog） |
| `docs/SPEC/phase3/02-syntax-rules.md` | 通用语法规则 |
| `docs/SPEC/phase3/11-lsp-implementation.md` | LSP 工具实施计划 |
| `docs/adr/0021-allocator-interface.md` | Zig 式可扩展分配器接口设计（22 子项全推荐） |
