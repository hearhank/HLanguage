# comptime 类型即值定案：type = 编译期对象，实例化即具体化

> 2026-08-18 定案（第三块前置裁决 A2）。关联：[ADR-0006 类型系统与脚本生成](0006-type-system-and-scriptgen.md)、[06-09-meta.md](../SPEC/06-09-meta.md)、[06-04-functions.md](../SPEC/06-04-functions.md)（泛型 where 基础）、执行细表 [10-part3-execution.md](../SPEC/10-part3-execution.md)。

## 背景

- **06-09 H5 定案（2026-08-14）**：script/comptime 双轨——comptime 管**类型级计算**（泛型），script 管**样板生成**；`fn List(T: type) type` 为 comptime 式泛型形态；comptime 块 = 语义分析阶段执行，可获取完整类型系统；脚本/comptime 失败 = 编译错误（同一错误机制）
- **既有泛型（M2.3，2026-08-16 落地）**：`where T: 接口` 约束 + **具体优先泛型**（重载歧义：具体函数胜泛型）、期望类型传播选返回类型——泛型目前仅「约束 + 调用点验证」，**无类型参数绑定与实例化**
- **缺口**：`fn List(T: type) type` 的类型参数语法、`type` 表达式（类型作为值）、实例化机制（何时、怎样具体化）、comptime_int/float 惰性宽度、`anytype`——06-09 列出形态但未定机制

## 决策

1. **`type` = 元类型**：类型参数声明为 `T: type`；`type` 表达式 = 类型名（`class Foo`/`enum E`/`interface I`/基础类型/内建容器）产生**编译期类型对象**。类型对象是**编译期值，无运行时表示**——降级后 IR 中不残留类型值，实例化即**具体化（monomorphization）**
2. **泛型实例化 = 名字 + 实参列表的具体化**：`List<i32>` 以 `(名, 实参列表)` 为键实例化并**缓存**（同签名同实例）；实例化在**调用点/声明点**触发（对齐 M2.3 具体优先泛型：具体函数仍胜泛型，泛型仅在无具体匹配时实例化）。**惰性实例化**：仅被实际使用的泛型声明才具体化
3. **comptime 块 = 语义分析阶段的编译期求值**：复用既有求值器子集（无运行时环境：io/alloc/argv 不可用），可见完整 `types` 元数据；块/函数失败（含 `return error.X`）= 编译错误带块内 + 所属块位置（沿 06-09 错误机制）。`comptime { ... }` 仅编译期存在，产物是**类型级副作用**（注册类型/实例化/常量折叠），不产生运行时代码
4. **comptime_int/comptime_float = 惰性宽度字面量**：编译期求值，实例化/赋值时按上下文收窄（对齐 M2.3 字面量惰性宽度语义）；溢出在收窄点诊断。**落地边界（2026-08-18）：`Value::Int(i128)` 无 bignum——实际以 i128 为上限（偏离「任意精度」），超大常量溢出在收窄点/运行时报错**
5. **`anytype` = 参数类型不预先绑定**：调用点按实参具体类型实例化；仅用于泛型函数参数
6. **与 script 分工不变（06-09）**：comptime 管类型级计算与泛型实例化；script 管数据定义驱动的样板生成。降级闸门 Q-S10 保留（脚本生成成本超预期 → 降级为编译期执行 H 子集函数，`script{}` 语法保留）

## 影响

- **语法/解析（组 D1）**：`T: type` 类型参数、`type` 表达式、`anytype`、`comptime { }` 块——lexer/parser/AST
- **语义（组 D2/D3）**：类型对象表示、实例化缓存、调用点具体化、comptime 块求值、错误机制
- **IR（组 D5）**：类型值仅在编译期，降级后无运行时残留；具体化后的函数进正常 IR（与既有函数表/重载池一致）
- **文档**：06-09（机制补定）、06-language-spec（type 关键字/语法速查）、04（comptime 内建若涉及）
- **未变**：M2.3 具体优先泛型与期望类型传播；script/comptime 双轨分工；「无宏」立场（12.22）

## 取舍

- 选择「类型值 = 编译期对象（无运行时表示）+ 具体化」而非「类型即运行时值（如 C++ RTTI）」：零运行时开销、与双模式一致（interp/IR 同型）、无动态类型分发；代价是泛型无法做动态多态（如需则 1.x 用接口 + 具体化组合达成）
- 选择「调用点具体化 + 缓存」而非「声明点全量实例化」：避免爆炸（只实例化用到的），对齐既有具体优先泛型；代价是实例化错误在调用点报告（位置稍远，编译器可附声明位置）
- 未采用 C++ 模板式「文本替换后重解析」：H 有类型系统，类型对象为第一类编译期值，无需重解析；实例化 = 类型级求值

---

## 落地状态（2026-08-18）

- **组 D D1（类型函数最小切片）已实现**：`fn Pair(T: type) type { return struct { first: T, second: T }; }` 解析/AST（`Expr::StructType` + `NamedLit.ty_args`）/语义（宽容放行）全落地；`hc::comptime` 具体化引擎（`is_type_fn`/`concrete_name`/`subst`/`instantiate`）三后端共享；interp 与 IR 各自惰性登记 `Pair<i32>` → `Pair<@i32>`（类型表缓存，纯查找 immutable）；`return T;` 透传 = 实参类型同义；类型函数体降级跳过（comptime-only，无运行时残留）；内建泛型（`Vec<T>` 等）回退基础名不受影响。
- 示例 **34-generics** interp / IR / 原生编译三模式全绿；一致性 `d1_comptime_type_application_consistent`。
- **组 D D3/D4 最小切片（comptime_int + 数组类型函数）已实现（2026-08-18）**：`fn ArrayLen(T: type, n: comptime_int) type { return [n]T; }` —— `Type::ComptimeInt(usize)`（类型实参位置的整数字面量，惰性宽度，实例化按上下文收窄）+ `Expr::ArrayType { len, elem }`（`[n]T` 类型值表达式；parser `in_type_fn` 标志下仅类型函数体 return 位置特殊解析）；`instantiate` 参数分类（类型参数/值参数）+ 长度求值（字面量或参数引用）→ `Type::Array(n, elem)`；具体化名 `ArrayLen<i32, 3>` → `ArrayLen<@i32,3>`。带 init 的 var-decl 惰性放行（示例 35 靠 init 驱动，标注仅设 expected_ret）；`anytype` 仍为普通运行时函数（`max_value` 非类型函数）。
- 示例 **35-comptime-branch** interp / IR / 原生编译三模式全绿；comptime 单测 +8（值参数/数组形态含错误路径）；一致性 `d35_comptime_array_type_fn_consistent`。
- **组 D D2（comptime 块最小切片）已实现（2026-08-18）**：`comptime { }` 块装载期编译期求值——AST/parser 增 `Decl::Comptime`（镜像 `Decl::Script`），`hc-tools/src/comptimegen.rs` 装载期 pass（script 展开后、语义检查前，经 `parse_with_scripts` 统一入口）：受限 Interp（`script_mode`：io/alloc/argv 不可用）求值块体，结果**丢弃**（仅编译期存在，无运行时代码/无源码替换）；失败 = 编译错误（`return error.X` → 「comptime 块返回错误 `error.X`」带块 span；运行时错误 → 原 RtError 渲染），与运行时错误同一机制。块内可见完整 `types` 元数据（含 script 生成类型——「script 展开后求值」顺序验证）。三后端跳过 `Decl::Comptime`（镜像 Script），IR/native 零改动。测试 `hc-tools/tests/comptime.rs` 5 项端到端全绿；门禁基线不变。
- **组 D D3（嵌套/递归实例化）已实现（2026-08-18）**：类型函数嵌套（`PairPair<i32>` 字段 `a: Pair<T>` → 具体化键 `Pair<@i32>`）与递归/自引用（`LinkedList<T> { value: T, next: ?LinkedList<T> }`）实例化。`hc::comptime` 增 `map_type_apps` 深度遍历辅助（后端注入 resolver 回调，hc 零依赖约束下纯函数）；interp 与 IR 的 `concrete_type_name` 预解析实参（内层先具体化登记）+ `instantiating` in-progress 守卫（自/互递归字段内自引用 → 返回自身键为叶）；IR `lower_default_value` 增类型函数应用臂并补 `var x: PairPair<i32>;` 声明式无初值路径（对齐 oracle `default_value`）。运行时递归靠 Optional 默认 `None` 终止。comptime 单测 +2（parser 嵌套回归、`map_type_apps` 复合形态）、一致性 +2（`d3_nested`/`d3_recursive`，含无初值）；门禁基线不变。
- **组 D D4（comptime_int 常量折叠最小切片）已实现（2026-08-18）**：comptime 块类型层补齐——`comptime_int` 类型名识别（`ty_of` → `SType::Int { width: IntWidth::Comptime }`）+ comptime 块语义检查（`Checker.in_comptime_block` + `check_decl` `Decl::Comptime` 臂；`Stmt::Return` 错误返回守卫放宽——comptime 块失败机制 = `return error.X`）。折叠核心（装载期受限 Interp 求值）已在 D2，本切片补类型安全：收窄溢出（`var x: u8 = 256`）、类型不匹配（`var x: comptime_int = "hello"`）在收窄点/赋值点诊断；`expect_eq` 断言折叠。hc 语义单测 +2、hc-tools 端到端 +5；门禁基线不变。**已知边界**：`Value::Int(i128)` 无 bignum，comptime_int 超大常量溢出（i128 上限，见决策 #4）；块内 `_ = x;` 丢弃语句装载期 Interp 不支持。
- **组 D D4（comptime_float 惰性宽度）已实现（2026-08-18）**：`comptime_float` 类型名识别——`ty_of` 增 `"comptime_float"` → `SType::Float`（H 浮点单一 f64 表示，惰性宽度浮点映射单一 Float）。comptime 块浮点折叠 + `expect_eq` 断言（`value_eq` `(Float, Float)` 精确相等）；类型不匹配（`var x: comptime_float = "hello"`）在赋值点诊断。hc 语义单测 +2、hc-tools 端到端 +3；门禁基线不变。
- **组 D D4b（anytype 完整语义）已实现（2026-08-18）**：`anytype` 参数 = 调用点按实参具体类型实例化（决策 #5）——**类型层具体化**：`hc::comptime` 增 `has_anytype` 判定；semantic `match_overloads` 增 anytype 分支（`anytype` 参数绑定实参具体类型 → 返回 `anytype` 解析为体 return 表达式在具体绑定下的重求值类型，`(qname, 具体化键)` 惰性缓存 + 自递归守卫）。运行时仍动态分派（值携带类型），interp/IR 零改动。效果：`max_value(2.5, 1.5)` = `f64`（误配 String → 编译错误），`max_value(3, 7)` = 惰性宽度整数赋 i32 收窄。hc 语义单测 +3、hc-tools 端到端 +2、consistency +1；门禁基线不变。
- **组 D D4c（comptime 值函数）已实现（2026-08-18）**：决策「参数含 `type`/`anytype` 触发编译期执行的普通函数」落地——参数含 `T: type`、**非返回 `type`** 的普通函数（`fn array_len(T: type) comptime_int`）调用点**编译期求值**：`hc::comptime` 增 `is_type_param`/`is_comptime_value_fn`/`expr_to_type`（类型实参绑定）；interp `eval_call` 挂钩 `try_comptime_value_call`（类型实参收已知类型表达式、值实参常量求值、`exec_fn_body` 折叠 + 自递归深度守卫 `ComptimeRecursion`）。comptime 块装载期求值与运行时 interp 共用。效果：`array_len(i32)` = 4、`byte_size(f64, 7)` = 8 折叠。hc 单测 +2、hc-tools 端到端 +5；门禁基线不变。**已知边界**：体引用类型参数值暂不支持（引用 → UndefinedName 编译错误）。
- **组 D D5（三后端类型值表示 + 一致性）已实现（2026-08-18）**：comptime 值函数**运行时调用点折叠三后端一致**——类型值仅编译期存在，IR/原生无类型值/调用残留（决策 #1「类型值无运行时表示」在 IR/原生落地）。`hc::ir` 增 `collect_value_fns`（name → params+body，镜像 `collect_type_fns`）+ `LowerCtx.value_fns` 贯穿全链；Call 降级 `callee_name` 后、实参降级前挂钩 `try_fold_comptime_value_call`——类型实参收已知类型表达式（`is_known_type_name`）、值实参常量求值、体经 `eval_const_block` 顺序执行（var/const/return/if 常量折叠 then/else/else-if），折叠成功发射 `Const`（无调用残留）。原生经共享 IR 继承折叠。consistency +1（`d5_comptime_value_fn_consistent`，interp == IR）；`cargo test --workspace` 全绿；门禁基线不变。
- **组 D 完结**：comptime_int/float 惰性宽度、anytype 完整语义、comptime 值函数、三后端类型值表示 + 一致性全部落地（D1 类型函数 / D2 comptime 块 / D3 嵌套递归实例化 / D4 常量折叠 / D4b anytype / D4c 值函数 / D5 一致性）。
