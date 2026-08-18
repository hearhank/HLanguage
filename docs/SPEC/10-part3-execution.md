# 第三块执行细表（扩展功能 + 未完成项 + 自举）

> 状态：**执行中**（2026-08-18 启动）。源：`07-bootstrap-plan.md` §五（第三块 E1–E7）。关联：`02-milestones.md`（1.0 里程碑映射）、`04-stdlib-scope.md`（标准库扩展）、`05-open-questions-and-risks.md`（开放问题/系统编程缺口）、`09-part2-execution.md`（第二部分执行，A–H 全完成）、`docs/adr/0011–0014`（前置裁决定案）。执行规则：所有修改同步更新到 SPEC；**每功能点 ≤ 2h，超出即分解**；每子任务完成即提交（「先提交再继续」）。
>
> **当前执行范围（2026-08-18 用户定）**：至 **「H 编译 H」前**——组 A–J + K1–K4（H 语言编译器实现）；**K5/K6 自举闭环留后续**。按 ADR-0011/0014：**组 F（四模式/@atomic）延迟 1.x**、**H5（K6 freestanding）移出**。

## 0. 范围与规则

### 0.1 范围

- **范围**：第三块 E1–E7（元编程完整 / 并发与异步 / 标准库扩展 / 系统编程 / 工具链 / 语言扩展 / 自举）
- **目标**（07 §五）：补齐扩展功能 + **自举要求**——用 H 语言重写编译器并自举闭环。**总验收：`用 H 编译 H` 达成（stage2）**
- **功能点定义**：同 09——可独立验收的**行为面**；每功能点 = 实现 + 测试 + 文档同步 + `cargo test` 验证；**≤2h 超出即分解**
- **双模式承诺延续**：ADR-0004 共享 IR 唯一语义源不变；第三块新增语言构造须进一致性套件（新语法与既有语法一样要 interp == IR 双模式一致）

### 0.2 优先级论证（E1 先行）

- **E1 元编程最先**：评审 B1（`01-language-design.md`）明确「脚本生成**必须实现**（核心特性）」；且 E1 是 E7 自举的跳板——用 H 写编译器需要类型即值 / comptime / 代码生成能力
- **顺序**：Phase 1 = 语言系统扩展（E1 元编程 → E2 并发/异步）→ Phase 2 = 外围（E3 标准库 / E4 系统编程 / E5 工具链）→ Phase 3 = E7 自举；E6 语言扩展与吃狗粮反馈贯穿
- **示例回归联动**：现有 10 项失败示例是第三块验收信号——**E1 落地转绿 34/35，E2 落地转绿 37/38/39/76–80**（见 §4）

### 0.3 前置设计裁决（先定案再施工，每裁决 1 个 ADR + 描述补定）

| # | 裁决 | 背景 | 建议方向 |
|---|---|---|---|
| 1 | **并发模型衔接** | 组 G 定案协作式延迟执行（单线程确定性）；四模式容器/`@atomic` 的「单写者无锁路径」「C11 五内存序」预设真并发 | 分两步：协作式模型上建 `async`/`Future`（保持确定性，`await` ≡ `join()` 复用 G 组机制）；**四模式 + @atomic 需要真 OS 线程则引入真并发**——破协作式前先一致性评估 + 用户裁决 |
| 2 | **comptime 类型即值形态** | `fn List(T: type) type` 语法、类型作为值在编译期的表示、惰性实例化 | 仿 Zig `comptime` 块 + 类型值 = **编译期对象**（非运行时值）；实例化缓存；与既有泛型 where 约束（M2.3）衔接 |
| 3 | **script 块语义** | `types` 元数据对象（`types.fields/type/all`，Q23）、「产物 = 代码字符串就地替换」插入点语义、构建时执行安全（评审 C3 供应链风险） | 脚本 = **H 核心子集**（受限分配器/IO）；产物替换 = 声明级插入点（AST 文本区间）；指纹校验（build.zon）+ 依赖来源审计 |
| 4 | **K6 freestanding 范围** | 无 OS / 无 libc / 无默认分配器；标准库全部假设有 OS | 裁决是否 1.0 纳入（LLVM 后端专用目标）；K3 内联汇编已标注 1.x 不阻塞 |

> ✅ **四项裁决已定案（2026-08-18）**：A1 → [ADR-0011](docs/adr/0011-concurrency-model-handoff.md)（async/Future 走协作式；四模式/@atomic 延迟 1.x → **组 F 移出本块**）；A2 → [ADR-0012](docs/adr/0012-comptime-type-values.md)（type = 编译期对象、实例化即具体化）；A3 → [ADR-0013](docs/adr/0013-script-block-semantics.md)（装载期求值 + 文本区间替换 + 受限子集 + 供应链信任）；A4 → [ADR-0014](docs/adr/0014-system-programming-scope.md)（K1–K5 纳入、K6 延迟 1.x → **H5 移出本块**）。§2.1 描述补定随各 ADR 落地。

## 1. 描述充分性审查表（逐 E 模块判定）

| 模块 | 判定 | 依据 | 缺口 / 动作 |
|---|---|---|---|
| E1.1 script 块 | ⚠️ 需补定 | 07 描述有 forms 但语义未定（types 对象 / 插入点 / 安全） | 裁决 #3 + 描述补定（§2.1） |
| E1.2 comptime 完整 | ❌ 不充分 | 「泛型实例化完整」缺语法与类型值表示；`fn List(T: type) type` 现不可解析 | 裁决 #2 + 描述补定 |
| E1.3 序列化定制 | ⚠️ 依赖 E1.1 | Q37/Q38 样板生成通道（数据定义 → 样板） | E1.1 落地后补定 |
| E2.1 四模式类型 | ⏸ 延迟 1.x | 缓冲语义 / 单写者无锁路径需真并发（ADR-0011 已定案） | 1.x 排期（设计保留 06-10） |
| E2.3 异步 | ✅ 基本充分 | 协作式上 `Future(R)` + `await` ≡ `join()` 可复用组 G 机制 | 落地时对齐 G 组捕获/取消语义 |
| E2.4 原子 | ⏸ 延迟 1.x | 单线程协作式下 `@atomic` 无意义（ADR-0011 已定案） | 1.x 排期 |
| E3 标准库扩展 | ✅ 充分 | net/ipc/storage/text/ffi 方法清单可直译（04-stdlib-scope 有明细） | 无 |
| E4 系统编程 | ⚠️ 部分 | K1 无标签 union 已落地（H1）、K6 freestanding 已裁决 1.x（ADR-0014）、K3 asm 已标注 1.x；剩 K2/K4/K5（H2–H4） | 裁决 #4 + 05 缺口表 |
| E5 工具链 | ✅ 充分 | format/lint/LSP/注册中心目标明确 | 无 |
| E6 语言扩展 | ⚠️ 待裁决 | 开放问题 #1/#3/#4/#5/#6 逐项裁决 | §2.2 登记 |
| E7 自举 | ⚠️ 大工程 | 07 §五已给渐进路线（H lexer → parser → 语义 → 后端） | 分阶段（组 J） |

## 2. 缺口清单（盘点 2026-08-18）

### 2.1 描述补定（✅ 已随裁决落地，2026-08-18）

1. **comptime 类型即值**（[ADR-0012](docs/adr/0012-comptime-type-values.md)）——`type` = 元类型；类型对象 = **编译期值无运行时表示**；实例化 = 名字 + 实参列表的**具体化（monomorphization）+ 缓存**（对齐 M2.3 具体优先泛型）；`comptime { }` = 语义分析阶段求值（复用求值器子集，失败 = 编译错误）；comptime_int/float 惰性宽度；`anytype` 调用点推断。
2. **script 块**（[ADR-0013](docs/adr/0013-script-block-semantics.md)）——`script { ... }` = **装载期求值 + 文本区间替换 + 重解析**：脚本块节点携带源码文本区间，求值产物字符串替换该区间后完整编译；脚本 = **受限 H 子集**（io/alloc/argv/网络不可用）；隐式 `types` 对象可见范围随块位置；依赖包脚本默认禁用（build.zon 信任声明）；失败 = 编译错误带块内 + 所属块位置；降级闸门 Q-S10 保留。
3. **四模式类型**（[ADR-0011](docs/adr/0011-concurrency-model-handoff.md)）——**延迟 1.x**：需真 OS 线程语义，本块不实现；`write/read/try_read/close/send/recv` 方法集与缓冲语义设计保留（06-10），示例 76–80 转绿信号归 1.x。

### 2.2 文档差异项 / 开放问题（登记归口）

| 项 | 出处 | 归口 |
|---|---|---|
| Debug 悬垂标记切换粒度（编译单元/函数/引用点） | 05 开放问题 #1 | E6.1 裁决 |
| 无 GC 长运行脚本（Arena 惯例） | 05 #3 | E3.4（time/rng 同组）推广 |
| 序列化 schema 演进（版本/迁移钩子） | 05 #4 | ✅ 已定案（组 C E1.3，2026-08-18，见 05 #4 设计） |
| 注册中心治理（冲突/审计/失联） | 05 #5 | E5.2 MVP 起步 |
| 跨线程引用传递（Send/Sync 式静态标记） | 05 #6 | E6.1 静态标记 |
| `io.stdout`/`io.stderr` 独立流、`list_dir → Vec(DirEntry)`、`String.to_upper`、`io.fs.open_dir/Dir` | 09 §2.2（第二部分标注归口） | E3.1/E3 落地 |
| Table 多索引（M8 记录） | 07 §八未实现表 | E6.1 |
| 原生 ABI 函数值/闭包（Phase 8 原生改造） | 组 G4b 定案 A | Phase 8 原生 ABI（E7 前端行 或 E6） |
| 跨包全局链接 / IR 参考解释器跨包 NoFunction | 组 C3/C4 已知限制 | E3/E7 后端行 |

## 3. 执行序与 ≤2h 任务分解

> 功能点 = 行为面；验收列 = 该行为面的完成定义（实现 + 测试 + 文档同步 + `cargo test` 验证计入 2h）。组内序号即执行序；组间依赖见各组合注。**裁决未落地前，对应组不排程**（见 §0.3）。

### A. 前置裁决产出（Phase 0：文档定案，无代码）

> 每个裁决 = ADR + 描述补定（§2.1） + SPEC 同步，全部 ≤1h 的文档工作。

| # | 任务（行为面） | 验收 | 依赖 | 预估 |
|---|---|---|---|---|
| A1 | 裁决 #1 并发模型衔接：协作式 vs 真 OS 线程（四模式/@atomic 的前提） | ADR-0011 + 06-10-concurrency 补定 | — | 1h |
| A2 | 裁决 #2 comptime 类型即值形态定案 | ADR-0012 + 06-language-spec 补定 | — | 1h |
| A3 | 裁决 #3 script 块语义定案（types 对象/插入点/安全） | ADR-0013 + 06 补定 + 04 指纹章节 | — | 1h |
| A4 | 裁决 #4 K6 freestanding 范围定案 | ADR-0014 + 02/04 标注 | — | 1h |

### B. E1.1 script 块（依赖 A3；元编程基础）

> ✅ **组 B 已完成（2026-08-18）**：B1–B5 实现（`hc-tools/src/scriptgen.rs` 装载期展开 + `types` 元数据 + 受限子集 + 三后端一致装载），B6 示例转绿。**计划标误修正**：B6 原列「34/35 转绿」有误——34/35 属 **组 D comptime**（`T: type`/`anytype`），非本组；本组实际示例 = 33/36/81（script 示例，由 stub 转为真实生成；81 修复展开回归）。依赖包 script 默认禁用（ADR-0013 §5，装载器跳过 `Decl::Script`），build.zon 信任声明归 I4。

| # | 任务（行为面） | 验收 | 依赖 | 预估 |
|---|---|---|---|---|
| B1 | `script { ... }` 词法/解析/AST（声明级）：块体 = 表达式序列（字符串拼接）；与 `comptime` 区分 | parse 单测绿；study.hc 可解析 | A3 | 2h |
| B2 | 隐式 `types` 元数据对象：当前文件/包类型清单（fields/type/all，Q23） | script 测试绿（types 查询） | B1 | 2h |
| B3 | 产物 = 代码字符串就地替换（声明级文本区间替换，脚本块位置为锚）+ 错误机制统一（失败 = 编译错误带块内 + 所属块位置） | 生成端到端绿（脚本生成 → 编译产物可解析） | B2 | 2h |
| B4 | 构建时执行安全：脚本 = H 核心子集（受限分配器/IO）；build.zon 指纹校验（供应链，评审 C3） | 安全负例测试绿（越界 IO/分配拒绝） | B3 | 1.5h |
| B5 | 三后端对齐：IR/字节码/native 对 script 产物的一致装载（产物在降级前替换，后端无感知） | consistency + ir 测试绿 | B3 | 1.5h |
| B6 | 示例转绿：33/36/81 script 示例 stub → 真实生成；81 修复展开回归（34/35 属组 D，非本组） | 示例回归绿（132/10/1，脚本示例全 PASS） | B4/B5 | 1h |

### C. E1.3 序列化定制（依赖 B；脚本生成样板通道）

> ✅ **组 C 已完成（2026-08-18）**：C1/C2 实现——脚本从 `types.fields` 生成校验 + to_json 样板（String 非空 / i32 >= 0 / ?String null 守卫；JSON String 带引号 / i32 裸值 / ?String→`null`），示例 36 扩为「字段统计 + 校验 + 序列化」三合一；`hc-tools/tests/scriptgen.rs` 新增端到端定制通道测试（`hc run` 与 `hc run --ir` 一致）；schema 版本/迁移钩子设计定案（05 #4）。**已知边界**：生成的样板用 String 方法（`.concat`/`.len`）与 `fmt_int`——原生运行时 String 方法属 Phase 7 缺口，示例 interpret/IR 全绿、原生计入 compile mismatch（基线同步，见 tag1/README.md）。

| # | 任务（行为面） | 验收 | 依赖 | 预估 |
|---|---|---|---|---|
| C1 | ✅ 序列化/校验/存储样板生成（数据定义 → 样板，Q37/Q38）；schema 版本/迁移钩子设计（05 #4） | 样板生成端到端绿 + 文档 | B | 2h |
| C2 | ✅ 定制通道测试 + 文档 | script 定制测试绿 | C1 | 1h |

### D. E1.2 comptime 完整（依赖 A2；类型即值）

| # | 任务（行为面） | 验收 | 依赖 | 预估 |
|---|---|---|---|---|
| D1 | `type` 关键字 + 类型参数：`fn List(T: type) type` 解析/AST/语义（类型值 = 编译期对象） | parse + semantic 测试绿 | A2 | 2h |
| D2 | ✅ `comptime { ... }` 块 + 编译期求值执行（对齐 interp 求值器子集） | comptime 测试绿 | D1 | 2h |
| D3 | ✅ 泛型实例化完整：名字 + 实参列表具体化 + 惰性实例化缓存（对齐 M2.3 具体优先泛型） | 泛型测试绿（含嵌套/递归实例化） | D2 | 2h |
| D4 | ✅ comptime_int 常量折叠；✅ comptime_float 惰性宽度；✅ `anytype` 完整语义（调用点具体化）；✅ `comptime` 值函数（编译期求值） | 语义测试绿 | D2 | 1.5h |
| D5 | ✅ 三后端对齐：类型值在 IR 的表示（编译期展开后无运行时残留）+ 一致性用例 | consistency 绿 | D3 | 2h |

> ✅ **组 D 最小切片（D1）已完成（2026-08-18）**：comptime **类型函数**——`fn Pair(T: type) type { return struct { first: T, second: T }; }` 解析/语义/具体化落地。AST 增 `Expr::StructType` + `NamedLit.ty_args`；`hc::comptime`（`is_type_fn`/`concrete_name`/`subst`/`instantiate`）三后端共享具体化引擎；interp 与 IR 各自**惰性具体化登记**（`Pair(i32)` → `Pair<@i32>`，类型表缓存；`return T;` 透传 = 实参类型同义）。类型函数体降级**跳过**（comptime-only，无运行时残留）；内建泛型（`Vec(T)` 等）回退基础名不受影响。示例 **34-generics** interp / IR / 原生编译三模式全绿（`5/3.5/3`，2 测试）；consistency 新增 `d1_comptime_type_application_consistent`；compile 门禁 55→53。**留 D2–D5**：`comptime { }` 块、comptime_int/float（35 例）、`anytype` 完整语义、嵌套/递归实例化、`comptime` 值函数（参数含 `type`/`anytype` 的编译期执行）。

> ✅ **组 D D3/D4 最小切片已完成（2026-08-18）**：comptime_int 值参数 + 数组类型函数——`fn ArrayLen(T: type, n: comptime_int) type { return [n]T; }`。AST 增 `Type::ComptimeInt(usize)`（类型实参位置的整数字面量，惰性宽度）与 `Expr::ArrayType { len, elem }`（`[n]T` 数组类型值表达式；parser `in_type_fn` 标志下仅类型函数体 return 位置特殊解析，避免与数组字面量歧义）；`instantiate` 参数分类（`T: type` = 类型参数 / `n: comptime_int` = 值参数）+ 长度求值（字面量/参数引用）→ `Type::Array(n, elem)`；具体化名 `ArrayLen(i32, 3)` → `ArrayLen<@i32,3>`，数组产物 `type_key` = `[3]i32`。带 init 的 var-decl **惰性放行**（不调用 `concrete_type_name`，标注仅设 expected_ret——示例 35 靠 init 驱动）；`max_value` anytype 为普通运行时函数非类型函数。示例 **35-comptime-branch** interp / IR / 原生编译三模式全绿（2 测试）；comptime 单测 +8（值参数/数组形态含错误路径）；consistency 新增 `d35_comptime_array_type_fn_consistent`。**仍待 D3/D4**：嵌套/递归实例化、comptime_float、`anytype` 完整语义、`comptime` 值函数。

> ✅ **组 D D2（comptime 块最小切片）已完成（2026-08-18）**：`comptime { }` 块装载期编译期求值——AST/parser 增 `Decl::Comptime { body, span }`（`KwComptime` 声明级解析，镜像 `Decl::Script`）；`hc-tools/src/comptimegen.rs` 装载期 pass（script 展开后、语义检查前，经 `parse_with_scripts` 统一入口）：受限 Interp（`script_mode`：io/alloc/argv 不可用）求值块体，结果**丢弃**（仅编译期存在，无运行时代码、无源码替换）；失败 = 编译错误（`return error.X` → 「comptime 块返回错误 `error.X`」带块 span；运行时错误 → 原 RtError 渲染）。块内可见完整 `types` 元数据（含 script 生成类型——「script 展开后求值」顺序验证）。三后端跳过 `Decl::Comptime`（镜像 Script），IR/native 零改动。测试 `hc-tools/tests/comptime.rs` 5 项端到端（通过 / return error / 未知类型 / io 禁用 / script 生成类型可见）；`cargo test --workspace` 全绿；门禁基线不变（interpret 135/8/1、compile 53 mismatch）。**仍待 D3/D4**：嵌套/递归实例化、comptime_float、`anytype` 完整语义、`comptime` 值函数。

> ✅ **组 D D3（嵌套/递归实例化）已完成（2026-08-18）**：类型函数**嵌套**（`PairPair(i32)` 字段 `a: Pair(T)` → 具体化键 `Pair<@i32>`）与**递归/自引用**（`fn LinkedList(T: type) type { return struct { value: T, next: ?LinkedList(T) }; }`）实例化。实现：`hc::comptime` 增深度遍历辅助 `map_type_apps`（后端注入 resolver 回调，规避 hc 零依赖约束——`Named(n, args)` 空 args 克隆 / 非空 → `Named(resolve(n, args), [])`，`Ptr/Slice/Optional/ErrorUnion/Tuple/Array/Owned` 递推）；interp 与 IR 的 `concrete_type_name` 重写为**先预解析实参**（内层类型函数先具体化登记，返回具体化键）→ `instantiating: Vec<String>` **in-progress 守卫**（自/互递归字段内自引用 → 返回自身键为叶，不无限重入）→ 具体化 Class 声明字段经 `normalize_decl_fields` 规范化。IR `lower_default_value` 增类型函数应用臂（惰性具体化后递归），并补 `var x: PairPair(i32);` 声明式无初值路径——对齐 oracle `default_value`（原 IR 无初值恒推 Void，字段访问报 NoField）。运行时递归靠 Optional 默认 `None` 终止（`next = null` / 无初值构造不递归）。测试：comptime 单测 +2（parser 嵌套回归、`map_type_apps` 嵌套/复合形态单测）、consistency +2（`d3_nested_instantiation_consistent`、`d3_recursive_instantiation_consistent`，均含声明式无初值）；`cargo test --workspace` 全绿；门禁基线不变（interpret 135/8/1、compile 53 mismatch）。**已知边界**：内建泛型外层嵌套 `Vec(List(i32))` 仍退化裸名 `Vec`；无限大小类型（非 `?` 自引用）语言层非法，未处理。**仍待 D4**：comptime_float、`anytype` 完整语义、`comptime` 值函数。

> ✅ **组 D D4（comptime_int 常量折叠最小切片）已完成（2026-08-18）**：comptime 块**类型层补齐**——`comptime_int` 类型名识别 + comptime 块语义检查。折叠核心已在 D2（装载期受限 Interp 求值），本切片补类型层：① `ty_of` 增 `"comptime_int"` → `SType::Int { width: IntWidth::Comptime }`（惰性宽度整数，Comptime 宽度跳过收窄检查；`check_int_width_st` 1513 行 `!matches!(w, Comptime)` 已有语义）；② `Checker` 增 `in_comptime_block` 标志 + `check_decl` 增 `Decl::Comptime` 臂（按函数体 `check_block(body, &mut scopes, None, None)` 类型检查），`Stmt::Return` 错误返回守卫放宽为 `!ret_is_error_union && !in_comptime_block`（comptime 块失败机制 = `return error.X`，非错误联合语义）。效果：comptime 块内 `var x: u8 = 256` 收窄溢出、`var x: comptime_int = "hello"` 类型不匹配均在**收窄点/赋值点诊断**；`expect_eq` 断言折叠（`comptime { var x: comptime_int = 1 + 2; expect_eq(x, 3); }`）。测试：hc 语义单测 +2（识别 comptime_int、拒绝 String 赋值）、hc-tools 端到端 +5（折叠通过 / 断言失败 = 编译错误 / comptime_int 变量折叠 / u8 收窄溢出 = 编译错误 / 范围内收窄通过）；`cargo test --workspace` 全绿；门禁基线不变（interpret 135/8/1、compile 53 mismatch）。**已知边界**：`Value::Int(i128)` 无 bignum，comptime_int 超大常量溢出（偏离 ADR 任意精度，i128 上限）；块内 `_ = x;` 丢弃语句装载期 Interp 不支持（`_` 未定义名）。**仍待 D4**：`anytype` 完整语义、`comptime` 值函数。

> ✅ **组 D D4（comptime_float 惰性宽度）已完成（2026-08-18）**：`comptime_float` 类型名识别——`ty_of` 增 `"comptime_float"` → `SType::Float`（H 浮点单一 f64 表示，惰性宽度浮点映射单一 Float）。comptime 块浮点折叠（装载期 Interp 求值已在 D2）+ `expect_eq` 断言（`value_eq` `(Float, Float)` 精确相等）；类型不匹配（`var x: comptime_float = "hello"`）在赋值点诊断。测试：hc 语义单测 +2（识别 comptime_float、拒绝 String 赋值）、hc-tools 端到端 +3（折叠通过 / 断言失败 = 编译错误 / 除法折叠）；`cargo test --workspace` 全绿；门禁基线不变（interpret 135/8/1、compile 53 mismatch）。**仍待 D4**：`anytype` 完整语义、`comptime` 值函数。

> ✅ **组 D D4b（anytype 完整语义）已完成（2026-08-18）**：`anytype` 参数 = 调用点按实参具体类型实例化（ADR-0012 #5）。**类型层具体化**（运行时仍动态分派，值携带类型——两后端零改动）：`hc::comptime` 增 `has_anytype`（参数含 `Type::Infer` 判定；`concrete_name("max_value", &[i32, i32])` → `max_value<@i32,i32>` 具体化键，对齐类型函数）；semantic `match_overloads` 增 anytype 分支——`anytype` 参数绑定调用点实参具体类型，返回 `anytype` 解析为**体 return 表达式在具体绑定下的重求值类型**（`Checker` 增 `anytype_bodies`/`anytype_ret_cache`/`anytype_resolving`；重求值经 `retype_return`/`collect_return_types` 收集多路径 return，首个 definite 为代表、其余须 mutual-compatible，冲突回落 `Infer`；`(qname, 具体化键)` 惰性缓存，自递归守卫）。效果：`max_value(2.5, 1.5)` 返回类型 = `f64`（`var m: f64 = ...` 无诊断；`var s: String = ...` → `cannot assign` 编译错误——具体化前 `Infer` 通配被静默放行）；`max_value(3, 7)` = 惰性宽度整数赋 i32 收窄。测试：hc 语义单测 +3（`has_anytype` 判定、具体化名、返回类型解析/误配/整型收窄）、hc-tools 端到端 +2（三后端一致 + 误配 = 编译错误）、consistency +1（`d4b_anytype_concrete_consistent`，f64/i32/异构实例 interp == IR）；`cargo test --workspace` 全绿；门禁基线不变（interpret 135/8/1、compile 53 mismatch）。

> ✅ **组 D D4c（comptime 值函数）已完成（2026-08-18）**：参数含 `T: type`、非返回 `type` 的普通函数（`fn array_len(T: type) comptime_int`）= comptime 值函数——调用点**编译期求值**（ADR-0012「参数含 type/anytype 触发编译期执行」落地）。实现：`hc::comptime` 增 `is_type_param`/`is_comptime_value_fn`（类型参数判定 + 值函数判定——返回 `type` 的类型函数归 D1，`anytype` 运行时函数归 D4b）/`expr_to_type`（调用点实参表达式 → 类型：`i32` → `Named(i32)`、`Vec(i32)` → 嵌套应用；值形态 → None）；interp 增 `try_comptime_value_call`（`eval_call` 两处挂钩——命名空间限定与平名调用：`T: type` 实参须为已知类型表达式作类型绑定，值实参（comptime_int/anytype/普通）常量求值，`exec_fn_body` 求值体 → 折叠结果；自递归深度守卫 `comptime_value_depth` 超限 → `ComptimeRecursion` 编译错误）+ `is_known_type_name`（基础/内建容器/已登记类型/类型函数名判定）。comptime 块装载期求值（script_mode）与运行时 interp 共用此路径——`array_len(i32)` = 4、`byte_size(f64, 7)` = 8 折叠。测试：hc 单测 +2（值函数判定、`expr_to_type` 形态）、hc-tools 端到端 +5（块内折叠含 IR 一致 / 混合参数 / 运行时调用 interp 折叠 / 自递归 = 编译错误 / 非类型实参 = 编译错误）；`cargo test --workspace` 全绿；门禁基线不变（interpret 135/8/1、compile 53 mismatch）。**已知边界**：最小切片体不引用类型参数值（引用 → UndefinedName 编译错误）。运行时调用点折叠 IR/原生已随 D5 落地。

> ✅ **组 D D5（三后端类型值表示 + 一致性）已完成（2026-08-18）**：comptime 值函数**运行时调用点折叠三后端一致**——类型值仅编译期存在，IR/原生无类型值/调用残留。实现：`hc::ir` 增 `collect_value_fns`（收集 comptime 值函数 name → params+body，镜像 `collect_type_fns` 扁平+限定名；值函数体为普通常量表达式，运行时不执行）+ `LowerCtx.value_fns` 贯穿（lower/lower_decl/lower_func/lower_init_func/collect_ns_funcs/闭包 LowerCtx 全链）；Call 降级 `callee_name` 后、实参降级前挂钩 `try_fold_comptime_value_call`——类型实参经 `comptime::expr_to_type` 收已知类型表达式（`is_known_type_name`：基础/内建容器/已登记 class/enum/类型函数，对齐 interp）、值实参常量求值入 bindings、体经 `eval_const_block` 顺序执行（var/const 初始化、return、if 常量条件折叠 then/else/else-if、分支未返回继续后续；不可常量求值语句 → 折叠回退既有路径），折叠成功发射 `Const`（无调用/类型值残留）。原生经共享 IR 继承折叠（`hc build` 验证 `array_len(i32)` = 4、`byte_size(f64, 7)` = 8）。测试：consistency +1（`d5_comptime_value_fn_consistent`，纯类型/混合参数/if 分支折叠，interp == IR）；D4c 运行时调用测试扩展为 interp + IR 双模式断言；`cargo test --workspace` 全绿（692）；门禁基线不变（interpret 135/8/1、compile 53 mismatch）。**组 D 完结**：类型函数（D1）、comptime 块（D2）、嵌套/递归实例化（D3）、comptime_int/float 常量折叠（D4）、anytype（D4b）、comptime 值函数（D4c）、三后端类型值表示 + 一致性（D5）全部落地。

### E. E2.3 异步（依赖 A1 协作式路径 + 组 G；确定性 Future）

| # | 任务（行为面） | 验收 | 依赖 | 预估 |
|---|---|---|---|---|
| E1 | ✅ `async fn` 解析/语义 → `Future(R)` 返回类型（复用 Thread 类名分派模式） | parse + semantic 测试绿 | A1 | 2h |
| E2 | ✅ `await` ≡ `join()`：任意函数可 `await`（对齐 G 组 join 机制）；协作式取消 | 异步测试绿 | E1 | 1.5h |
| E3 | ✅ `Io.threaded()`/`Io.evented()`（单线程事件循环）：IO 事件队列 + 轮询 | io 异步测试绿 | E2 | 2h |
| E4 | ✅ 一致性 + 示例转绿（37/38/39） | consistency + 示例绿 | E3 | 1.5h |

> ✅ **组 E 异步（E2.3，协作式 Future）已完成（2026-08-18）**：async/await + 单线程事件循环落地。**E1**：`async fn` 解析/语义——`Future(R)` 返回类型（R = 声明返回类型含错误联合 `Future(!R)`）、任意函数可 `await`（Q19 无 async 传染）、`await` 解包取 R。**E2**：`await` ≡ `join()`——async fn 调用点返回**惰性** `Future` 值（体延迟到 await），复用组 G Thread 协作式机制（`make_future`/`future_run` 镜像 thread_run）；协作式取消（`cancel` 置标志 → await 返回 `error.Cancelled`）、`is_done` 状态转移、await 幂等缓存；consistency `e2_async_await_consistent`（interp 惰性 == IR 急切，纯函数一致；副作用时序/取消为 interp 特有，IR 子集边界）。**E3**：`Io.threaded()`/`Io.evented()` 单线程事件循环——构造器写 `runtime` 字段（默认 io = threaded）、`io.poll()` 排空根回收队列（作用域退出提升的未 join 线程 → 运行到完成并返回计数；threaded 恒 0）；interp-only（原生构造器未实现 → 示例 39 main 计入 58 mismatch）。**E4**：示例 37/38/39/76/80 的 `[test]` 异步断言**双后端全绿**（interpret 142/5/1 保持；IR 侧 async fn 调用同步执行 + await 透传对齐纯函数），consistency +1（`e4_async_pointer_capture_consistent`，示例 37/76 `async_scope_binding` 模式 `&base` + Future(i32)）；hc-rt async.rs 直测 11（E2 7 + E3 4）。门禁基线不变（interpret 142/5/1、compile 58 mismatch——5 例文件级 MISMATCH 来自 `main` 特性：四模式容器 37/76 组 F 延迟、io.net/JsonValue 38/80 G1 net 待、Io.evented 39 interp-only）。

### F. E2.1 四模式类型 + E2.4 原子（⏸ 延迟 1.x——ADR-0011 定案）

> **本组移出当前执行范围（2026-08-18 ADR-0011）**：四模式容器与 `@atomic*` 需真 OS 线程语义，协作式模型下无并发对象；破协作式违背确定性承诺与一致性套件可比性。设计保留（06-10、ADR-0007），排期 1.x。示例 76–80 转绿信号归 1.x。以下任务表为 1.x 排期预案（不排程）。

| # | 任务（行为面） | 验收 | 依赖 | 预估 |
|---|---|---|---|---|
| F1 | OS 线程后端接入（interp：真线程 + 同步原语；G 组协作式标记保留为默认模式） | 线程测试绿（真并发） | A1 | 2h |
| F2 | `@atomicLoad/Store/Rmw` + C11 五内存序（@atomic 内建四后端对齐） | 原子测试绿 + consistency | F1 | 2h |
| F3 | `OneToOne` 容器（write/read/try_read/close/send/recv） | 四模式测试绿（OneToOne） | F1 | 2h |
| F4 | `OneToMany/ManyToOne/ManyToMany` + 有界缓冲语义（共享内存无容量、通道 `init(alloc, cap)`） | 四模式测试绿（全形态） | F3 | 2h |
| F5 | 单写者无锁路径（读/写分离快路径） | 性能/正确性测试绿 | F4 | 2h |
| F6 | 示例转绿（76–80）+ 文档（06-10） | 示例回归绿 | F5 | 1.5h |

### G. E3 标准库扩展（依赖：无特殊；逐模块独立）

| # | 任务（行为面） | 验收 | 依赖 | 预估 |
|---|---|---|---|---|
| ✅ G1 | net 完整：UDP（bind/send_to/recv_from）+ HTTP 客户端/服务端 | net 测试绿（UDP + HTTP） | — | 2h |
| ✅ G2 | io 差异项补全：`stdout`/`stderr` 独立流、`list_dir → Vec(DirEntry)`、`String.to_upper`、`fs.open_dir/Dir` | io 测试绿（差异项） | — | 1.5h |
| ✅ G3 | ipc：管道、共享内存 | ipc 测试绿 | — | 1.5h |
| ✅ G4 | storage/archive：键值存储接口、数据库连接抽象、归档与压缩 | storage 测试绿 | — | 2h |
| ✅ G5 | text/time/rng：正则等文本处理、时间与时区完整、伪随机数 | text/time/rng 测试绿 | — | 2h |
| G6 | ffi：`extern fn` + `@cImport`（Q-S4 内建 C 解析器）+ C 指针外置 + 错误码映射 + `hc cc` | ffi 测试绿（端到端 C 链接） | — | 2h |

> ✅ **G1 net 已完成（2026-08-18）**：UDP（`io.net.udp.bind(port)` / `bind(host, port)` + `send_to`/`recv_from`/`local_port`/`close`，Q20 双语命名空间形式）——UdpSocket 值持 fd 注册表，读超时 200ms → `error.TimedOut`（空队列不挂起测试）；recv_from 返回 2 元素数组 `[addr, data]`（无 `Value::Tuple`）。HTTP 客户端 `io.net.get(url)`（仅 `http://`，非 200 → `error.Http{code}`，体按 Content-Length 截取）；HTTP 服务端复用 `io.net.listen`+`accept`+`read_all`/`write`（HTTP 为应用协议层）。**Q20 双语补齐**：`io.net.read_all(&conn, alloc)` / `write` / `shutdown` / `close` / `local_port` / `accept(&server)` 命名空间形式 ≡ 实例方法（解引用剥 Ptr 委托）。hc-rt net.rs 直测 6（UDP 3 + HTTP 2 + TCP 双语 1）全绿；门禁基线不变（interpret 142/5/1、compile 58 mismatch——示例 38/80 仍文件级 MISMATCH：38 主函数旧 URL 形式 `connect(url)`/`read_all(&conn)` + `JsonValue` 类型未实现、80 主函数 `https://` 网络不可达（仅 `http://` 支持），均非 G1 范围）。

> ✅ **G2 io 差异项补全 已完成（2026-08-18）**：`io.stdout` / `io.stderr` 独立字节流（Stdout/Stderr 类值，`write_all(data)` 写真实句柄，返回 void；无 fd 注册表，类名分派）；`String.to_upper` / `to_lower`（ASCII 大小写转换，非 ASCII 字节不变）；`io.fs.list_dir` 改为返回 `Vec(DirEntry)`——每条 `{name, is_dir}`（不再裸文件名数组），路径形态 `list_dir(path)` 与句柄形态 `list_dir(&dir, alloc)` 双支持（Dir 值 deref 剥 Ptr）；`io.fs.open_dir(path) !Dir`（读校验 → fd→路径注册表，`dir.list_dir(alloc)` 重开枚举 / `dir.close()` 注销）。hc-rt io.rs 直测 9→13（新增 open_dir / DirEntry.is_dir / to_upper_lower / stdout-stderr 4 例，原 list_dir 改 DirEntry 形态）；示例 82-directory / 85-grep-tool 主函数（此前按 G2 目标形态书写、open_dir 未实现时仅测试占位绿）现可实际运行。门禁基线不变（interpret 142/5/1、compile 58 mismatch）。

> ✅ **G3 ipc 已完成（2026-08-18）**：进程内 IPC 原语——`io.ipc.pipe() !(PipeReader, PipeWriter)`（匿名管道 → 2 元素数组 `[reader, writer]`，同 UDP recv_from 约定）：写端 `writer.write(data) !void` / `writer.close() !void`（置写端关闭标记）；读端 `reader.read(alloc) !&[u8]`（排空可读字节；空且写端开 → 空切片，不阻塞——协作式模型）/ `read_all(alloc)` / `is_closed() bool`（写端已关）/ `close() !void`（注销管道；close 幂等，管道已拆除后再 close 为 no-op）。`io.ipc.shm(name, size) !Shm`（命名共享内存，定长字节区）：`shm.write(data) !void`（覆盖内容、截断到 size）/ `shm.read(alloc) !&[u8]` / `shm.close() !void`。设计取舍：真实 OS 进程/共享内存依赖 FFI 与进程模块 → 1.x；Interp 全局 pipe/shm 注册表（`Rc<RefCell<>>`），经 `spawn` 传 Pipe 值即可在 H 线程间传数据（协作式模型下无阻塞读）。hc-rt ipc.rs 直测 6 全绿（pipe 流/累积排空/关闭语义/线程生产者 + shm 流/定长截断）；门禁基线不变（interpret 142/5/1、compile 58 mismatch——无示例用 ipc）。

> ✅ **G4 storage/archive 已完成（2026-08-18）**：`io.storage.open(path) !KvStore`（文件持久化键值存储——`put(key, value) !void` / `get(key) !?&[u8]`（缺失 → null）/ `contains(key) bool` / `remove(key) !void`（幂等）/ `len() usize` / `close() !void`：落盘（二进制 u32 键长+键+u32 值长+值，小端）+ 注销注册表，close 幂等）——数据库连接抽象依赖真实 DB 驱动 → 1.x；`io.archive.compress(data) !&[u8]` / `decompress(data) !&[u8]`（RLE：token 0x00 字面跑 / 0x01 重复跑；重复输入明显变短、round-trip 任意字节保真、非法数据 → `error.InvalidFormat`）——通用压缩算法（gzip/zip）留 1.x。hc-rt storage.rs 直测 7 全绿（KV 4：put/get 含 missing-null / contains-remove-len / persist-reopen / close-idempotent；archive 3：roundtrip 缩短 / 二进制含 token 字节 / 非法 InvalidFormat）；门禁基线不变（interpret 142/5/1、compile 58 mismatch——无示例用 storage/archive）。

> ✅ **G5 text/time/rng 已完成（2026-08-18）**：`io.text.*` 正则文本处理——`matches(pattern, text) bool`（是否含匹配；`^`/`$` 锚定控制全串）/ `find(pattern, text) ?int`（首个匹配起点；无 → null）/ `replace(pattern, text, repl) &[u8]`（替换全部非重叠匹配、每处取最长）/ `split(pattern, text) Vec(&[u8])`（按匹配分割，含空段）；正则子集：字面量 / `.` / `[...]`（范围、`^` 取反、`\d` `\w` `\s`）/ 分组 / `*` `+` `?` `{n,m}` / `|` / `^` `$` / `\n` `\t` `\r` `\xNN` 及转义元字符——平坦 AST + 记忆化集合回溯（`(节点, 位置) → 结束位置集合`，Repeat 闭包收敛，无灾难性回溯）；非法模式 → `error.InvalidFormat`。`io.time` 补 `tick()`（纳秒计数）/ `elapsed(tick)`（毫秒）单调测量——时区完整依赖 tz 库 → 1.x。`io.rng.*` 伪随机数——`seed(v)` / `next()`（xorshift64* 原始 64 位）/ `int(n)`（[0,n) 均匀，拒绝采样免模偏差）/ `float()`（[0,1)，高 53 位）；全局态在 Interp 实例（协作式单线程安全）；命名空间类名 `RngNs` 避开示例 84-rng 的用户类 `Rng`（内建先于用户方法分派，同名会被拦截）。hc-rt text_rng.rs 直测 10 全绿（text 6：matches 基础/锚定交替量词/find/replace/split/invalid-pattern；time 1；rng 3：seed 确定性+金标 / int 界限 / float 范围）；门禁基线不变（interpret 142/5/1、compile 58 mismatch——84-rng 的 `rng_range` 用户类测试保持绿，无示例用 io.text/io.rng）。

### H. E4 系统编程（依赖 A4 + 05 缺口表）

| # | 任务（行为面） | 验收 | 依赖 | 预估 |
|---|---|---|---|---|
| H1 | ✅ K1 无标签 union（裸内存双关：字段重叠，无判别标签） | union 语义测试绿 | A4 | 2h |
| H2 | K2 volatile：`@volatileLoad/Store`（LLVM volatile 语义，防优化掉） | volatile 测试绿（含 LLVM 发射断言） | — | 1.5h |
| H3 | K4 `@ptrFromInt`/`@intFromPtr`（整数 ↔ 指针，物理地址 → 虚拟指针） | @ 内建测试绿 | — | 1h |
| H4 | K5 `export fn`（符号导出）+ 链接脚本钩子（段布局/对齐） | export 测试绿（符号表断言） | — | 1.5h |
| ⏸ H5 | K6 freestanding（裸机模式）——**移出本块（2026-08-18 ADR-0014）**，1.x 排期 | — | — | (2h) |
| H6 | 文档同步：05 缺口表状态勾选 + 04 stdlib 系统编程扩展 + 02 里程碑 | 文档绿 | H1–H4 | 0.5h |

> ✅ **组 H H1（K1 无标签 union）已完成（2026-08-18）**：`union { a: i32, b: f32 }` 裸内存双关（字段重叠、无判别标签，ADR-0014 定案）。**表示**：interp union 值 = `Value::Class(ClassData)`，带 `@union` 标记字段 + 所有声明字段零初始化；**写同步**——写字段 F 时把其他每个字段重新解释为 F 的字节（buffer 大小 = 写入字段宽度），读字段用 `bytes.get(..N)`，目标宽度 > 写入宽度 → `InvalidBytes: truncated union bytes` 错误；转换规则 int = `trunc i128 to iN` 后符号扩展、f32 = `trunc to i32` + `bitcast to float` + `fpext to double`、f64 = `trunc to i64` + `bitcast to double`、bool = `trunc to i8` + `icmp ne 0`。**约束**：union 仅允许标量字段（编译时错误）、union 字面量恰好接受一个字段。**引用类型**：`var b = a;` → 「cannot assign reference type」需 `copy(&a)`（对齐 Value::Class）。**原生边界（响亮拒绝）**：IR `UnionSync` 发射 `call void @hc_abort_builtin() + unreachable`——与闭包/notcallable 同类，编译期不拒、运行期在**首个 union 字面量处**中止（`error.NotBuiltin`），绝不静默误编译；门禁 compile mismatch 保持 60。**测试**：consistency 96 全绿（含 6 union：int 宽→窄/float↔int 重解释/bool 窄读/写同步/相等性/截断读失败）；frontend 56 全绿（含 5 union：声明解析、标量仅限、单字段字面量、未知字段、字段访问 clean）；bytecode union 往返（opcode 48 UnionSync + unions 表，`run_bc` == `run_ir`）。门禁基线不变（interpret 143/4/1、compile 60 mismatch）。**已知边界**：未加 union 示例（原生会中止），语义完全由一致性/前端/字节码套件覆盖。

### I. E5 工具链扩展（依赖：无特殊；与语言扩展并行）

| # | 任务（行为面） | 验收 | 依赖 | 预估 |
|---|---|---|---|---|
| I1 | `hc fmt`：格式化（token 级重排：缩进/换行/空格规范，AST 保真） | fmt 测试绿（幂等 + 样例断言） | — | 2h |
| I2 | `hc lint`：静态诊断（命名规范补全——缩写全大写、未用变量、可简化构造） | lint 测试绿 | — | 1.5h |
| I3 | `hc lsp` MVP：编辑器诊断（复用语义检查 + script 实时预览通道） | lsp 测试绿（诊断/补全最小集） | — | 2h |
| I4 | 注册中心 MVP：自托管（build.zon 指纹 + 依赖来源审计 + 供应链校验） | 注册中心端到端绿 | B4 | 2h |

### J. E6 语言扩展 + 吃狗粮（贯穿；E7 自举前的语言成熟）

| # | 任务（行为面） | 验收 | 依赖 | 预估 |
|---|---|---|---|---|
| J1 | 开放问题裁决：#1 切换粒度、#3 Arena 惯例、#4 schema 演进（C1 已带）、#5 注册治理（I4 已带）、#6 跨线程标记 | 05 状态表关闭项 + ADR | — | 1h |
| J2 | 惰性迭代、switch 守卫、Send/Sync 静态标记（编译期诊断） | 语义测试绿 | — | 2h |
| J3 | 并发测试（`[test]` 并发形态：异步/线程测试 runner） | 测试基建绿 | E/F | 1.5h |
| J4 | Table 多索引（M8 记录项） | Table 测试绿 | — | 1.5h |
| J5 | 吃狗粮反馈：编译器编写（E7）暴露的语言缺口反馈回设计 + 修订 | 06 修订记录 | E7 首段 | 1h |

### K. E7 自举（Phase 3：用 H 编译 H）

> 07 §五 E7 渐进路线：H lexer → parser/AST → 语义（类型/所有权/错误集）→ 后端（IR/VM/LLVM）。**双实现对照**：与 Rust 版 token/AST/执行结果对比，差异即 bug。**Rust 参考实现长期保留**（自举失败风险对策，05 风险登记）。

| # | 任务（行为面） | 验收 | 依赖 | 预估 |
|---|---|---|---|---|
| K1 | H 版 lexer（.hc 源码 → token 流）+ 对照测试（Rust lexer 输出 diff） | H lexer 可跑 + 对照绿 | D（comptime 支撑编译期工具） | 2h |
| K2 | H 版 parser/AST（token → 声明树）+ 对照测试 | H parser 可跑 + 对照绿 | K1 | 2h |
| K3 | H 版语义（名称解析/类型检查/所有权/错误集）+ 对照测试 | H 语义可跑 + 对照绿 | K2 | 2h |
| K4 | H 版后端：IR 参考解释器（跑 H 自身测试）+ 对照 | H 后端可跑 + 执行结果对照绿 | K3 | 2h |
| K5 | 自举闭环 stage2：H 编译器（H 程序）用 stage1 编译自身；产物再编译产物（二次自举验证） | **用 H 编译 H 达成** | K4 | 2h |
| K6 | 可复现构建 + 规范一致性（Rust/H 双实现交叉验证全语法/语义/内存/并发） | 一致性套件扩展绿 | K5 | 2h |

**当前执行范围合计 ≈ 73.5h，45 个任务**（A 4 / B 10 / C 3 / D 9.5 / E 7 / G 11 / H 6.5 / I 7.5 / J 7 / K1–4 8）。**延迟/留后续**：F（四模式/@atomic）11.5h 6 任务 → 1.x（ADR-0011）；H5（K6 freestanding）2h → 1.x（ADR-0014）；K5/K6（自举闭环 + 可复现）4h 2 任务 → 后续。

## 4. 验收与门禁

- **功能点级**：`cargo test` 相关套件绿 + 文档同步（本文件 §5 清单对应项）
- **组级**：示例回归基线——现 interpret **143/4/1** + compile **60 mismatch**；**E 组已落地**：37/38/39/76/80 的 `[test]` 异步断言双后端全绿（interpret 由 E1 前双解析失败转 143/4/1；4 项失败 = 四模式容器 37/76/77/78（组 F 延迟 1.x）——78 由解析错误转为运行时 `error.UndefinedName`，79 已随捕获语法解析落地转全绿）；**76–80（四模式）转绿信号归 1.x**（ADR-0011，本块不实现）；compile mismatch 随原生 ABI 扩展下降（Phase 8 原生函数值/闭包，K4 H 后端编写时联动；60 中 5 例（37/38/39/76/80）文件级 MISMATCH 来自 `main` 特性——四模式容器 37/76、**38/80（G1 net 已落地仍红：38 主函数旧 URL 形式 `connect(url)`/`read_all(&conn)` + `JsonValue` 类型未实现、80 主函数 `https://` 网络不可达，仅 `http://` 支持）**、Io.evented 39（interp-only E3）；78/79 捕获语法解析副作用 +2，见 `tag1/scripts/check-examples.sh` 注释）
- **第三块总验收**（07 §五）：**`用 H 编译 H` 达成（stage2）**；可复现构建（同源码同结果）；规范一致性（Rust/H 双实现交叉验证）——**留后续执行**（当前范围至 K4，K5/K6 不排程）

## 5. 文档同步清单

**计划同步（裁决落地时）**：

| 文件 | 同步内容 | 时机 |
|---|---|---|
| ✅ `docs/adr/0011–0014` | 四个前置裁决 ADR | **A 组已完成（2026-08-18）** |
| ✅ `06-language-spec.md` | comptime 类型即值 / script 块语法 | **A/B/D 组已完成（2026-08-18）** |
| ✅ `06-08-modules.md` | 供应链指纹 / 依赖来源审计 | **A3/B4 已完成（2026-08-18）**；I4 注册中心 MVP 待 |
| ✅ `06-10-concurrency.md` | 异步/四模式/原子（协作式衔接） | **E 组已完成（2026-08-18，组 E 注已补）**；F 组四模式/@atomic 延迟 1.x 已标注 |
| ⚠️ `04-stdlib-scope.md` | 标准库扩展明细（net/ipc/storage/text/ffi）+ 系统编程扩展 | G 组标准库已同步（2026-08-18）；H 组系统编程扩展待 |
| ⚠️ `02-milestones.md` | M9/M10 状态勾选 | M3/M5 已按 tag1 实现勾选（2026-08-18）；M9/M10 待自举（K 组） |
| `05-open-questions-and-risks.md` | 开放问题逐项关闭 | A/J 组（J 待） |
| ✅ `07-bootstrap-plan.md` | 实现状态表与测试基线更新 | **T1–T5 状态已标注（2026-08-18）** |
| ✅ `CONTEXT.md` | 术语（comptime/script/异步/原子/裸机） | **各组完成时已同步** |

---

## 6. 完成注记

### 组 B（E1.1 script 块，2026-08-18）

**已落地**：
- **B1/B3**：`script { }` 声明级解析（`Decl::Script` 携带 `close_end` = 块闭合 `}` 之后字节偏移，parser 从 `tokens[pos-1].span.end` 精确捕获）。`hc-tools/src/scriptgen.rs` 装载期展开：解析 → 求值首个 script 块 → 产物字符串**替换块文本区间** → 重解析，循环至无 script 块（上限 1000 轮防自引用死循环）；无 script 块时零开销快速路径。失败 = 编译错误（`diag::render` + 产物非字符串提示值种类）。
- **B2**：`types` 元数据对象（受限脚本模式注入）：`fields(name)` → `[["字段名","类型串"],...]`（class 字段 / enum 变体，经 `fmt_type_str` 渲染）；`all` → 可见类型清单；`type` → 当前类型名（顶层 = ""）。`types.type` 需 parser 放行关键字作点号字段名（`expect_name_or_keyword` 增 `KwType`）。
- **B4**：受限 H 核心子集 = 复用解释器 + `script_mode` 门控（io/alloc/stdout/stderr → `error.ScriptForbidden`）；依赖包 script **默认禁用**（装载器 `exec_decl_top` 跳过 `Decl::Script`），build.zon 信任声明归 I4。
- **B5**：三后端一致——展开在降级前完成（IR/字节码/native 对展开后 AST 无感知）；`run`/`run --ir`/`build`（native）/`check`/`test`/`errors` 全部走 `parse_with_scripts`。验证：interpret `hc run` 与 `hc run --ir` 同输出；native 产物（`hc build` + 运行）同输出。
- **B6**：示例 33/36 由 stub 转为真实生成（types.fields 驱动生成字段计数函数，测试断言联动）；81 修复展开回归（脚本块补字符串产物占位）。**计划标误**：34/35 属组 D（comptime），非本组。

**测试**：`hc-tools/tests/scriptgen.rs` 10 项（生成端到端 / types 元数据 / 多轮展开 / check / --ir / test 模式 / io·alloc 负例 / 非字符串产物）。`cargo test --workspace` 全绿；示例回归 143/4/1（基线 ≥125/≤11），compile 60 mismatch（基线 ≤60）。

### 组 C（E1.3 序列化定制，2026-08-18）

**已落地**：C1/C2——脚本从 `types.fields` 生成**校验 + to_json 样板**（String 非空 / i32 ≥ 0 / ?String null 守卫；JSON String 带引号 / i32 裸值 / ?String → `null`），示例 36 扩为「字段统计 + 校验 + 序列化」三合一；`hc-tools/tests/scriptgen.rs` 定制通道端到端测试（`hc run` 与 `hc run --ir` 一致）；schema 版本/迁移钩子设计定案（05 #4）。**已知边界**：生成的样板用 String 方法（`.concat`/`.len`）与 `fmt_int`——原生运行时 String 方法属 Phase 7 缺口，示例 interpret/IR 全绿、原生计入 compile mismatch（见 tag1/README.md）。

### 组 D（E1.2 comptime 完整，2026-08-18）

**已落地**（组 D 完结）：
- **D1**：comptime **类型函数**（`fn Pair(T: type) type { return struct { first: T, second: T }; }`）解析/语义/具体化——AST 增 `Expr::StructType` + `NamedLit.ty_args`；`hc::comptime`（`is_type_fn`/`concrete_name`/`subst`/`instantiate`）三后端共享引擎；interp/IR **惰性具体化登记**；类型函数体降级跳过（无运行时残留）。示例 34 三模式全绿。
- **D2**：`comptime { }` 块**装载期编译期求值**（`hc-tools/src/comptimegen.rs`，script 展开后、语义检查前）：受限 Interp（io/alloc/argv 禁用）求值块体，结果丢弃；失败 = 编译错误；块内可见完整 `types` 元数据。
- **D3**：嵌套/递归实例化——`map_type_apps` 深度遍历 + `instantiating` in-progress 守卫；`PairPair(i32)` / `LinkedList(T)` 自引用可用（Optional 默认 None 终止）。consistency +2。
- **D4**：`comptime_int`（惰性宽度整数 + 收窄点诊断 + `expect_eq` 折叠）/ `comptime_float`（惰性宽度浮点 + 折叠）最小切片 + **D4b anytype 完整语义**（调用点按实参具体类型实例化，返回 `anytype` 重求值体 return 类型，`(qname, 具体化键)` 惰性缓存）+ **D4c comptime 值函数**（参数含 `T: type` 的普通函数调用点编译期求值，`try_comptime_value_call` + 自递归守卫）。示例 35 三模式全绿。
- **D5**：三后端类型值表示——comptime 值函数**运行时调用点 IR 折叠**（`collect_value_fns` + `LowerCtx.value_fns` + `try_fold_comptime_value_call`/`eval_const_block` 常量求值，折叠成功发射 `Const`，无类型值/调用残留）；原生经共享 IR 继承折叠。

**测试**：comptime 单测逐切片扩展（hc 语义 + hc-tools 端到端，见组内注）；consistency 增 `d1_comptime_type_application_consistent` / `d35_comptime_array_type_fn_consistent` / `d3_nested_instantiation_consistent` / `d3_recursive_instantiation_consistent` / `d4b_anytype_concrete_consistent` / `d5_comptime_value_fn_consistent`（共 +6）。`cargo test --workspace` 全绿；门禁基线不变（interpret 143/4/1、compile 60 mismatch）。**已知边界**：内建泛型外层嵌套 `Vec(List(i32))` 仍退化裸名 `Vec`；comptime_int 无 bignum（i128 上限，偏离 ADR 任意精度）；值函数体引用类型参数值 → UndefinedName（最小切片）。

### 组 E（E2.3 异步，2026-08-18）

**已落地**：
- **E1**：`async fn` 解析/语义——`Future(R)` 返回类型（R = 声明返回类型含错误联合 `Future(!R)`）、任意函数可 `await`（Q19 无 async 传染）、`await` 解包取 R。
- **E2**：`await` ≡ `join()`——async fn 调用点返回**惰性** `Future` 值（体延迟到 await），复用组 G Thread 协作式机制（`make_future`/`future_run` 镜像 thread_run）；协作式取消（`cancel` → `error.Cancelled`）、`is_done` 状态转移、await 幂等缓存。
- **E3**：`Io.threaded()`/`Io.evented()` 单线程事件循环——构造器写 `runtime` 字段（默认 io = threaded）、`io.poll()` 排空根回收队列（未 join 线程运行到完成）；interp-only（原生构造器未实现）。
- **E4**：示例 37/38/39/76/80 的 `[test]` 异步断言双后端全绿（IR 侧 async fn 同步执行 + await 透传对齐纯函数）。

**测试**：hc-rt async.rs 直测 11（E2 7 + E3 4）；consistency 增 `e2_async_await_consistent` / `e4_async_pointer_capture_consistent`。`cargo test --workspace` 全绿；门禁基线不变（interpret 143/4/1、compile 60 mismatch——5 例文件级 MISMATCH 来自 `main` 特性：四模式容器 37/76、38/80（G1 net 已落地仍红：38 旧 URL 形式 + `JsonValue` 未实现、80 `https://` 不可达）、`Io.evented` 39 interp-only）。

### 组 G（E3 标准库扩展，2026-08-18）

**已落地**（G6 ffi 按用户决定跳过）：
- **G1 net**：UDP（`io.net.udp.bind(port)`/`bind(host, port)` + `send_to`/`recv_from`/`local_port`/`close`，Q20 双语；空队列 200ms 读超时 → `error.TimedOut`；`recv_from` 返回 2 元素数组）+ HTTP 客户端 `io.net.get(url)`（仅 `http://`，非 200 → `error.Http{code}`）+ HTTP 服务端（`io.net.listen`+`accept`+`read_all`/`write`）+ Q20 双语补齐（命名空间形式 ≡ 实例方法）。hc-rt net.rs 直测 6。
- **G2 io 差异项**：`io.stdout`/`io.stderr` 独立字节流；`String.to_upper`/`to_lower`（ASCII）；`io.fs.list_dir` → `Vec(DirEntry)`（`{name, is_dir}`，路径/句柄双形态）；`io.fs.open_dir(path) !Dir`（fd→路径注册表，`dir.list_dir`/`dir.close`）。hc-rt io.rs 直测 9→13。
- **G3 ipc**：`io.ipc.pipe() ![PipeReader, PipeWriter]`（匿名管道：`write`/`close`/`read`（排空不阻塞）/`read_all`/`is_closed`，close 幂等）+ `io.ipc.shm(name, size) !Shm`（定长共享内存：`write` 截断/`read`/`close`）。进程内注册表，跨 H 线程经 spawn 传值。hc-rt ipc.rs 直测 6。
- **G4 storage/archive**：`io.storage.open(path) !KvStore`（文件持久化键值：`put`/`get !?&[u8]`/`contains`/`remove`/`len`/`close` 落盘+注销幂等）+ `io.archive.compress`/`decompress`（RLE：token 0x00 字面跑 / 0x01 重复跑，round-trip 保真，非法 → `error.InvalidFormat`）。hc-rt storage.rs 直测 7。
- **G5 text/time/rng**：`io.text.*` 正则（字面量/`.`/`[...]` 范围与取反/`\d` `\w` `\s`/分组/`*` `+` `?` `{n,m}`/`|`/`^` `$`/`\n` `\t` `\r` `\xNN` 及转义；记忆化集合回溯无灾难性回溯；非法 → `error.InvalidFormat`）+ `io.time.tick`/`elapsed` 单调测量 + `io.rng.seed`/`next`（xorshift64*）/`int`（拒绝采样）/`float`（高 53 位；命名空间类名 `RngNs` 避开示例用户类）。hc-rt text_rng.rs 直测 10。

**测试**：hc-rt 标准库直测合计 42（net 6 + io 13 + ipc 6 + storage 7 + text_rng 10）；IR 后端同步 G1-G5（Q20 双语，消除 interp-only 私语义——`hc::comptime` 提取正则/RLE/xorshift64 纯函数共享层）；consistency 增 g1–g5 共 11 用例（79→90）。`cargo test --workspace` 全绿；门禁基线不变（interpret 143/4/1、compile 60 mismatch）。
