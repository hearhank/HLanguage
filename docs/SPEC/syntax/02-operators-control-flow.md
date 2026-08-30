# 02 运算符与控制流

> 大模块：运算符与控制流 | 对齐状态：**✅ 对齐完成（2026-08-30，E1–E3 + `**` 设计定案）** | 初稿：2026-08-30
>
> 事实基础：定案记录 Q4/Q21/Q24/Q-S6/Q27/Q28/Q29/Q31/H3/C3/L4（原载 `06-01-syntax.md` §3/§4，已废弃）、tag1 实现（`tag1/hc/src/parser/expr.rs`、`stmt.rs`、`ir/lower_impl.rs`、`semantic/infer.rs`）。
> 证据总库：`tag1/hc/tests/frontend.rs`。

## 2.1 运算符总览与优先级

- 规则：优先级从低到高（同层左结合，注明的除外）：

| 层 | 运算符 |
|---|---|
| 1 | `or`（符号别名 `\|\|`，E1） |
| 2 | `and`（符号别名 `&&`） |
| 3 | `..` 区间（**非结合**） |
| 4 | `==` `!=` `<` `<=` `>` `>=`（**非结合**） |
| 5 | `\|` 位或 |
| 6 | `^` 位异或 |
| 7 | `&` 位与 |
| 8 | `<<` `>>` 移位 |
| 9 | `+` `-` |
| 10 | `*` `/` `%` `%%` |
| 11 | `**` 幂（**右结合**，⏳ 待实现，§2.2 幂小节） |
| 12 | 一元（前缀）：`-` `!` `~` `&` `&mut` `try` `await` `move` `spawn(...)` |
| 13 | 后缀：`.字段` `.字段()` `.?` `[i]` `[i, j]` `.*` `()` `orelse` `catch`（裸 `?` 已废弃，E2） |

- 赋值**不在优先级链中**——仅语句级与 while 步进位（§2.7）。
- `orelse`/`catch` 属于后缀层（绑定最紧）：`a orelse b + c` ≡ `a orelse (b + c)`。
- 状态：✅ 已实现
- 证据：`tag1/hc/src/parser/expr.rs` 优先级链 `parse_or → parse_and → parse_range → parse_comparison → parse_bitor → parse_bitxor → parse_bitand → parse_shift → parse_addsub → parse_muldiv → parse_unary → parse_postfix`（L9-425）
- 与旧文档差异：旧优先级表（Q4）**漏记 `and`/`or`/`..` 的层级位置**；`orelse`/`catch` 旧归「optional 运算」未给优先级——本文按实现补全。

## 2.2 算术与除模

- 规则：
  - `+` `-` `*` `/` `%` `%%`；`/` = 截断除法，`%` = 截断取余，`%%` = 欧几里得取模（结果非负）。
  - 运算符绑定内建接口族（H3 定案）：`a + b` ≡ `a.add(b)`（`INumber` 族）；`%`/`%%` 编译器派生；**无用户运算符重载**。接口契约见 `06-interfaces.md`。
  - 整数溢出按模式（Q-S6 定案）：Debug/脚本模式检测并 `@panic`（带位置）；Release 裸 wrap；显式原语 `@addWithOverflow`/`@subWithOverflow`/`@mulWithOverflow` 返回元组 `(T, bool)`（value, overflow）——见 `13-builtins.md`。
  - 越界/溢出的模式定义（Debug/Release/脚本）归 `03-types.md` 双模式章节。
  - 字符串**无** `+`/`++` 拼接（Q28 定案）：`s.concat(other)`（见 `06-interfaces.md`）。

### 2.2.1 幂运算符 `**`（设计定案 2026-08-30，⏳ 待实现）

- 规则：
  - `a ** b` = a 的 b 次方；优先级介于 `*`/`/`/`%` 与一元之间，**右结合**：`2**3**2` = 2⁹；`-2**2` = -4（`**` 绑定高于一元负号）。
  - 操作数：整数与浮点均支持；浮点走库 pow。
  - 整数溢出按 Q-S6 模式：Debug/脚本检测并 `@panic`（带位置），Release 裸 wrap；显式替代 = `@mulWithOverflow` 链或 comptime 校验。
  - 接口绑定（对齐 H3「无用户重载」）：`INumber` 族新增 `pow` 方法，`a ** b` ≡ `a.pow(b)`。
  - comptime：常量上下文直接折叠（`2**3` ≡ `8`）。
- 状态：⏳ 未实现·目标（lexer/parser/语义/IR 四层；见 `00-index.md` backlog #4）
- 证据：无（设计定案，待实现后回填）

```hc
var a = 2 ** 10;          // 1024（comptime 折叠）
var b = 2.0 ** 0.5;       // √2（浮点 pow）
var c = base ** exp;      // 运行时：Debug 溢出 panic / Release wrap
```
- 状态：✅ 已实现（`%%` 欧氏语义、溢出检测的证据归语义/运行时，见 `03-types.md` 双模式矩阵核对）
- 证据：`tag1/hc/src/parser/expr.rs` `parse_muldiv`（L147-163）；`semantic/infer.rs` `check_binary`

## 2.3 逻辑运算

- 规则：
  - `and` / `or` / `!`——关键字形态；符号别名：`&&`（词法直接产出 `and`，见 `01` §1.2.2）、`\|\|`（表达式位置 = 逻辑或，**E1 已裁决**，见 `01` §1.2.2 / §2.20）。
  - **短路求值**：`and` 左侧为假不求右侧；`or` 左侧为真不求右侧。
  - 结果类型 = `bool`（两操作数须 bool）。
- 状态：✅ 已实现
- 证据：`ir/lower_impl.rs` `lower_expr` L1193-1225（JumpIfNot/JumpIf 标签实现短路）；`eval_const_expr` L2886-2895（常量短路）；`semantic/infer.rs` L355-362

```hc
if (x != null and x.*.ready) { ... }   // x 为空时右侧不求值
```

## 2.4 位运算

- 规则：`&` `|` `^` `~`（按位取反，前缀）`<<` `>>`；复合赋值 `&=` `|=` `^=`（§2.7）。
- 移位位数超宽行为：**未验证**（以 IR/运行时实现为准，`03-types.md` 双模式矩阵核对时收口）。
- 状态：✅ 已实现（移位溢出 ⚠️ 待核对）
- 证据：`tag1/hc/src/parser/expr.rs` L93-129、L178-182

## 2.5 比较与相等

- 规则：
  - `==` `!=` `<` `<=` `>` `>=`。
  - `==` = **值比较**，内部调用 `ICompare`（H3 定案）：标量/枚举/元组/String/集合按值；指针比较 = 指向对象地址；数组/切片含位置 + 长度。class 无默认 `==`——需实现 `ICompare`（用户重载不存在）。接口契约见 `06-interfaces.md`。
  - 序比较 `< <= > >=` 绑定 `ICompare`。
  - **非结合**：每层表达式最多一次比较（语法结构强制——`parse_comparison` 无循环）；`a < b < c` 中第二个 `<` 成为孤立 token → 解析错误。
- 状态：✅ 已实现（非结合 = 语法层；`==` 的 ICompare 分派归语义/运行时）
- 证据：`parse_comparison` L62-80（单次匹配，无循环）；诊断文案 = 后随 token 的 `expected ...` 类错误

## 2.6 区间

- 规则：
  - `a..b`（闭开区间语义由使用场景定义：切片取段、for 区间）。
  - **开上界** `a..`：右侧可省（`]` `)` `,` `;` 前），表示「到末尾」——如 `&arr[1..]`。
  - 独立优先级层（`and` 与比较之间）；非结合（每层最多一个 `..`）。
  - 用途：切片取段 `&arr[1..3]` / `&mut arr[0..2]`（ADR-0005 R2）、for 区间糖（§2.14）。
- 状态：✅ 已实现（开上界为**旧文档漏记补录**）
- 证据：`parse_range` L33-60（`__end__` 哨兵）

```hc
var mid = &arr[1..arr.len - 1];
var tail = &arr[2..];          // 到末尾
```

## 2.7 赋值

- 规则：
  - 语句级：`目标 = 表达式;`；复合赋值 `目标 op= 表达式;`（op ∈ `+` `-` `*` `/` `|` `&` `^`）。
  - 合法目标：标识符 / 索引 `t[i]`、`t[i, j]` / 字段 `s.f` / 限定名 `A.b` / 解引用 `p.*`；其它 → 诊断 `invalid assignment target`。
  - 赋值**不是表达式**——无链式赋值 `a = b = c`，不可嵌入表达式位。
- 状态：✅ 已实现
- 证据：`parse_assign_or_expr` L294-329（目标校验 L313-320）

## 2.8 取地址与解引用

- 规则：
  - `&x` = 只读地址（类型 `*T`），对只读/可写变量均合法；`&mut x` = 可写地址（类型 `*mut T`），**仅** `var mut` 变量合法（ADR-0005 修订）。
  - `p.*` = 解引用取值（显式）；字段/索引访问对指针**自动解引用**（`p.x`、`s[i]`，评审 A3 定案）。
  - 指针类型与所有权规则归 `03-types.md` / `07-ownership-memory.md`。
- 状态：✅ 已实现
- 证据：`parse_unary` L183-193（AddrOf + mut_）、`parse_postfix` L285-288（DotStar）

## 2.9 后缀运算符

- 规则：
  - 字段访问 `.name`（字段名可为关键字）；方法调用 `.name(args)`；自由调用 `f(args)`——实参支持尾逗号。
  - 索引 `[i]`；**多参索引** `[i, j]`——语法通用，语义仅 `Table` 合法（M8 定案，行、列；其它类型单参）。索引列表内**无**尾逗号（与实参不对称，实现如此）。
  - optional 解包 `.?`——**唯一合法形态**（裁决 E2，2026-08-30：裸 `?` 后缀废弃；当前实现仍接受裸 `?`（同为 Unwrap），改为报诊断 → backlog #3）。
  - 解引用 `.*`。
  - 类型字面量：`Type{ field = 值, ... }`（含限定 `Orders.Line{...}`、泛型 `Pair<i32>{...}`）、`class { ... }`/`struct { ... }` 表达式（类型字段 `name: T` 与值字段 `name = e` 二选一混排非法）、容器字面量 `Vec<i32>[1, 2, 3]`（ADR-0027）——类型系统规则归 `04-extended-types.md`。
- 状态：✅ 已实现
- 证据：`parse_postfix` L235-425、`parse_call_args` L427-446、`parse_primary` L508-556 / L709-812

## 2.10 optional 与错误的运算符形态

- 规则（语法形态；类型语义归 `03-types.md`，错误模型归 `08-errors.md`）：
  - `x orelse 默认值`；控制流兜底：`x orelse return [expr];`、`x orelse break;`、`x orelse continue;`（不消费分号）。
  - `e catch |err| 体`（体 = 块或表达式）、`e catch 默认值`、`e catch return [expr]`、`e catch break/continue`。
  - `try e`（前缀，错误传播）。
- 状态：✅ 已实现
- 证据：`parse_postfix` L326-420（orelse/catch 全形态）、`parse_unary` L194-198

```hc
var v = m.get("k") orelse return error.NotFound;
var n = parse(s) catch 0;
var r = try parse(s);
```

## 2.11 语句与表达式

- 规则：
  - 表达式语句：`表达式;`。块 `{ ... }` 是表达式。
  - `if` 与 `switch` 是表达式（作表达式时形态约束见 §2.12/§2.15）；`while`/`for` 仅语句。
  - 语句清单：`var`/`const`（`01` §1.9/§1.10）、`if`、`while`、`for`、`switch`、`break`、`continue`、`return [expr];`、`defer`、`errdefer`、块、空语句 `;`。
- 状态：✅ 已实现
- 证据：`parse_stmt` L22-116

## 2.12 if

- 规则：
  - 语句形态：`if (cond) 块|单语句 [else ...]`——else 链支持 `else if`；体可为块或**单语句**。
  - optional 捕获：`if (opt) |v| ...`（Q10 同款语法）。
  - 错误双向捕获（Q9 定案：必须成对）：`if (e!) |v| ... else |err| ...`——错误路径显式。
  - 表达式形态：`if (cond) expr else expr`——**else 强制**；分支为表达式（可用块）。
- 状态：✅ 已实现
- 证据：`parse_if_stmt` L189-226（err_capture L201-207）、`parse_primary` L607-630

## 2.13 while

- 规则：
  - `while (cond) [|capture|] [: (步进)] 体`——capture 与 `: (步进)` **顺序可互换**（对齐 Zig）。
  - 步进：`: (赋值或表达式)`，如 `while (i < n) : (i += 1)`。
  - optional 捕获：`while (opt) |v|`——空则退出循环（Q10 定案）。
  - 错误捕获：`while (e!) |v| ... else |err| ...`——⏳ **目标**（裁决 E3，2026-08-30：保留形态与 if 对称，低优先级待实现；`parse_while_stmt` 现无 else 分支）。
  - 标签：`:label while (...)`（§2.16）。
- 状态：⚠️ 部分实现（错误捕获缺失）
- 证据：`parse_while_stmt` L243-275

## 2.14 for

- 规则：
  - `for (迭代) |[模式] 名称| 体`——**捕获必填**；模式：缺省 = 只读捕获、`|mut x|` = 可写、`|move x|` = 拥有迭代（IIterable(owned T)）。
  - 区间糖（Q29 定案）：`for (0..10) |i|`——复用 `..` 记号，底层等价 while 计数。
  - 值类型迭代自动取引用（L4 定案）：`for (fib)` ≡ `for (&mut fib)`；只读绑定时可写迭代编译错误——迭代契约与自动取引用规则归 `06-interfaces.md`。
  - 标签：`:label for (...)`。
- 状态：✅ 已实现（L4 语义证据归 `06-interfaces.md` 核对）
- 证据：`parse_for_stmt` L277-293、`parse_capture` L374-388（三模式）

## 2.15 switch

- 规则：
  - `switch (subject) { 模式[, 模式 ...] [if 守卫] => [|capture|] 体, ... }`。
  - 模式全集：整型/浮点/字符串/字符字面量、裸标识符、**限定变体 `Type.variant`**（变体名可为关键字，如 `JsonValue.null`）、**错误模式 `error.Name`**、`null`/`true`/`false`、`else`（兜底）。
  - **守卫**（C3 定案，旧文档漏记）：`模式 if 条件 => ...`——进入臂前求值。
  - 负载捕获：`=> |v| ...`。
  - 臂体：块 / 表达式 / 控制流语句（`return`/`break`/`continue`/`var`/`const`——免分号，臂间 `,` 分隔）。
  - 表达式形态语法同上（KwSwitch 在 primary 位置）；穷举检查语义（Q31：有 else 免穷举；无 else 须穷举）归 `03-types.md`/`08-errors.md`（实现以 `has_else` 标志 + 语义检查为准）。
- 状态：✅ 已实现（穷举检查证据归语义层核对）
- 证据：`parse_switch` L390-472（守卫 L412-418）、`parse_switch_pattern` L474-530

```hc
switch (kv) {
    JsonValue.null => 0,
    JsonValue.num if n > 0 => |n| n,      // 守卫 + 负载捕获
    error.OutOfBound, error.Invalid => -1, // 多模式
    else => -2,
}
```

## 2.16 break/continue 与标签

- 规则：
  - `break;` `continue;`；带标签 `break :label;` `continue :label;`。
  - 标签声明 = **循环前缀** `:label while (...)` / `:label for (...)`（`label_if_ident` 对称形式）；**块不可加标签**。
- 状态：✅ 已实现（标签前缀声明为**旧文档漏记补录**——旧文档只写了 break :label）
- 证据：`parse_stmt` L22-31（前缀标签 + 非循环报错「循环标签后需跟 while 或 for」）、L81-94

```hc
:outer for (rows) |r| {
    for (r) |c| {
        if (c == target) break :outer;
    }
}
```

## 2.17 defer/errdefer

- 规则：`defer 表达式;` / `errdefer 表达式;`——表达式可为块；defer 作用域退出执行、**LIFO**（Q21：后声明先执行）；errdefer 仅错误路径执行。执行语义归 `07-ownership-memory.md`/`08-errors.md`。
- 状态：✅ 已实现
- 证据：`stmt.rs` L93-108

## 2.18 排除与已知边界

- 无 `do-while`、无 `loop`（用 `while (true)`）；无 `goto`；块标签不支持。
- **表达式位置的类型实参识别依赖类型名单**（`Vec` `Map` `Deque` `Table` `List` `Pipe` `Tee` `Funnel` `Hub` `Pair` `PairPair` `LinkedList` `Opt`——`parse_primary` 硬编码）：用户自定义泛型 `MyBox<i32>` 在表达式位置会落入比较运算解析。用户泛型的表达式位置边界归 `05-functions.md`/`04-extended-types.md` 裁决（关联测试 `tag1/hc/tests/generics_angle.rs`）。
- `@` 内建在表达式位置必须带调用 `@f(args)`（无裸引用形态）。

## 2.19 变更记录（相对旧 06-01 §3/§4）

| 变更 | 依据 |
|---|---|
| 优先级表补全 `and`/`or`/`..` 层级、`orelse`/`catch` 归后缀层 | 实现 `parse_or..parse_postfix` 链 |
| 补录 `&&` 别名（`01` §1.2.2）、`\|\|` 表达式 = 逻辑或（E1 裁决点） | `parse_or` L11 接受 PipePipe |
| 补录 `..` 开上界 `a..` | `parse_range` 哨兵实现 |
| 补录 switch 守卫 `模式 if 条件`（C3） | `parse_switch` L412-418 |
| 补录 switch 限定变体模式（变体名可关键字）、多模式臂 | `parse_switch_pattern` |
| 补录循环标签前缀 `:label while/for` | `parse_stmt` L22-31 |
| 补录 orelse/catch 控制流兜底形态（return/break/continue） | `parse_postfix` L326-420 |
| 补录裸 `?` ≡ `.?`（E2） | `parse_postfix` L289-292 |
| 补录短路求值实现（IR 跳转级） | `lower_impl.rs` L1193-1225 |
| 补录类型字面量/容器字面量表达式形态（细节归 04） | `parse_primary` L508-812 |
| `while (e!) \|v\| else \|err\|` 标 ⏳（旧文档有、实现无） | `parse_while_stmt` 无 else 分支 |
| 赋值目标集、非表达式化补录 | `parse_assign_or_expr` |
| `**` 幂运算符设计定案（右结合/按模式溢出/浮点/INumber.pow/comptime 折叠），标 ⏳ | 项目所有者裁决（2026-08-30） |

## 2.20 裁决记录（2026-08-30，项目所有者）

| # | 条目 | 裁决 | 影响 |
|---|---|---|---|
| E1 | `\|\|` 的身份 | **按推荐落定**：`\|\|` = 逻辑或别名（与 `&&`/`and` 对称）；错误集联合限于 `const E = A \|\| B;` 特例与类型位置（`08-errors.md` 复核）；`01` §1.2.2 已同步修正 | §2.1/§2.3、`01` §1.2.2、`08-errors.md` |
| E2 | 裸 `?` 后缀 | **只保留 `.?`**；裸 `?` 废弃，实现移除 → backlog #3 | §2.9、backlog #3 |
| E3 | while 错误捕获 | **⏳ 目标**（与 if 对称，低优先级） | §2.13 |

## 2.21 幂运算符 `**`（裁决记录 + 特性条目）

**裁决（2026-08-30，项目所有者，全推荐）**：优先级介于 `*`/`/`/`%` 与一元之间；右结合；`-2**2` = -4；整数溢出按 Q-S6 模式；浮点支持；绑定 `INumber.pow`；comptime 折叠。特性条目见 §2.2.1，实现任务 = `00-index.md` backlog #4。
