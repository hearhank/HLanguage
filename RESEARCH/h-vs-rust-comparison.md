# H vs Rust 功能比对（规划对照）

> **状态**：H 为**规划中的语言**（设计文档，无实现）——本表为设计阶段的对照参考；Rust 为已实现语言（2024 生态）。
> **H 设计裁定编号**见 `docs/review/2026-08-13-spec-examples-review.md`；H 语言规格见 `docs/SPEC/`。

## 1. 内存与所有权

| 功能 | Rust | H | 优缺点 |
|---|---|---|---|
| 所有权 | 编译期借用检查器（move 语义） | 作用域所有权 + **分配来源判定**（非 Arena 默认拥有）+ move = 销毁责任转移 | Rust：全静态零运行时成本，学习曲线陡；H：编译简单、Debug 诊断兜底、Release 裸——错误发现晚（靠测试） |
| 生命周期标注 | `'a` 泛型生命周期 | **无显式标注**（作用域 + 可选诊断，12.17） | Rust：精确但样板多；H：简单，复杂场景静态保证弱 |
| 析构 | `Drop` trait + RAII | `defer`/`errdefer` + 作用域自动销毁（无隐式析构） | Rust：自动清理优雅但顺序隐式；H：清理在创建处显式可见（「没有隐藏控制」），LIFO 可预测 |
| 内存分配 | 全局分配器 + allocator 生态 | **显式 Allocator 参数** + Arena + 默认分配器回退 | Rust：方便但分配隐藏；H：显式（无隐式分配）但样板多 |
| 共享所有权 | `Rc`/`Arc` + `RefCell`/`Mutex` 内部可变性 | **无引用计数**（Avoid）+ 四模式类型；共享数据走显式指针（用户负责）——**引用类型赋值 = 编译错误**（Q1'，2026-08-14，取代 Q3c 别名共享） | Rust：共享便利但运行时开销；H：单一所有权更纯粹，共享场景引导到四模式类型 |

## 2. 引用 / 借用

| 功能 | Rust | H | 优缺点 |
|---|---|---|---|
| 引用 | `&T`/`&mut T`（编译期借检，**唯一写者编译期强制**） | `*T`/`*mut T`（**多个读写指针自由**，指针问题用户负责——Zig 式，2026-08-13 修订） | Rust：静态保证强；H：更接近 Zig 裸指针哲学——简单自由，安全靠用户 + Debug 可选诊断 |
| reborrow | 自动（编译期借用转移） | 概念取消（无写者资格；指针自由，传参即指针传递） | Rust 有借用资格概念；H 无资格概念（更简单） |
| 悬垂防护 | 编译期拒绝 | **用户负责**（Zig 式）+ Debug 可选悬垂标记（诊断工具） | Rust 更安全；H 与 Zig 同路线（性能/简洁优先） |
| 裸指针/逃生舱 | `unsafe` 块 + `*const T`/`*mut T` | **`@ptrCast` 单点逃生舱**（无 unsafe 块） | Rust：unsafe 块粒度控制强；H：单点放弃更简单，但放弃面无块级隔离 |

## 3. 类型系统

| 功能 | Rust | H | 优缺点 |
|---|---|---|---|
| 类型定义 | `struct`/`enum` + `impl` 分离 | **`class` 统一 + `Continuous` 布局特性**（2026-08-14：struct/class 合并）+ 方法即函数成员（Zig 式无 impl）+ 元组/Table | Rust 分离清晰；H 就近声明更紧凑、单关键字更简单 |
| 枚举 | `enum` + match 模式匹配（嵌套/绑定/守卫） | enum 合一 + switch 穷举 + 捕获（简化） | Rust 模式匹配更强；H 简单、穷举保证一致 |
| trait/接口 | trait + impl for + **关联类型/默认方法/GAT** | interface + 冒号标注 + where 约束（三用途） | Rust 功能强得多；H 简单（方法契约）——1.x 参考关联类型 |
| 泛型 | 单态化 + trait bound + const 泛型 | comptime 式（类型即值 `fn List(T: type) type`）+ anytype + where | Rust 成熟（const 泛型）；H comptime 可计算更灵活但实现复杂 |
| 类型推断 | 局部推断（`let`） | **推断优先**（变量/字面量/泛型 T/指针/返回类型） | 都推断；H 更激进（返回类型也推断，TS 式） |
| 动态分发 | `dyn Trait` 胖指针 | `*Shape` 胖指针（自动收窄 + box） | 类似；H 显式 box 更透明 |

## 4. 错误处理

| 功能 | Rust | H | 优缺点 |
|---|---|---|---|
| 错误模型 | `Result<T, E>` + `?` | error union `E!T` + `try` | 类似；H 错误名全局唯一更轻 |
| 错误集检查 | 类型层面（`Result<T, MyErr>`） | **显式错误集 + return 未声明编译报错 + `!T` 推断** | H 更强：switch 可穷举错误、推断集自动收集 |
| panic | `panic!` + unwind/abort + hook + `catch_unwind` | `@panic` + abort（无 unwind） | Rust 可恢复（unwind）；H 无恢复——更简单、无隐藏控制流 |
| 表达式级处理 | `match Result`（无语法糖） | `catch` 表达式 + `if (e!) \|v\| else \|err\|` | H 语法更直接 |

## 5. 并发 / 异步

| 功能 | Rust | H | 优缺点 |
|---|---|---|---|
| 共享状态 | `Arc<Mutex<T>>`/`RwLock`/atomics 自由组合 | **四模式类型**（OneToOne…ManyToMany，写者数由类型保证）+ 通道 `send`/`recv` | Rust 原语自由、生态大；H 模式化更语义化——牺牲灵活换正确性 |
| async/await | `async fn` + Future + tokio 生态 | `async fn` + `Future(R)` + `Io.threaded/evented` | 类似；H **await 任何函数可用**（无 async 传染）；Rust 生态成熟 |
| Send/Sync | 编译期自动 trait 推导 | 静态并发安全标记（编译器内置，未细化） | Rust 推导强且久经考验；H 需在 M2/M5 细化 |
| 无锁结构 | std 原子 + crossbeam | `@atomic` 原语 + 五内存序 | 原语齐备；Rust 生态更成熟 |

## 6. 元编程

| 功能 | Rust | H | 优缺点 |
|---|---|---|---|
| 宏 | `macro_rules!` + 过程宏（derive/attribute） | **无宏**——脚本生成（`script{}` + types 元数据）+ comptime | Rust 宏强大但复杂难调试；H 用 H 脚本（受限子集）更可读可控 |
| 编译期计算 | `const fn`（受限）+ 过程宏 | comptime（类型即值）+ 脚本生成双轨 | H 更灵活（脚本可任意 IO/集合）；Rust const fn 受限但零依赖 |
| 派生 | `#[derive]` 自动实现 | 脚本生成样板（等价） | 功能等价；H 定制性更强 |

## 7. 模块 / 包

| 功能 | Rust | H | 优缺点 |
|---|---|---|---|
| 模块组织 | `mod` 文件树 + `use` 路径 | `namespace` 块 + `using`（C# 式，跨文件/一文件多组） | Rust 文件树严格一致；H 更灵活 |
| 包管理 | cargo + crates.io（最大生态） | 内置包管理器 + `build.zon`（**H 数据字面量**）+ 官方注册中心 | H 清单即数据（脚本生成器可读）；Rust 生态碾压但依赖清单非数据 |

## 8. 测试

| 功能 | Rust | H | 优缺点 |
|---|---|---|---|
| 单元测试 | `#[test] fn` + `assert!` 宏 + `cargo test` | `test fn` + **断言五件套**（expect_eq 带值输出）+ `hc test` | 类似；H 断言失败直接输出期望 vs 实际（Debug） |
| 测试环境 | 无内置 io 注入 | **`test_io` 隐式注入** | H 更强：IO 测试原生支持 |
| 双模式测试 | 无概念 | `hc test` 脚本模式 + `--mode=compile` 交叉验证 | H 独有（双模式承诺延伸到测试） |

## 9. 序列化

| 功能 | Rust | H | 优缺点 |
|---|---|---|---|
| 序列化 | **serde（第三方）** + derive | **内建分层**（2026-08-14 修订）：Continuous↔bytes（直映射）/class↔JSON/集合（含 Table）→字节 | H 一等公民（数据为中心）；Rust 靠生态（serde 成熟但第三方） |
| 类型元数据 | 无原生（serde 宏生成） | **`types` 元数据**（脚本生成输入） | H 原生支持——数据驱动生成的核心通道 |

## 10. 系统级设施

| 功能 | Rust | H | 优缺点 |
|---|---|---|---|
| 无 GC / 确定性销毁 | 有 | 有 | **一致** |
| 底层转换 | unsafe + 指针 | `@ptrCast`/`@intCast` | H 无 unsafe 块（单点逃生舱） |
| 原子 | `std::sync::atomic` | `@atomicLoad/Store/Rmw` | **一致** |
| 内存布局 | `#[repr(C)]`/packed/align | `packed class`（仅 Continuous）+ `align(N)`（2026-08-14 修订） | **一致** |
| 内联控制 | `#[inline]` | **无**（未定义） | H 缺——1.x 可补 |

## 11. 工具链

| 功能 | Rust | H | 优缺点 |
|---|---|---|---|
| 构建 | cargo build | `hc build`（静态链接默认、系统库自带） | H 零依赖跨编译（Zig 式）更强 |
| LSP | rust-analyzer（成熟） | hc LSP + **脚本实时预览** | H 有独特优势（编辑期脚本预览）；整体生态待建 |

## 12. Rust 有而 H 没有的功能（参考建议）

| Rust 功能 | H 现状 | 建议 |
|---|---|---|
| **unsafe 块**（局部放弃安全） | `@ptrCast` 单点逃生舱 | 可接受（更简单）；若需更大放弃面，1.x 加 unsafe 块 |
| **生命周期标注 `'a`** | 无（作用域 + 可选诊断） | 1.0 不引入（12.17 已定）；暴露痛点再演进 |
| **trait 关联类型/默认方法** | interface 仅方法签名 | 1.x 参考 |
| **模式匹配增强**（嵌套/绑定/守卫） | switch 穷举 + 捕获 | 1.x 扩展 |
| **宏**（声明/过程） | 脚本生成替代 | **保持无宏** |
| **Deref 自动解引用链** | 字段/索引自动解引用 | 部分已有；链式 Deref 1.x |
| **Drop trait 自动清理** | defer | 刻意不用（显式优先） |
| **迭代器惰性**（Iterator trait） | 立即求值迭代器 | 惰性留 1.x（与「无隐藏控制」张力） |
| **Result 组合子**（map/and_then） | try/catch 表达式 | 1.x 参考 |
| **const 泛型** | comptime 类型即值 | 已有等价（更灵活） |
| **`#[inline]` / `#[cold]`** | 无 | 1.x（性能控制） |
| **SIMD / asm!** | 无 | 1.x（系统级扩展） |
| **底层访问机制**（`union` / `read_volatile` / `from_exposed_addr`） | 无 | 规划中（2026-08-14 评估）：union/volatile/整数↔指针见系统编程缺口 K1/K2/K4（建议 1.0）；asm 1.x（K3） |
| **异步运行时生态**（tokio） | Io.threaded/evented 自研 | 生态待建（注册中心自托管后生长） |

## 总评

- **H 相对 Rust 的优势**：一等序列化（数据为中心）、双模式执行（单一语义源）、测试原生注入（test_io/双模式交叉）、comptime 泛型（类型即值可计算）、四模式类型（并发模式类型化）、错误集可穷举（!T 推断）、无宏的脚本生成、推断优先（更低样板）、指针自由（Zig 式简洁）
- **Rust 相对 H 的优势**：编译期全静态安全（借检/生命周期/Send-Sync）、模式匹配与 trait 生态（关联类型/GAT/默认方法）、unsafe 块粒度控制、unwind 恢复、宏生态、**成熟工具链与最大库生态**、迭代器惰性与组合子

**本质差异**：Rust =「编译期保证一切安全，代价是复杂度与学习曲线」；H =「显式一切 + Debug 诊断 + 测试兜底 + 指针自由（Zig 式）」，代价是运行时可选簿记、Release 裸路径靠用户纪律。这是两种安全哲学（静态 vs 显式+检测）的取舍，与 H 的双模式定位（脚本 + 系统）一致。
