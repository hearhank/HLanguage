# 03 类型

> 大模块：类型 | 对齐状态：**✅ 对齐完成（2026-08-30，F1 裁决：f16/f32 真实宽度化）** | 初稿：2026-08-30
>
> 事实基础：ADR-0005（引用/指针语法及修订）、ADR-0012（comptime 类型值）、定案 Q-S9/Q24/Q-S6、历史 `06-02-types.md`（已废弃）、tag1 实现（`tag1/hc/src/parser/type.rs`、`parser/mod.rs` `parse_type`、`semantic/{mod,infer,resolve,validate}.rs`）。
> 证据总库：`tag1/hc/tests/frontend.rs`、`tag1/hc/tests/infinite_size.rs`、`tag1/hc/tests/owned_check.rs`。

## 3.1 类型文法总览

- 规则：类型文法 = **前缀修饰**（递归）+ **基础类型**：

```
类型 := owned 类型 | *类型 | *mut 类型 | &类型 | &mut 类型
      | ?类型 | !类型 | 错误集!类型 | 基础类型
基础类型 := void | anytype | type
          | (类型, 类型, ...)                     // 元组
          | [N]类型                               // 定长数组
          | 名称(.名称)* [泛型实参]                // 命名/限定/泛型
```

- 状态：✅ 已实现
- 证据：`parser/mod.rs` `parse_type`（L69-128：owned/`*`/`&`/`?`/`!`/`E!`）；`parser/type.rs` `parse_type_base`（L9-110：void/anytype/type/元组/数组/命名）

## 3.2 标量类型

### 3.2.1 整数

- 规则：
  - 定宽整数 12 种：`i8` `i16` `i32` `i64` `i128` `isize`、`u8` `u16` `u32` `u64` `u128` `usize`。
  - 字面量惰性定型（comptime_int）：整数字面量无固定宽度，在**期望类型处定型**（变量注解、参数、返回、比较对象等）；带宽度后缀直接定宽（`42i32`，`01` §1.4.1）；定型时超范围 = 编译期报错。
  - **无隐式跨宽度转换**（无宽化/窄化路径）；跨宽度显式 `@intCast`（见 `13-builtins.md`）。
- 状态：✅ 已实现
- 证据：`semantic/mod.rs` `SType::Int{width}` + `IntWidth`（I8..I128/ISize/U8..U128/USize/**Comptime**）；`resolve.rs` `ty_of`（按名映射）；`infer.rs` `stype_key` L2211-2221；隐式转换：semantic 层无 coerce/widen 路径（全文核查）

```hc
var a: i32 = 42;        // 字面量定型 i32
var b = 42i64;          // 后缀定宽
var c: u8 = 255;
// 错误示例：var d: u8 = 256;          → 编译期报错（超范围）
// 错误示例：var e: i64 = a + 1i8;     → 无隐式转换，需 @intCast
```

### 3.2.2 浮点

- 规则（裁决 F1 → **修订 D15，ADR-0037 2026-08-31**）：浮点 = **两个真实宽度** `f32` `f64`：
  - `f64`：✅ 已实现。
  - `f32`：**真实宽度重新实现，`f16` 移除**（词法后缀 `f16` 不再合法；语义层 FloatWidth 划分 + IR/VM 算术 + LLVM + 字面量定型——实现专项，backlog #5 改写）；升级完成前写 `f32` 等价于 f64。
  - `f16`：❌ 移除（D15：不实现，词法后缀待诊断化）。
  - `f128`：❌ 废弃（F1 裁决未含 f128；如需 128 位浮点单独立项）。当前实现映射 f64，实现侧同步移除该名 → backlog #5。
  - `comptime_float` 字面量惰性定型 → 可定 f32/f64。
  - 跨浮点宽度转换：显式 `@floatCast`（见 `13-builtins.md`）；无隐式转换。
- 证据：现状 `semantic/resolve.rs`（f16\|f32\|f64\|f128 → SType::Float，注释「H 浮点单一 f64 表示」）；D15 + ADR-0037

```hc
var a: f64 = 3.14;
var b = 1.5f32;          // 定宽字面量（f32 真实宽度升级前等价 f64）
var c: f16 = x;          // ✋ f16 已移除（D15）
```

### 3.2.3 bool

- 规则：`bool`，字面量 `true`/`false`；逻辑/比较运算结果类型（`02` §2.3/§2.5）；无隐式整数互换。
- 状态：✅ 已实现
- 证据：`semantic/mod.rs` `SType::Bool`、`infer.rs` L355-362

### 3.2.4 字符与字节

- 规则：**无独立 char 类型**——字符字面量 = **Unicode 标量码点，定型 comptime_int**（修订 D11，ADR-0037 2026-08-31；取代 D1「单 ASCII 字节」）。词法支持 `\u{...}` 转义与直接书写非 ASCII 字符（`'中'`，`01` §1.5）；ASCII 场景赋给 `u8` 行为不变（零迁移）；非 ASCII 码点赋给定宽整数超范围 = 编译期报错。显式 `char` 类型 = 后续提案。
- 状态：✅ 已实现（按 D11 裁决口径）
- 证据：`lexer/mod.rs` `lex_char`（u32 码点 + `\u{...}`）；`parser/ast.rs` `Expr::CharLit(u32)`；`infer.rs` `Expr::CharLit` → comptime_int + `check_int_width_value`（码点范围检查）

### 3.2.5 void 与 comptime 类型名

- 规则：`void` 仅函数返回位置（「无值」，`01` §1.12）；`comptime_int`/`comptime_float` 可作类型名书写（惰性整数/浮点的显式写法）→ 定型规则同 §3.2.1/§3.2.2。
- 状态：✅ 已实现
- 证据：`resolve.rs`（`comptime_float` 映射；`comptime_int` → Comptime 宽度）

## 3.3 数组与切片

- 规则：
  - 定长数组 **`[N]T`**——类型位 N 必须是整数字面量；✅ 实现文法即此（`native-types.md` 的 `[T, N]` 是实现快照笔误，非语法）。
  - 类型函数中的数组类型值 `[n]T`（n 可为 comptime 参数）属表达式路径 `Expr::ArrayType`——归 `10-meta.md`。
  - 切片：`&[T]` 只读 / `&mut [T]` 可写；取段 `&arr[1..3]` / `&arr[2..]`（`02` §2.6，开上界）。
  - **引用形态 `&T` / `&mut T`**：实现接受，映射为切片节点（`parser/mod.rs` L104-106 注释「引用类型 &T（Vec 等）」）——借用语义与所有权交互归 `07-ownership-memory.md`（⚠️ F2 注）。
  - 越界按模式检测（Q24）：Debug/脚本运行时 panic 带位置、Release 裸、编译期可证越界编译期报错（`02` §2.2 策略同源）。
- 状态：✅ 已实现
- 证据：`parser/type.rs` L36-59（`[N]T`，N 限 Int 字面量）；`semantic/mod.rs` `SType::Slice`

```hc
var arr: [3]i32 = [1, 2, 3];
var s: &i32 = &arr[0];            // 引用形态
var slice: &[i32] = &arr[1..];
// 错误示例：var bad: [i32, 3];     → 语法错误（数组记号为 [N]T）
```

## 3.4 指针

- 规则：`*T` 只读指针 / `*mut T` 可写指针；取地址 `&x`（→ `*T`）/ `&mut x`（→ `*mut T`，仅 `var mut` 目标，ADR-0005 修订）；所有权形态 `owned *mut T`（`01` §1.9.2，语义归 `07`）；自动解引用 `p.x`/`s[i]`、显式 `p.*`（`02` §2.8/§2.9）。
- 状态：✅ 已实现
- 证据：`parser/mod.rs` L76-87（Ptr + mut_）；`semantic/mod.rs`（Ptr 变体）

## 3.5 可选类型 `?T`

- 规则：`?T` = 可包含 `null` 或 T；`null` 字面量（`01` §1.12）；解包 `x.?`（唯一形态，E2）、兜底 `x orelse 默认`（`02` §2.10）；捕获 `if (opt) |v|` / `while (opt) |v|`（空则退出，Q10）。表示与空检查语义（tag/判别实现）为运行时细节，不在语法规范展开。
- 状态：✅ 已实现
- 证据：`parser/mod.rs` L108-113（Optional）；`semantic/mod.rs`（Optional 变体）

```hc
var maybe: ?i32 = null;
if (maybe) |v| { use(v); } else { ... };
```

## 3.6 错误联合 `!T` / `E!T`

- 规则：`!T` = anyerror!T；`E!T` = 命名错误集 E 的联合；错误集定义 `const E = error{...};`（`01` §1.10）；传播 `try`、处理 `catch`/`orelse return`（`02` §2.10）；错误模型全集 → `08-errors.md`。
- 状态：✅ 已实现
- 证据：`parser/mod.rs` L114-126（ErrorUnion None/Some）

## 3.7 元组类型

- 规则：`(T1, T2, ...)`；值形态 `(a, b)`（`02` §2.9）；访问 `t.0`；解构 `var (a, b) = t`（`01` §1.9.3，元组只读）。
- 状态：✅ 已实现
- 证据：`parser/type.rs` L25-35（Tuple）；`semantic/mod.rs`（Tuple 变体）

## 3.8 命名、限定与泛型类型

- 规则：
  - 命名类型直接书写；限定名 `Orders.Line`（命名空间限定，双注册后按全名查找）。
  - 泛型实例 `Name<T1, T2>`——实参可为类型或 **comptime 整数字面量**（`ArrayLen<i32, 3>` 的 `3` → `Type::ComptimeInt(3)`，按 `n: comptime_int` 参数绑定）。
  - **FnN 特例**：`Fn1<i32> i32`（闭包/函数指针类型，返回类型直接跟在 `>` 后、并入参数表）——详见 `05-functions.md`。
  - 表达式位置的类型实参识别名单限制见 `02` §2.18。
- 状态：✅ 已实现
- 证据：`parser/type.rs` L60-109（限定/泛型/ComptimeInt 实参/FnN）

## 3.9 类型推断（Q-S9 → H1 → **修订 D8，ADR-0037 2026-08-31**）

- 规则：
  - **必须显式**：函数参数类型（含 `*`/`*mut`/`owned` 形态）；**函数返回类型**（H1：必须标记，省略报错）；class/struct 字段类型；接口实现标注；`owned` 不参与推断。
  - **变量绑定推断 ✅（修订 D8，ADR-0037 2026-08-31；取代 H1 的「注解必填」排期）**：`var x = init` 省略注解，类型从初始化表达式推断（`declared.or(init_ty)`）；**仅无法推断时报诊断**（无初始化器且无注解 → `cannot infer type`）；`global` 有初始化器可推断，无初始化器（零值）必须显式。指针形态推断（`var x = &mut t` → `*mut T`）随推断路径成立。
  - **字面量惰性宽度**：✅ 保留（字面量在使用处定型，如 `var a: i32 = 42` 的 42 定型 i32；超范围编译期报错）——属字面量定型，非变量推断。
  - 泛型参数推断（`anytype`/`where T`）属泛型实例化机制，不在本条（`06-interfaces.md`）。
- 状态：✅ 已实现（D8）
- 证据：`infer.rs`（Comptime 惰性定型）；`check.rs` check_stmt VarDecl（`declared.or(init_ty)` + `cannot infer type` 诊断）；D8 + ADR-0037

## 3.10 双模式与检测策略

- 规则：**Debug/脚本模式全检测**（越界/溢出/悬垂 → 运行时 panic 带位置）；**Release 裸路径零开销**；编译期可证明的错误在所有模式编译期报错（Q24/Q-S6 总策略）。覆盖矩阵核对 → `12-testing.md`（Q-T5 双模式测试矩阵）。
- 状态：⚠️ 策略 ✅ 定案，逐项覆盖证据在 `12-testing.md` 收口
- 证据：`06-01` Q24/Q-S6 历史定案；`02` §2.2/§2.18

## 3.11 变更记录（相对旧 06-02-types.md）

| 变更 | 依据 |
|---|---|
| 数组记号确认 `[N]T`；`native-types.md` 的 `[T, N]` 判为快照笔误 | 实现 `parse_type_base` L36-59 |
| 浮点明确「单一 f64 表示」→ F1 裁决推翻：f16/f32 升级真实宽度（⏳）、f128 废弃 | `resolve.rs` L80-85 现状 + 项目所有者裁决（2026-08-30） |
| 补录 `&T`/`&mut T` 引用形态（映射切片节点，借用语义归 07） | `parser/mod.rs` L104-106 |
| 补录 `?T`、`!T`、`E!T` 完整文法 | `parser/mod.rs` L108-126 |
| 补录泛型实参 comptime 整数字面量、FnN 返回类型特例 | `parser/type.rs` L67-104 |
| 字符字面量 = u8（D1 裁决，旧「comptime_int 惰性宽度」对 char 的表述废弃） | 裁决 D1 + `lex_char` |
| 明确无隐式跨宽度转换（@intCast 显式） | semantic 层无 coerce/widen 路径 |
| u128/usize 纳入整数清单（旧文档未列全） | `IntWidth` 全集 |
| String 归位 `04-extended-types.md`（布局矛盾在该模块裁决） | 禁止双写 |

## 3.12 裁决记录（2026-08-30，项目所有者）

| # | 条目 | 裁决 | 影响 |
|---|---|---|---|
| F1 | 浮点宽度体系 | **f16/f32 升级为真实宽度**（⏳，backlog #5）；f64 维持；**f128 废弃**（如需单独立项）；comptime_float 可定三宽度；跨宽度 @floatCast 显式 | §3.2.2、`13-builtins.md`、LLVM/VM 后端 |
| — | F2（非争议，核查注）：`&T` 借用形态与所有权的交互规则 | 在 `07-ownership-memory.md` 盘点时核对并收口 | `07` |
