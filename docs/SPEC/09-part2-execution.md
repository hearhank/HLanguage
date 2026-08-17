# 第二部分执行细表（第二块最小外围 + 调整项）

> 状态：**定案**（2026-08-17，评审 Q1–Q17）。源：`07-bootstrap-plan.md` §三（第二块最小外围 M5–M7）。关联：`02-milestones.md`（引用）、`06-08-modules.md`（import 定案）、`04-stdlib-scope.md`（io 模块形态）、`docs/adr/0010-entry-and-import.md`（入口与导入 ADR）、`CONTEXT.md`（术语）。执行规则：所有修改同步更新到 SPEC。

## 0. 定案摘要（2026-08-17）

### 0.1 范围与规则

- **范围**：第二块最小外围（M5–M7）从 tag1 垂直切片到**完整交付** + 调整项（io 入口/导入、线程、代码管理组）
- **优先级**：依赖骨架为硬约束，同层内按价值排序（验收相关性优先）
- **功能点定义**：可独立验收的**行为面**；每个功能点 = 实现 + 测试 + 文档同步 + `cargo test` 验证（AI 辅助，参照 tag1 梯队节奏）；**每功能点 ≤ 2h，超出即分解**
- **第二部分验收**（沿用 07 §三）：`hc build`/`hc run`/`hc test` 完整可用；示例套件双模式一致；测试全绿

### 0.2 设计定案（评审结论）

| # | 定案 | 出处 |
|---|---|---|
| 1 | **入口与导入**：`fn main(args: o Vec<String>) !void`——main 不再注入 io；命令行参数仅经入口 args 注入（**`io.args()` 取消**）；`import pkg.mod.{sym as 别名}` 文件级导入**取代 `using`**（推翻 06-08「无文件级 import」），导入对象 = 模块（`[module]` 标注的命名空间/包）；`H.std` = 标准库根路径；`Io` 接口**保留**（并发 E2 的 `Io.threaded()/evented()` 仍是设计）；`test_io` **取消**，测试直接调 `main()` | Q9/Q13/Q14/F1，ADR-0010 |
| 2 | **io = 标准库模块**：函数直接调用（`my.print(...)`）；模块内环境状态（env/exit/fs/net/time，**args 经入口 `main(args)` 注入，`io.args()` 取消**）；非对象注入 | Q13/F1 |
| 3 | **库符号访问规则**：库函数可直接调用；库类型需创建（`alloc.init(T)` 堆上 / 值字面量栈上）；**值类型栈上分配，经 `alloc` 堆上分配** | Q9 |
| 4 | **扩展类型暂留语言内建**（String/集合族/Table），后期再评估分离标准库 | Q10 |
| 5 | **线程进第二部分**：仅 E2.2 线程生命周期（`spawn(f, args...) o Thread(T)` / `join() !T` / `cancel()` / `is_done()` / `detach()` + 02 E2.2 捕获规则 + 每线程 alloc）；四模式/async/@atomic/mutex 留第三块 | Q10/Q15 |
| 6 | **代码管理组**（第二块扩展，排最后）：① 项目代码结构 ② 代码引用库 ③ 模块(domain) ④ 文档自动生成 | Q11/Q12 |
| 7 | **模块(domain)** = `[module]` 特性标注的命名空间（F2 定案）——内容与其它命名空间**隔离**；需要其它库的数据经**上下文（init 参数列表）**初始化注入；第二部分以工程约定落地，模块实例化语法归第三块 E6 候选 | Q11/Q16/F2 |
| 8 | **文档自动生成** = `hc doc` 输出 **Markdown**（标准库 + 用户项目，`///` 注释 + 声明签名）；HTML 归第三块 E5.1 | Q17 |
| 9 | **命名规范**（Q22 定案）：类型与命名空间 `PascalCase`（缩写词全大写 `HTTPRequest`/`TCPSocket`）；标识符（变量/函数/方法/字段/参数）`snake_case`；常量 `SCREAMING_SNAKE`；既有内建类型名（Vec/Map/Deque/String/Table/Io/ExitType）与标准库模块名（io/fs/net…）维持现状——见 `01-language-design.md` §10、CONTEXT 命名规范 | Q22 |
| 10 | **应用与库形态**（Q23/Q24 定案）：应用 = 含 `main` 的包（`Kind::exe`，产出 exe）；库 = **无 main** 的包（`Kind::lib`，1+ 模块，产出 **lib 静态库**（编译时链接）或 **dll 动态库**（exe 运行时加载），构建参数选择）；分层 = 包（应用/库）→ 模块（`[module]`）→ 命名空间；模块间：`import` = 符号引用（类型/函数），**上下文（init 参数）= 数据/依赖注入**——两者正交 | Q23/Q24 |

## 1. 描述充分性审查表（逐模块判定）

> 判定标准：模块描述是否足以直接实现且做出 ≤2h 估算；不充分处本文件补齐描述（§2.1）。

| 模块 | 判定 | 依据 | 缺口 / 动作 |
|---|---|---|---|
| M5.1 mem | ✅ 充分 | `08-mem-allocator-design.md` 定稿（方法集/失败语义/对齐/归属规则） | 缺口 E：`arena.init(T)` typed 构造（tag1 为 Void 占位） |
| M5.2 collections | ✅ 充分（最小集边界明确） | 07 方法集已列（append/len/get/put/remove/迭代）；实现一致 | 差异项 `String.to_upper` 归第三块（见 §2.2） |
| M5.3 serialize 库 | ❌ **不充分** | 描述仅「解析辅助、格式辅助」两词；`fmt_int`/`fmt_float` 语义层承认、运行时零实现；无库封装形态 | 补齐描述 §2.1 + 执行组 D |
| M5.4 io | ⚠️ 需重构描述 | 方法清单具体，但形态被 Q13/Q14 推翻（接口对象 → 标准库模块函数） | 以新形态重写（04 同步）；格式串/环境项见组 B/A |
| M5.5 时间/调试 | ✅ 基本充分 | now/sleep/sort/binary_search/parse 已实现；「debug 断言」= 断言五件套（04 Q-T1，测试内隐式可用） | 澄清表述（无 debug 命名空间，不立项） |
| M6.1 测试 | ✅ 充分 | 五件套/[PASS]/[FAIL]/[SKIP]/非零退出/注入/双模式交叉已实现 | 测试空白：组 F（SKIP 分支/io.exit/基建自测/直测） |
| M7.1 命令 | ⚠️ 一处缺口 | build/test/check 已实现；`hc run` 仅单文件，缺目录/包形态 | 组 C |
| M7.2 build.zon | ✅ 充分 | 已实现（清单/pub 边界/本地依赖）；指纹/注册中心归第三块 | 引用库完善归组 H2 |

## 2. 缺口清单（盘点 2026-08-17）

### 2.1 硬缺口 + 描述补定

1. **M5.3 serialize 库（描述补定）**——`serialize` 为命名空间形态（库封装，非内建）：内建序列化（M4.4）之上的**格式辅助** `fmt_int(i32) String` / `fmt_float(f64) String`（含宽度/精度参数形态待定，对齐 04「待定归属」清单）+ **解析辅助组**（parse_int/parse_float/json.parse/csv.parse/parse_number/skip_space/peek/advance/is_digit/expect——已实现，组织为库并补测试）。`fmt_int`/`fmt_float` 运行时零实现 → 组 D1。
2. **M7.1 `hc run` 目录/包形态**——`hc run <目录>`：包加载（入口 `main.hc` 或首个 `.hc`）+ 兄弟文件合并 + build.zon 依赖装载（复用 `load_deps`）→ 组 C。
3. **io.print 格式串静默输出**——`{d}`/`{X}`/宽度/精度/对齐未实现且**被当字面量静默输出（无诊断）** → 组 B。
4. **arena.init(T) typed 构造**——08 设计 §4.1/§4.2 已定（bump + 字段默认值填充，与 `alloc.init(T)` 同一构造逻辑），tag1 返回 Void 占位 → 组 E。
5. **库产出形态未实现**——`Kind::lib` 已解析（buildzon）但 `hc build` 无库产出；库无 main 校验未显式 → 组 C3/C4。

### 2.2 文档差异项（标注归口第三块，**不混入第二部分执行**）

| 差异项 | 04 文档 | tag1 现状 | 归口 |
|---|---|---|---|
| `io.stdout`/`io.stderr` 独立字节流 | read_all/write_all | 与 io 同实例 | 第三块 E3 |
| `io.fs.list_dir` → `Vec(DirEntry{name,is_dir})` | Vec(DirEntry) | 字符串数组 | 第三块 E3 |
| `String.to_upper` | API 清单 | 未实现 | 第三块 E3 |
| `io.net.get` / UDP | API 清单 | 未实现 | 第三块 E3.1 |
| `io.fs.open_dir`/`Dir` | API 清单 | 未实现 | 第三块 E3 |
| `io.args()` | 04 API（0 号 = 程序名） | tag1 skip(1) | **取消**（F1 定案：命令行参数仅经入口 `main(args)` 注入） |

> 注：`Kind::script`（脚本包）与入口（main?）的关系未定义——`hc run` 单文件脚本已实现，脚本包形态随组 A 入口定案后明确（非第二部分阻塞，标注于此）。

### 2.3 测试空白

- ~~`[SKIP]` 分支（`error.SkipTest`）零覆盖（23-tests.hc 中为注释）→ F1~~ ✅ F1 已完成（2026-08-17，见 §3 组 F 注）
- ~~`io.exit`/`ExitType` 零覆盖 → F2~~ ✅ F2 已完成（2026-08-17，见 §3 组 F 注）
- ~~测试基建自身（退出码/注入/汇总）无独立测试文件 → F3~~ ✅ F3 已完成（2026-08-17，见 §3 组 F 注）
- fs 余项（append/rename/remove/list_dir/read_int/write_int）、`io.stdin`、`parse_int/parse_float` 无直测 → F4

## 3. 执行序与 ≤2h 任务分解（A–H）

> 功能点 = 行为面；验收列 = 该行为面的完成定义（实现 + 测试 + 文档同步 + `cargo test` 验证计入 2h）。组内序号即执行序；组间依赖见各组合注。

### A. io 入口与导入调整（依赖：无；**最先执行**——波及全部示例）

> 语言层变更，ADR-0010。完成标志：`main(args)` + `import` 全量生效，示例基线重设。

| # | 任务（行为面） | 验收 | 依赖 | 预估 |
|---|---|---|---|---|
| A1 | `import` 语句词法与解析：`import pkg.mod.{sym as 别名}` 多符号选择 + `import pkg.mod;` 整模块 + `H.std` 路径；lexer/parser/AST 新顶层声明 | parse 单测绿；`study.hc` 可解析 | — | 1.5h |
| A2a | `import` 语义与可见性：符号登记（前缀 + 别名）、冲突规则（显式优先通配，沿 06-08 既有规则）；`using` 迁移（旧语法诊断提示或兼容） | semantic 测试绿（import 用例） | A1 | 1.5h |
| A2b | 模块识别与命名规范检查：`[module]` 标注命名空间 = 模块、隔离检查、上下文 init 签名；类型/命名空间 PascalCase 编译期诊断（与接口 `I` 前缀同机制；缩写全大写归第三块 lint） | semantic 测试绿（模块/命名规范用例） | A2a | 1h |
| A3 | 入口 `main(args: o Vec<String>) !void`：运行时 args 注入（0 号 = 程序名）；**`io.args()` 取消**；main 不再注入 io；run_main/run_tests 调整 | 01-hello 等入口示例绿；args 测试绿 | A2a/A2b | 2h |
| A4 | 入口三后端对齐：IR/native（`emit_main_wrapper`/`@__init__` 前置）+ `hc run --ir`/字节码 + 一致性套件入口用例 | consistency 绿；native 端到端绿 | A3 | 2h |
| A5a | interp io 模块函数化：`io.*` 从对象方法 → 模块函数/子模块函数形态（print/fs/net/time/env/exit；args 经入口注入）；环境状态模块内管理；`Io` 接口保留（不再注入） | io 测试绿（interp 改造后形态） | A2a/A2b | 1.5h |
| A5b | IR/native io 调用路径同步：`call_builtin`/llvm.rs io 分支对齐模块函数形态 + 一致性 io 用例 | consistency + native 绿 | A5a | 1.5h |
| A6a | 示例 main 签名迁移 + `test_io` 取消：136 个示例入口改 `main(args)` | 示例可解析/运行（签名形态） | A3/A4 | 1.5h |
| A6b | `using`→`import` 迁移 + 基线回归重设 | `check-examples.sh` 通过（新基线） | A6a/A5b | 1.5h |

### B. io.print 格式串（依赖 A5）

| # | 任务（行为面） | 验收 | 依赖 | 预估 |
|---|---|---|---|---|
| B1 | 格式说明符补齐（interp）：`{d}`/`{X}`/`{e}`/宽度/精度/对齐（05-format.hc 清单） | 05-format 示例输出正确 | A5 | 2h |
| B2 | 静默输出修复：非法/未实现说明符 → 编译期诊断（comptime 已知格式串）或运行期错误（动态串），不再按字面量输出 | 负例测试绿 | B1 | 1h |
| B3 | IR/原生对齐：`parse_print_fmt` 同步（llvm.rs）+ 一致性套件格式用例 | consistency + native 绿 | B1/B2 | 1.5h |

### C. 包形态：目录运行与库产出（依赖 A3）

| # | 任务（行为面） | 验收 | 依赖 | 预估 |
|---|---|---|---|---|
| C1 | `hc run <目录>`：包加载（入口 `main.hc` 或首个 `.hc`）+ 兄弟文件合并 + build.zon 依赖装载（复用 `load_deps`） | 02-packages 目录可 run；CLI 测试绿 | A3 | 2h |
| C2 | 文档：入口/目录运行约定（06-08 同步）+ hc-tools 单测 | 文档 + 测试绿 | C1 | 1h |
| C3 | `hc build` 库形态（`Kind::lib` → **lib 静态归档**）：zig cc 静态归档产出 + exe 链接本地库端到端（02-packages 改造为链接形态） | native 测试绿（库产出 + exe 链接运行） | C1 | 2h |
| C4 | `hc build` 库形态（`Kind::lib` → **dll 动态库**，构建参数选择）：zig cc `-shared` 产出 dll + exe 运行时加载端到端；**库无 main 校验**（`Kind::lib` 含 main → 诊断） | native 测试绿（dll 加载运行 + 无 main 诊断） | C3 | 2h |

> ✅ **C1/C2 已完成（2026-08-17，梯队 33）**：`hc run <目录>` 目录包运行（package_entry 入口解析）+ 02-packages 示例 import 迁移 + cli.rs 3 测试；06-08 目录运行约定已同步。

> ✅ **C3 已完成（2026-08-17，梯队 34）**：`hc build` 库形态（`Kind::lib` → **lib 静态归档**）——`build_lib`（codegen_lib 包前缀 + runtime helper 转 declare + `zig cc -c` → `.o` → `zig ar rcs lib{name}.a` + `.sym` 符号表，剔除 test 函数）；exe 链接本地依赖库端到端（`check_and_merge_deps` 依赖 pub 登记 + IR 文件级 import 展开表 `collect_imports` + `codegen_with_links` 外部链接符号路由 + 模块级 ext_decls 去重声明）。02-packages 改造为链接形态；cli.rs 新增 `build_lib_static_archive_and_link_exe` 测试。**已知限制**：库全局变量链接留后续（`@.h_globals` 跨 .o 撞符号）；`hc run --ir` 跨包调用仍 NoFunction（IR 参考解释器不装载依赖）。

> ✅ **C4 已完成（2026-08-17，梯队 35）**：`hc build --dll` 库形态（`Kind::lib` → **dll 动态库**）——`build_lib` 加 `dll` 分支（`codegen_lib` dll_mode **自包含** helper + `zig cc -shared` → `{name}.dll`）；exe 依赖按 dll 构建并**链接 dll**（OS 运行时加载，dll 复制到 exe 目录供加载器定位）；**库无 main 校验**（`Kind::lib` 含 `main` → 诊断，06-08 定案）。cli.rs 新增 `build_lib_dll_and_runtime_load` + `build_lib_with_main_is_diagnosed` 测试。**已知限制**：库全局变量链接留后续（同 C3）；dll 模式的 `--dll` 为构建参数选择（构建参数形态第三块再评估）。

### D. M5.3 serialize 库（依赖：无特殊）

| # | 任务（行为面） | 验收 | 依赖 | 预估 |
|---|---|---|---|---|
| D1 | `fmt_int`/`fmt_float` 实现（interp + IR `call_builtin`；语义层已承认） | 63-template-render 场景测试绿（从 E1 失败池移出） | — | 2h |
| D2 | `serialize` 库封装：解析辅助组（parse_int/parse_float/json.parse/csv.parse/parse_number/skip_space/peek/advance/is_digit/expect）组织为 `serialize` 命名空间 + 直测补全 | serialize 测试绿 | D1 | 2h |
| D3 | 原生侧：`fmt_int`/`fmt_float` 原生 codegen（若 compile 门禁需要） | native 测试绿 | D1 | 2h |

> ✅ **D1 已完成（2026-08-17）**：`fmt_int`/`fmt_float` 落地 interp + IR `call_builtin`（含 `is_free_builtin` 登记 → bytecode 路径复用）。interp：`Value::Int(i)` → `i.to_string()`；`fmt_float` 接受 Float/Int（Int 转 f64），display 语义与 IR 一致（整数值 → `{:.1}`，否则 `to_string()`）。`ex63_template_render` 加入 examples.rs（D1 前失败于 `fmt_int` 缺失，现绿）。
>
> ✅ **D2 已完成（2026-08-17）**：解析辅助组（parse_int/parse_float/json.parse/csv.parse/parse_number/skip_space/peek/advance/is_digit/expect）组织为 `serialize` 命名空间。interp：eval_call Field 块 + Dot 臂按 `serialize.` 前缀路由到 `call_serialize_builtin`（json.parse/csv.parse 复用虚拟根逻辑，其余助手走 `call_builtin` + Option 解包）；IR：`is_dotted_implicit_root` 增 `serialize`、`call_dotted_implicit` 前置 `serialize.` 剥离 → `call_serialize_builtin_ir`（同形）。直测：serialize.rs 增 `serialize_parse_int_float`/`serialize_json_csv_parse`/`serialize_parser_helpers`，consistency.rs 增 `p7_serialize_namespace`。**顺带修复两个一致性套件暴露的既有 IR bug**：① `orelse` 非空分支现发 `IrInst::Unwrap` 取载荷（此前直存 `a` 导致 `Opt` 泄漏）；② `parser_pos` 不再 `deref_value`（此前追 Ptr 到 pointee 使 `&pos` 的 Ptr 匹配失败 → "expected pointer"，对齐 oracle interp get_pos）。serialize 测试 + 全 workspace 测试绿。
>
> ✅ **D3 已完成（2026-08-17）**：原生 `hc_fmt_int`/`hc_fmt_float` LLVM helper（emit_scalar_builtin_helpers 注册；`fmt_int` 为 i128→十进制→`hc_alloc` 堆串 tag5，带符号处理；`fmt_float` 用 `sprintf`（`%.1f` 整数值 / `%.15g` 小数），接受 Int 输入 `sitofp`），llvm 单测 + native.rs `fmt_int_float_native` 绿。**已知限制**：63-template-render 的 compile 模式仍 mismatch——阻塞来自 `String.from`/`String.replace`/`String.find` 原生未实现（预先存在的原生子集缺口，非 D3 范围；compile 门禁预算内）。

### E. arena.init(T) typed 构造（依赖：M5.1 已落地）

| # | 任务（行为面） | 验收 | 依赖 | 预估 |
|---|---|---|---|---|
| E1 | `arena.init(T)` 实现（interp）：bump 分配 + 字段默认值填充，对齐 `alloc.init(T)` 双形态；替换 Void 占位 | arena.rs 测试绿（typed 构造用例） | — | 1.5h |
| E2 | IR 对齐 + 一致性 + 08 文档同步 | consistency 绿 | E1 | 1.5h |

> ✅ **E1 已完成（2026-08-17）**：`call_arena_method("init")` 替换 Void 占位——`arena.init(T)` 类型名形态按类型建空实例（class 字段逐默认值 / enum 空变体，未知类型 `UnknownType`）+ 按 `type_size_of` bump 记账（堆上 class = 指针宽 8，连续 class = 布局总大小）；`arena.init(T{...})` 字面量形态求值即实例 + 按实例类型 bump。deinit 后 init → `ArenaDeinitialized`，OOM → `error.OutOfMemory`（对齐 alloc 规则）。arena.rs 新增 5 直测：typed_default / typed_literal（含二次 bump 对齐填充）/ continuous_size / after_deinit_errors / unknown_type_errors。全 workspace 绿。
>
> ✅ **E2 已完成（2026-08-17）**：IR 对齐——降级阶段 `alloc.init` 分支扩为 `alloc.init`/`arena.init`（已知 class → `lower_alloc_init_defaults` 默认字段 MakeClass），`is_type_arg_pos` 登记 `arena.init`；运行期 `call_arena_method_ir("init")` 双形态构造（Str 类型名 → 空 class 实例 / Class 字面量 → 原样返回）+ `bump(8)` 记账（堆上 class = 指针宽；连续 class IR 无布局表也按 8，与 alloc.init IR 同源简化）。顺带清理 D2 遗留 `parser_pos` 未用参数告警。一致性：consistency.rs 增 `e1_arena_init_typed`（类型名 + 字面量 + bytes 记账双模式一致）与 `e2_arena_init_after_deinit_fails_both`（deinit 后 init 双模式一致失败）。08 设计文档 G1 行同步 typed 构造落地。全 workspace 绿（consistency 68）。

### F. 测试空白补全（依赖：F2/F4 依赖 A5b——io 模块函数化后形态）

| # | 任务（行为面） | 验收 | 依赖 | 预估 |
|---|---|---|---|---|
| F1 | `[SKIP]` 分支触发测试（`error.SkipTest`）+ 23-tests.hc 启用 | SKIP 测试绿 | — | 0.5h |
| F2 | `io.exit`/`ExitType` 测试（Exit 静默/Error 打印/退出码） | exit 测试绿 | A5b | 0.5h |
| F3 | 测试基建自测：独立测试文件（收集/退出码/注入/汇总） | 基建测试绿 | — | 1h |
| F4 | 直测补全：fs 余项（append/rename/remove/list_dir/read_int/write_int）+ `io.stdin` + parse_int/parse_float | io.rs/parse 测试绿 | A5b | 1.5h |

> ✅ **F1 已完成（2026-08-17）**：`[SKIP]` 分支触发——23-tests.hc 的 `skip_example` 自注释恢复为实际 `return error.SkipTest;`。interp `run_tests` 值通道在通用 FAIL 臂前增 `Ok(Value::Err{name:"SkipTest"})` → 统计 SKIP（输出 `[SKIP]`，不计 passed/failed）；原生跑器 `emit_test_runner` 传入 `ErrorCodeTable`，`error.SkipTest` 经「错误码载荷 == SkipTest 码」识别 → 打印 `[SKIP]` 续跑下一测试（其余错误仍 `hc_abort_unhandled` abort），并修正跑器为按实际 func 索引命名标签（`@__init__` 穿插时原 `t0`/`t{i+1}` 假设失效）。回归：ex23_tests 断言 `s>=1`；interp 汇总 125 passed/1 skipped、compile 23-tests.hc 仍 MATCH、门禁 53 mismatch 不变。
>
> ✅ **F2 已完成（2026-08-17）**：`io.exit`/`ExitType` 测试（Exit 静默/Error 打印/退出码）。CLI 端到端（cli.rs 4 项）：Exit code0 静默成功且 exit 后代码不执行、Exit code5 进程退出码 5 静默、Error code3 打印 `error: program exited with code 3` + 进程退出码 3、`--ir` 同语义。interp 直测（errors.rs 3 项）：Exit 非零码 exit_code 记录、少参 ArityMismatch。**顺带修复 IR 侧退出码丢失**：`IrRuntime.ctx` 增 `exit_code` 字段，`call_io_method_ir("exit")` 记录请求码；`IrRunOutcome` 增 `Exited(u8)`，`execute_ir`/`ir_exit` 映射进程退出码（此前 `hc run --ir` 打印错误消息却恒退出 0，违背四后端同语义）。main.rs 单测 `ir_io_exit_maps_code` 锁定。
>
> ✅ **F3 已完成（2026-08-17）**：测试基建自测——新增 `hc-tools/tests/harness.rs`（5 项，独立测试文件驱动 `hc` 二进制）：①收集：仅 `[test]` fn 运行（普通 fn 不收集）、目录内多文件各自跑且汇总合并；②退出码：全过 → 0、有 FAIL → 1、解析失败文件 → 1（stderr `[FAIL]`）；③注入（Q-T4）：测试 fn 隐式可用 `io`/`alloc`（无参声明 `io.print`/`alloc.alloc` + `alloc.free`）；④汇总：逐项 `[PASS]/[FAIL]/[SKIP]` 行 + `N passed, M failed, K skipped`。全 workspace 575 项测试绿（README 测试表刷新至当前明细）。

### G. 线程生命周期（依赖：M5 运行时 + 每线程 alloc（Q8）；捕获规则 02 E2.2 既有定义）

> 仅 E2.2 线程生命周期；四模式/async/@atomic/mutex 留第三块（02 与 07 同步标注）。

| # | 任务（行为面） | 验收 | 依赖 | 预估 |
|---|---|---|---|---|
| G1 | `spawn(f, args...) o Thread(T)` 内建（interp）：每线程 alloc 实例（Q8）；线程栈与作用域根提升 | 线程测试绿（基本 spawn/join） | A3 | 2h |
| G2 | `join() !T`（返回值传递）/ `cancel()`（协作式）/ `is_done()` / `detach()` | 线程生命周期测试绿 | G1 | 2h |
| G3 | 捕获规则与所有权：值复制/move/global + Q18 绑定例外 + Q19 冻结窗口（02 E2.2） | 捕获规则测试绿 | G1 | 2h |
| G4a | 三后端对齐 I：IR 线程指令 + 字节码 opcode 扩展 | ir/bytecode 测试绿 | G1/G2/G3 | 1.5h |
| G4b | 三后端对齐 II：原生 codegen + 一致性套件线程用例 | consistency + native 绿 | G4a | 1.5h |
| G5 | 示例 + 文档：37/76–80 线程示例迁移入列或新增最小示例；CONTEXT/SPEC 同步 | 示例回归绿 | G4b | 1.5h |

### H. 代码管理四项（依赖：A（import）之后；工具链性质，排最后）

| # | 任务（行为面） | 验收 | 依赖 | 预估 |
|---|---|---|---|---|
| H1 | 项目代码结构：`hc init <name>` 脚手架（目录骨架：`main.hc`/`build.zon`/源码与测试目录约定）+ 约定文档 | CLI 测试 + 脚手架示例绿 | A6 | 2h |
| H2 | 代码引用库：build.zon 依赖引用完善（本地路径引用、版本声明、`hc pkg add` 本地形态、缺失依赖诊断） | dep 测试绿 | A6 | 2h |
| H3 | 模块(domain)约定（**不含语法实现——已含于 A2b**）：`[module]` 标注 + 边界（owns 数据/对外 pub API）+ 上下文（init 参数列表）约定文档 + 示例（如 orders 域） | 约定文档 + 示例绿 | A6 | 1.5h |
| H4 | `hc doc` 生成（Markdown）：`///` 注释 + 声明签名收集（标准库 + 用户项目）；输出目录约定 | doc 生成测试绿（标准库页 + 项目页） | A6 | 2h |
| H5 | `hc doc` 细化：索引/链接/格式回归 | doc 测试绿 | H4 | 1.5h |

**预估合计 ≈ 57.5h，36 个任务**（A 14 / B 4.5 / C 7 / D 6 / E 3 / F 3.5 / G 10.5 / H 9）。

## 4. 验收与门禁

- **功能点级**：`cargo test` 相关套件绿 + 文档同步（本文件 §5 清单对应项）
- **组级**：示例回归基线——interpret ≥125/136、compile ≤52 mismatch（组 A 完成后**基线重设**，因 136 个示例入口签名全部变更）
- **第二部分总验收**（07 §三）：`hc build`/`hc run`/`hc test` 完整可用；示例套件双模式一致；测试全绿

## 5. 文档同步清单

**已同步（2026-08-17，评审产出）**：

| 文件 | 同步内容 |
|---|---|
| `01-language-design.md` | 命名规范（§10/§12.1）；§12.18 io 模块形态 + 入口修订 |
| `06-04-functions.md` | 入口签名示例（`main(args)`）；环境经 import |
| `06-language-spec.md` | 程序环境（`io.args()` 取消） |
| `06-08-modules.md` | import 定案 + `[module]` 模块 + 包形态分层（应用/库）+ 库无 main + Q24 分工 |
| `04-stdlib-scope.md` | io 模块形态；test_io / io.args() 取消 |
| `02-milestones.md` | 引用 09；线程提前；术语一致（「第二部分要求」→第三块） |
| `07-bootstrap-plan.md` | 引用 09；M5.4/M7.1 修正；L122 笔误；线程/库形态标注 |
| `docs/adr/0010-entry-and-import.md` | 入口与导入 ADR（含 F1 args / F2 模块决策） |
| `CONTEXT.md` | import / 模块 / 程序环境 / 测试环境 + 命名规范 + 应用程序 / 库 / 包形态 |

**待同步（组级完成时）**：

| 文件 | 同步内容 | 时机 |
|---|---|---|
| `08-mem-allocator-design.md` | arena.init(T) 状态勾选 | 组 E 完成时 |
| `07-bootstrap-plan.md` | 实现状态表与测试基线更新 | 各组完成时 |
