# H — 以数据生命周期为第一原则的语言

编程语言的核心是数据的生命周期：**定义 → 写 → 读 → 传输 → 存储**。H 让这五件事成为语言的基石——先表示数据，再谈行为。

> 一门以函数为核心、静态编译为主、可解释执行的系统编程语言。单一源码、双后端（编译器/解释器）、一切数据结构皆可降为字节数组、手动内存管理（显式 allocator）、无隐藏控制流。

## 快速开始

无需安装——Node.js 直接运行：

```bash
node src/h.js run examples/demo.hc            # 解释器执行
node src/h.js run examples/concurrency.hc --threads 4   # M:N 多线程并行
node src/h.js build examples/tree.hc --exec   # 编译为原生二进制（zig cc/gcc）并运行
node src/h.js check examples/wrong.hc         # 静态检查（R1-R11）
node tests/smoke.js                          # 34 项回归测试
```

## 语言核心（60 秒）

- **两种内存形状**：`struct` 块（连续、复制语义、memcpy 字节化）+ `class` 树（非连续、带生命周期、序列化压平为字节、可逆恢复）
- **一切皆字节**：任何数据（含函数）自动获得 `to_bytes`/`from_bytes` 及派生操作；字节格式可逆、自描述
- **作用域生命周期**：单一所有权 + 显式 `move`；作用域退出销毁；可写指针强制双向引用、不跨执行体；显式 allocator
- **函数契约**：`x: T`（块复制 / 树只读指针）、`ref T`（可写指针）、`move T`（所有权转移）；返回 `error`/`move`/`ref`/值
- **class 组合**：无继承——`import` 提升方法（深度传递、循环拒绝、可隐藏 `hide`/`alias`）；接口纯静态、声明处绑定
- **枚举 + 穷尽 match**：缺变体在编译期拒绝（R10）
- **并发**：执行体统一线程/协程；`spawn`/`yield`/`Channel(n)`；**M:N** 多线程（主线程 Channel 路由，引用不跨执行体）
- **双后端**：`h run`（解释器）与 `h build`（编译器）共享语义——同一源码两路执行，输出逐行一致

## 目录结构

```
├── CONTEXT.md            术语表（项目词汇的权威定义）
├── SPEC.md               总纲 + 模块索引
├── docs/spec/01~06       规格：数据模型 / 字节化 / 生命周期 / 并发 / 行为 / 工具链
├── docs/adr/0001~0006    难逆转决策记录
├── prototype/            五个可双击试玩的交互原型（历史验证，可丢弃）
├── src/                  运行时 + 编译器
│   ├── lexer.js          词法
│   ├── parser.js         语法（AST + 位置）
│   ├── checker.js        静态检查 R1-R11
│   ├── evaluator.js      求值器（生命周期/move/权限/error + 协作调度 + Channel）
│   ├── cgen.js           C 代码生成（原生二进制，zig cc/gcc 编译）
│   ├── jsgen.js          JS 代码生成（无 C 编译器时的回退）
│   ├── parallel.js       主线程协调器（M:N Channel 路由）
│   ├── worker_host.js    worker 执行体宿主
│   └── h.js              CLI
├── examples/             十个示例程序（tree/ref/ref_param 为 class 生命周期与引用的双后端一致性验证）
└── tests/smoke.js        34 项断言回归
```

## 静态规则（R1-R11）

| 规则 | 含义 |
|---|---|
| R1 | 块只含块（无引用、无树字段） |
| R2 | class 字段分型（ref 必须指树、值字段必须块） |
| R3 | 写指针需可写源 |
| R4 | 只读不能写 |
| R5 | 类型/字段存在性 |
| R6 | 接口实现（含导入继承的接口） |
| R7 | 变量已定义 |
| R8 | 全局必须声明访问模式（`Exclusive<T>` 等） |
| R9 | move 后变量失效 |
| R10 | match 穷尽性（缺变体/未知变体/非枚举目标） |
| R11 | 导入冲突强制处理、导入循环拒绝 |

## 文档索引

- 设计总纲：[SPEC.md](SPEC.md) · 术语：[CONTEXT.md](CONTEXT.md)
- 规格：`docs/spec/01-data.md` ~ `07-comparison.md`
- 决策：`docs/adr/0001-*.md` ~ `0006-*.md`

## 状态

- ✅ 设计树完整（每支走到叶子）· 5 个原型验证 · 全功能运行时 · M:N 并行 · **C 原生编译（zig cc/gcc）** · 双后端一致性（块/enum/match/数组/class 树/ref 字段通知/ref·move 参数/error/并发 M:N 跨平台/字节化+版本字段+类型注册表）
- ⏳ 未启动：真竞争检测；POSIX 并发运行时仅交叉编译验证（运行验证待 Linux/macOS 环境）
- 已知取舍：多线程模式 print 输出顺序不保证（单线程默认确定）；Windows 直接运行 exe 中文输出需 UTF-8 代码页（`chcp 65001`）

## 设计旅程

从一次设计拷问开始：数据生命周期 → 块/树根基 → 所有权与双向引用通知 → 并发模型 → 五个原型逐个验证并抓到真实缺口 → 运行时/编译器/并行落地。完整过程记录在 `docs/` 与 `prototype/`。
