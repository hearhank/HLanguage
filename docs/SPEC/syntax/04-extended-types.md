# 04 扩展类型

> 大模块：扩展类型 | 对齐状态：**✅ 对齐完成（2026-08-30，G1–G4 裁决落定）** | 初稿：2026-08-30
>
> 事实基础：**ADR-0022**（struct 类型与特性系统，2026-08-24 定案 Q1–Q27）、ADR-0014 K1（无标签 union）、ADR-0015（Table 密封表 init_with，2026-08-22）、ADR-0027（容器字面量）、历史 `06-03-extended-types.md`（已废弃）、tag1 实现（`parser/type_decl.rs`、`parser/decl.rs` 特性注册、`codegen/llvm/preamble.rs`）。
> 证据总库：`tag1/hc/tests/frontend.rs`、`tag1/hc/tests/infinite_size.rs`。

## 4.1 class

- 规则：
  - `[特性...] class 名称[: 接口1, 接口2] { 成员 }`——**堆上引用类型**（默认，ADR-0022 §1）。
  - 成员两类：**方法**（`fn` 完整签名：泛型参数表/参数/返回/where/体；**默认公开**）与**字段**（`[pub] name: Type,`——**默认私有**，`pub` 显式导出，Q3）。
  - 字段**无默认值**（默认值为 struct 专属，§4.2）；冒号后接口列表 = 接口实现声明（契约见 `06-interfaces.md`）。
  - **字段不支持名称前 `mut` 标注**（裁决 G3，2026-08-30：`mut` 是**变量声明**的可写标注；字段可写性经**类型形态 `mut T`** 表达——K1/ADR-0036，所有权语义 → `07` §7.1.1）。class/struct/union 三处实现当前静默忽略字段 `mut` → 改报诊断，见 backlog #8。
  - **字段修饰**：`[pub] [owned] 名称: 类型`——`owned` 名称前缀（K1/ADR-0036，所有权语义 → `07` §7.1.1）；**✅ 已落地（2026-08-31）**，值类型 + owned = 编译错误（`semantic/check.rs` check_decl）。
  - ❌ `[continuous]` 特性已删除（ADR-0022 §10 Q21/Q22）——实现未注册该处理器（注册表仅 pad/module/align/test/extension），使用即报未知特性错误。
- 状态：✅ 已实现（[continuous] ❌ 按定案移除）
- 证据：`type_decl.rs` `parse_class`（L9-88）；`decl.rs` `register_system_trait_handlers`（L51-57，无 continuous）

```hc
class Person {
    name: String,               // 私有字段
    pub age: i32,               // 公开字段
    fn greet(self: *Person) void { io.print("hi"); }   // 方法默认公开
}
class Point: ICompare { x: f64, y: f64 }   // 接口实现声明
```

## 4.2 struct（ADR-0022 定案）

- 规则：
  - `[Align(n)] struct 名称 { 字段 }`——**天然连续内存值类型**：栈分配、C ABI 兼容布局（字段顺序/padding 与 C 一致，Q1）、`@sizeOf`/`@offsetOf` 行为与 C 一致。
  - **仅字段**（无方法、无接口列表——与 class 的关键差异）。
  - 字段：`[Align(n)] [pub] name: Type [= 默认值],`（不支持 `mut` 标注，G3）；字段默认值（Q13）——`ABC{}` 使用默认值初始化；字段级对齐（Q12）。
  - **字段类型限制**（Q3）：仅标量、定长标量数组、嵌套 struct。
  - 类型级 `[Align(n)]`：n ∈ 1, 2, 4, 8（Q2/Q11）；语义 = 末尾对齐（C ABI，Q19）；默认 = 自然对齐。
  - 分配模型：栈 `var p = Point{x = 1, y = 2.1};`（生命周期绑作用域）；堆 `alloc.init(Point)` / `alloc.init(Point{...})`；装箱 `box p` → 堆 + 指针，RAII 自动释放、所有权绑当前作用域（Q7/Q14；`box` 归 `13-builtins.md`，所有权归 `07`）。默认全局页分配器（Q16）。
  - 注：ADR-0022 示例中的 `let` 为笔误——语言声明关键字是 `var`（`01` §1.9）。
- 状态：⚠️ 部分实现——解析 ✅（字段/默认值/字段级 Align）；Q3 字段类型限制检查、C ABI 布局计算的证据归语义/LLVM 层核对
- 证据：`type_decl.rs` `parse_struct`（L90-152，字段 trait/默认值 L100-128）；`parser/expr.rs` L508-556（`struct{...}` 类型/值字面量）；ADR-0022

```hc
[Align(8)] struct Point {
    x: f64 = 0.0,          // 字段默认值
    [Align(4)] y: f32,
    tag: [4]u8,            // 定长标量数组
}
var p = Point{};           // 全默认值
var q = Point{x = 1.0, y = 2.0, tag = [0, 0, 0, 0]};   // 栈分配
var h = alloc.init(Point{x = 1.0, y = 2.0, tag = [0, 0, 0, 0]});   // 堆
```

## 4.3 enum

- 规则：
  - `enum 名称 { 变体[, 变体...] }`——合一式枚举。
  - 变体：`名` 或 `名: 载荷类型`（**单类型载荷**；多载荷用元组类型 `名: (i32, String)`）；变体名可为关键字（如 `null`）。
  - 构造/访问：`Type.变体`；**推断枚举字面量 `.名`**——参数/字段类型已知时（L1 定案，`02` §2.9）。
  - switch 穷举与负载捕获见 `02` §2.15。
- 状态：✅ 已实现
- 证据：`type_decl.rs` `parse_enum`（L154-203，关键字变体名 L161-177、载荷 L178-183）

```hc
enum Kind { player, enemy }
enum Shape {
    circle: f64,              // 载荷 = 半径
    rect: (f64, f64),         // 多载荷用元组
    none,
}
fn area(s: Shape) f64 {
    switch (s) {
        Shape.circle => |r| 3.14159 * r * r,
        Shape.rect => |dims| dims.0 * dims.1,
        Shape.none => 0,
    }
}
```

## 4.4 union（ADR-0014 K1）

- 规则：`union 名称 { 字段 }`——**无标签联合**：字段内存重叠、无判别标签；仅字段（无方法、无接口）；成员可见性同 class（默认私有、`pub` 导出）；重叠读写语义在语义/运行时层落实。
- 状态：✅ 解析实现（重叠语义证据归语义/运行时核对）
- 证据：`type_decl.rs` `parse_union`（L207-248）及注释（K1，ADR-0014）

## 4.5 tree

- 规则（裁决 G2，2026-08-30）：`tree` = **保留关键字，功能暂不实现**（⏳）——语义方向（层级数据结构）待未来立项；当前实现与 class 同一解析路径（临时行为，不构成语义承诺）；规范代码不使用 `tree`（linter 可提示）。
- 状态：⏳ 暂不实现
- 证据：`parser/decl.rs` L128-136（`KwClass \| KwTree → parse_class`）

## 4.6 tuple

→ 见 `03-types.md` §3.7（类型）与 `01-lexical-declarations.md` §1.9.3（解构）。本模块不重复（禁止双写）。

## 4.7 String（裁决 G1，2026-08-30）

- 规则：`String` = **`&[u8]` 的类型别名**，**栈上分配**——无堆包装、无内部指针结构；字符串字面量 = 静态只读 `&[u8]`（与 String 同型，`01` §1.6）；拼接等操作产生新的字节缓冲（分配策略归实现/`07` 所有权衔接）；`==` = 内容比较（H3，与切片比较语义在 `06-interfaces.md` ICompare 处收口）。
- 现状：⚠️ 实现为 64 字节内联缓冲 `{ buf: [u8; 64], len: i64 }`（72 字节、值语义、无堆）且**长度超过 64 静默截断**——与「`&[u8]` 别名」裁决不符，整体改造 → backlog #7；**静默截断必须先行消除**（改诊断）。
- 状态：⚠️ 部分实现（backlog #7）
- 证据：`codegen/llvm/preamble.rs` L22-26；`codegen/llvm/text.rs` `HC_STR_CONCAT`（min(len, 64) 截断）；`semantic/mod.rs` `SType::Str`
- **内建方法集**（校订补充，2026-08-30；证据 `ir/builtin.rs` L3132-3136）：`concat` `split` `find` `substring` `replace` `len` `to_upper` `to_lower` `as_slice` `into_array` `to_bytes`；比较 `==` = 内容序（ICompare 内建，`06` §6.4）；迭代产出字节（`06` §6.6）

## 4.8 Table（ADR-0015 定案）

- 规则：
  - `Table<T>` = **密封二维表**，定长 `rows × cols`（构造时指定）；`native-types.md` 的「动态行 `add_row`」**无 ADR 依据、无实现**——判为虚构。
  - 访问：`t[i]` 行视图（→ 切片）、`t[i, j]` 单元格读写/复合赋值（多参索引，`02` §2.9，M8）。
  - 迭代：扁平迭代（逐单元格）；`len()` / `cols()`。
  - 序列化：`to_bytes` 双前缀（集合 → 字节，长度前缀 u64 LE 之上的表头）；空表合法；`copy` = 深复制；支持嵌套 `Table<Table<T>>`。
  - **密封构造**（ADR-0015 B 方案）：`Table<T>.init_with(alloc, rows, cols, 回调)`——回调 `|i, j, cell: *mut T|` 构造期写格；构造完成返回**编译期强制只读表**：直接赋值/复合赋值/`&mut t` 一律编译错误，**不可解除**（无 unseal）。动机：`Table<*mut T>` 构造后仍可写会破坏读安全。
  - 普通表（`init` 构造）元素替换当前允许；绑定级默认只读（A 方案）记 1.x 待办（依赖语义层 `VarInfo.mut_`，C4 条目）。
- 状态：⚠️ 部分实现——基础 init/索引存在；**`init_with` 密封未实现**（tag1 全文无 `init_with`/`add_row`，→ backlog #6）；行视图/单元格写的 C1 修复状态待核对
- 证据：ADR-0015（Q1–Q4 全定案）；`02` §2.9 多参索引；grep 证实 `init_with`/`add_row` 不存在于 `tag1/hc/src`

```hc
var t = Table<i32>.init(alloc, 4, 8, 0);        // 4 行 8 列，初值 0
t[1, 2] = 5;                                    // 单元格写
var row: &i32 = t[1];                           // 行视图
// ⏳ 密封构造（目标）：
// var sealed = Table<*mut T>.init_with(alloc, r, c, |i, j, cell: *mut T| { ... });
```

## 4.9 特性标注（attribute）系统（ADR-0022 §5）

- 规则：
  - 语法：`[名]` / `[名{field = 值, ...}]` / `[名(值)]`（单参简写，Q5）；参数 = 编译期常量表达式（数字/字符串/枚举，Q25）；**编译后完全擦除**（Q20）。
  - 系统特性注册表（Q24 字典式）：`pad` / `module`（→ 硬报错，ADR-0026）/ `align` / `test` / `extension`。
  - `[Align(n)]`：见 §4.2（struct 类型级 + 字段级）。
  - `[Extension(类型名)]`：扩展方法（`05-functions.md` 承接方法规则；ADR-0022 §8「不能访问私有字段」）；解析已实现，访问限制核对归 `05`。
  - `[test(...)]`：→ `12-testing.md`。
  - `[pad]`（裁决 G4，2026-08-30）：**系统特性**，归 IAttribute 特性体系（ADR-0022 §6），由编译器处理——**语义暂不实现**（⏳）；解析与注册保留。
  - 用户特性与编译器插件（`@import` 加载、`IAttribute`）：⏳（ADR-0022 §6/§7——第一阶段内置处理，插件 API 不暴露）。
- 状态：✅ 已实现（注册表五项；pad 语义 G4）
- 证据：`decl.rs` L10-57；`test_attribute.rs`；`semantic/trait_registry.rs`

## 4.10 变更记录（相对旧 06-03-extended-types.md）

| 变更 | 依据 |
|---|---|
| **class/struct 并存**（struct = 连续内存值类型；class = 堆引用）——取代旧「统一关键字 class、struct 已删除」表述 | ADR-0022 §1（2026-08-24）+ 实现 `parse_struct` |
| `[continuous]` ❌（旧文档速查仍示例 [continuous] class） | ADR-0022 §10 + 注册表无此处理器 |
| struct 字段默认值、字段级 `[Align(n)]` 补录 | ADR-0022 Q12/Q13 + `parse_struct` |
| enum 载荷明确「单类型 `名: Type`」；变体名可关键字 | `parse_enum` |
| `tree` 标注 ⏳ 暂不实现（G2：保留关键字，现行为 class 同路径临时行为） | 裁决 G2 + `parse_decl` L128 |
| String 定案「`&[u8]` 别名、栈上分配」；现状 64 内联 + 静默截断 → backlog #7 | 裁决 G1 + LLVM `%StringData` |
| `[pad]` 定案：系统特性、IAttribute 体系、语义暂不实现 | 裁决 G4 + ADR-0022 §6 |
| Table：以 ADR-0015 为准（定长 + 密封 init_with）；`add_row` 动态行判为虚构；init_with 标 ⏳ | ADR-0015 + grep 证实未实现 |
| union 独立成节（无标签、内存重叠） | ADR-0014 K1 + `parse_union` |
| 匿名 `struct {...}` 字面量占位归 `10-meta.md`（匿名类型） | `skip_anon_struct` 注释 |
| tuple 移交 `03` §3.7（禁止双写） | — |

## 4.11 裁决记录（2026-08-30，项目所有者）

| # | 条目 | 裁决 | 影响 |
|---|---|---|---|
| G1 | String 表示 | **`String` = `&[u8]` 的类型别名，栈上分配**；现状 64 字节内联缓冲与静默截断 → backlog #7（截断先改诊断） | §4.7、`01` §1.6、标准库 text/io、`07` |
| G2 | `tree` 关键字 | **保留关键字，功能暂不实现**（⏳）；现状同 class 路径仅为临时行为 | §4.5、`01` §1.2.1 |
| G3 | 字段 `mut` 前缀 | **`mut` 是变量声明的可写标注**，字段不支持；三处静默忽略 → backlog #8 改报诊断 | §4.1/§4.2/§4.4、`07` |
| G4 | `[pad]` 特性 | **系统特性（IAttribute 体系、编译器处理），语义暂不实现**（⏳）；解析注册保留 | §4.9、ADR-0022 §6 |
