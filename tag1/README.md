# H 语言工具链 · tag1（第一部分「最小功能集」）

> **状态**：第一部分（最小功能集）已实现 —— 第一块语言系统（M0–M4）+ 第二块最小外围（M5–M7），**不自举**。
> 实现状态详见 [`docs/SPEC/07-bootstrap-plan.md`](../docs/SPEC/07-bootstrap-plan.md) 第八节。

## 这是什么

H 是一门**以数据为中心**、同时支持**系统编程与脚本编程**的编程语言（源码后缀 `.hc`）。核心哲学：语言的职责是「**定义数据、修改数据、传输数据、保存数据**」。同一份源码既可编译为原生二进制，也可作为脚本解释执行，**两种模式语义一致**。

`tag1/` 是 H 语言的第一阶段**垂直切片实现** —— 用 Rust 实现「源码 → 解析 → 语义检查 → 双后端（解释执行 / 原生编译）」的最小闭环，对应 `07-bootstrap-plan.md` 的**第一部分最小功能集**（`hc build` / `hc run` / `hc test` 完整可用）。

## 仓库结构

```
tag1/
├── hc/        # 编译器前端：lexer / parser / AST / 诊断 / 语义检查 / IR / LLVM 发射
├── hc-rt/     # 运行时：值模型 + tree-walking 解释器 + 标准库内建
├── hc-tools/  # 工具链 CLI：hc run / hc test / hc build / hc check / hc errors
└── examples/  # tag1 演示用例（hello / 错误集 / struct / 02-packages 跨包）
```

| crate | 说明 |
|---|---|
| `hc` | 编译器前端：词法、语法、AST、诊断、语义检查、共享 IR（`ir.rs`）、字节码（`bytecode.rs`）、LLVM 发射（`llvm.rs`） |
| `hc-rt` | 运行时：`Value` 值模型、tree-walking 解释器（脚本模式）、语言内建与最小标准库 |
| `hc-tools` | CLI：`hc run`（脚本 / 字节码 VM / IR 参考解释器）、`hc test`（含 `--mode=compile` 原生交叉验证）、`hc build`（原生编译）、`hc check`、`hc errors` |

## 构建

依赖：

- **Rust**（`cargo`）—— 编译工具链本体
- **zig**（可选，仅原生编译模式需要）—— `hc build` / `hc test --mode=compile` 用 `zig cc` 链接发射的 LLVM IR；缺失时 `hc build` 回退字节码产物、`hc test --mode=compile` 报错不静默降级

```bash
cd tag1
cargo build                    # 构建全部 crate
cargo build --release          # Release 构建
cargo test --workspace         # 运行全部测试
```

## 快速开始

```hc
// hello.hc
fn main(io: Io) !void {
    io.print("hello, world\n");
}
```

```bash
# 脚本模式（tree-walking 解释器，全语言）
cargo run -p hc-tools -- run examples/hello.hc

# 原生编译（LLVM IR + zig cc，M3.1–Phase 6 子集）
cargo run -p hc-tools -- build examples/hello.hc
```

## CLI 命令

```
hc run <file.hc>           运行脚本模式（解释执行）
hc run <file.hbc>          运行字节码 VM（M3.2，装载 HBC2；M3.1–Phase 6 子集）
hc run --ir <file.hc>      用 IR 参考解释器运行（M3.1–Phase 6 子集）
hc test [--mode=interpret|compile] [file.hc|dir]
                          运行 `[test]` 测试函数（默认当前目录全部 .hc；--mode=compile 原生交叉验证）
hc check <file.hc>         仅检查（词法/语法/装载）
hc errors <file.hc>        输出错误码表（M2.6：错误名 ↔ 码 + 位置）
hc build <file.hc>         编译为原生可执行（LLVM IR + zig cc）
hc init <name>             创建新项目骨架（build.zon + main.hc，组 H1；约定见 06-13-project-structure.md）
hc doc [target] [--out <dir>]
                          生成 Markdown 文档（/// 注释 + 声明签名；target 默认当前目录包，
                          `std` = 标准库内置目录页；输出默认 <target 目录>/docs/api/，组 H4）
hc --version
hc --help
```

## 已实现功能（第一部分最小功能集）

按 `07-bootstrap-plan.md` 里程碑组织，**均已落地**（`✅`）：

| 里程碑 | 内容 |
|---|---|
| M0 地基 | cargo 三 crate 工作区（零外部依赖） |
| M1 前端 | 关键字/运算符/字符串/数字全集；Parser + AST；多错误诊断；跨文件模块（namespace / using / 兄弟文件符号登记、目录 = 包） |
| M2 语义 | 名称解析（重载池）；完整类型检查（表达式级 + 期望类型传播 + 字段/索引校验）；推断（泛型 T / 指针形态 / 多路径返回 / 重载歧义）；所有权（分配来源 + move 合法性 + 引用逃逸）；错误集（显式 / `!T` 推断收集 / anyerror + 错误码表）；函数（重载 / 可选参数 / 闭包 move 捕获） |
| M3 双后端 | 共享 IR（`ir.rs`，唯一语义源）；字节码 VM（HBC2）；LLVM 原生后端（`llvm.rs`，emit-.ll + zig cc）；双模式一致性套件（CI 硬门槛） |
| M4 内建 | 内存运行时（作用域 LIFO 销毁 + Arena）；错误/终止（错误码 + `@panic` + `ExitType`）；`@` 内建基础集（sizeOf/alignOf/offsetOf/typeOf/intCast/ptrCast/compileError/addWithOverflow 等）；序列化内建（to_bytes/from_bytes/to_json/from_json/box）；标量接口族（ICompare/INumber/IInt/IUint/IFloat + 运算符绑定）；迭代内建（IIterable 三态 + iter()）；Debug 悬垂标记 |
| M5 标准库最小 | mem（Allocator/Arena）；collections（Vec/String/Map/Deque）；序列化封装；io 最小（print / fs / net TCP / 程序环境）；时间/调试 |
| M6 测试 | `[test]` 测试标记；断言五件套；`[PASS]/[FAIL]/[SKIP]` + 汇总；失败非零退出码 |
| M7 工具链 | `hc build`（目录 = 包，多文件合并静态链接）/ `hc run` / `hc test`（含 `--mode=compile` 交叉验证）；build.zon 包基础（清单解析 + pub 边界 + 本地依赖装载） |

### 双后端

| 后端 | 入口 | 覆盖 |
|---|---|---|
| tree-walking 解释器 | `hc run <file.hc>`（默认） | **全语言** |
| IR 参考解释器 | `hc run --ir <file.hc>` | M3.1–Phase 6 子集（唯一语义源） |
| 字节码 VM | `hc run <file.hbc>`（HBC2） | M3.1–Phase 6 子集，复用 IR 语义 |
| LLVM 原生 | `hc build <file.hc>` | M3.1–Phase 6 子集（emit-.ll + zig cc） |

四个后端共享同一语义源（`IrModule` + `run_ir`，ADR-0004），禁止后端私语义 —— 这是「双模式一致」承诺的根基。

## 测试

`cargo test --workspace` 共 **623 项测试**（580 单元/集成 + 43 示例回归），全部通过。逐测试文件明细：

| crate | 测试文件 | 通过 |
|---|---|---|
| hc | `src` 单元测试（bytecode 往返 + llvm.rs 纯文本发射） | 42 |
| hc | `tests/bytecode.rs`（VM == 参考解释器一致性，opcode 0–46 往返 + 组 G4a 线程） | 31 |
| hc | `tests/frontend.rs`（lexer/parser/semantic） | 51 |
| hc | `tests/inferred_errors.rs`（`!T` 推断收集） | 6 |
| hc | `tests/thread_capture.rs`（组 G3：spawn 捕获规则 Q18 绑定/逃逸 + Q19 冻结窗口） | 9 |
| hc | `tests/ir.rs`（共享 IR，M3.1 + Phase 1 指针/Phase 2 聚合/Phase 3 switch+for + Phase 4 闭包方法重载 + Phase 5 全局 + Phase 6 defer/errdefer/带标签 + Phase 8 闭包捕获精确化 + 组 G4a 线程） | 100 |
| hc-rt | `tests/semantics.rs`（M2.2 类型检查） | 47 |
| hc-rt | `tests/errors.rs`（错误码/传播 + io.exit/ExitType） | 22 |
| hc-rt | `tests/consistency.rs`（M3.4 双模式一致，含 Phase 1–8 + 组 G 线程） | 69 |
| hc-rt | `tests/arena.rs`（arena.init typed 构造） | 13 |
| hc-rt | `tests/box.rs`（box 内建） | 6 |
| hc-rt | `tests/collection.rs`（collections） | 8 |
| hc-rt | `tests/leak.rs`（G5 Debug 泄漏检测） | 5 |
| hc-rt | `tests/import.rs`（A 入口/import 全量） | 11 |
| hc-rt | `tests/inference.rs`（类型推断） | 11 |
| hc-rt | `tests/interfaces.rs`（M2.1 接口三用途） | 10 |
| hc-rt | `tests/io.rs`（net/fs/环境 + F4 fs 余项 append/rename/remove/list_dir/read_int/write_int） | 9 |
| hc-rt | `tests/closures.rs`（闭包，含 Phase 8 捕获精确化） | 12 |
| hc-rt | `tests/deque.rs`（Deque） | 4 |
| hc-rt | `tests/iter.rs`（迭代内建） | 4 |
| hc-rt | `tests/serialize.rs`（序列化内建 + serialize 命名空间） | 7 |
| hc-rt | `tests/dep.rs`（M7.2 跨包/pub 边界） | 3 |
| hc-rt | `tests/scalar.rs`（标量接口族） | 2 |
| hc-rt | `tests/thread.rs`（组 G 线程生命周期：spawn/join/cancel/is_done/detach + Q8 每线程 alloc） | 7 |
| hc-rt | `tests/examples.rs`（43 示例回归） | 43 ✅ |
| hc-tools | `src` 单元测试（CLI/buildzon/merge_modules + IR 退出码 + H4 docgen） | 27 |
| hc-tools | `tests/cli.rs`（C1 包目录运行 + C3/C4 库形态 + F2 io.exit 端到端 + F4 io.stdin 管道读取 + H1 hc init 脚手架 + H2 pkg add 本地依赖与缺失/版本诊断 + H4 hc doc 目录/标准库/单文件页 + H5 格式回归） | 20 |
| hc-tools | `tests/harness.rs`（F3 测试基建自测：收集/退出码/注入/汇总） | 5 |
| hc-tools | `tests/native.rs`（M3.3 原生端到端，含 Phase 6 defer/errdefer/带标签 + 组 G 线程原生边界，zig 缺失自动 SKIP） | 39 |

补充：

- **示例回归**（CLI `hc test examples/`）：**133/142 通过 + 1 跳过**；9 项失败属第三块（第二部分）特性 —— E1 元编程-comptime（35 comptime_int/数组形态，34 已随组 D 类型函数转绿；script 33/36/81 已随组 B/C 转绿为真实生成）、E2 并发/异步（37/38/39/76–80），均非本阶段范围（63-template-render 已随 D1 `fmt_int` 落地转绿；23-tests 的 `skip_example` 自 F1 起实际触发 `error.SkipTest` → 统计为 SKIP，双后端一致；90-thread-lifecycle 为组 G 新增在列示例，5 测试全绿；91-orders-domain 为组 H3 `[module]` 领域模块约定示例，2 测试全绿——采用固定数组使 interpret + compile 双全绿；34-generics 为组 D `comptime` 类型函数（`fn Pair(T: type) type`），interp/IR/原生编译三模式全绿，compile 门禁 55→53）。
- **原生交叉验证**（`hc test --mode=compile examples/`）：编译模式 53 项 mismatch —— 均为未实现原生内建/方法/降级缺口（`error.NotBuiltin`/`error.NoMethod`/`error.Unsupported`/`error.NotCallable` 响亮运行时中止，原生 ABI 留后续阶段全标准库），按文件粒度正确标记（defer/errdefer/带标签、global/const、io.print/alloc.init/标量 @ 内建/用户类方法/math.* 等降级期失败点已于 Phase 6/7 消除；连续类值语义已于 P11d 经 `DeepCopy` 指令 + 运行时门落地——`13-struct`/`58-copy-semantics` 的连续复制 AssertFailed 修复；隐式环境全局原生播种已于 P11d 经 `emit_implicit_env_seed` 落地——`pi`/Vec/Deque/Table/io 族 30-interface 转 MATCH；`alloc.init`/`Type.new` 构造降级已于 P11d 落地——`31-class`/`46-recursion` 类树构造 + 原生 `Vec.append`（Arr 接收者内建集合方法 `hc_append`/`hc_append_u64`/`hc_extend`）转 MATCH；组 G 线程为原生子集边界——90-thread-lifecycle 因 spawn 需函数引用（FnRef）在原生下 `error.NotCallable` 中止，属 G4b 定案 A；组 D `comptime` 类型函数（34-generics）类型函数体降级跳过 + NamedLit 具体化名，原生编译转绿——55→53）。

CI（`.github/workflows/ci.yml`）在每次 push/PR 运行 `cargo test --workspace` 与完整示例套件回归（`tag1/scripts/check-examples.sh`，interpret ≥125 passed / ≤11 failed + compile ≤55 mismatch，低于基线即失败）。compile 基线 52→53 的 +1 为 D1 副作用：interpret 侧 `fmt_int` 落地使 63-template-render 转绿，但原生侧 `String.from/replace/find` 仍缺（预先存在的原生子集缺口，D3 注）——该例由「双失败」转为「interpret 绿 / 原生红」的 mismatch。53→54 的 +1 为 G1 副作用：`spawn(f, …)` 解析落地使 77-producer-consumer 由双解析失败转为 interpret 运行至 `error.UndefinedName`（四模式类型 `OneToOne` 未实现，第三块）而原生 LLVM 在 `spawn` 处 `error.Unsupported` 拒绝（G4b 前）——两后端均失败不变，仅失败模式改变计入 mismatch。54→55 的 +1 为 G5 副作用：新增在列示例 90-thread-lifecycle（组 G 线程，interpret 全绿 5 测试）在原生下于 `spawn` 处 `error.NotCallable` 中止——原生 ABI 无函数值表示（FnRef/CallIndirect/MakeClosure 属 Phase 8 原生 ABI 改造），G4b 定案 A 定为原生子集边界（三后端 interp/IR/字节码线程一致，原生响亮拒绝、不静默误编译），线程原生支持留 Phase 8。

## 已知取舍

- **原生/IR 后端为标量 + 指针 + 聚合 + switch/for + 闭包/函数引用/方法/重载 + global/const + defer/errdefer + 带标签 break/continue + 全核心标准库（IR）子集**：`hc build` / `hc test --mode=compile` 覆盖 M3.1 切片 + Phase 1 指针 + Phase 2 聚合 + Phase 3 switch/for（字段/索引/切片/数组/class/enum/元组解构/move/unwrap/switch 全模式/for 迭代含 mut 写回）+ Phase 4 闭包/函数引用/实例方法/重载 + Phase 5 global/const（声明序初始化 + 跨函数/跨测试可变全局 + `&global` 取址写穿）+ Phase 6 defer/errdefer（LIFO + 仅错误路径）+ 带标签 break/continue（跨层定位）+ Phase 7 全核心标准库（`run_ir` 全量；LLVM 原生仅已实现内建子集——io.print / `alloc.init` 无字段 / 标量 @ 内建 / min/max/sqrt/box/read_u64_le/copy / 用户类实例方法 + `Io.print` + math.*：nan/inf/inf_neg/sqrt/abs/pow/floor/ceil/round）+ Phase 8 闭包捕获精确化（自由变量精确分析含嵌套传递 + 非 mut 只读强制 + move 深拷贝）+ P11d 连续类值语义（`DeepCopy` 指令 + 运行时门：`[continuous]` 类 var 声明即深拷贝，非连续类/数组恒等别名，对齐 oracle `type_is_continuous`）；Table 多索引、defer 体控制流等子集外特性在 IR 降级时以 `error.Unsupported` 硬错误拒绝（**不静默丢弃**），`hc build` / `hc run --ir` 直接报错并提示改用 tree-walking 模式；未实现原生内建/方法在运行时以 `error.NotBuiltin`/`error.NoMethod` 响亮中止（原生 ABI 留后续阶段全标准库）。
- **LLVM 值盒全精度载荷**：`%Value = { i32, i128 }`（i128 修复 i64 截断；浮点位模式存低 64 位）；`hc build` 依赖外部 `zig cc`，无优化 pass，硬错误消息依赖 libc。
- **LLVM Mut/Move for 捕获 = copy-in/copy-out 写回**：迭代体内中读源容器在 LLVM 见旧值（`run_ir` 槽 cell == 源 cell 无此问题），接受近似。
- **原生交叉验证为文件粒度**：全绿 vs 有失败，非逐测试 PASS/FAIL 清单（断言失败在测试函数 ret 路径直接 abort）。
- **字节码 VM 复用 `run_ir`**：未做紧凑运行时 dispatch / 寄存器式 VM（性能优化留后续，须一致性套件证明等价）。
- **跨包静态链接（M7.2 后续）**：`build.zon` 的 `deps` 已支持本地依赖装载（解释/检查路径），但原生编译目前仅同目录包内合并，跨包链接归后续。
- **tree-walking 求值递归栈深**：`hc run` 与示例回归测试均在 64MB 栈线程中运行（Windows 主线程默认 1MB、测试线程默认栈更小，不足以承载深递归/大帧），非语义限制。

## 本阶段明确不实现（第三块 / 第二部分）

脚本生成（`script` 块元编程）、comptime 完整（类型即值）、并发/异步/线程、标准库扩展（UDP/HTTP/ipc/FFI 等）、系统编程（K1–K11）、工具链扩展（LSP/format/lint/注册中心）、**自举**（stage1 → stage2）—— 详见 `07-bootstrap-plan.md` 第四节。

## 文档索引

| 文档 | 内容 |
|---|---|
| [`docs/SPEC/README.md`](../docs/SPEC/README.md) | 1.0 实现计划总纲 |
| [`docs/SPEC/07-bootstrap-plan.md`](../docs/SPEC/07-bootstrap-plan.md) | 三块实现计划 + 实现状态表 |
| [`docs/SPEC/09-part2-execution.md`](../docs/SPEC/09-part2-execution.md) | 第二部分执行细表（A–H 全完成） |
| [`docs/SPEC/10-part3-execution.md`](../docs/SPEC/10-part3-execution.md) | 第三块执行细表（计划） |
| [`docs/SPEC/06-language-spec.md`](../docs/SPEC/06-language-spec.md) | 语言规范总纲 |
| [`CONTEXT.md`](../CONTEXT.md) | 术语表与项目背景 |
| [`examples/README.md`](../examples/README.md) | 示例套件说明 |
