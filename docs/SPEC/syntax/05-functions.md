# 05 函数

> 大模块：函数 | 对齐状态：**✅ 对齐完成（2026-08-30，H1–H3 裁决落定）** | 初稿：2026-08-30
>
> 事实基础：**ADR-0009**（函数重载与可选参数）、ADR-0020（extern）、ADR-0022 §8（扩展方法暂缓）、ADR-0027（容器读写权限）、定案 Q-S9/Q13、历史 `06-04-functions.md`（已废弃）、tag1 实现（`parser/decl.rs` `finish_fn_decl`/`parse_fn_rest`/`parse_params`、`parser/expr.rs` `parse_closure`、`ir/runtime.rs` 分派、`semantic/infer.rs` `match_overloads`）。
> 证据总库：`tag1/hc/tests/frontend.rs`、`tag1/hc/tests/owned_check.rs`。

## 5.1 函数声明

- 规则：
  - `fn 名称[<T, U, ...>](参数表) [返回类型] [where 约束] { 体 }`；声明前缀 `pub` / `export`（`01` §1.8）。
  - **泛型参数表** `<T, U>`：仅声明类型参数名；约束一律走 where 子句（M2.2）；`<T: type>` 型内联约束暂不支持。
  - 函数名一般须是标识符；**例外放行关键字**：`where`、`null`、`script`、`type`（`expect_name_or_keyword`）。
  - 同名声明 = 重载候选（§5.5）。
- 状态：✅ 已实现
- 证据：`parser/decl.rs` `parse_fn_rest`（L600-680，泛型表 L618-631 + 注释「`<T: type>` 暂不在此表」）；`parser/expr.rs` `expect_name_or_keyword`（L952-983）

## 5.2 参数

- 规则：
  - 参数形态：`[var [mut]] [owned] 名称: 类型 [= 默认值]`——**`owned` 修饰参数名（名称前，K1/ADR-0036）**；所有权语义按形态判定（`T`/`mut T` 必定拥有、`*T`/`*mut T` 借用、值类型 + owned = 编译错误）→ **`07` §7.1.1**。
  - **可选参数**：尾部、默认值 = **编译期常量表达式**（ADR-0009）；调用点缺失尾参自动补齐。
  - `var [mut]` 前缀：输出参数形态（如 `var mut out: Vec<u8>`）——语法 ✅；可写性语义（ADR-0027 容器读写权限 = 变量绑定）归 `07-ownership-memory.md` 核对。
  - 参数类型**必须显式**（推断不适用于参数，`03` §3.9）。
- 状态：⚠️ `owned` 名称前缀待实现（backlog #16）；其余 ✅
- 证据：`parse_params`（L682-727，var mut 前缀 L688-700、默认值 L704-709）；`ir/models/ir_func.rs`（ADR-0009）；裁决 K1 + ADR-0036

```hc
fn add(a: i32, b: i32 = 0) i32 { return a + b; }
fn fill(var mut out: Vec<u8>, n: i32) void { ... }
```

## 5.3 返回类型

- 规则（裁决 H1，2026-08-30）：返回类型**必须显式标注**——`fn f() void { }` 也须写 `void`；省略 = 编译错误（实现当前允许省略 = void → 改报诊断，backlog #9）。
- 旧定案 Q-S9「返回类型可推断（单返回路径/一致类型）」**收回**（推断整体延后，见 `03` §3.9）。
- `async fn` 的调用点返回 `Future(R)`（R = 声明返回类型）——并发语义归 `11-concurrency.md`。
- 状态：⚠️ 规范已定，实现待改（backlog #9）
- 证据：`parse_fn_rest` L635-639（当前无 `{` 才解析返回类型）

## 5.4 where 子句

- 规则：`where T: 接口, U: 接口`——位于参数表/返回类型之后、函数体之前；逗号分隔；调用点做约束验证（M2.2）。接口契约见 `06-interfaces.md`。
- 状态：✅ 已实现
- 证据：`parse_fn_rest` L640-660；`semantic/infer.rs` L1223（调用检查 = 重载匹配 + where 约束验证）

```hc
fn add(a: *T, b: *T) T where T: INumber { return a.add(b.*); }
```

## 5.5 函数名唯一与重载（裁决 2026-08-30）

- 规则（**现行规则——简单签名**）：同一作用域（全局 / namespace / class 体内）下**函数名不得相同**——同名声明 = 编译错误，**不比对参数**（项目所有者裁决：签名判断只看名称）。
- **完整重载 = ⏳ 后续目标**（ADR-0009 机制——同名多候选 + ①参数数精确 ②实参类型匹配具体优先泛型 ③尾参默认回退——已在实现中存在；规范收紧后需补同名冲突诊断 → backlog #10，机制保留待启用）。
- 不受影响的原则：`[test]` 函数不入重载池；命名空间函数按**限定名**登记（`jsonlib.parse`）；顶层函数**文件私有**（兄弟文件不并入同名池，M1.4）；无用户运算符重载（`02` §2.2）。
- 状态：⚠️ 规范收紧，实现待改（当前允许多候选 → backlog #10）
- 证据：`ir/runtime.rs` L5-6（分派 ①②③，重载启用后生效）；`ir/lower_impl.rs` `register_func` L594-603；`semantic/collect.rs` L274-284

```hc
fn f(a: i32) void { ... }
fn f(a: String) void { ... }   // ✋ 现行规则：同名冲突 = 编译错误（重载启用前）
fn g(a: i32, b: i32 = 1) void { ... }
g(1);                          // 尾参默认回退（可选参数不受名称唯一影响）
```

## 5.6 闭包

- 规则：
  - 语法：`|v| 表达式` / `|v, w| { ... }`；**捕获模式前缀**：`mut |v| ...`（可写捕获）、`move |v| ...`（转移捕获）——**可叠放**（`mut move |v|`）；捕获粒度 = 整个闭包（单变量粒度留 1.x）。
  - 闭包类型 = `FnN` 接口族（Q13：`Fn0`..`FnN` 调用接口；类型形态 `Fn1<i32> i32`，`03` §3.8）。
  - 闭包函数**独立于**重载池（`closures` 表，绝不参与按名分派）。
- 状态：✅ 已实现（FnN 接口契约归 `06-interfaces.md` 核对）
- 证据：`parser/expr.rs` `parse_closure`（L842-888，前缀叠放 L853-862）；`ir/models/ir_module.rs` L9-12（closures 注释）

```hc
var inc = |x: i32| x + 1;
var addn = move |x: i32| x + n;    // 转移捕获 n
```

## 5.7 extern fn（ADR-0020 A1）

- 规则：`extern fn 名称[<T>](参数) [返回类型];`——**纯声明**：无函数体、以 `;` 结束、无 where 子句；链接期解析外部 C 符号（FFI）；不可 `export`（实现：export 只认 fn/async fn，extern 分支不带 is_export）。
- 状态：✅ 已实现（链接行为证据归 ADR-0020 实现核对）
- 证据：`parser/decl.rs` `parse_extern_fn_decl`（L534-598）

```hc
extern fn puts(s: *u8) i32;
```

## 5.8 方法与扩展方法

- 规则：
  - **方法** = class 体内 `fn`（`04` §4.1）；接收者为**惯例首参** `self: *T`（可写 `self: *mut T`）——无语言级接收者关键字；调用 `obj.method(args)`（`02` §2.9）；方法可变接收者检查（ADR-0027：容器读写权限 = 变量绑定）归 `07`。
  - **扩展方法/扩展函数**（裁决 H3，2026-08-30：**确认实现**，取代 ADR-0022 §8 的「暂缓」）：`[Extension(类型名)] fn ...`——可为**任意类型**（含内建类型）扩展方法；约束沿用 ADR-0022 Q15：**不能访问私有字段**；调用形态同方法（`p.dist(q)`）；内建类型的调用路径与 `self` 参数形态归实现核对。
- 状态：⚠️ 规范确认（语法 ✅ 已实现；语义/调用路径核对中）
- 证据：`parser/decl.rs` `parse_extension_trait`（L12-18）+ `finish_fn_decl` `extension_of`；`semantic/infer.rs` L1363-1373（类型限定名方法分派 + `check_method_mutability`）；裁决 H3（2026-08-30）

```hc
[Extension(Point)] fn dist(self: *Point, other: *Point) f64 { ... }
// 调用：p.dist(q)
```

## 5.9 async fn

- 规则：`[pub] [export] async fn 名称(参数) R { ... }`——调用点返回 `Future(R)`；`await` 解包（`02` §2.1 前缀层）；执行模型归 `11-concurrency.md`。
- 状态：✅ 语法实现（Future 模型证据归 `11` 核对）
- 证据：`parse_decl` L104-109（KwAsync → finish_fn_decl(is_async=true)）；`semantic/mod.rs` `make_sig`

## 5.10 内联函数 `[Inline]`（新特性 H2，⏳）

- 规则（裁决 H2，2026-08-30）：`[Inline] fn f(...) ...`——内联函数；**所有调用点在编译期将函数体插入调用位置**（非提示、非优化器启发：内联函数的全部调用点必须展开）。
- 语法形态（裁决确认，2026-08-30）：**`[Inline]` 特性标注**（与特性系统 `[Align]`/`[Test]`/`[Extension]` 同体系，不新增关键字）。
- 细则（实现期定）：递归函数不适用内联展开；内联函数与重载/名称唯一规则的交互；内联后局部所有权与作用域销毁的展开语义（`07` 衔接）。
- 状态：⏳ 未实现·目标（backlog #11）
- 证据：无（设计定案，待实现后回填）

```hc
[Inline] fn sq(x: i32) i32 { return x * x; }
var a: i32 = sq(3);   // 编译期展开为 3 * 3
```

## 5.11 变更记录（相对旧 06-04-functions.md）

| 变更 | 依据 |
|---|---|
| 重载分派 ①②③ 明确成文（具体优先泛型、尾参默认回退） | ADR-0009 + `ir/runtime.rs` 分派注释 |
| `[test]` 不入重载池补录 | `semantic/collect.rs` |
| 闭包捕获前缀可叠放（mut + move）、粒度 = 整闭包补录 | `parse_closure` |
| 闭包独立于重载池补录 | `ir_module.rs` closures 注释 |
| 函数名关键字例外（where/null/script/type）补录 | `expect_name_or_keyword` |
| extern fn 不可 export 明确 | `parse_decl` export 校验 |
| 返回类型省略 = void（旧 Q-S9「返回类型可推断」待裁决 H1） | `parse_fn_rest` L635-639 |
| 扩展方法：语法超前于 ADR-0022 §8 暂缓定案，状态 ⚠️ → **H3 裁决确认实现（任意类型，2026-08-30）** | `parse_extension_trait` + 裁决 H3 |
| **返回类型必须显式标注**（省略报错）；Q-S9「返回类型可推断」收回 | 裁决 H1（2026-08-30）→ backlog #9 |
| **函数名唯一先行**：同作用域同名冲突（不看参数）；ADR-0009 完整重载降为 ⏳ 目标 | 项目所有者裁决（2026-08-30）→ backlog #10 |
| **新增内联函数 `[Inline]`**（⏳） | 裁决 H2（2026-08-30）→ backlog #11 |
| `move` 形态修订（调用点显式；ADR-0030 语义）→ `07-ownership-memory.md` 收口 | 禁止双写 |

## 5.12 裁决记录（2026-08-30，项目所有者）

| # | 条目 | 裁决 | 影响 |
|---|---|---|---|
| H1 | 返回类型与推断 | **返回类型必须标记**；**变量类型推断 = 高级功能，自举后实现**（⏸，见 `00-index.md` 排除列表）；Q-S9 相应条款收回 | §5.3、`03` §3.9、`01` §1.9.1 |
| H2 | 内联函数 | **新增**：调用点编译期插入函数体；语法形态推荐 `[Inline]`（待确认） | §5.10、backlog #11 |
| H3 | 扩展函数 | **新增/确认**：`[Extension(T)]` 为任意类型扩展方法；不能访问私有字段（Q15 维持） | §5.8 |
| — | 函数签名 | **先行简单规则**：同作用域下函数名唯一（只判断名称）；完整重载 = ⏳ | §5.5、backlog #10 |
