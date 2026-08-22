# 通用语法规则（H 语言基本语法整合）

> **目的**：把散落于各 SPEC 的基本语法功能整合为**统一语法规则**，作为第三阶段（及后续自举对照）实现的通用参考。权威细节以 `docs/SPEC/` 对应文件为准（本文末尾标注出处）。
>
> 约定：`(...)` 可选 / `{...}` 重复 / `|` 选择 / 行内 `code` 为语法记号；语义要点 + 反例（_Avoid_）随条列出。

---

## 1. 词法规则（Lexical）

### 1.1 关键字表（冻结，2026-08-13 Q7 / 2026-08-14 修订）

| 类别 | 关键字 |
|---|---|
| 声明 | `var` `const` `fn` `global` |
| 控制流 | `if` `else` `while` `for` `break` `continue` `return` `switch` `defer` `errdefer` |
| 类型构造 | `class` `enum` `tree` `interface` `where`（`struct` 已并入 `class`） |
| 模块 | `namespace` `using`（已由 `import` 取代，ADR-0010）`pub` |
| 内存/所有权 | `o` `move` |
| 操作 | `and` `or` `try` `catch` `orelse` |
| 元编程 | `script` `comptime` `anytype` `type` |
| 并发 | `async` `await` `spawn` |
| 字面量 | `void` `null` `true` `false` |
| 修饰 | `mut`（可写修饰，Rust 标杆） |

**内建函数（非关键字）**：`box`（装箱）、`copy`（复制）、`@` 前缀内建（`@sizeOf`/`@intCast`/`@ptrCast`/`@alignCast`/`@typeOf`/`@compileError`/`@intFromEnum`/`@enumFromInt`/`@ptrFromInt`/`@intFromPtr`/`@volatileLoad/Store`/`@atomicLoad/Store/Rmw`/`@addWithOverflow` 等，见 06-04）。

### 1.2 标识符

- `snake_case` 惯例；标识符延续 Unicode 字母数字（含 CJK 表意文字）。
- 接口名强制 `I` 前缀（2026-08-16 定案：`interface X` 名不以 `I` 开头 → 编译诊断）。

### 1.3 字面量

| 种类 | 语法 | 类型 / 语义 |
|---|---|---|
| 整数 | `0x/0b/0o` 进制、`_` 分隔符、惰性宽度后缀 | **comptime_int**（使用处定型，超范围编译期报错） |
| 浮点 | 含 `f16–f128` 后缀 | comptime_float 惰性宽度 |
| 字符 | `'x'` 单引号单字符 | **comptime_int**（无独立 char；非 ASCII 用 u32 码点 `0x1F600`） |
| 字符串 | `"..."`；多行/原始 `"""..."""`（无转义） | 静态只读切片 `&[u8]`（无 `o`，不可 move） |
| 布尔 | `true` / `false` | `bool` |
| 空 | `void` / `null` | 函数返回 / 可选空态 |
| 枚举值 | `.Name`（类型已知时推断） | 如 `copy(&x, .shallow)` ≡ `copy(&x, CopyMode.shallow)` |

**转义（字符串与字符共用）**：`\n` `\r` `\t` `\\` `\"` `\'` `\xNN`（十六进制字节）`\u{...}`（Unicode 码点 → UTF-8）；非法转义编译期报错。

### 1.4 注释

- `//` 行注释；`///` 文档注释（关联声明，驱动文档生成）；`/* */` 块注释。

### 1.5 标点 / 运算符 token

`{}` `()` `[]` `;` `,` `.` `:` `=>` `=` `==` `!=` `<` `<=` `>` `>=` `+` `-` `*` `/` `%` `%%` `+=` `-=` `*=` `/=` `&` `|` `^` `~` `<<` `>>` `!` `?` `..` `.*` `|x|`（捕获）`||`（错误集联合）`_`（忽略绑定）。

> 注意：相邻 `||` 为错误集并运算；空参闭包须写作 `| |`（两 Pipe 间空格）。

---

## 2. 声明规则（Declarations）

### 2.1 变量绑定

```hc
var x: i32 = 5;          // 声明；T 可推断时省略
var mut x: o *mut T = t; // 全写：可写、拥有、类型 T
const Name = 值;          // 常量/类型别名（const 不可变，var mut 可变）
```

| 记号 | 语义 |
|---|---|
| `var` | 声明关键字 |
| `mut` | 可写修饰；**默认只读**（Rust 式；注意：绑定级只读当前未实现，见 `01-unimplemented-features.md` C4） |
| `o` | 所有权注册当前作用域（退出销毁）；仅用于分配器分配对象；标量/Continuous 上写 `o` 编译错误；Arena 分配无所有权 |
| `*` / `*mut` | 只读 / 可写指针 |
| `T` | 类型；`T` 与 `*`/`*mut` 可省略（系统推断），`o` 不可省略 |

**类型推断原则（Q-S9）**：推断优先，显式兜底——可推断：变量绑定、字面量惰性宽度、泛型参数、指针形态、函数返回；必须显式：函数参数类型、class 字段类型、接口实现标注。

**`global`**：根作用域静态生命周期（程序退出销毁）；有所有权归根作用域；不可 move；初始化 = 程序启动时执行（跨文件按依赖图拓扑排序，循环依赖编译错误）。

### 2.2 函数

```hc
fn add(a: i32, b: i32) i32 { return a + b; }
fn f(a: i32, b: i32 = 0) i32 { ... }        // 可选参数（尾部、编译期常量默认值）
async fn g() Future<i32> { ... }            // 异步（调用点返回惰性 Future）
export fn main() i32 { ... }                // 原生符号导出（K5）
```

- 支持**重载**（具体优先于泛型）、**多值返回**（元组）、**错误联合返回** `E!T` / `!T`。
- 方法 = 类型声明的函数成员（Zig 式，无 `impl`）；实例方法默认公开。
- 闭包：`|x| { ... }` / `|mut x|` / `|move x|`；捕获精确化（只捕获实际引用变量）+ 非 mut 闭包只读强制 + move 深拷贝。

### 2.3 类型定义

```hc
[continuous]                    // 特性标注：连续内存值类型（H1）
class Point { x: f32, mut y: f32, fn dist(a: *Point, b: *Point) f32 }
class Person { name: String, age: i32 }      // 未标注 → 堆上
enum Kind { player, enemy }                  // 合一式枚举
enum Value { int: i32, none }                // 带负载枚举
union { a: i32, b: f32 }                     // 无标签 union（K1，内存双关）
interface INumber: ICompare { fn add(self: *Self, other: Self) Self }
tree Node { ... }                            // 递归/层级
```

- **`class`** 统一（struct 删除）；存储形态由特性标注决定：`[continuous]` → 连续内存值类型（无分配器/可内嵌/赋值即复制/`to_bytes` 直映射/`[pad]`/`[align(T)]`/字面量 `X{...}`）；未标注 → 堆上。
- **特性标注**：类型声明上方中括号 `[name]` / `[name(参数)]`；多特性叠放 `[continuous] [pad]`。
- **成员可见性**：方法默认公开；属性默认私有（`private` 显式可选）——`pub mut A: o T`。
- 无继承；无 `==` 重载（运算符只绑定接口族）。
- 内建泛型：`Vec<T>`、`Map<K,V>`、`Deque<T>`、`Table<T>`、`String`（newtype = `Vec<u8>`）。

### 2.4 模块 / 包

```hc
namespace io { ... }
import pkg.{sym};        // 选择导入（取代 using，ADR-0010）
pub fn f() ...            // pub = 包边界可见性
```

- `build.zon`：`const build = Build{ name, version, kind, files, deps }` 数据字面量。
- `pub` 管语言层可见性；`export` 管原生符号层（K5，正交）。

### 2.5 元编程声明

```hc
script { ... }            // 装载期求值 + 文本区间替换 + 重解析（E1.1）
comptime { ... }          // 编译期求值块（E1.2）
fn List(T: type) type { return struct { first: T, second: T }; }   // 类型函数
```

- `script` = 受限 H 子集（io/alloc/argv 不可用）；失败 = 编译错误带位置。
- `comptime` 块结果丢弃，仅编译期存在；失败 = 编译错误。
- 类型函数：类型值 = 编译期对象，实例化 = 具体化（monomorphization）+ 缓存。

---

## 3. 类型规则（Types）

### 3.1 类型总表

| 类别 | 语法 | 说明 |
|---|---|---|
| 整数 | `iN` / `uN`（i8–i128、u8–u128）+ `isize` / `usize` | 实现 `INumber` / `ICompare` |
| 浮点 | `f16` / `f32` / `f64` / `f128` | 实现 `INumber` / `ICompare` |
| 字符 | 字面量定型（comptime_int） | 无独立 char 类型 |
| 布尔/空 | `bool` / `void` | `void` 仅函数返回 |
| 数组 | `[N]T` | 定长，引用类型 |
| 表格 | `Table<T>` | 内建二维结构，代替 `[M][N]T` |
| 元组 | `(T1, T2, ...)` | 匿名值类型（初始化后字段只读） |
| 切片 | `&[T]` / `&mut [T]` | 带位置与长度的指针视图 |
| 指针 | `*T` / `*mut T` | 只读 / 可写；**不可空** |
| 可选 | `?T` | 使用前显式解包（`x orelse 默认` / `x.?`） |
| 错误联合 | `E!T` / `!T` | 错误集联合；`try`/`catch` 处理 |
| 连续类 | `[continuous] class` | 值语义（赋值即复制） |
| 堆类 | `class`（未标注） | 引用语义（复制需显式 `copy`） |
| 枚举 | `enum` | 变体 + 可选负载 |
| 接口 | `interface I...` | 契约集合（`where T: I` 约束） |
| union | `union { ... }` | 无标签内存双关（仅标量字段） |

### 3.2 类型形态规则

- **指针不可空**（2026-08-13 Q16）：空指针用 `?*T` / `?*mut T`；悬垂 ≠ null。
- **引用类型赋值禁止**（2026-08-14 修订）：数组/集合/堆类绑定级赋值不合法——共享走显式指针 `var p = &s1;`，复制走 `var s2 = copy(&s1);`（`copy` 默认深复制，浅复制 `copy(&s1, .shallow)`）。
- **连续类型**：赋值即复制（值语义全集）。
- **数据栈对象**：自包含连续内存值类型（标量 + Continuous 类）；数组/集合为引用类型，不属数据栈对象。
- **接口指针 = 胖指针**（三字宽 = data + 虚表 + alloc 引用）；具体类型指针 → 接口指针编译期自动收窄。
- **泛型**：`where T: INumber` 约束；`anytype` 调用点按实参具体化；类型函数 `fn List(T: type) type`。

---

## 4. 表达式规则（Expressions）

### 4.1 运算符

| 类别 | 记号 |
|---|---|
| 算术 | `+ - * /` |
| 除模 | `/` 截断除法；`%` 截断取余；`%%` 欧几里得取模（非负） |
| 逻辑 | `and` `or` `!`（关键字，非符号） |
| 比较 | `== != < <= > >=`（`==` = 值比较，内部调用 ICompare） |
| 位运算 | `&` `\|` `^` `~` `<<` `>>` |
| 范围 | `..`（切片取段 / 区间糖 `0..10`） |
| 解引用 | `p.*` 显式取值；字段/索引自动解引用 |
| 取地址 | `&x` 只读 / `&mut x` 可写（仅 `var mut` 变量） |
| optional | `x orelse 默认`；`x.?` 显式断言解包 |
| 复合赋值 | `+= -= *= /=` |
| 重载 | **无**（运算符只绑定内建接口族） |

**运算符绑定（2026-08-14 / H3 修订）**：算术 `+ - * /`（及一元 `-`）绑定 `INumber` 族——`a + b` ≡ `a.add(b)`；`%`/`%%` 编译器派生；相等 `==`/`!=` = 值比较调用 ICompare；序比较 `< <= > >=` 绑定 ICompare；class 身份比较删除（需实现 ICompare 才有 `==`）；指针比较 = 指向对象地址；字符串拼接走方法 `s.concat(other)`。

### 4.2 优先级（Q4 定案）

```
后缀（解引用/调用/索引/字段/断言） > 前缀/一元（- ! ~ &x &mut x try）
> * / % %% > + - > << >> > & > ^ > | > 比较（非结合，a < b < c 编译报错）
```

### 4.3 索引与访问

- **单参索引** `s[i]`：数组/切片/Vec/String 等。
- **多参索引** `expr[i, j]`：**仅 `Table` 类型合法**（行、列）；其它类型单参索引（M8 定案）。
- **行视图（2026-08-22 Table 补全）**：`t[i]` 返回切片 `&[T]`/`&mut [T]`（`t[i][j]` ≡ `t[i,j]`）。
- **字段**：`p.x` / `p.0`（元组）/ 方法链。
- 越界按模式（Q24）：Debug 运行时检查（抛错带位置）/ Release 裸；编译期可证越界编译期报错。
- 整数溢出按模式（Q-S6）：Debug 检测并 `@panic` / Release 裸 wrap；显式 `@addWithOverflow` 等返回 `(T, bool)`。

### 4.4 字面量构造

- 元组：`(1, "a", 2.5)`；解构 `var (a, b, _) = t;`。
- struct：`Point{x = 1.0, y = 2.0}`（连续类）/ `alloc.init(Person{ name = ..., age = 30 })`（堆类带参）。
- 数组：`[1, 2, 3]`（尾逗号合法）。
- 枚举：`Kind.player` 或 `.player`（推断）。
- 闭包：`|x| x + 1`。

---

## 5. 语句规则（Statements / Control Flow）

### 5.1 语句形态

```hc
if (cond) a else b                 // if 是表达式；作表达式时 else 强制，作语句时可省
if (opt) |v| { ... }               // optional 捕获
if (e!) |v| else |err| { ... }     // error union 双向捕获（Q9：必须成对）
switch (v) { int => |i| ..., none => ... }   // 穷举 + 负载捕获；也是表达式；else 兜底
while (cond) { ... }
while (i < n) : (i += 1) { ... }   // 续步表达式
while (opt) |v| { ... }            // optional 捕获（空则退出）
while (e!) |v| { ... } else |err| { ... }    // error union 捕获
for (items) |item| { ... }         // 数组/切片迭代，捕获默认只读
for (items) |mut item| { ... }     // 可写捕获
for (items) |move item| { ... }    // 拥有迭代（IIterable<o T>）
for (0..10) |i| { ... }            // 区间糖（复用 ..，底层仍是 while）
break :label / continue :label     // 带标签退出嵌套
defer expr / errdefer expr         // 作用域退出执行；errdefer 仅错误路径；多 defer LIFO
```

- 无 `do-while`、无 `loop`（用 `while (true)`）。
- **`for` 值类型自动取引用（L4）**：`for (fib)` ≡ `for (&mut fib)`（可写）/ `for (&fib)`（只读）；只读绑定时可写迭代编译错误。
- 多值返回：`fn divmod(a, b) (i32, i32)`；调用 `var (q, r) = divmod(a, b);`。

### 5.2 错误处理

- `E!T` / `!T` 函数：错误沿值通道传播直到 `try`/`catch`（try 不转抛错通道，catch 全链可拦截）。
- 未标记错误类型 `return error.X` → 编译错误；未处理错误到根 → 记录位置 + panic 式中止（非零退出）。
- `x orelse 默认` / `x.?`（空则运行时错误带位置）。
- 错误码表（M2.6）：名 ↔ 码全局唯一（包 ID + 包内码）；`hc errors <file>` 输出表。

---

## 6. 属性与测试（Attributes / Tests）

### 6.1 特性标注（类声明上方）

- `[continuous]`：连续内存值类型（编译器验证字段全为值类型）。
- `[pad]`：紧凑打包（原 packed，仅连续类型）。
- `[align(T)]`：对齐到类型 T 的对齐值。
- `[test]` / `[test("名称")]`：标记测试函数（2026-08-16 从 `test fn` 改特性标记）。

### 6.2 测试规则（Q8 / Q-T1–T6）

```hc
[test] fn add_basic() !void {
    try expect(add(1, 2) == 3);
}
```

- 测试函数可被普通代码调用；显示名 = 名称 ?? 函数名。
- 断言 API：`expect` / `expect_eq` / `expect_neq` / `expect_error` / `expect_eq_slices`（返回 `anyerror!void`）。
- 输出：`[PASS]/[FAIL]/[SKIP]` + 汇总；失败非零退出码。
- 隔离：每 test 独立块作用域；默认串行；`return error.SkipTest` = SKIP。
- 环境注入：test 内隐式 `test_io` 与 `alloc`。
- 双模式：`hc test` 默认 interpret Debug；`--mode=compile` 编译 Debug（交叉验证）；`--release` 仅正常路径子集。

---

## 7. 序列化与迭代契约（内建接口）

### 7.1 序列化

- 连续类型 `to_bytes`/`from_bytes` 直映射（尊重 `[pad]`/`[align(T)]`）。
- 集合（Vec/Map/Table/切片）→ 字节：长度前缀 u64 LE + 元素字节（内建）。
- 堆类型 `to_json`/`from_json`（class/Map）。
- Table（2026-08-22）：to_bytes = u64 LE 行数 + u64 LE 列数 + 行主序元素字节。

### 7.2 迭代契约（IIterable 三态）

- `IIterable<*T>`（只读，`for (x) |item|`）/ `IIterable<*mut T>`（可写，`|mut item|`）/ `IIterable<o T>`（拥有，`|move item|`——迭代器持有容器所有权，迭代后容器不可再用）。
- 内建类型（数组/切片/Vec/Map/Table/String）编译器内建实现三态；用户类型实现 `next(self: *mut Self) ?T`。
- `arr.iter()` 显式迭代器对象；`filter()/map()` 立即求值链。

---

## 8. 出处映射（权威规范位置）

| 规则 | 权威文件 |
|---|---|
| 词法/声明/运算符/语句/测试 | `docs/SPEC/06-01-syntax.md` |
| 基础类型 | `docs/SPEC/06-02-types.md` |
| 扩展类型（class/枚举/元组/Table/String/tree） | `docs/SPEC/06-03-extended-types.md` |
| 函数/闭包/@ 内建 | `docs/SPEC/06-04-functions.md` |
| 接口/标量接口族/迭代/序列化 | `docs/SPEC/06-05-interfaces.md` |
| 所有权/内存模型 | `docs/SPEC/06-06-ownership.md` |
| 错误处理 | `docs/SPEC/06-07-errors.md` |
| 模块与包 | `docs/SPEC/06-08-modules.md` |
| 元编程 | `docs/SPEC/06-09-meta.md` |
| 并发 | `docs/SPEC/06-10-concurrency.md` |
| 设计总纲（§12 语言特性） | `docs/SPEC/01-language-design.md` |
| 查询手册（语法速查） | `docs/H Language.md` |
| 术语表（单上下文） | `CONTEXT.md` |
