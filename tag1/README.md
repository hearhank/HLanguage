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

# 原生编译（LLVM IR + zig cc；原生后端未全标准库，边界见 ADR-0004）
cargo run -p hc-tools -- build examples/hello.hc
```

## CLI 命令

```
hc run <file.hc>           运行脚本模式（解释执行）
hc run <file.hbc>          运行字节码 VM（M3.2，装载 HBC2；全语言，同 IR）
hc run --ir <file.hc>      用 IR 参考解释器运行（全语言，interp == IR）
hc test [--mode=interpret|compile] [file.hc|dir]
                          运行 `[test]` 测试函数（默认当前目录全部 .hc；--mode=compile 原生交叉验证）
hc check <file.hc>         仅检查（词法/语法/装载）
hc fmt <file.hc|dir> [--check]
                          格式化（token 级重排，AST 保真；默认原地写回，--check 幂等门——
                          将改动则 exit 1，组 I1）
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
| IR 参考解释器 | `hc run --ir <file.hc>` | **全语言**（含 G1-G5 标准库；唯一语义源） |
| 字节码 VM | `hc run <file.hbc>`（HBC2） | **全语言**（同 IR，复用 `run_ir`） |
| LLVM 原生 | `hc build <file.hc>` | 未全标准库（`compile mismatch ≤ 60` 边界，ADR-0004） |

四个后端共享同一语义源（`IrModule` + `run_ir`，ADR-0004），禁止后端私语义 —— 这是「双模式一致」承诺的根基。

## 测试

`cargo test --workspace` 共 **792 项测试**（749 单元/集成 + 43 示例回归），全部通过。逐测试文件明细：

| crate | 测试文件 | 通过 |
|---|---|---|
| hc | `src` 单元测试（bytecode 往返 + llvm.rs 纯文本发射 + 组 H 导出 thunk 发射） | 46 |
| hc | `tests/bytecode.rs`（VM == 参考解释器一致性，opcode 0–46 往返 + 组 G4a 线程 + 组 H exported 标志） | 32 |
| hc | `tests/frontend.rs`（lexer/parser/semantic，含组 H export fn/union/volatile/ptr 解析） | 65 |
| hc | `tests/inferred_errors.rs`（`!T` 推断收集） | 6 |
| hc | `tests/thread_capture.rs`（组 G3：spawn 捕获规则 Q18 绑定/逃逸 + Q19 冻结窗口） | 9 |
| hc | `tests/ir.rs`（共享 IR，M3.1 + Phase 1 指针/Phase 2 聚合/Phase 3 switch+for + Phase 4 闭包方法重载 + Phase 5 全局 + Phase 6 defer/errdefer/带标签 + Phase 8 闭包捕获精确化 + 组 G4a 线程） | 100 |
| hc | `tests/comptime.rs`（组 D comptime：类型函数/值函数/折叠引擎与语义单测） | 32 |
| hc | `tests/async_future.rs`（组 E：async fn/await 解析与语义） | 8 |
| hc-rt | `tests/semantics.rs`（M2.2 类型检查） | 47 |
| hc-rt | `tests/errors.rs`（错误码/传播 + io.exit/ExitType） | 22 |
| hc-rt | `tests/consistency.rs`（M3.4 双模式一致，含 Phase 1–8 + 组 G 线程 + 组 D 值函数 + 组 E async + 组 G 标准库 G1-G5 + 组 H union/volatile/ptr/export + 组 F 四模式/原子） | 104 |
| hc-rt | `tests/arena.rs`（arena.init typed 构造） | 13 |
| hc-rt | `tests/box.rs`（box 内建） | 6 |
| hc-rt | `tests/collection.rs`（collections） | 8 |
| hc-rt | `tests/leak.rs`（G5 Debug 泄漏检测） | 5 |
| hc-rt | `tests/import.rs`（A 入口/import 全量） | 11 |
| hc-rt | `tests/inference.rs`（类型推断） | 11 |
| hc-rt | `tests/interfaces.rs`（M2.1 接口三用途） | 10 |
| hc-rt | `tests/io.rs`（net/fs/环境 + F4 fs 余项 append/rename/remove/list_dir/read_int/write_int + G2 io 差异项 open_dir/Dir/DirEntry/to_upper_lower/stdout-stderr） | 13 |
| hc-rt | `tests/closures.rs`（闭包，含 Phase 8 捕获精确化） | 12 |
| hc-rt | `tests/deque.rs`（Deque） | 4 |
| hc-rt | `tests/iter.rs`（迭代内建） | 4 |
| hc-rt | `tests/serialize.rs`（序列化内建 + serialize 命名空间） | 7 |
| hc-rt | `tests/dep.rs`（M7.2 跨包/pub 边界） | 3 |
| hc-rt | `tests/scalar.rs`（标量接口族） | 2 |
| hc-rt | `tests/thread.rs`（组 G 线程生命周期：spawn/join/cancel/is_done/detach + Q8 每线程 alloc） | 7 |
| hc-rt | `tests/async.rs`（组 E：E2 await≡join 惰性 Future + 协作式取消 + 幂等缓存；E3 Io.threaded/evented 事件循环 + poll 排空） | 11 |
| hc-rt | `tests/net.rs`（组 G G1 net：UDP bind/send_to/recv_from/local_port/close + 双语、HTTP 客户端 get + 服务端 listen/accept、TCP 命名空间双语 Q20） | 6 |
| hc-rt | `tests/ipc.rs`（组 G G3 ipc：匿名管道 pipe + 命名共享内存 shm——写/读排空/累积/关闭语义/线程生产者 + 定长截断） | 6 |
| hc-rt | `tests/storage.rs`（组 G G4 storage/archive：文件持久化键值存储 open/put/get/contains/remove/len/close + RLE 压缩 compress/decompress） | 7 |
| hc-rt | `tests/text_rng.rs`（组 G G5 text/time/rng：io.text 正则 matches/find/replace/split + io.time tick/elapsed + io.rng xorshift64* seed/next/int/float） | 10 |
| hc-rt | `tests/examples.rs`（43 示例回归） | 43 ✅ |
| hc-tools | `src` 单元测试（CLI/buildzon/merge_modules + IR 退出码 + H4 docgen） | 27 |
| hc-tools | `tests/cli.rs`（C1 包目录运行 + C3/C4 库形态 + F2 io.exit 端到端 + F4 io.stdin 管道读取 + H1 hc init 脚手架 + H2 pkg add 本地依赖与缺失/版本诊断 + H4 hc doc 目录/标准库/单文件页 + H5 格式回归） | 20 |
| hc-tools | `tests/harness.rs`（F3 测试基建自测：收集/退出码/注入/汇总） | 5 |
| hc-tools | `tests/native.rs`（M3.3 原生端到端，含 Phase 6 defer/errdefer/带标签 + 组 G 线程原生边界，zig 缺失自动 SKIP） | 39 |
| hc-tools | `tests/scriptgen.rs`（组 B/C：script 生成 + validate/to_json 定制通道） | 11 |
| hc-tools | `tests/comptime.rs`（组 D 端到端：comptime 块/值函数三模式 + expect_eq 断言） | 20 |
| hc-tools | `tests/fmt.rs`（组 I1 hc fmt：代表性排版形态幂等 + 空块/仅注释块回归 + 示例语料一次格式化收敛） | 4 |

补充：

- **示例回归**（CLI `hc test examples/`）：**147/148 通过 + 1 跳过**（0 失败）；**组 F 已落地（2026-08-18，ADR-0011 逆转）**——四模式容器 37 `four_mode_shared_container` + 76 `four_mode_types` + 77 `producer_consumer` + 78 `task_dispatch` 全部由失败转全绿（此前 4 项失败即此四例：37/76/77 四模式类型未实现、78 运行时 `error.UndefinedName`，组 F 延迟 1.x；79 已随捕获语法解析落地转全绿），非本阶段范围（63-template-render 已随 D1 `fmt_int` 落地转绿；23-tests 的 `skip_example` 自 F1 起实际触发 `error.SkipTest` → 统计为 SKIP，双后端一致；90-thread-lifecycle 为组 G 新增在列示例，5 测试全绿；91-orders-domain 为组 H3 `[module]` 领域模块约定示例，2 测试全绿——采用固定数组使 interpret + compile 双全绿；34-generics 为组 D `comptime` 类型函数（`fn Pair(T: type) type`），interp/IR/原生编译三模式全绿，compile 门禁 55→53；35-comptime-branch 为组 D D3/D4 最小切片——comptime_int 值参数 + 数组类型函数（`fn ArrayLen(T: type, n: comptime_int) type { return [n]T; }`），interp/IR/原生编译三模式全绿；组 D D2 comptime 块（`comptime { }`）最小切片已落地——装载期受限 Interp 求值、结果丢弃、失败 = 编译错误，无示例用 comptime 块故门禁基线不变；组 D D3 嵌套/递归实例化已落地——`map_type_apps` 深度遍历 + 登记期 `instantiating` 守卫，`PairPair(i32)`/`LinkedList(T)` 自引用可用，无示例用故门禁基线不变；组 D D4 comptime_int 常量折叠最小切片已落地——`comptime_int` 类型名识别（`ty_of` → `Int { width: Comptime }`）+ comptime 块语义检查（收窄溢出/类型不匹配在收窄点诊断，`expect_eq` 断言折叠），comptime 单测 +7（hc 语义 2 + hc-tools 端到端 5）全绿；组 D D4 comptime_float 惰性宽度已落地——`comptime_float` 类型名识别（`ty_of` → `Float`）+ 浮点折叠 + `expect_eq` 断言，comptime 单测 +5（hc 语义 2 + hc-tools 端到端 3）全绿；组 D D4b anytype 完整语义已落地——`anytype` 参数调用点按实参具体类型实例化（`has_anytype` 判定 + semantic `match_overloads` 具体化分支：返回 `anytype` 解析为体 return 表达式在具体绑定下的类型，`(qname, 具体化键)` 惰性缓存），`max_value(2.5, 1.5)` = `f64`（误配 String → `cannot assign` 编译错误）、`max_value(3, 7)` = 惰性宽度整数，comptime 单测 +6（hc 语义 3 + hc-tools 端到端 2 + consistency 1）全绿；组 D D4c comptime 值函数已落地——参数含 `T: type`、非返回 `type` 的普通函数调用点编译期求值（`is_comptime_value_fn`/`expr_to_type` 判定 + interp `try_comptime_value_call` 折叠：`T: type` 实参收已知类型表达式、值实参常量求值，自递归深度守卫 `ComptimeRecursion`），`array_len(i32)` = 4、`byte_size(f64, 7)` = 8 在 comptime 块与运行时 interp 折叠，comptime 单测 +7（hc 单测 2 + hc-tools 端到端 5）全绿；组 D D5 三后端类型值表示 + 一致性已落地——comptime 值函数运行时调用点 IR 折叠（`collect_value_fns` 收集 + `LowerCtx.value_fns` + `try_fold_comptime_value_call` 常量求值 `eval_const_block` 顺序执行含 if 分支，`is_known_type_name` 对齐 interp），类型值仅编译期存在、IR/原生无类型值/调用残留，原生经共享 IR 继承折叠，D4c 运行时测试扩展 interp + IR 双模式 + consistency +1（`d5_comptime_value_fn_consistent`），组 D 完结；均无示例用 comptime 块故门禁基线不变；组 E 异步已落地——E1 async fn/await 解析与语义（`Future(R)` 类型 + await 解包，含错误联合 `Future(!R)`）、E2 await ≡ join()（async fn 调用点返回惰性 `Future` 值、体延迟到 await、协作式取消 `error.Cancelled`、幂等缓存）、E3 `Io.threaded()`/`Io.evented()` 单线程事件循环（`runtime` 字段 + `io.poll()` 排空根回收队列驱动未 join 线程，threaded 恒 0）、E4 一致性 + 示例转绿（示例 37/38/39/76/80 的 `[test]` 异步断言双后端全绿，consistency `e2_async_await_consistent` + `e4_async_pointer_capture_consistent`；hc-rt async.rs 直测 11——E2 7 + E3 4）。组 G G1 net 已落地——UDP（`io.net.udp.bind(port)`/`bind(host, port)` + `send_to`/`recv_from`/`local_port`/`close`，Q20 双语命名空间形式，空队列 200ms 读超时 → `error.TimedOut`，recv_from 返回 2 元素数组 `[addr, data]`）+ HTTP 客户端 `io.net.get(url)`（仅 `http://`，非 200 → `error.Http{code}`，体按 Content-Length 截取）+ HTTP 服务端（`io.net.listen`+`accept`+`read_all`/`write` 应用协议层）+ Q20 双语补齐（`io.net.read_all(&conn, alloc)`/`write`/`shutdown`/`close`/`local_port`/`accept(&server)` ≡ 实例方法）；hc-rt net.rs 直测 6（UDP 3 + HTTP 2 + TCP 双语 1）全绿，门禁基线不变（无示例用 UDP/HTTP，38/80 主函数仍红——38 旧 URL 形式 + `JsonValue` 未实现、80 `https://` 网络不可达，见原生交叉验证）。组 G G2 io 差异项补全已落地——`io.stdout`/`io.stderr` 独立字节流（Stdout/Stderr 类值，`write_all(data)` 写真实句柄返回 void）、`String.to_upper`/`to_lower`（ASCII 大小写转换，非 ASCII 字节不变）、`io.fs.list_dir` 改为返回 `Vec(DirEntry)`（每条 `{name, is_dir}`，路径形态 `list_dir(path)` 与句柄形态 `list_dir(&dir, alloc)` 双支持）、`io.fs.open_dir(path) !Dir`（读校验 → fd→路径注册表，`dir.list_dir(alloc)` 重开枚举 / `dir.close()` 注销）；hc-rt io.rs 直测 9→13 全绿，门禁基线不变（示例 82-directory / 85-grep-tool 主函数此前按 G2 目标形态书写、open_dir 未实现时仅测试占位绿，现可实际运行）。组 G G3 ipc 已落地——`io.ipc.pipe() ![PipeReader, PipeWriter]`（匿名管道，2 元素数组同 UDP recv_from 约定）：写端 `write(data)`/`close()`，读端 `read(alloc)`（排空可读字节，空且写端开 → 空切片，不阻塞——协作式模型）/`read_all(alloc)`/`is_closed()`/`close()`（close 幂等）；`io.ipc.shm(name, size) !Shm`（命名共享内存定长字节区）：`write(data)` 覆盖截断到 size / `read(alloc)` / `close()`。真实 OS 进程/共享内存依赖 FFI 与进程模块 → 1.x；Interp 全局注册表 + 协作式 spawn 传 Pipe 值跨 H 线程传数据；hc-rt ipc.rs 直测 6 全绿，门禁基线不变。组 G G4 storage/archive 已落地——`io.storage.open(path) !KvStore`（文件持久化键值存储：`put(key, value)` / `get(key) !?&[u8]` 缺失 → null / `contains(key)` / `remove(key)` 幂等 / `len()` / `close()` 落盘+注销幂等；二进制格式 u32 键长+键+u32 值长+值；数据库连接抽象依赖真实 DB 驱动 → 1.x）+ `io.archive.compress(data)` / `decompress(data)`（RLE：token 0x00 字面跑 / 0x01 重复跑；重复输入变短、任意字节 round-trip、非法 → error.InvalidFormat；通用压缩算法 gzip/zip 留 1.x）；hc-rt storage.rs 直测 7 全绿，门禁基线不变。组 G G5 text/time/rng 已落地——`io.text.matches(pattern, text)` / `find` / `replace` / `split`（正则子集：字面量 / `.` / `[...]` 范围与取反 / `\d` `\w` `\s` / 分组 / `*` `+` `?` `{n,m}` / `|` / `^` `$` 锚定 / `\n` `\t` `\r` `\xNN` 及转义元字符；非法模式 → `error.InvalidFormat`；`ends` 记忆化集合回溯保证无灾难性回溯，`find_at` 左起 + 每处最长）+ `io.time.tick()`（纳秒计数，epoch 基准）/ `io.time.elapsed(tick)`（自 tick 起毫秒）+ `io.rng.seed` / `next`（xorshift64* 原始 64 位，seed(1) → `0xbafacf624f01c45d`…）/ `int(n)`（拒绝采样免模偏差）/ `float()`（高 53 位）；命名空间类名 `RngNs` 避开示例 84-rng 用户类 `Rng`（内建先于用户方法分派，同名会被拦截）；hc-rt text_rng.rs 直测 10 全绿，门禁基线不变（无示例用 text/time/rng；84-rng 本阶段已全绿，rng_range 直测亦过）。组 H 系统编程四特性已落地（ADR-0014，K1/K2/K4/K5，2026-08-18）——H1 无标签 union：字段内存重叠、无判别标签（C 头 union 对接/寄存器多视图内核场景），union 声明/字段索引/赋值/拷贝三后端（interp/IR/字节码/原生）一致；H2 `@volatileLoad(p)`/`@volatileStore(p, v)`：机制级读写穿，原生发射 LLVM `load volatile`/`store volatile` 防优化掉（MMIO 场景），interp/IR 按普通内存访问语义对齐；H3 `@ptrFromInt(addr) *mut Unknown`/`@intFromPtr(p) usize`：整数 ↔ 指针转换——interp/IR 地址注册表 + 匿名槽（未登记地址同地址幂等重建）、原生 i128 载荷 tag 交换（真实地址往返安全，任意物理地址 deref 为未定义行为）；H4 `export fn`：原生符号级导出——LLVM 外部 thunk `define %Value @"name"(...)` 内部 `call` 带前缀别名 `"{prefix}hc_fn{idx}"` 转发，模块末尾 `; exports: a, b` 清单注释、导出 `_start` 追加 `; entry: _start`（链接脚本入口钩子标记）；与 `pub` 正交（语言可见性 vs 符号导出）、仅作用于 fn/async fn（其余声明 → 解析错误），interp/IR/字节码按普通函数调用运行时透明，库形态下 thunk 保留经 `runtime_to_declares` 变换（`llvm-nm` 可见 `T add` 干净符号）；hc 单测 42→46、frontend 51→65（H1–H4 解析/语义 + export 非 fn 拒绝）、bytecode 31→32（exported 标志往返）、consistency 90→103（四特性双后端一致，含 `agg_export_fn_transparent_at_runtime`）；无示例用 union/volatile/ptr/export，门禁基线不变（compile 60 已达上限，本组不加示例）。组 F 四模式 + @atomic 已落地（2026-08-18，ADR-0011 逆转——用户指令「完成并发和异步」）——四模式容器（OneToOne/OneToMany/ManyToOne/ManyToMany）运行时 = `Value::Class`/`IrValue::Class` + 类名分派（fields `queue`/`closed`/`alloc`/`cap`），方法 `init`/`write`/`read`/`try_read`/`close`/`send`/`recv`（协作式透明：write 队尾追加、read 空 `error.Empty`、try_read 空 → null、close 后 write `error.Closed`、send 满 `error.ChannelFull`）；`@atomicLoad/Store/Rmw` 透明实现（load = deref、store = 写穿、Rmw add/sub/exchange 返回旧值，内存序求值后丢弃）；interp（语义 oracle）== IR（共享语义源）+ 字节码（decode + run_ir 自动继承），原生 LLVM 为子集边界（四模式容器 `error.Unsupported` 响亮拒绝）；semantic `is_builtin_type` 四模式名 + `call_at_builtin` 原子类型；consistency 103→104（`f_four_mode_and_atomic_consistent` 5 子测试）、示例 37/76/77/78 由失败转全绿——interpret 143/4/1 → **147/0/1**；compile 60→**57 mismatch**（77/78 级联计数消化，37/76 仍因四模式容器原生 LLVM 子集边界 `error.Unsupported` 计入文件级 MISMATCH）；真 OS 并行与 `mutex` 仍 1.x。组 I I1 `hc fmt` 已落地（2026-08-18）——token 级重排（缩进/换行/空格规范化，**AST 保真**：格式化前后 token 序列签名一致自检，不一致即报错拒绝写回）、注释三类保留（独立/行内/行尾，行尾对齐空白原样保留）、垂直布局保留（多行数组 / 垂直实参式多行调用 / 多行 struct 字面量 / 方法链跨行延续）、空块 `{}` 行内而仅含注释的块不折叠（幂等修复）、`--check` 幂等门（将改动则 exit 1，CI 用）；已应用到全部 examples/ 与 tag1/examples/（一次格式化后 `fmt --check` exit 0 收敛）；fmt.rs 直测 4 全绿；`cargo test --workspace` 792→**796**；门禁基线不变（interpret 147/0/1、compile 57 mismatch）。
- **原生交叉验证**（`hc test --mode=compile examples/`）：编译模式 57 项 mismatch —— 均为未实现原生内建/方法/降级缺口（`error.NotBuiltin`/`error.NoMethod`/`error.Unsupported`/`error.NotCallable` 响亮运行时中止，原生 ABI 留后续阶段全标准库），按文件粒度正确标记（defer/errdefer/带标签、global/const、io.print/alloc.init/标量 @ 内建/用户类方法/math.* 等降级期失败点已于 Phase 6/7 消除；连续类值语义已于 P11d 经 `DeepCopy` 指令 + 运行时门落地——`13-struct`/`58-copy-semantics` 的连续复制 AssertFailed 修复；隐式环境全局原生播种已于 P11d 经 `emit_implicit_env_seed` 落地——`pi`/Vec/Deque/Table/io 族 30-interface 转 MATCH；`alloc.init`/`Type.new` 构造降级已于 P11d 落地——`31-class`/`46-recursion` 类树构造 + 原生 `Vec.append`（Arr 接收者内建集合方法 `hc_append`/`hc_append_u64`/`hc_extend`）转 MATCH；组 G 线程为原生子集边界——90-thread-lifecycle 因 spawn 需函数引用（FnRef）在原生下 `error.NotCallable` 中止，属 G4b 定案 A；组 D `comptime` 类型函数（34-generics）类型函数体降级跳过 + NamedLit 具体化名，原生编译转绿——55→53；53→58 为组 E E1 副作用：async/await 解析落地使含 `async fn`/`await` 的 5 例（37/38/39/76/80）由双后端解析失败转为 interpret 绿 + 原生红——原生/IR 后端尚无 Future/async 与四模式容器（`ManyToMany` 等），`error.Unsupported` 响亮中止。组 E E2-E4 后该 5 例的 `[test]` 异步断言已在双后端全绿（IR 侧 async fn 调用同步执行 + await 透传，子集边界对齐纯函数结果）；文件级 MISMATCH 剩余 58 中这 5 例来自 `main` 函数特性而非 async——四模式容器（37/76——组 F 已落地（2026-08-18 逆转），但原生 LLVM 仍为子集边界，四模式容器 `error.Unsupported` 响亮拒绝，不计入 interpret）、**38/80（G1 net 已落地仍红：38 主函数旧 URL 形式 `connect(url)`/`read_all(&conn)` + `JsonValue` 类型未实现、80 主函数 `https://` 网络不可达——仅 `http://` 支持，均非 G1 范围）**、`Io.evented` 原生构造器（39，interp-only E3）——回落依赖原生 ABI 扩展而非 E/F 组）。58→60 的 +2 为捕获语法解析副作用：78-task-dispatch / 79-retry 由双解析失败转为可解析——04-concurrency 同组包级原生编译因 76 的 `four_mode_types`（组 F 四模式类型）在原生 LLVM 处 `error.Unsupported` 响亮拒绝 → 组内每个已解析文件均计入文件级 MISMATCH，78/79 各 +1；79 单独原生编译已验证 MATCH（捕获语法本身原生可编），78 的失败模式由解析错误转为运行时 `error.UndefinedName`（组 F）。**组 F 落地后 60→57**：四模式容器 37/76/77/78 interpret 全绿（4 例失败消除），编译侧 77/78 的级联计数随四模式解释层实现消化、79 维持 MATCH（37/76 仍因原生 LLVM 子集边界计入文件级 MISMATCH）。

CI（`.github/workflows/ci.yml`）在每次 push/PR 运行 `cargo test --workspace` 与完整示例套件回归（`tag1/scripts/check-examples.sh`，interpret ≥125 passed / ≤11 failed + compile ≤60 mismatch，低于基线即失败）。compile 基线 55→58 的 +3 为组 E E1 副作用：async/await 解析落地使含 `async fn`/`await` 的 5 例（37/38/39/76/80）由双后端解析失败转为 interpret 绿 + 原生红——原生/IR 后端尚无 Future/async 与四模式容器，`error.Unsupported` 响亮中止。组 E E2-E4 后该 5 例的 `[test]` 异步断言已双后端全绿（IR 侧 async fn 调用同步执行 + await 透传，子集边界对齐纯函数结果）；58 中这 5 例的文件级 MISMATCH 来自 `main` 函数特性而非 async——四模式容器（37/76——组 F 已落地（2026-08-18 逆转），原生 LLVM 仍为子集边界 `error.Unsupported`）、**38/80（G1 net 已落地仍红：38 主函数旧 URL 形式 `connect(url)`/`read_all(&conn)` + `JsonValue` 类型未实现，80 主函数 `https://` 网络不可达——仅 `http://` 支持，均非 G1 范围）**、`Io.evented` 原生构造器（39，interp-only E3），回落依赖原生 ABI 扩展而非 E/F 组。compile 基线 52→53 的 +1 为 D1 副作用：interpret 侧 `fmt_int` 落地使 63-template-render 转绿，但原生侧 `String.from/replace/find` 仍缺（预先存在的原生子集缺口，D3 注）——该例由「双失败」转为「interpret 绿 / 原生红」的 mismatch。53→54 的 +1 为 G1 副作用：`spawn(f, …)` 解析落地使 77-producer-consumer 由双解析失败转为 interpret 运行至 `error.UndefinedName`（四模式类型 `OneToOne` 未实现，第三块）而原生 LLVM 在 `spawn` 处 `error.Unsupported` 拒绝（G4b 前）——两后端均失败不变，仅失败模式改变计入 mismatch。54→55 的 +1 为 G5 副作用：新增在列示例 90-thread-lifecycle（组 G 线程，interpret 全绿 5 测试）在原生下于 `spawn` 处 `error.NotCallable` 中止——原生 ABI 无函数值表示（FnRef/CallIndirect/MakeClosure 属 Phase 8 原生 ABI 改造），G4b 定案 A 定为原生子集边界（三后端 interp/IR/字节码线程一致，原生响亮拒绝、不静默误编译），线程原生支持留 Phase 8。58→60 的 +2 为捕获语法解析副作用：78-task-dispatch / 79-retry 由双解析失败转为可解析——04-concurrency 同组包级原生编译因 76 的 `four_mode_types`（组 F 四模式类型，延迟 1.x）在原生 LLVM 处 `error.Unsupported` 响亮拒绝 → 组内每个已解析文件均计入文件级 MISMATCH，78/79 各 +1；79 单独原生编译已验证 MATCH（捕获语法本身原生可编），78 的失败模式由解析错误转为运行时 `error.UndefinedName`（组 F）。**组 F 落地（2026-08-18 逆转）后 60→57**：四模式容器 37/76/77/78 interpret 全绿（4 例失败消除，门禁 143/4/1 → 147/0/1）；编译侧 77/78 级联计数消化、79 维持 MATCH，37/76 仍因原生 LLVM 子集边界（四模式容器 `error.Unsupported` 响亮拒绝）计入文件级 MISMATCH。

## 已知取舍

- **原生/IR 后端为标量 + 指针 + 聚合 + switch/for + 闭包/函数引用/方法/重载 + global/const + defer/errdefer + 带标签 break/continue + 全核心标准库（IR）子集**：`hc build` / `hc test --mode=compile` 覆盖 M3.1 切片 + Phase 1 指针 + Phase 2 聚合 + Phase 3 switch/for（字段/索引/切片/数组/class/enum/元组解构/move/unwrap/switch 全模式/for 迭代含 mut 写回）+ Phase 4 闭包/函数引用/实例方法/重载 + Phase 5 global/const（声明序初始化 + 跨函数/跨测试可变全局 + `&global` 取址写穿）+ Phase 6 defer/errdefer（LIFO + 仅错误路径）+ 带标签 break/continue（跨层定位）+ Phase 7 全核心标准库（`run_ir` 全量；LLVM 原生仅已实现内建子集——io.print / `alloc.init` 无字段 / 标量 @ 内建 / min/max/sqrt/box/read_u64_le/copy / 用户类实例方法 + `Io.print` + math.*：nan/inf/inf_neg/sqrt/abs/pow/floor/ceil/round）+ Phase 8 闭包捕获精确化（自由变量精确分析含嵌套传递 + 非 mut 只读强制 + move 深拷贝）+ P11d 连续类值语义（`DeepCopy` 指令 + 运行时门：`[continuous]` 类 var 声明即深拷贝，非连续类/数组恒等别名，对齐 oracle `type_is_continuous`）；Table 多索引、defer 体控制流等子集外特性在 IR 降级时以 `error.Unsupported` 硬错误拒绝（**不静默丢弃**），`hc build` / `hc run --ir` 直接报错并提示改用 tree-walking 模式；未实现原生内建/方法在运行时以 `error.NotBuiltin`/`error.NoMethod` 响亮中止（原生 ABI 留后续阶段全标准库）。
- **LLVM 值盒全精度载荷**：`%Value = { i32, i128 }`（i128 修复 i64 截断；浮点位模式存低 64 位）；`hc build` 依赖外部 `zig cc`，无优化 pass，硬错误消息依赖 libc。
- **LLVM Mut/Move for 捕获 = copy-in/copy-out 写回**：迭代体内中读源容器在 LLVM 见旧值（`run_ir` 槽 cell == 源 cell 无此问题），接受近似。
- **原生交叉验证为文件粒度**：全绿 vs 有失败，非逐测试 PASS/FAIL 清单（断言失败在测试函数 ret 路径直接 abort）。
- **字节码 VM 复用 `run_ir`**：未做紧凑运行时 dispatch / 寄存器式 VM（性能优化留后续，须一致性套件证明等价）。
- **跨包静态链接（M7.2 后续）**：`build.zon` 的 `deps` 已支持本地依赖装载（解释/检查路径），但原生编译目前仅同目录包内合并，跨包链接归后续。
- **tree-walking 求值递归栈深**：`hc run` 与示例回归测试均在 64MB 栈线程中运行（Windows 主线程默认 1MB、测试线程默认栈更小，不足以承载深递归/大帧），非语义限制。

## 本阶段明确不实现（第三块 / 第二部分）

脚本生成（`script` 块元编程）、comptime 完整（类型即值）、**真 OS 并发**（协作式线程/异步/四模式/@atomic 已落地——组 G/E/F，真并行 + `mutex` 1.x）、标准库扩展（UDP/HTTP/ipc/FFI 等）、系统编程（K1–K11 —— 组 H 已落地 K1 无标签 union / K2 volatile / K4 整数↔指针 / K5 export fn，剩 K3 asm / K6 freestanding / K7–K11 1.x 候选）、工具链扩展（LSP/format/lint/注册中心）、**自举**（stage1 → stage2）—— 详见 `07-bootstrap-plan.md` 第四节。

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
