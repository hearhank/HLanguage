# Rust 实现事实清单（截至 2026）

> 为 H 语言设计调研而整理。Rust 的事实高度稳定，本文件基于公开的官方文档与社区公认事实整理；如需精确引用，以 https://doc.rust-lang.org/reference 与 https://blog.rust-lang.org 为准。

## 1. 时间线与 1.0

- 2009 年由 Graydon Hoare 在 Mozilla 发起；2015-05-15 发布 **1.0**（从启动到 1.0 约 6 年）。
- 1.0 之后承诺向后兼容（semver 承诺），通过 **edition** 机制（2018/2021/2024）做非破坏性的演进。
- 现状：2026 年稳定版在 1.8x 系列，Rust 2024 edition 已稳定。

## 2. 编译器架构与自举

- rustc 流水线：lexer/parser → AST → HIR → MIR（中间表示，承载借用检查与大部分优化）→ LLVM 后端（另有 Cranelift 实验后端、gcc backend 实验项目）。
- 最初用 OCaml 编写（rustboot，2010 年前后），约 2011 年起自举（Rust 写 Rust）；引导方式为 stage0（旧版 rustc 编译新版）。
- 后端默认 LLVM。

## 3. 类型系统

- 静态强类型 + **局部类型推断**（绑定处推断：`let x = vec![1,2,3]`；函数签名仍需显式标注）。
- 核心抽象：trait（接口/泛型约束）、泛型、生命周期参数。
- 无空值（无 null），用 `Option<T>`。

## 4. 内存管理

- **所有权 + 借用 + 生命周期**：无 GC，编译器静态保证内存安全。
- 所有权语义：每个值有唯一所有者，移动（move）而非拷贝（copy）为默认，借用（&T）不可变 / （&mut T）互斥可变。
- 运行时可零成本，无运行时内存管理。

## 5. 错误处理

- `Result<T, E>` 枚举 + **`?` 运算符**传播错误；无异常机制。
- `panic!` 用于不可恢复错误（可配置为中止或 unwind）。

## 6. 并发

- OS 线程（`std::thread`）+ 消息传递（`std::sync::mpsc` 通道）。
- async/await（`std::future`，生态以 tokio 为主流运行时）。
- **Send/Sync** 静态标记保证跨线程安全性（编译期检查）。

## 7. 工具链

- **cargo**：构建 + 包管理 + 测试一体（Cargo.toml / crates.io）。
- rustup（工具链管理）、rustfmt（格式化）、clippy（lint）。

## 8. 与 H 设计的相关性要点

- 「1.0 后冻结 + edition 演进」模式可借鉴。
- 局部推断的取舍（推断仅限绑定处，签名显式）是 H「显式为主、推断为辅」的直接参照。
- Rust 证明了「无 GC 也能做安全系统编程」，但代价是学习曲线——这是 H 权衡 Zig 式手动与 TS 式 GC 时的参照。
