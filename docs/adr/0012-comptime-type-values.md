# comptime 类型即值定案：type = 编译期对象，实例化即具体化

> 2026-08-18 定案（第三块前置裁决 A2）。关联：[ADR-0006 类型系统与脚本生成](0006-type-system-and-scriptgen.md)、[06-09-meta.md](../SPEC/06-09-meta.md)、[06-04-functions.md](../SPEC/06-04-functions.md)（泛型 where 基础）、执行细表 [10-part3-execution.md](../SPEC/10-part3-execution.md)。

## 背景

- **06-09 H5 定案（2026-08-14）**：script/comptime 双轨——comptime 管**类型级计算**（泛型），script 管**样板生成**；`fn List(T: type) type` 为 comptime 式泛型形态；comptime 块 = 语义分析阶段执行，可获取完整类型系统；脚本/comptime 失败 = 编译错误（同一错误机制）
- **既有泛型（M2.3，2026-08-16 落地）**：`where T: 接口` 约束 + **具体优先泛型**（重载歧义：具体函数胜泛型）、期望类型传播选返回类型——泛型目前仅「约束 + 调用点验证」，**无类型参数绑定与实例化**
- **缺口**：`fn List(T: type) type` 的类型参数语法、`type` 表达式（类型作为值）、实例化机制（何时、怎样具体化）、comptime_int/float 惰性宽度、`anytype`——06-09 列出形态但未定机制

## 决策

1. **`type` = 元类型**：类型参数声明为 `T: type`；`type` 表达式 = 类型名（`class Foo`/`enum E`/`interface I`/基础类型/内建容器）产生**编译期类型对象**。类型对象是**编译期值，无运行时表示**——降级后 IR 中不残留类型值，实例化即**具体化（monomorphization）**
2. **泛型实例化 = 名字 + 实参列表的具体化**：`List(i32)` 以 `(名, 实参列表)` 为键实例化并**缓存**（同签名同实例）；实例化在**调用点/声明点**触发（对齐 M2.3 具体优先泛型：具体函数仍胜泛型，泛型仅在无具体匹配时实例化）。**惰性实例化**：仅被实际使用的泛型声明才具体化
3. **comptime 块 = 语义分析阶段的编译期求值**：复用既有求值器子集（无运行时环境：io/alloc/argv 不可用），可见完整 `types` 元数据；块/函数失败（含 `return error.X`）= 编译错误带块内 + 所属块位置（沿 06-09 错误机制）。`comptime { ... }` 仅编译期存在，产物是**类型级副作用**（注册类型/实例化/常量折叠），不产生运行时代码
4. **comptime_int/comptime_float = 惰性宽度字面量**：编译期任意精度，实例化/赋值时按上下文收窄（对齐 M2.3 字面量惰性宽度语义）；溢出在收窄点诊断
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

- **组 D D1（类型函数最小切片）已实现**：`fn Pair(T: type) type { return struct { first: T, second: T }; }` 解析/AST（`Expr::StructType` + `NamedLit.ty_args`）/语义（宽容放行）全落地；`hc::comptime` 具体化引擎（`is_type_fn`/`concrete_name`/`subst`/`instantiate`）三后端共享；interp 与 IR 各自惰性登记 `Pair(i32)` → `Pair<@i32>`（类型表缓存，纯查找 immutable）；`return T;` 透传 = 实参类型同义；类型函数体降级跳过（comptime-only，无运行时残留）；内建泛型（`Vec(T)` 等）回退基础名不受影响。
- 示例 **34-generics** interp / IR / 原生编译三模式全绿；一致性 `d1_comptime_type_application_consistent`。
- **待组 D2–D5**：`comptime { }` 块编译期求值、comptime_int/float（35 例）、`anytype` 完整语义、嵌套/递归实例化、`comptime` 值函数（参数含 `type`/`anytype` 触发编译期执行的普通函数）。
