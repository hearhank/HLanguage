# H 语言示例（分类目录）

按**使用层次 + 语法子类**分类，全局连续编号 01–85（沿目录顺序递增）。每类一个文件夹。

## 目录结构

```
examples/
├── 01-syntax/          语法（45 + math.hc + build.zon）
│   ├── 01-basic/       词法基础（01–05）
│   ├── 02-types/       类型（06–19）
│   ├── 03-functions/   函数与错误（20–26）
│   ├── 04-memory/      所有权与内存（27–29）
│   ├── 05-oop/         面向对象（30–31）
│   ├── 06-data/        数据与集合（32）
│   ├── 07-meta/        元编程（33–36）
│   ├── 08-concurrency/ 并发语法（37–39）
│   └── 09-io-modules/  IO 与模块（40–44 + math.hc + build.zon）
├── 02-idioms/          惯用法（45–63）
├── 03-patterns/        设计模式（64–75）
├── 04-concurrency/     并发模式（76–80）
└── 05-tools/           实用工具（81–85）
```

## 01-syntax/01-basic 词法基础（01–05）

01-hello · 02-variables · 03-control-flow · 04-ranges · 05-format

## 01-syntax/02-types 类型（06–19）

06-integers · 07-floats · 08-bool-void · 09-arrays · 10-functions · 11-number-literals · 12-bitops · 13-struct · 14-enum · 15-pointers · 16-slices · 17-optional · 18-optional-chain · 19-nested-data

## 01-syntax/03-functions 函数与错误（20–26）

20-errors · 21-closures · 22-errdefer · 23-tests · 24-interface-errors · 25-error-context · 26-error-set-union

## 01-syntax/04-memory 所有权与内存（27–29）

27-ownership · 28-dangling · 29-globals

## 01-syntax/05-oop 面向对象（30–31）

30-interface · 31-class

## 01-syntax/06-data 数据与集合（32）

32-collections

## 01-syntax/07-meta 元编程（33–36）

33-script · 34-generics · 35-comptime-branch · 36-script-boilerplate

## 01-syntax/08-concurrency 并发语法（37–39）

37-concurrency · 38-async · 39-evented

## 01-syntax/09-io-modules IO 与模块（40–44）

40-io · 41-namespaces · 42-pricing · 43-orders · 44-multi-file-main · math.hc · build.zon

## 02-idioms 惯用法（45–63）

45-strings · 46-recursion · 47-sort-search · 48-iterator-chain · 49-arena-pool · 50-serialization · 51-collection-bytes · 52-string-deep · 53-map-deep · 54-nested-json · 55-time · 56-csv-parse · 57-protocol-parse · 58-copy-semantics · 59-pipeline · 60-binary-search · 61-json-walk · 62-custom-sort · 63-template-render

## 03-patterns 设计模式（64–75）

64-interface-poly · 65-composition · 66-builder-chain · 67-callbacks · 68-memoize · 69-config-load · 70-logger · 71-recursive-parser · 72-observer · 73-rate-limit · 74-state-machine-adv · 75-transaction

## 04-concurrency 并发模式（76–80）

76-threads-edge · 77-producer-consumer · 78-task-dispatch · 79-retry · 80-batch-async

## 05-tools 实用工具（81–85）

81-end-to-end · 82-directory · 83-wordcount · 84-rng · 85-grep-tool

> 示例性质：语法规格示例（设计文档形态），非可运行程序——编译器实现时逐文件作为语法验收测试。

## 测试

所有示例均附带 `test` 块（Q8/Q-T1~Q-T6 定案，2026-08-13）：

- **运行**：`hc test`（默认脚本模式；`hc test --mode=compile` 在编译模式交叉验证，Q-T5）
- **断言 API**（Q-T1）：`expect` / `expect_eq` / `expect_neq` / `expect_error` / `expect_eq_slices`（测试块内隐式可用）
- **输出统计**（Q-T2）：逐项 `[PASS]/[FAIL]/[SKIP] 文件::测试` + 汇总 `N passed, M failed, K skipped`；失败非零退出码
- **隔离/跳过**（Q-T3）：独立作用域、默认串行；`return error.SkipTest;` 标记跳过
- **环境**（Q-T4）：测试块内隐式 `test_io` + `alloc`
- **形态**（Q-T6）：S1 纯逻辑断言 / S2 main smoke / S3 局部逻辑 / S4 演示标注（依赖外部环境，断言留 1.x）
- **覆盖**：85 个示例 + math.hc 全部具备至少 1 个 test 块（23-tests 为断言 API 全家福示例）
