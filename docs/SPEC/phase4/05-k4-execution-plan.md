# K4 执行计划：H 版执行引擎（AST 树遍历解释器）+ 对照

> 来源：`01-bootstrap-plan.md` K4 任务细化（2026-08-29）。
> 执行规则：**每个任务 ≤1h**；完成即测试验证 → 更新本文档进度 → 提交 → `node .gitnexus/run.cjs analyze --index-only` 索引 → 下一任务。
> 前置状态：K1 ✅ K2 ✅ K3 ✅；P1.5 自检崩溃已修复（`04-execution-status.md`，提交 `060aad2`）。
> **目录约定（用户裁定）**：K4 相关代码与 `.hc` 测试/探针/语料文件一律写 `stage1/` 下（探针 `stage1/probes/`、执行语料 `stage1/exec-corpus/`、对照语料 `stage1/corpus/`）；Rust 测试 harness（`k4_interp.rs`）仍在 `tag1/hc-tools/tests/`。

## 目标与验收

- **K4 目标**：H 语言编写的执行引擎（`stage1/interp.hc`），可执行 H 程序（语料切片），执行结果与 Rust 参考一致。
- **验收标准**（对齐 `01-bootstrap-plan.md` K4「H 后端可跑 + 执行结果对照绿」）：
  1. `hc run stage1/interp.hc <prog.hc>` 对全部执行语料（组 B 定义）输出与 `hc run <prog.hc>`（Rust tree-walking 参考一致的 stdout；
  2. 对照以 Rust 集成测试固化（`tag1/hc-tools/tests/k4_interp.rs`），进 `cargo test --workspace` 门禁；
  3. `interp.hc` 自身可被 checker.hc 检查（吃狗粮不崩）。

## 设计裁决：后端形态

| 方案 | 内容 | 结论 |
|------|------|------|
| A：IR 文本协议（原文计划「IR 参考解释器」字面执行） | Rust 加 IrModule→JSON 序列化 + H 侧 JSON 解析 + H 版 IR dispatch | **弃（本轮）**：`ir/json.rs` 仅有解析无序列化，无 IR dump 命令；需新建两层序列化链路，任务量放大且与 K1–K3「H 程序自包含读源码」模式不符 |
| B：AST 树遍历解释器 | `interp.hc` 内嵌复用 stage1 lexer/parser/checker，直接遍历 AST 求值 | **采纳**：复用 K1–K3 全部资产；与 Rust 四后端中 tree-walking 解释器同构；对照纪律（与 Rust 执行结果 diff）承担语义一致职责；K5 自举闭环以本引擎为 stage2 后端 |

**影响与债务登记**：方案 B 的 H 管线无独立 IR 层，「IR 唯一语义源」约束由「执行结果对照绿」等价承担；若 K5/K6 验收要求产物形态（.hbc/IR），届时补 H 版 lower 为独立任务组（不阻塞本轮）。

## 前置事实（2026-08-29 探针实证）

- checker.hc 的 `parse_method`/`parse_field` **建 AST 后全部丢弃**（代码实证）；`check_decl` 无 Class 分支。
- 探针（`/tmp/probe`）：类含方法 + main 使用该类 → `undefined name '字段'` 误报（pa2 有方法报错、pa3 无方法同 main 不报错、pa5 有方法但 main 不用不报错）；`+=` 非触发因素（pa4 排除）。
- checker.hc 自检 lexer.hc/parser.hc/自身分别 690/1616/2387 行误报（top：`self`×430、方法参数、`Self` 类型、部分顶层函数 `hexval`/`utf8_width` 被报未定义而 `is_digit` 等正常 —— 注册异常需 AST dump 定位）。
- **机制疑点已决（A0 dump 实证，2026-08-29）**：
  1. **类体失步**：`parse_class` 丢弃字段/方法后 token 流未正确收束 —— lexer.hc 的 4 个 class 全变成空 `UnknownDecl`；最坏情况（pc.hc：仅字段无方法）连后续 `fn main` 都被吞掉；其余情况泄漏 token 被后续顶层 fn 解析重收，方法体嵌进其他函数的 if 分支（lexer.hc：run() 体挂进 `is_ident_cont`，AST 深度 366，内含 469 个 `self` Ident → 自检 `self`×430 误报的直接来源）。
  2. **ClassLit 未实现**：`L{pos=0}` / `Counter{n=1}` 的 `{...}` 被当块语句解析，字段名泄漏成外层作用域的 `ExprStmt(Ident pos)` → `undefined name 'pos'/'n'` 误报的直接来源（pa2/cls 探针）。
  3. else-if 链本身健康（无类探针 5 分支正常嵌套）；失控嵌套是类体失步的次生灾害。
  4. **A4 范围修正**：ClassLit 需在表达式位置新增解析（`Type{field=val,...}`），而非仅修检查逻辑。

## 任务分解

### 组 A：K3.5 checker 误报收敛（K4 硬前置：interp.hc 内嵌 checker，方法/类执行依赖其建模）

| # | 任务（行为面） | 验收 | 预估 |
|---|---|---|---|
| A0 | **AST dump 调试工具**：checker.hc 加 `--dump-ast` 分支（main 判 args），缩进文本打印 kind+props+children 递归 | dump pa2/pc 差异可见，定位 `pos` 误报的真实解析路径 | ≤1h |
| A1 | `parse_field` 建节点：kind=`FieldDecl`，props `name`/`mut`，类型名入 children，`node_add_child` 入 cls；逗号/分号分隔容错 | dump 可见字段节点；K3 语料对照全绿 | ≤1h |
| A2 | `parse_method` 建节点：复用 finish_fn_decl 形态（kind=`Fn` + prop `method`=`类名`），参数/体入 children | dump 可见方法节点；pa2 探针方法体进入 AST | ≤1h |
| A3 | `check_decl`/`collect_program` 增加 Class 分支：collect 注册类名 + 字段集（类→字段名表）；check 阶段对每个方法注册 `self`（ty=`unknown`）后走 `check_fn` | cls.hc 探针 `self`/参数不再误报 | ≤1h |
| A4 | 字段与字面量检查修正：ClassLit 的字段赋值左值不作 ident 检查；`self.x`/`x.y` 的字段名段不查 undefined（Field/DotCall 已只查 base，核对 ClassLit/Assign 路径）；`Self` 在 ty_of 解析为当前类 | pa2 探针 0 误报 | ≤1h |
| A5 | 顶层函数注册异常修复：`hexval`/`utf8_width`/`kw_of` 等被误报未定义而 `is_digit` 等正常 —— 用 A0 dump 定位 collect 断点并修复 | lexer.hc 自检无「顶层函数已定义却报 undefined」 | ≤1h |
| A6 | 收敛验收：三源自检误报数登记（目标 0，不可收敛项逐条登记原因）；k3 测试断言更新；探针文件固化为语料 | 自检误报数写入本文档；`cargo test --workspace` 全绿 | ≤1h |

### 组 B：执行语料与对照 harness（先于解释器，锚定语义）

| # | 任务 | 验收 | 预估 |
|---|---|---|---|
| B1 | 执行语料集 `stage1/exec-corpus/`：10–15 个 ≤60 行递进小程序（01 算术/02 变量/03 控制流/04 fn 递归/05 Vec/06 String/07 class 方法/08 Map/09 错误路径/10 综合），每个程序首注释声明覆盖面与预期 stdout | 每程序 `hc run` 输出确定性结果并人工登记预期 | ≤1h |
| B2 | 对照测试骨架 `tag1/hc-tools/tests/k4_interp.rs`：对每个语料文件断言 `hc run interp.hc <f>` stdout == `hc run <f>` stdout（Rust 参考）；先行 `#[ignore]` | 测试骨架入库（ignored 不影响门禁） | ≤1h |

### 组 C：解释器核心（`stage1/interp.hc`，每任务以「对照绿子集递增」为验收）

| # | 任务 | 验收 | 预估 |
|---|---|---|---|
| C1 | 骨架：内嵌 lexer/parser（源自 checker.hc），main 读文件 → parse → AST 就绪；`--dump-ast` 复用 A0 | interp.hc 对 01 程序 parse 成功（无求值） | ≤1h |
| C2 | 值模型 + 环境：H 枚举负载建模 Value（int/float/bool/str/void/vec/map/obj/fnref/null）；作用域栈（Vec+size 回滚，对齐 checker 模式） | 单元自测程序（interp 内嵌 self-test 或最小探针） | ≤1h |
| C3 | 表达式求值 A：字面量/标识符/算术/比较/逻辑短路/赋值（含 `var mut`/复合赋值）+ `io.print`/`println`（含 `{}` 格式化） | 01/02 语料对照绿（首个 `#[ignore]` 摘除） | ≤1h |
| C4 | 语句求值：if/else、while、for（含 `|x|` 载荷）、break/continue | 03 语料对照绿 | ≤1h |
| C5 | 函数：调用/参数/返回/递归 + 顶层函数注册 | 04 语料对照绿 | ≤1h |
| C6 | Vec/Map：init/push/len/get/put/remove/contains + 索引 `[]`/切片 `..` | 05/08 语料对照绿 | ≤1h |
| C7 | String：拼接/len/索引/substring/find/replace/比较 + `@intCast` 等所需内建 | 06 语料对照绿 | ≤1h |
| C8 | class：类字面量/字段读写/方法调用/`self` 绑定 | 07 语料对照绿 | ≤1h |
| C9 | 错误路径最小子集：`!T` 返回/`try`/`catch`/`orelse`/`.?`（语料所需切片） | 09 语料对照绿 | ≤1h |
| C10 | 全量验收：10 综合语料对照绿 + k4 测试全部摘除 ignore + interp.hc 通过 checker 自检（不崩） | `cargo test --workspace` 全绿含 k4 | ≤1h |

### 组 D：收尾

| # | 任务 | 验收 | 预估 |
|---|---|---|---|
| D1 | 文档同步：README 路线图 E7 行、`00-feature-inventory.md` §十七、`01-bootstrap-plan.md` 当前状态、stage1/README 进度表 | 文档一致；提交 | ≤1h |
| D2 | 性能 sanity：interp.hc 执行 10 综合语料耗时记录（基线供 K5 回归） | 耗时数字登记本文档 | ≤1h |

## 执行状态

| 任务 | 状态 | 提交 | 备注 |
|---|---|---|---|
| A0 | ✅ | 本提交 | dump 工具落地；失步机制实证（见前置事实） |
| A1 | ✅ | 下一提交 | FieldDecl 节点落地（ty 存 props 对齐 Param 模式，非 children）；显式消费 KwMut（expect_ident 无条件推进是失步引擎）；逗号/分号容错；parser.hc 与 checker.hc 内嵌副本同步修改；K3 14 项+示例门 161 全绿 |
| A2 | ✅ | 下一提交 | 方法节点经 finish_fn_decl 入树（prop method=类名）；验证中发现并同步修复整链解析缺陷：① parse_block 缺 `{` 时不再吞语句到任意 `}`（失控根因）；② 新增 parse_block_or_stmt 支持无括号 if/while/for 体；③ 载荷捕获 `) \|v\|` 移到右括号后（45 处用法，原先从未生效）；④ 参数/返回类型的泛型实参消费 + var mut 前缀；⑤ 合并重复 import 分支（`.{io}` 选择集）。自检误报 lexer 690→**4**、parser 1616→**13**、checker 2387→**40**；剩余几乎全为 ClassLit 字段名泄漏（→A4） |
| A3 | ✅ | 下一提交 | check_decl 增加 Class 分支；check_class 遍历方法逐个 check_fn；Checker 增 current_class 字段，check_fn 内注册 self（显式 self 参数随后覆盖）；cls/pa2 探针方法体 0 误报；K3 14 项+示例门 161 全绿 |
| A4 | ✅ | 下一提交 | ClassLit 表达式解析（Ident+`{` → ClassLit+FieldInit 子节点，parser/checker 双副本同步）+ checker 宽容分支（只查字段值表达式）；泛型类型表达式修复：新增 `generic_args_ahead` 前瞻（匹配 `>` 后跟 `.`/`(`/`{` 才判定泛型；遇 `;`/`{`/`}`/`and`/`or`/if/while/for/return 等语句边界即否决，防跨语句误扫；`Shr` 深度≥2 视为嵌套闭合），解决 `Vec<u8>.init`/`Vec<Vec<u8>>.init` 在表达式位被比较运算吞 `<`（checker main 的 `Vec<Vec<u8>>.init` 泄漏 7 个字段名+`init`，即本项最后 8 误报）。三源自检全部归零：lexer 690→**0** / parser 1616→**0** / checker 2387→**0**；探针 3 + K3 14 项 + 示例门 161 全绿 |
| A5 | 🔴 | — | 原目标（顶层函数误报）已被 A2 修复消解（hexval/utf8_width 不再误报）；剩余验证并入 A6 |
| A6 | ✅ | 下一提交 | 三源自检 0/0/0 由 k3 断言锁死：self_check_completes_on_stage1_sources 从「不中止」升级为「零误报」（输出恰为 OK），防回退；探针 pa2/cls/pc 固化为语料（新增 probes_check_ok 回归测试，k3 14→15 项）；`cargo test --workspace` 全绿 |
| B1 | ✅ | 下一提交 | `stage1/exec-corpus/` 10 个递进语料（各 ≤60 行，首注释声明覆盖面+预期 stdout，真实 hc 实测登记）；全部被 stage1 checker 判 OK（顺带修复 check_for 未注册 for 载荷绑定的缺口，清零 03/05/10 的 8 条 undefined 误报）；AST 缺口扫描（k4test/ast-unknown-scan.txt）锁定 C 组解析工作量：赋值语句 `=`/复合 `+=`（C3）、`.?` 后缀（C6/C9）、`orelse` 表达式（C9）、`catch` 表达式（C9）；`try` 已可解析。测试输出/基线存 stage1/k4test/ |
| B2 | ✅ | 下一提交 | `tag1/hc-tools/tests/k4_interp.rs` 骨架入库：逐语料断言 `hc run interp.hc <f>` stdout == `hc run <f>` stdout；10 个测试全 `#[ignore]`（当前 0 passed/10 ignored，门禁不受影响），按 C 组任务标注摘除点（C3→01/02、C4→03、C5→04、C6→05/08、C7→06、C8→07、C9→09、C10→10） |
| C1 | 🔴 | — | |
| C2 | 🔴 | — | |
| C3 | 🔴 | — | |
| C4 | 🔴 | — | |
| C5 | 🔴 | — | |
| C6 | 🔴 | — | |
| C7 | 🔴 | — | |
| C8 | 🔴 | — | |
| C9 | 🔴 | — | |
| C10 | 🔴 | — | |
| D1 | 🔴 | — | |
| D2 | 🔴 | — | |

## 风险登记

- **checker.hc 解析失步疑点**（前置事实第 4 条）：若 A0 揭示 parser 存在结构性失步（而非局部缺口），组 A 任务需重估，A6 的「误报 0」目标可降级为「登记余量」。
- **H 语言表达力边界**：interp.hc 需要枚举负载/闭包/递归 class 等 —— checker.hc 已用同类构造验证可行；若遇 H 限制（如枚举负载方法分发缺失），以 class+kind 字符串分发兜底（checker.hc 现行模式）。
- **对照噪声**：io.print 浮点格式化双端差异 → 语料避免浮点精确输出（用整数/字符串比较）。
