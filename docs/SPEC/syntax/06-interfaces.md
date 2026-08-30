# 06 接口与标量接口族

> 大模块：接口与标量接口族 | 对齐状态：**✅ 对齐完成（2026-08-30，I1 裁决落定）** | 初稿：2026-08-30
>
> 事实基础：定案 Q14/Q17/M5/Q22/Q22b/Q-R9/R-4/H3/Q13/C4/L4（原载 `06-05-interfaces.md`，已废弃）、通用接口定案（2026-08-29）、tag1 实现（`semantic/infer.rs`、`ir/builtin.rs`、`ir/iter.rs`、`ir/models/cell.rs`、`codegen/llvm/text.rs`）。
> 证据总库：`tag1/hc/tests/frontend.rs`、`tag1/examples/86-scalar-interfaces.hc`（历史示例）、`tag1/hc/tests/thread_capture.rs`。

## 6.1 接口声明

- 规则：
  - `interface 名称[: 父接口, ...] { 方法声明... }`——方法 = 完整签名（泛型参数表/参数/返回/where）+ `;`，**无方法体**（纯契约）；`Self` = 实现类型（Q14）。
  - 接口方法为**成员契约**，不参与全局重载解析（2026-08-14 定案；函数名唯一规则见 `05` §5.5）。
  - **实现标注**：`class X: I1, I2 { ... }`（冒号后缀，逗号分隔）；泛型接口可实例化标注（C4）：`class Fib: IIterable<i32>`。
  - 可实现/被实现的主体：class（显式标注）、标量与内建类型（**编译器内建实现**，不可用户重载）。struct 无方法体 → 无用户实现途径（`[Extension]` 能否为 struct 补方法从而满足契约——归 `05` §5.8 联动核对）。
  - **三用途**（Q-R9）：① 标记 class 功能；② 参数约束 `where T: IShape`；③ 类型参数编译期验证。
- 状态：✅ 已实现
- 证据：`parser/type_decl.rs` `parse_interface`（L312-348，supers/方法/无体）；`parse_class` 冒号接口列表（L16-27）；`semantic/infer.rs` `implements`

```hc
interface IShape {
    fn area(self: *Self) f64;
}
class Rect: IShape {
    w: f64, h: f64,
    fn area(self: *Rect) f64 { return self.*.w * self.*.h; }
}
```

## 6.2 接口指针与胖指针（Q17 + M5 定案，实现 G3）

- 规则：
  - `*I` = 只读接口引用、`*mut I` = 可写接口引用（接口 = 类型标注）。
  - 接口指针 = **胖指针，三字宽**：`data`（实例）+ `vtbl`（虚表/具体类型）+ `alloc`（分配器引用——销毁 `owned *I` 时用携带的 alloc 释放 data，M5）。
  - `box(rect, alloc)` 装箱赋给接口指针 = **编译期自动收窄**（实现检查通过即合法）；data 部分参与 Debug 悬垂标记，vtbl 静态不参与。
- 状态：⚠️ 部分——IR/解释器 ✅（`Cell::Boxed` 三字宽 + `Boxed.alloc()`）；**LLVM 用户类型方法动态分派 = 硬错误**（Phase 4 待做，内建类型可用）。**校订（J1/ADR-0035，2026-08-30）**：box 改返回 `owned T`、显式销毁——本节「box(rect, alloc) 装箱自动收窄」的表述随实现核对后修订；接口胖指针机制本身保留。
- 证据：`ir/models/cell.rs` L22-26（G3 注释）、`ir/models/ir_value.rs` L19-23；`exec_call_dynamic` L1082-1092；`codegen/llvm/text.rs` L1758-1759（NotIterable 硬错误注释）

## 6.3 接口参数与静态分发（Q22/Q22b 定案）

- 规则：
  - 接口类型传参 = **带约束的虚拟类型 T**，约束放签名末尾 where 子句：`fn add(a: *T) void where T: INumber`。
  - 形态映射：`&T` → `*T`（只读）/ `&mut T` → `*mut T`（可写）/ `move T` → `owned T`（拥有）；调用点显式：`add(&a)` / `add(&mut a)` / `add(move a)`。
  - **静态分发（单态化、无虚表）为主路径**；动态分发（胖指针装箱）保留给异构集合。
  - 接口工厂返回具体实现类型（R-4 定案）：`Io.threaded() -> ThreadedIo`——具体类型参与 `T: Io` 单态化；具体类型值传接口句柄参数自动装箱。Io 契约归 `09-modules-entry.md`（io 模块）/`11-concurrency.md`。
- 状态：✅ 定案（单态化证据归泛型实例化核对）
- 证据：`ir/runtime.rs` `pick_func` L57-67（泛型 T 约束「编译时验证归 M2」）；`06-05` 历史 Q22/Q22b/R-4

## 6.4 标量接口族（2026-08-14 定案）

- 规则：

```hc
interface ICompare {
    fn eq(self: *Self, other: Self) bool;
    fn lt(self: *Self, other: Self) bool;
}   // ne/le/gt/ge 由编译器派生

interface INumber: ICompare {
    fn add(self: *Self, other: Self) Self;
    fn sub(self: *Self, other: Self) Self;
    fn mul(self: *Self, other: Self) Self;
    fn div(self: *Self, other: Self) Self;
    fn neg(self: *Self) Self;
    fn pow(self: *Self, exp: Self) Self;   // H2 裁决（2026-08-30）：pow 并入 INumber 族（** 运算符绑定）
}

interface IInt: INumber    { fn mod(self: *Self, other: Self) Self; fn abs(self: *Self) Self; }
interface IUint: INumber   { fn mod(self: *Self, other: Self) Self; }
interface IFloat: INumber  { fn abs(self: *Self) Self; }
```

  - 内建标量编译器内建实现：整数（`i8`–`i128`/`isize`）→ `IInt`；无符号（`u8`–`u128`/`usize`）→ `IUint`；浮点 → `IFloat`；**String 内建实现 `ICompare`**（内容序）；`bool`/`char`/`void`/指针**不实现 INumber 与 ICompare 序比较**（裁决 I1，2026-08-30：bool/char 走字符串转换，见 §6.5）。
  - **序比较细则**：仅实现 `ICompare` 的类型可 `< <= > >=`；bool/char 序比较 = 编译错误（实现当前放行 bool → 改报错，backlog #12）。
  - **运算符绑定**（H3 定案，`02` §2.2/§2.5 交叉引用）：`a + b` ≡ `a.add(b)`；`%`/`%%` 编译器派生；`==`/`!=` = 值比较（内部 ICompare）；序比较绑定 `ICompare`——**未实现则编译错误**。
  - 比较细则：指针比较 = 指向对象地址（装箱胖指针 = 同 cell 身份）；数组/切片含位置 + 长度（实现核对注：装箱 `Boxed` 比较已证实身份语义，普通数组逐元素比较待核对）。
  - **`**` 幂**（H2 裁决）：`a ** b` ≡ `a.pow(b)`，绑定 INumber 族（`02` §2.2.1；IFloat 原有 `pow` 由族继承覆盖，不双写）。
- 状态：✅ 已实现（标量方法内建：add/sub/mul/div/neg/mod/abs/eq/lt/pow）
- 证据：`ir/builtin.rs` `call_scalar_method_ir`（L2044-2045 注释 + L3100-3110 分派）；`semantic/infer.rs` `check_binary` L830-838（序比较绑定 ICompare）；`pow` 位置 = 裁决 H2

## 6.5 通用接口（2026-08-29 定案：所有类型默认实现）

- 规则：

```hc
interface IToString  { fn to_string(self: *Self) String; }     // 文本化统一入口
interface IHashCode  { fn get_hashcode(self: *Self) i32; }     // 哈希契约（Map 分桶等）
```

  - **所有 struct/class 与内建类型自动实现**（编译器内建合成），无需声明；`where T: IToString` / `where T: IHashCode` 对任意类型成立。
  - 默认语义：`to_string` = 类名（class 实例）/ display 形式（标量/Str/容器）；**`bool` → `"true"`/`"false"`；`char` → 单字符字符串**（裁决 I1，2026-08-30，实现待补 → backlog #13）；`get_hashcode` = FNV-1a32(`to_string` 结果)。
  - 用户同名方法覆盖默认实现（签名不变，方法解析优先用户方法）。
- 状态：⚠️ 部分——语义放行 + interp/IR ✅；**LLVM 原生后端未合成**（需新 `@hc` helper，后续任务）
- 证据：`ir/builtin.rs` L3104-3110（to_string/get_hashcode 内建实现 + 2026-08-29 注释）

## 6.6 迭代契约 IIterable（2026-08-14 定案）

- 规则：
  - 按**元素访问形态**三态（泛型实例化语法与 `Vec<i32>` 一致）：`IIterable(*T)` 只读（默认 `for (x) |item|`）/ `IIterable(*mut T)` 可写（`|mut item|`）/ `IIterable(owned T)` 拥有（`|move item|`）。
  - 契约方法：`next(self: *mut Self) ?T`（按对应形态返回元素/空）；元素类型与形态由 next 推断。
  - 内建类型（数组/切片/Vec/Map/Table/String）编译器内建实现三态；String 迭代产出**字节**（u8 Int）。
  - **用户类型实现 `next` 方法即可参与 `for`**（实现即接口——IIterable 三态）。
  - **拥有迭代语义**（M4 定案）：`for (x) |move item|` = 迭代器持有容器所有权——x 被 move 进迭代器、next 逐元素转移、迭代后容器不可再用；内建实现 = Vec/String/Deque 逐个 pop 转移、数组逐元素 move。
  - 值类型自动取引用（L4 定案）：`for (fib)` ≡ `for (&mut fib)`——语法见 `02` §2.14，契约收口于本条。
  - `arr.iter()` = **显式迭代器对象**（可传递）；**组合子 `iter`/`filter`/`map` 已实现**（旧文档「留 1.x」过时；惰性/急切语义归实现核对 ⚠️）；一次性迭代器即 1.0 形态。
- 状态：✅ 已实现（组合子语义 ⚠️ 待核对；LLVM 用户类型迭代 = 硬错误，见 §6.2）
- 证据：`ir/iter.rs` L8-9 + `make_iter` L95-102（用户 next 展开）；`semantic/infer.rs` `check_iterable` L770-773；`ir/builtin.rs` L3270-3276（`iter|filter|map` 分派）；`iter_to_arr_ir` L1568

```hc
class Fib {
    fn next(self: *mut Fib) ?i32 { ... }    // 实现 next 即可 for
}
for (f) |move v| { consume(v); }            // 拥有迭代：迭代后 f 不可再用
```

## 6.7 序列化内建契约（2026-08-14 定案：序列化 = 默认接口）

- 规则：
  - **连续类型 ↔ bytes**：`to_bytes`/`from_bytes` 内存直映射（仅连续类型；`packed`/`align(N)` 布局尊重，`@offsetOf`/`@alignOf` 可验证）。
  - **堆类型 ↔ JSON**：`to_json`/`from_json` 内建；脚本定制通道归 `10-meta.md`（E1）。
  - **集合 → 字节**：Vec/Map/Table/切片 → 长度前缀 u64 LE + 元素字节。
  - 序列化为**编译器实现的内建契约**，用户不可重载。
- 状态：⚠️ 部分——String/数组 `to_bytes`/`from_bytes` ✅（u64 LE 前缀）；class `to_json`/Map `to_json`/`from_json` ✅；**class `to_bytes` = 硬错误**（`Unsupported: class to_bytes requires type layout`——符合「堆类型走 JSON」口径，但连续 struct 的直映射待核对）；**数组聚合元素序列化为空**（Phase 7 取舍 ⚠️）
- 证据：`ir/builtin.rs` L2595-2599（String.to_bytes）、L2920-2955（数组 from_bytes/to_bytes，u64 LE）、L3267-3276（class to_bytes 硬错误 / to_json）；`ir/json.rs`

## 6.8 FnN 调用接口（Q13 定案）

- 规则：闭包/函数值的调用契约 = `Fn0`..`FnN` 内置调用接口；类型形态 `Fn1<i32> i32`（参数类型表 + 返回，`03` §3.8 特例文法）；闭包按此契约调用（`f(x)` 语法糖）。
- 状态：⚠️ 语法 ✅，语义（FnN 类型检查/闭包签名匹配）核对中
- 证据：`parser/type.rs` L96-104（FnN 返回类型并入）；`codegen/llvm/tests.rs` L567（`hc_fn1` 命名）

## 6.9 变更记录（相对旧 06-05-interfaces.md）

| 变更 | 依据 |
|---|---|
| `pow` 定位修订：并入 INumber 族（H2 `**` 裁决），IFloat 原有 pow 由族覆盖 | 裁决 H2（2026-08-30） |
| 组合子 `iter`/`filter`/`map` 已实现——旧「留 1.x」过时 | `ir/builtin.rs` L3270-3276 |
| 胖指针实现证据收口（三字宽 data+vtbl+alloc，G3） | `cell.rs` L22-26 |
| 通用接口 IToString/IHashCode 实现证据收口（interp/IR ✅、LLVM ⚠️） | `builtin.rs` L3104-3110 |
| 序列化实现现状明确（class to_bytes 硬错误、数组聚合元素空序列化） | `builtin.rs` L3267-3276 |
| 用户类型「实现 next 即可迭代」写明（实现即接口） | `check_iterable` L770-773 |
| String 方法集（concat/split/find/substring/replace/to_upper/to_lower/len 等）补录 `04` §4.7 | `builtin.rs` L3132-3136 |
| struct 接口实现途径标注待核对（无方法体；`[Extension]` 联动） | `parse_struct` 无冒号列表 |

## 6.10 裁决记录（2026-08-30，项目所有者）

| # | 条目 | 裁决 | 影响 |
|---|---|---|---|
| I1 | bool/char 接口地位 | **实现 IToString 转换接口**：bool → `"true"`/`"false"`，char → 单字符字符串；**不实现序比较**（bool 序比较现状放行 → backlog #12 收紧报错） | §6.4/§6.5、backlog #12/#13 |
