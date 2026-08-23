# 未实现功能清单（第三阶段实施 Backlog）

> **阶段定义（2026-08-22）**：第三阶段 = 标准库扩展 + 前两阶段未实现功能；自举 = 第四阶段（见 [`docs/SPEC/phase4/`](../phase4/)）；1.x 延迟项迁移至 `docs/SPEC/phase4/02-1x-delayed-items.md`。
>
> 每条目格式：**编号｜功能｜状态**（🔴 待实现 / 🟡 部分实现 / ⏳ 1.x / 🟣 第四阶段）｜**出处**｜**落点**（实现模块）｜**备注**。
>
> 实施规则：每功能点 ≤2h，超出即分解；完成即提交 + 测试 + 文档同步。

---

## A. 标准库扩展（第三阶段主项）

### A1｜FFI（`extern fn` + `@cImport` + `hc cc`）｜🟢（**已实施**）
- **出处**：`10-part3-execution.md` 组 G6；`07-bootstrap-plan.md` E3.5；`04-stdlib-scope.md`
- **落点**：`hc/src/parser/decl.rs`（`extern fn` 解析）/ `hc/src/llvm/emit.rs`（LLVM `declare` 发射）/ `hc-tools/src/cli.rs`（`hc cc` 子命令）/ `hc/src/ir/`（`IrFunc.is_extern`）
- **备注**：`extern fn` 已实现（parser + IR + LLVM declare + 6 测试）；`hc cc`（zig cc 封装）已实现；`@cImport` 尚未实现（C 解析器依赖外部库，留 1.x）

### A2｜数据库连接抽象｜⏳ 1.x
- **出处**：`10-part3-execution.md` G4 完成注记
- **备注**：`io.storage` KV 已落地（G4）；数据库连接依赖真实 DB 驱动 → 1.x

### A3｜通用压缩算法（gzip / zip）｜⏳ 1.x
- **出处**：`10-part3-execution.md` G4 完成注记
- **备注**：RLE 已落地；通用压缩留 1.x

### A4｜时区完整（tz 库）｜⏳ 1.x
- **出处**：`10-part3-execution.md` G5 完成注记
- **备注**：`io.time.tick/elapsed` 已落地；时区依赖 tz 库 → 1.x

### A5｜真 OS 进程 / 共享内存｜⏳ 1.x
- **出处**：`10-part3-execution.md` G3 完成注记
- **备注**：进程内 ipc（pipe / shm）已落地；真实 OS 进程依赖 FFI 与进程模块 → 1.x（与 A1 联动）

### A6｜标准库缺口：bitmap / 侵入式链表 / 环形缓冲 / 树 / 页内存｜⏳ 1.x
- **出处**：`02-milestones.md` 系统编程缺口表（标准库缺口行）
- **备注**：底层机器前提 K1/K2/K4/K5 已就绪（组 H1–H4）

### A7｜惰性 / 组合子迭代器（1.x 项）｜⏳ 1.x
- **出处**：`CONTEXT.md` 迭代契约条目；`02-milestones.md` M4.6
- **备注**：`iter()`/`filter()/map()` 立即求值链已落地；**迭代器对象 API 已补定（2026-08-22，ADR-0017 C3-1）**——`iter()` 返回迭代器方法签名（`next()` + `filter`/`map` 组合子返回显式迭代器对象）；惰性求值（`next()` 按需求值、链式延迟计算）真实现仍 1.x

### A8｜端到端示例程序（四大支柱同时使用）｜🟣（**已迁移至第四阶段**）
- **出处**：`02-milestones.md` M7 验收（「端到端示例程序（同时用到四大支柱）作为验收基准」）
- **落点**：`examples/` 新示例 + 测试
- **备注**：设计已定案（2026-08-22 grilling 会话），**已移至第四阶段**。TCP 聊天服务器（`server.hc` + `client.hc`），行文本协议，广播 + 昵称 + 私信，异步事件循环架构，双模式集成测试，代码注释标注四支柱映射。见 [`docs/SPEC/phase4/02-1x-delayed-items.md`](../phase4/02-1x-delayed-items.md)

---

## B. 工具链扩展（第三阶段主项）

### B1｜`hc lint`｜🟢（**已实施**）
- **出处**：`10-part3-execution.md` 组 I2；`07-bootstrap-plan.md` E5.1
- **落点**：`tag1/hc-tools/src/lintgen.rs`
- **验收**：静态诊断（命名规范补全——缩写全大写、未用变量、可简化构造）+ lint 测试绿
- **备注**：6 条规则（L001–L006）已实现：`unused_var` / `unused_import` / `simplifiable_construct` / `upper_case_abbr` / `simplifiable_if_else` / `redundant_eq_false`；4 条支持 `--fix`（接口预留）；`// @lint(off rule_name)` 内联关闭；`hc lint` 独立子命令 + `hc check` 默认集成；文本+JSON 输出

### B2｜LSP 完整化（`hc lsp` 子命令 + 脚本实时预览）｜🟢（**已实施**）
- **出处**：`10-part3-execution.md` 组 I3；`07-bootstrap-plan.md` E5.1
- **落点**：`tag1/hc-lsp/`（独立 crate：诊断 / 补全 / 跳转 / hover）+ `hc-tools/src/cli.rs`（`hc lsp` 子命令）+ Zed 扩展
- **备注**：LSP 已独立实现（诊断 / 补全 / 跳转 / hover 全功能）；`hc lsp` 子命令已整合（`hc-lsp` 作为 `hc-tools` 依赖，通过 `hc_lsp::run_server()` 启动）；Zed 扩展配置指向 `hc lsp`。脚本实时预览通道（M3 实时预览）未接通，留 1.x

### B3｜注册中心 MVP（`hc pkg` 完整：指纹 / 审计 / 供应链校验）｜🟢（**已实施**）
- **出处**：`10-part3-execution.md` 组 I4；`07-bootstrap-plan.md` E5.2
- **落点**：`hc-tools`（build.zon 指纹 + 依赖来源审计）+ 自托管 MVP
- **实施详情**：
  - `fingerprint` 类型：`Option<u64>` → `Option<String>`（SHA-256 十六进制），兼容旧 `0x...` 格式
  - `hc pkg publish`：读 build.zon → 收集文件 → 计算 SHA-256 指纹 → 写入 `~/.hc/registry/<name>/<version>/`
  - `hc build` 注册中心 fetch：无 `path` 的依赖自动从 `~/.hc/registry/` 解析
  - `hc run` 注册中心 fetch：`load_deps_into` 同样支持注册中心依赖
  - 指纹校验：发布时生成，fetch 时对比，不匹配报硬错误
  - 依赖：`sha2` + `hex` crate
- **文件更改**：`buildzon.rs`（指纹类型 + 解析兼容）、`project.rs`（`pkg_publish` + `resolve_registry_dep`）、`cli.rs`（`pkg publish` 子命令）、`build.rs`（注册中心 fetch）、`run.rs`（注册中心 fetch）

### B4｜包管理器正式版 + 官方注册中心（1.0 项）｜⏳ 1.x
- **出处**：`02-milestones.md` M8 / M10
- **备注**：M10 冻结前正式版；B3 为基础

### B5｜`hc cc`（C 互操作编译）｜🟢（**已实施**）
- **出处**：`02-milestones.md` M8；`10-part3-execution.md` G6（ffi 验收依赖）
- **落点**：`hc-tools/src/cli.rs`（`hc cc` 子命令 = zig cc 薄封装）
- **备注**：`hc cc` 已实现为 zig cc 薄封装 + build.zon C 源声明（ADR-0020），与 A1（FFI）统一设计

### B6｜脚本启动时间指标（TS 式低摩擦）｜🟡（**设计已完成，待实施**）
- **出处**：`02-milestones.md` M5（「脚本启动时间指标」）
- **备注**：字节码 VM 复用 `run_ir`（盒式表示），性能优化留后续。设计定案（2026-08-22）：`hc run --bench` 分阶段输出，空脚本 <10ms 基线；`~/.hc/cache/script/<source_hash>` 缓存展开结果

### B7｜质量工具完整（LSP / 格式化 / lint 集）｜🟢（**已实施**）
- **出处**：`02-milestones.md` M8
- **备注**：`hc fmt` 已落地（I1）；lint / LSP 整合 = B1/B2；LSP 跳转定义/hover/补全已完整实现

---

## C. 语言扩展（第三阶段主项）

### C1｜J4 Table 多索引完整（M8 记录项）｜🟢（**已实施**）
- **出处**：`10-part3-execution.md` 组 J4；2026-08-22 Table 设计会话（grill-with-docs）
- **落点**：`semantic.rs`（`check_index` 放宽 1/2 索引）、`interp.rs`（多索引写 `eval_assign` 修 bug）、`ir.rs`（`lower_expr`/`lower_assign` 链式降级）、新测试
- **备注**：**设计定案全部就绪**（见 SPEC `06-03-extended-types.md` Table 段 + `CONTEXT.md`）：行视图 `t[i]`、单元格读写 `t[i,j]`/复合赋值、扁平迭代、`len()/cols()`、to_bytes 双前缀、空表、`init_with` 密封构造（B 方案）、copy 深复制、嵌套、指针元素替换规则。实施影响清单：
  - `semantic.rs check_index`（L2874）放宽 Table 为 1 或 2 索引（1 索引 → 行视图 `Slice`）
  - `interp.rs eval_assign`（L3221-3292）修多索引写（当前单索引静默退化整行赋值 = bug）
  - `ir.rs lower_expr`（L2218）`lower_assign`（L2628）链式降级（`Index(base,i)` 取行 → `Index(row,j)` 取格 / `StoreIndex` 写格）；字节码/LLVM 复用既有嵌套 Arr 指令 → **零改动**
  - `init_with` 密封表：编译期强制只读（`t[i,j]=v`/复合/`&mut t` 编译错误）
  - 新测试：多索引写 / 行视图 / `init_with` 密封 / 复合赋值 / to_bytes 往返 / 空表

### C2｜开放问题裁决（J1：E6.1）｜🟢（**已实施**）
- **出处**：`10-part3-execution.md` 组 J1；`05-open-questions-and-risks.md`
- **落点**：ADR + SPEC 补定 + `hc-tools/src/cli.rs`（`--dangle` 标志）+ `hc-rt/src/interp/call.rs`（`with_arena` 解释器）+ `hc/src/ir/builtin.rs`（`with_arena` IR）+ `hc/src/semantic/mod.rs` + `hc/src/semantic/infer.rs`（语义注册）
- **条目**：① `--dangle=on|off|auto` CLI 标志；② `mem.with_arena(fn)` stdlib 包装
- **备注**：**全部实施完毕（2026-08-23）**——① `--dangle` CLI 标志（`hc run`/`hc test` 均支持 `DangleMode::On/Off/Auto`，`Auto` 默认 Debug 开）；② `mem.with_arena(fn)` 创建临时 Arena，调用函数后自动释放（无论成功或失败），解释器 + IR 双后端实现，语义层注册为内建函数。`debug_dangling` 与 `--dangle` 已对齐（tag1 默认 Auto = Debug 开）。

### C3｜惰性迭代、switch 守卫、Send/Sync 静态标记（编译期诊断）｜🟢（**已实施**）
- **出处**：`10-part3-execution.md` 组 J2；`07-bootstrap-plan.md` E6.1
- **落点**：`hc/src/semantic/infer.rs`（`type_is_send`/`type_is_sync`/`type_has_interface`）/ `hc/src/semantic/check.rs`（类声明验证 + spawn 边界诊断）/ `hc-rt/src/interp/eval.rs`（`exec_switch` 守卫检查）/ `hc/src/ir/lower_impl.rs`（IR 守卫降级）/ `hc/tests/send_sync.rs`（11 测试）/ `hc-rt/tests/consistency.rs`（4 守卫测试）
- **备注**：J2 三项全部实施完毕：
  - ① **惰性迭代**（选项 C）：`iter()`/`filter()`/`map()` 立即求值链已落地；`iter()` 返回迭代器方法签名（`next()` + `filter`/`map` 组合子返回显式迭代器对象）；真惰性求值留 1.x（A7 不动）
  - ② **switch 守卫**：`switch (v) { 模式 if 守卫 => 表达式 }`，守卫失败继续下一分支；语义层验证守卫为 bool + 穷举性检查（至少一个非守卫臂或 else）；解释器 + IR 双后端完整实现；4 测试
  - ③ **Send/Sync 编译期诊断**：`type_is_send`/`type_is_sync` 递归类型检查（标量自动 Send+Sync、`*mut` 非 Sync、集合看元素、用户类检查接口声明）；`class Foo: Send/Sync` 字段验证；spawn 边界非 Send 捕获 → 编译错误；11 测试

### C4｜绑定级只读（默认只读，Rust 式）｜⏳ 1.x（**文档已写未实现**）
- **出处**：2026-08-22 Table 设计会话关键发现
- **落点**：语义层 `VarInfo` 增 `mut_` 字段 + `check_stmt` 赋值检查 + `expr_ty` AddrOf 校验
- **备注**：**已核实未实现**——AST 有 `Stmt::VarDecl { mut_: bool }` 但 `VarInfo`（semantic.rs L167-174）无 `mut_`；赋值检查（L1744-1757）仅 `check_assignable`/`check_ptr_write`/`check_thread_freeze`，无绑定级只读检查；`&mut x` 不校验 x 是否 `var mut`。今天 `var t = ...; t[0,0]=5` 能编译。**A 方案（全局绑定只读）需大迁移**（示例 + 测试套件几十处 `var x = 1; x = 100`），记 1.x 待办，第三阶段不做

### C5｜泛型边界：内建泛型外层嵌套退化｜🟢（**已实施**）
- **出处**：`10-part3-execution.md` 组 D 完成注记（已知边界）
- **落点**：`hc/src/comptime.rs` + `hc/src/semantic/check.rs`
- **备注**：**已实施（2026-08-22，ADR-0018）**——① **内建泛型嵌套具体化**：`Vec<List<i32>>` 具体化后类型 = `Vec<List<@i32>>`（`hc/src/ir/lower_impl.rs` + `hc-rt/src/interp/eval.rs` `concrete_type_name` 修复）；② **无限大小类型语言层拒绝**：值内嵌自引用/互递归（无间接层）= 编译错误（报类型名 + 循环链位置），合法间接层 = 指针/装箱/堆容器/`?T`，`tree`/`LinkedList` 既有递归不受影响。14 个测试用例覆盖（`hc/tests/infinite_size.rs`）

### C6｜comptime_int 超大常量 bignum｜⏳ 1.x
- **出处**：`10-part3-execution.md` 组 D D4 完成注记（已知边界）
- **备注**：`Value::Int(i128)` 无 bignum，偏离 ADR 任意精度

### C7｜原生 ABI 函数值 / 闭包（Phase 8 原生改造）｜🟡（**设计已完成，待实施**）
- **出处**：`10-part3-execution.md` §2.2（「原生 ABI 函数值/闭包（Phase 8 原生改造）」+ 组 G4b 定案 A）
- **落点**：LLVM 后端（`llvm.rs`）
- **备注**：**设计定案就绪（2026-08-22，ADR-0019，grill-with-docs 访谈 3 子项全确认）**——① 函数值 = 胖闭包对象 `{ fn_ptr, env_ptr }`（堆上分配，`%Value` 载荷存指针，新增闭包 tag）；`FnRef` = LLVM 函数符号地址；② 调用复用 `%Value` 参数/返回值通道，闭包隐藏 env 首参，零动态分发；③ spawn 原生子集边界解除（G4b 定案 A「响亮拒绝」被真实支持替换），`NotCallable` mismatch（10-functions / 21-closures / 48-iterator-chain）归零，K4 H 后端编写时联动。实施 = LLVM 后端 Phase 8 大改造，实现另计

#### C8｜LLVM 原生内建子集扩展（mismatch 归零）｜🟣（**已迁移至第四阶段**）
- **出处**：`07-bootstrap-plan.md` §八 P11d 收束注记（2026-08-17 用户裁定到此收束）
- **落点**：`llvm.rs` 原生内建 / 方法 / 序列化 / 标量接口族
- **备注**：compile mismatch **52–57** 构成——21 Unsupported（12 defer-try-f 设计内硬错误 + 6 `Orders.Line` 跨包字面量 + 3 匿名 struct）+ 31 运行时（20 NotBuiltin + 6 NoMethod + 3 NotCallable + 2 AssertFailed）。**已移至第四阶段**，若重开需授权。见 [`docs/SPEC/phase4/02-1x-delayed-items.md`](../phase4/02-1x-delayed-items.md)

---

## D. 测试基建（第三阶段主项）

### D1｜并发测试 runner（`[test]` 并发形态：异步 / 线程测试）｜🟡（**设计已完成**）
- **出处**：`10-part3-execution.md` 组 J3；`07-bootstrap-plan.md` E6.1
- **落点**：`hc-rt` 测试基建 + `hc test`
- **备注**：当前测试串行（Q-T3）。**设计定案（2026-08-22 grilling 会话）**：`[test(async)]` 共享事件循环（复用 `Io.threaded()`）+ `[test(thread)]` 串行化独立线程；`[test]` 保持串行（向后兼容）；可配置超时 `[test(timeout=5)]` 默认 5s；每测试输出缓冲避免交错；测试间串行化保持确定性

### D2｜一致性套件扩展（新增语言构造纳入）｜🟡
- **出处**：`10-part3-execution.md` §0.1（双模式承诺延续）
- **备注**：第三阶段新增构造（如 Table 多索引 C1）须进一致性套件（interp == IR）

---

## E. 系统编程（第三阶段主项）

### E1｜K3 内联汇编 asm｜⏳ 1.x
- **出处**：`02-milestones.md` 系统编程缺口表；ADR-0014
- **备注**：特权指令非本块范围，已标注 1.x

### E2｜K6 freestanding（裸机模式，H core）｜⏳ 1.x
- **出处**：ADR-0014（H5 移出本块）；`02-milestones.md` 缺口表
- **备注**：无 OS / 无 libc / 无默认分配器独立目标（LLVM 后端专用）+ H core 抽取；自举后 1.x 更顺

### E3｜K7–K11：裸 fn 指针 / 位域 / 指针算术 / `@byteSwap` / `Atomic<T>`｜⏳ 1.x
- **出处**：`02-milestones.md` 系统编程缺口表（K7–K11 行）；`07-bootstrap-plan.md` E4.2
- **备注**：1.x 候选；裸函数指针与 C7 联动

### E4｜真 OS 并行 + `mutex` + 单写者无锁快路径（F1/F5）｜⏳ 1.x
- **出处**：`10-part3-execution.md` 组 F 完成注记（ADR-0011 逆转保留项）
- **备注**：协作式透明实现已落地；真 OS 线程 / 锁归 1.x，不破坏确定性承诺

---

---

## 统计

- **第三阶段活动项**：A 8（0 🔴 / 0 🟡 / 6 ⏳ / 1 🟣 / 1 🟢）+ B 7（0 🔴 / 2 🟡 / 1 ⏳ / 4 🟢）+ C 8（0 🔴 / 2 🟡 / 2 ⏳ / 1 🟣 / 3 🟢）+ D 2（0 🔴 / 2 🟡）+ E 4（4 ⏳）
  - 注：⏳ 标记项（1.x 延迟）已迁移至 [`docs/phase4/02-1x-delayed-items.md`](../phase4/02-1x-delayed-items.md)；🟣 标记项（A8 端到端示例、C8 LLVM 原生内建）已移至第四阶段
- **第三阶段待实施（🟡）**：**D1 并发测试 runner**、**B6 启动时间指标**、**C7 原生 ABI 函数值/闭包**、**D2 一致性套件扩展**

## 第四阶段（自举 + 1.x）

自举（K2–K6 H 版编译器）和 1.x 延迟项已迁移至独立文件夹 [`docs/SPEC/phase4/`](../phase4/)：

- [`docs/SPEC/phase4/01-bootstrap-plan.md`](../phase4/01-bootstrap-plan.md)（自举计划）
- [`docs/SPEC/phase4/02-1x-delayed-items.md`](../phase4/02-1x-delayed-items.md)（1.x 延迟项）
