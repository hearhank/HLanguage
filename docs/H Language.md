# H 语言语法功能查询手册

> 本手册基于 `docs/SPEC/` 下的语言规范整理，作为语法与功能速查参考。详细规范请查阅对应 `06-0X-*.md` 文件。
>
> H 是一门**以数据为中心**、同时支持系统编程与脚本编程的语言，源码后缀 `.hc`。核心哲学：定义数据、修改数据、传输数据、保存数据。同一份源码既可编译为原生二进制，也可作为脚本解释执行，两种模式**语义一致**。

## 目录

- [1. 关键字与词法](#1-关键字与词法)
- [2. 类型系统](#2-类型系统)
- [3. 声明](#3-声明)
- [4. 运算符](#4-运算符)
- [5. 语句与控制流](#5-语句与控制流)
- [6. 函数与闭包](#6-函数与闭包)
- [7. 内建函数（box / copy / @）](#7-内建函数box--copy--)
- [8. class（连续类型 / 堆上类型）](#8-class连续类型--堆上类型)
- [9. 枚举 / 元组 / Table / String / tree](#9-枚举--元组--table--string--tree)
- [10. 接口与标量接口族](#10-接口与标量接口族)
- [11. 所有权与内存模型](#11-所有权与内存模型)
- [12. 错误处理](#12-错误处理)
- [13. 模块与包](#13-模块与包)
- [14. 元编程（script / comptime）](#14-元编程script--comptime)
- [15. 并发与异步](#15-并发与异步)
- [16. 项目结构与代码约定](#16-项目结构与代码约定)
- [17. 双模式执行与入口](#17-双模式执行与入口)
- [18. 测试](#18-测试)

---

## 1. 关键字与词法

### 1.1 关键字清单

| 类别 | 关键字 |
|---|---|
| 声明 | `var` `const` `fn` `global` `test` |
| 控制流 | `if` `else` `while` `for` `break` `continue` `return` `switch` `defer` `errdefer` |
| 类型构造 | `class` `enum` `tree` `interface` `where` |
| 模块 | `namespace` `pub` |
| 内存/所有权 | `owned` `move` |
| 操作 | `and` `or` `try` `catch` `orelse` |
| 元编程 | `script` `comptime` `anytype` `type` |
| 并发 | `async` `await` `spawn` |
| 字面量 | `void` `null` `true` `false` |
| 可写修饰 | `mut`（默认只读，`mut` 修饰可写） |

- 内建函数（非关键字）：`box`（装箱）、`copy`（复制）
- `@` 前缀：内建函数前缀（如 `@sizeOf` / `@intCast`），不与用户标识符冲突

### 1.2 标点与运算符 token

`{}` `()` `[]` `;` `,` `.` `:` `=>` `=` `==` `!=` `<` `<=` `>` `>=` `+` `-` `*` `/` `%` `%%` `+=` `-=` `*=` `/=` `&` `|` `^` `~` `<<` `>>` `!` `?` `..` `.*` `|x|`（捕获） `||`（错误集联合） `_`（忽略绑定）

### 1.3 字面量

| 类别 | 语法 | 说明 |
|---|---|---|
| 数字 | `0x1F` / `0b101` / `0o77` / `1_000` | 无固定宽度（comptime_int / comptime_float），使用处定型，超范围编译期报错；`_` 数字分隔符 |
| 字符 | `'x'` | 单引号包裹；类型 = comptime_int（惰性宽度，如 `split(',')` 定型为 u8）；非 ASCII 用 u32 码点显式（如 `0x1F600`） |
| 字符串 | `"..."` | 类型是String, 字符串赋值，默认move所有权。,&String(&[u8]只读切片)是String的引用类型，不拥有字符串数据。 &mut String(&mut [u8]可读写切片)是String的可变引用类型，拥有字符串数据。 |
| 多行/原始字符串 | `"""..."""` | 无转义 |
| 推断枚举值字面量 | `.name` | 参数/字段类型已知时 `.Name` 推断为对应枚举变体（如 `copy(&x, .shallow)` ≡ `copy(&x, CopyMode.shallow)`） |

### 1.4 转义序列（字符串与字符共用）

`\n` `\r` `\t` `\\` `\"` `\'` `\xNN`（十六进制字节） `\u{...}`（Unicode 码点 → UTF-8）；非法转义编译期报错。

### 1.5 注释

- `//` 行注释
- `///` 文档注释（关联声明，驱动 `hc doc` 文档生成）
- `/* */` 块注释（项目所有者选择保留）

### 1.6 命名约定

| 元素 | 约定 | 示例 |
|---|---|---|
| 文件 | `snake_case.hc` | `math.hc` |
| 类型 / 命名空间 / 模块 | `PascalCase`（缩写词全大写） | `Point` / `HTTPRequest` / `TCP` |
| 标识符（变量/函数/方法/字段/参数） | `snake_case` | `add` / `total` |
| 常量 | `SCREAMING_SNAKE` | `MAX_SIZE` |

---

## 2. 类型系统

### 2.1 类型总表

| 类别 | 语法 | 说明 |
|---|---|---|
| 整数 | `iN` / `uN`（i8–i128、u8–u128）+ `iSize` / `uSize` | 实现 `INumber` / `IEqual`/`ICompare` |
| 字符 | `'x'` | 单引号包裹；类型 = comptime_int（惰性宽度，如 `split(',')` 定型为 u8）；非 ASCII 用 u32 码点显式（如 `0x1F600`） |
| 浮点 | `f16` / `f32` / `f64` / `f128` | 实现 `INumber`/ `IDecimal` / `IEqual`/`ICompare` |
| 布尔/空 | `bool` / `void` | `void` 仅函数返回 |
| 数组 | `[N]T` | 定长，引用类型（传引用、复制需显式 `copy`） |
| 表格 | `Table<T>` | 内建二维结构，代替 `[M][N]T` |
| 元组 | `(T1, T2, ...)` | 匿名值类型 |
| 切片 | `&[T]` / `&mut [T]` | 带位置与长度的指针视图 |
| 可选 | `?T` | 使用前显式解包 |
| 错误联合 | `E!T` / `!T` | error union |
| 函数 | `fn(参数) 返回类型` | 见 §6 |
| 泛型 | comptime 式 | 见 §14 |

### 2.2 标量

- 内建标量编译器内建实现数字接口族（不可重载）：`i8–i128`/`iSize` → `INumber`；`u8–u128`/`uSize` → `INumber`；`f16–f128` → `IDecimal`
- `==` / `!=` = 值比较（语言内建）；`< <= > >=` 绑定 `ICompare`
- 字面量惰性宽度（comptime_int / comptime_float）；字符 = comptime_int
- 整数溢出按模式：Debug 检测并 `@panic`（带位置）/ Release wrap；显式 `@addWithOverflow` 等返回元组
- `bool`：`and`/`or`/`!` 关键字操作；无独立 char 类型；`void` 仅函数返回

### 2.3 数组与切片

- `[N]T` 定长数组：**引用类型**（传参走引用、复制需显式 `copy`）；`[1, 2, 3]` 字面量推断式；可作字段内嵌
- `&[T]` / `&mut [T]` 切片：`*T`/`*mut T` 指针 + 起始索引 + 长度，对连续内存的**视图**，**不拥有数据**（无 `o`）
- **数组的引用默认就是切片**：`&arr` 直接产生切片（起始 + 长度），无需显式取段；`&arr[1..3]` 指定范围
- 字符串字面量 → 静态只读切片 `&[u8]`（无 `owned`、不可 move）
- 比较：数组/切片 `==` 经 `ICompare`（按值逐元素）；越界检查按模式

### 2.4 可选值 `?T`

- 可能没值；使用前显式解包：
  - `x orelse 默认值`
  - `x.?` 断言（空则运行时错误带位置）
  - `if (opt) |v| { ... }` 捕获
  - `while (opt) |v| { ... }` 捕获（空则退出循环）
- 与 error union（`E!T`）**正交**

### 2.5 指针与引用

| 记号 | 语义 |
|---|---|
| `*T` | 只读指针，**不可空**；多个可同时存在 |
| `*mut T` | 可写指针，**不可空**；多个可同时存在 |
| `&x` | 只读地址（只读/可写变量均合法） |
| `&mut x` | 可写地址（仅 `var mut` 变量） |
| `p.*` | 显式解引用取值 |
| `p.x` / `s[i]` | 字段/索引访问自动解引用 |

- **指针不可空**：`*T`/`*mut T` 无空态，空指针用 `?*T`/`?*mut T`；无 NULL 字面量；悬垂 ≠ null
- **指针自由**：多 `*mut`/`*T` 合法、`*mut` 可复制；指针问题（悬垂/别名）由用户负责
- 临时引用生命周期：表达式产生的临时 `*T`/`*mut T` 存活到所在完整表达式/语句结束；绑定到变量的引用存活到作用域结束
- Debug 悬垂标记：可选诊断工具（编译时选项），非安全保证
- 指针比较 `==`：引用地址是否相同，**不比较值**（`*p == q` 无意义）；
- 指针不能再生成指针，只能复制指针。即没有指向指针的指针。

---

## 3. 声明

### 3.1 变量声明

```hc
var x: T = 5;          // 声明；T 可推断时省略
var mut x: owned *mut T = t; // 全写：可写、拥有、类型 T
var mut x: owned *mut = t;   // 类型推断
var mut x: owned = t;        // 类型与 *mut 推断，owned 显式
var x1: *T = &x;         // 只读指针、类型推断
var a = alloc.init(T);  //可以根据T进行类型推荐
var a:i32 = alloc.init(4);//参数是长度，不能推荐类型，需要手动标注/i32,float都是4个byte
var a:T = arena.init(T);  // 无 owned：所有权在 arena（禁止 move）
const Name = 值;          // 常量/类型别名（const 不可变，var mut 可变）
```

| 记号 | 语义 |
|---|---|
| `var` | 声明关键字 |
| `mut` | 可写修饰；**默认只读**（Rust 式） |
| `owned` | 所有权注册在当前作用域（退出销毁）；**仅适用于分配器分配的对象**（复杂类型/装箱指针）；标量类型上写 `owned` 编译错误；非 Arena 分配默认拥有（`owned` 冗余）；Arena 分配无所有权；**无所有权禁止 move** |
| `*` / `*mut` | 只读 / 可写指针类型 |
| `T` | 类型；T 与 `*`/`*mut`/`owned` 可省略（系统推断） |

### 3.2 类型推断原则

- **可推断**：变量绑定、字面量惰性宽度、泛型参数（anytype / where T）、指针形态（`var x = &mut t` → `*mut T`）、函数返回类型（单返回路径/一致类型）
- **必须显式**：函数参数类型（含 `*`/`*mut`/`owned` 形态）、class 字段类型、接口实现标注；

### 3.3 global（根作用域）

- 静态生命周期（程序退出时销毁）；**有所有权**（归根作用域），不可 move；`owned` 标注不能用于 global
- **初始化 = 程序启动时执行**（main 前）；跨文件按初始化表达式依赖图拓扑排序，无依赖按文件声明序，循环依赖 = 编译错误

### 3.4 const

- 常量/类型别名；`const` 不可变，`var mut` 可变

---

## 4. 运算符

| 类别 | 记号 | 说明 |
|---|---|---|
| 算术 | `+ - * /` | 绑定 `INumber` 族：`a + b` ≡ `a.add(b)` |
| 除模 | `/`（截断除法） `%`（截断取余） `%%`（欧几里得取模，非负） | `%`/`%%` 由编译器派生 |
| 逻辑 | `and` `or` `!` | 关键字，非符号 |
| 比较 | `== != < <= > >=` | `==` = 值比较（内部调用 `ICompare`）；序比较绑定 `ICompare` |
| 位运算 | `& \| ^ ~ << >>` | |
| 范围 | `..` | 切片取段 / 区间糖 |
| 解引用 | `p.*` 显式取值；字段/索引访问自动解引用 | |
| 取地址 | `&x` 只读地址；`&mut x` 可写地址（仅 `var mut`） | |
| optional | `x orelse 默认值`；`x.?` 显式断言解包（空则运行时错误带位置） | |
| 复合赋值 | `+= -= *= /=` | |
| 重载 | **无** | 运算符只绑定内建接口族 |

### 4.1 优先级（从高到低）

1. 后缀（解引用/调用/索引/字段/断言）
2. 前缀/一元（`-` `!` `~` `&x` `&mut x` `try`）
3. `*` `/` `%` `%%`
4. `+` `-`
5. `<<` `>>`
6. `&`
7. `^`
8. `|`
9. 比较（非结合，`a < b < c` 编译报错）

### 4.2 模式相关行为

- 越界检查按模式：Debug 运行时检查（抛错带位置）、Release 裸；编译期可证越界所有模式编译期报错
- 整数溢出按模式：Debug/脚本检测并 `@panic`（带位置）；Release 裸 wrap
- **字符串拼接走方法**：`s.concat(other)`——无 `++` 运算符、无 `+` 重载

---

## 5. 语句与控制流

```hc
if (cond) a else b              // if 是表达式；作表达式时 else 强制，作语句时可选
if (opt) |v| { ... }            // optional 捕获
if (e!) |v| else |err| { ... }  // error union 双向捕获（必须成对，错误显式处理）

switch (v) {
    int => |i| ...,             // 穷举 + 负载捕获；switch 也是表达式
    int => |i| i if i > 0 => ..., // switch 守卫（2026-08-22：模式后 if 守卫，失败继续下一分支）
    none => ...,
    else => ...,                // else 兜底（有 else 免穷举）
}

while (cond) { ... }
while (i < n) : (i += 1) { ... }            // 续步表达式
while (opt) |v| { ... }                    // optional 捕获（空则退出循环）
while (e!) |v| { ... } else |err| { ... }  // error union 捕获（错误走 else 后退出）

for (items) |item| { ... }       // 数组/切片迭代，捕获默认只读；值类型迭代自动取引用
for (items) |mut item| { ... }   // 可写捕获
for (items) |move item| { ... }  // 拥有迭代（IIterable<o T>）
for (0..10) |i| { ... }          // 区间糖（底层仍是 while）

break :label / continue :label   // 带标签退出嵌套
defer expr / errdefer expr       // 作用域退出执行；errdefer 仅错误路径；多 defer 按 LIFO
```

### 5.1 要点

- 无 `do-while`、无 `loop`（用 `while (true)`）
- **switch 守卫（2026-08-22 定案）**：`switch (v) { 模式 if 守卫 => 表达式 }`——守卫为任意布尔表达式，可引用负载捕获变量；**守卫失败 → 继续尝试下一个分支**；**穷举性**：有守卫的分支仍算覆盖该值形态，但守卫可能失败 → 需存在无守卫分支或 `else` 兜底保证穷举（否则编译错误）；仍是表达式，与 `else` 兜底正交叠加
- **`for` 值类型自动取引用**：`for (fib)` ≡ `for (&mut fib)`（可写迭代）/ `for (&fib)`（只读迭代）；只读绑定（无 `mut`）时可写迭代编译错误
- **多参索引**：`expr[i, j]` 逗号索引——**仅 `Table` 类型合法**（行、列）；其它类型单参索引 `s[i]`

---

## 6. 函数与闭包

### 6.1 函数声明

```hc
fn main() !void {}    // 入口：args 注入（0 号 = 程序名）；!void 入口错误运行时报告
export fn foo(a: i32) i32 {}             // 原生符号级导出——链接器可见干净符号（与 pub 正交，仅作用于 fn/async fn）
extern fn c_add(a: i32, b: i32) i32;      // C ABI 外部声明（A1，ADR-0020）：纯声明无 body、链接期解析；MVP = 标量+指针+POD
fn fun(y: owned *mut T) void {}              // owned T：参数拥有（退出销毁）
fn add(a: *T) void where T: INumber {}  // 接口约束：where 子句在签名末尾
fn f(x: &[u8]) !i32                      // 返回 error union
async fn af() T                          // 返回 Future<T>
fn f(a: i32, b: i32 = 0) i32 { ... }    // 重载 + 可选参数（尾部、编译期常量默认值）
```

### 6.2 重载与可选参数

- 允许重载——签名 = 函数名 + 参数类型列表 + 返回类型（共同决定）
- **解析顺序**：① 精确参数数量匹配（忽略默认值）→ ② 具体非泛型候选优先于泛型候选 → ③ 泛型候选按 where 约束编译时验证 → ④ 多个同等匹配 → 歧义编译错误
- **期望类型传播**：表达式在已知目标类型时优先选择返回类型匹配的重载（`var x: f64 = get();`）
- 可选参数 `fn f(a: i32, b: i32 = 0)`：默认值须编译期常量、只能尾部
- 接口方法为成员契约，不参与全局重载解析

### 6.3 参数形态

| 形态 | 语义 |
|---|---|
| `T` 标量 / 连续类型 | 默认复制 |
| `*T` | 只读引用 |
| `*mut T` | 可写引用 |
| `owned T` | 引用类型（引用类型,拥有语义,调用时必须 move t，作用域接管，退出销毁） |
| `*T where T: 接口` | 接口约束参数（虚拟类型 T + where 子句） |

- **接口约束参数**：调用点按形态显式：`add(&a)` / `add(&mut a)` / `add(move a)`
- **io 参数**：库函数统一虚拟类型制 `io: *T where T: Io`（调用点 `&io`）；

### 6.4 move 规则

- 仅本作用域拥有（非 Arena/global）的变量可 move；拥有参数用 `owned T` 标注
- `owned` 与 `*`/`*mut` 正交（`owned *mut T` 允许）
- 调用点显式 `move`：`take(move s);` / `return move s;`

### 6.5 返回值

- `fn() owned T` 返回拥有（所有权移出）
- `fn() *T` / `*mut T` 返回引用
- 函数内新建的值必须 move 返回（无所有权的除外）
- 返回引用必须指向函数参数，不得返回局部变量引用
- **元组多值返回**：`fn f() (T1, T2)`；`E!(T1, T2)` 合法（元组负载）

### 6.6 方法（类内函数成员）

```hc
class Point {
    x: f32,
    y: f32,
    fn dist(a: *Point, b: *Point) f32   // 方法 = 函数成员（Zig 式，无 impl）；方法默认不公开，公开需要明确标注 `pub`
}
// 双语调用：p.dist(q) ≡ Point.dist(p, q)
// 接收者自动取引用：首参 *Self 取 &p、*mut Self 取 &mut p
```

### 6.7 闭包

```hc
|x| expr              // 默认只读捕获
mut |x| ...           // 可写捕获
move |x| ...          // 转移捕获（粒度 = 整个闭包）
f(x)                  // 调用
```

- 闭包是**数据对象**（捕获为字段的结构体 + 调用约定）；遵循所有权模型（可 `owned`、可 move、捕获随闭包销毁）
- **捕获精确化**：捕获集合 = 自由变量精确分析（只捕获 body 实际引用、未被体内绑定遮蔽的外部变量，含嵌套闭包传递）
- 只读捕获内重绑定被捕获变量（含复合赋值展开）→ `error.ReadonlyCapture`（写穿指针/字段/索引仍允许）
- `move` 捕获深拷贝独立副本（原绑定/原闭包捕获变量后续变更不影响闭包）
- 返回值规则：闭包可按值返回——捕获全为值类型（栈上数据对象随返回值复制，无悬垂）；捕获引用/堆数据的闭包返回时：捕获对象须可 move（`move` 捕获）或为 global，否则编译错误
- 单变量粒度捕获留 1.x

### 6.8 作用域退出

- 销毁顺序 = 声明逆序（LIFO）

---

## 7. 内建函数（box / copy / @）

### 7.1 box / copy

| 内建 | 语义 |
|---|---|
| `box(value, alloc) -> owned *mut T` | 装箱——分配堆内存、值写入堆、返回带所有权的可写指针；标量可装箱为接口指针（`*INumber` 等） |
| `copy(&value, mode) -> T` | 按类型复制——标量 = 复制、引用类型 = 深拷贝、class = 递归复制（含内存树） |

- **`copy` 默认深复制**，浅复制需显式标注
- **内建枚举 `enum CopyMode { deep, shallow }`**：`copy(&x)` ≡ `copy(&x, CopyMode.deep)`、`copy(&x, CopyMode.shallow)`（`.name` 推断枚举值字面量；浅复制引用字段共享，内存问题用户负责）

### 7.2 @ 内建函数

| 类别 | 内建 | 语义 |
|---|---|---|
| 类型查询 | `@sizeOf(T)` / `@alignOf(T)` / `@offsetOf(T, "字段")` / `@typeOf(expr)` | 编译期常量（序列化/FFI 布局依赖） |
| 整数转换 | `@intCast(T, x)` | 宽度/符号转换；超范围 Debug 检测（Release 裸） |
| 枚举转换 | `@intFromEnum(e) usize` / `@enumFromInt(E, i)` | 变体序索引（0 起）/ 反向；越界 Debug 检测；仅纯常量枚举；返回 `usize`（序列化/固定宽度场景显式 `@intCast`） |
| 指针转换 | `@ptrCast(T, p)` | 指针类型转换——**显式放弃类型安全的唯一逃生舱**（替代 Rust unsafe / C 强转） |
| 对齐 | `@alignCast(T, p)` | 对齐提升断言（Debug 检查） |
| 内存访问（volatile） | `@volatileLoad(p) T` / `@volatileStore(p, v)` | 防优化掉的读穿/写穿（LLVM `load volatile`/`store volatile`，MMIO 场景） |
| 指针转换（地址） | `@ptrFromInt(addr) *mut Unknown` / `@intFromPtr(p) usize` | 整数 ↔ 指针转换；`@intFromPtr` 取地址（round-trip 保真）、`@ptrFromInt` 重建原指针/合成匿名槽；`@ptrFromInt` 恒返回 `*mut Unknown` |
| 溢出 | `@addWithOverflow(a, b)` / `@subWithOverflow` / `@mulWithOverflow` | 返回元组 `(T, bool)`（value, overflow）；不受模式影响 |
| 编译期 | `@compileError("msg")` | 显式编译失败（comptime/脚本用） |
| FFI | `@cImport("header.h")` | 编译期解析 C 头文件生成 H 声明（A1，ADR-0020）：顶层 `const c = @cImport(...);` 导入对象 + 限定名引用；MVP 只解析直接声明体（struct/enum/typedef/函数）；自动生成 `[continuous] class`；FFI 原生-only |
| 原子操作 | `@atomicLoad(T, p, order)` / `@atomicStore(T, p, v, order)` / `@atomicRmw(T, p, op, v, order)` | 无锁原语；`op` = `.add`/`.sub`/`.exchange`（返回旧值）；`.cmpxchg` 等 1.x |

- **内存序**（C11 五序子集）：`relaxed` / `acquire` / `release` / `acq_rel` / `seq_cst`——**默认 `seq_cst`**（弱序需显式写）
- `@` 前缀不与用户标识符冲突；转换显式可见（「没有隐藏控制」）
- 其余 Zig 内建（`@bitCast`/`@mulAdd` 等）按需在 1.x 扩展

---

## 8. class（连续类型 / 堆上类型）

### 8.1 类型定义

```hc
[fit]                          // 特性标注：连续内存值类型,默认在栈创建，也可以在堆创建
class Point {
    x: f32, //只读的变量，只能在初始化时赋值，后续不能修改
    mut y: f32,                        // 字段默认只读，mut 修饰可写
    fn dist(a: *Point, b: *Point) f32   // 方法 = 函数成员（Zig 式，无 impl）；方法默认公开
    private fn private_dist(a: *Point, b: *Point) f32   // 私有方法
    private mut temp: f32,                        // 字段默认只读，mut 修饰可写
}

class Foo { ... }   // 未标注 → 堆上（需分配器、接口、可 move）
```

### 8.2 特性标注

- 统一关键字 `class`；**存储形态由特性标注决定**，特性可以作用于任何作用域上面，格式是[特性名称(参数列表)]

| 标注 | 语义 |
|---|---|
| `[packed]` | 紧凑打包（仅连续类型；字段限：标量/枚举/连续内嵌/元组/定长数组） |
| `[align(T)]` | 对齐到类型 T 的对齐值（如 `[align(u64)]` = 8 字节） |

- **多特性标注叠放**：`[align(8)] [packed]` 连续中括号行叠放，均作用于同一类型

### 8.3 连续类型能力

- 无分配器（栈/内嵌）、可内嵌、赋值即复制
- `to_bytes` / `from_bytes` 直映射
- 字面量构造 `Point{x = 1.0, y = 2.0}`
- `[packed]` / `[align(T)]` 布局控制

### 8.4 堆上类型能力

- 需分配器、`to_json` / `from_json`
- 字段可带 `owned`、可 move

### 8.5 构造（`alloc.init` 唯一通用构造）

| 形态 | 语法 | 说明 |
|---|---|---|
| 无参 | `alloc.init(Foo)` | 分配器按类型自动获取大小并创建实例，字段后续显式赋值（无默认零值、definite assignment 检查） |
| 带参 | `alloc.init(Foo{字段 = 值, ...})` | 类型字面量作构造器参数——分配 + 字段初始化 |

- `box()` 装箱值

### 8.6 成员可见性

| 修饰 | 语义 |
|---|---|
| `pub`（方法默认） | 公开（包外可见） |
| `private`（属性默认） | 私有（显式可选项） |
| `pub mut A: owned T` | pub 可见性 + mut 字段可写 + `owned T` 字段类型形态分离 |

- **属性有所有权标注**：即使字段类型 `owned T`，所有权由类型实例管理，随实例销毁
- 字段可含：标量/连续类型/元组/定长数组/切片/复杂类型（String/集合/class）
- 写字段需持有实例的可写引用（`*mut`）；字段默认只读，`mut` 修饰可写

### 8.7 布局控制

- 默认布局 = 平台 ABI 对齐（与 C 兼容）
- `[packed]`——紧凑布局
- `[align(T)]`——类型级对齐
- `to_bytes` 直映射尊重显式布局（`@offsetOf`/`@alignOf` 可验证）

---

## 9. 枚举 / 元组 / Table / String / tree

### 9.1 枚举（合一式）

```hc
enum Value { int: i32, float: f64, none }   // 变体可选负载；纯常量枚举 = 变体均无负载
```

- switch 穷举（漏分支编译期报错）+ 负载捕获 `|x|`；`else` 兜底
- **负载可为任意类型**（标量/连续/元组/引用/String/class）——含引用负载 → 自动堆上
- 数据栈对象自动判定：负载全为值类型 → 可复制；含引用 → 只能引用/move
- **枚举 ↔ 整数**：`@intFromEnum(e) usize`（变体序索引，0 起）/ `@enumFromInt(E, i)`（反向，越界 Debug 检测 / Release 未定义值）；仅纯常量枚举可转换；`@intCast` 不处理枚举
- 枚举 `==`：按标签（通用相等）

### 9.2 元组

```hc
var t = (1, "a", 2.5);              // 字面量
var t: (i32, &[u8], f64) = ...;     // 类型标注
var a = t.0;                        // 元素访问 t.0 / t.1
var (a, b, _) = t;                  // 解构（_ 占位符放弃值）
fn divmod(a: i32, b: i32) (i32, i32); // 多值返回
var (q, r) = divmod(a, b);          // 调用解构
```

- 无名称、**初始化后字段只读**的匿名值类型（天然连续内存）
- 字段级只读（无 `mut`）
- **多值返回**：`fn divmod(a: i32, b: i32) (i32, i32)`；`E!(T1, T2)` 合法
- **比较**：`==` 逐元素（通用相等延伸）；可作 `Map` 键（元组哈希）
- **序列化**：天然 `to_bytes`/`from_bytes`；可内嵌进 class 字段
- `@addWithOverflow` 等返回 `(T, bool)` 元组

### 9.3 Table

```hc
// 构造（定长）：分配器、行数、列数、填充初始值
var tbl = Table<i32>.init(alloc, 4, 8, 0);
var v = tbl[i, j];                  // 多参索引（仅 Table 合法）：读单元格

tbl[i, j] = 5;                      // 写单元格
// tbl[i] = 行;                     // 整行赋值不支持（行视图只读）

// 密封构造（B 方案，2026-08-22）：回调内写格，返回编译期强制只读表
var sealed = Table<*mut Obj>.init_with(alloc, 4, 8, |i, j, cell: *mut *mut Obj| {
    cell.* = &mut obj;              // 回调内可写单元格（cell = 单元格可写引用）
});
// sealed[i, j] = v;  sealed[i, j] += v;  &mut sealed;   → 均编译错误（不可解除密封）
```

- 内建泛型二维结构 `Table<T>`，**代替二维数组**（`[M][N]T` 语法不再提供，一维数组保留）
- **引用类型**（与数组/集合一致：传参走引用、复制需显式 `copy`）；**底层 = 每行一个连续 `T[]` 数组，行主序**（逻辑连续、非整表单缓冲；可 `to_bytes`）
- **访问**：`t[i, j]` 单元格（行、列，仅 Table 合法的多参索引）；`t[i]` 行视图（返回切片 `&[T]`/`&mut [T]`，`t[i][j]` ≡ `t[i,j]`，行视图 `.len()` = 列数）；越界检查按模式（Q24）
- **写**：单元格赋值 `t[i,j] = v`、复合赋值 `t[i,j] += v`；**整行赋值 `t[i] = 行` 不支持**
- **方法**：`t.len()` = 行数、`t.cols()` = 列数；不提供 `.get/.set`（索引即语法）
- **迭代**：`for x in t` 产出**扁平单元格**（行主序，元素 = T，IIterable 三态套用）；行用 `t[i]` / 嵌套 for
- **密封构造 `init_with`**：`Table<T>.init_with(alloc, rows, cols, cb)`——回调 `|i, j, cell: *mut T|` 内写格（`cell.* = v`）；返回**编译期强制只读表**（直接赋值 / 复合赋值 / `&mut t` 均编译错误，不可解除密封）；只读操作（读 / 行视图 / `&t` / `copy(t)` / 迭代 / to_bytes）全部可用。背景：H 绑定级只读（默认只读 Rust 式）**文档已写但未实现**（C4，1.x 待办），故用类型级密封补只读保证
- **泛型参数**：T 可为任意类型（含指针）——`Table<T>` 拥有元素 / `Table<*T>` 存只读引用 / `Table<*mut T>` 存读写引用；`t[i,j]` 返回元素指针，`Table<*mut T>` 经 `t[i,j].*` 可写 pointee；**单元格指针替换**：密封表一律不可替换，普通表当前允许（待 1.x 绑定级只读门控）；只读指针表用 `init_with` 逐格赋 `&mut obj`，空指针表用 `Table<?*mut T>` + `null`
- **copy / 所有权**：内建 `copy(t)` 深复制整表（保留 alloc，`CopyMode.shallow` 可选）；作用域退出释放所有行数组；`move` 合法
- **嵌套**：`Table<Table<T>>` 合法（泛型递归）
- **序列化**：to_bytes = u64 LE 行数 + u64 LE 列数 + 行主序元素字节（自描述，from_bytes 可恢复定长表）；**空表**（0×N / N×0 / 0×0）合法
- 不提供 `==`（显式遍历比较）；变长需求用集合组合（`Vec<Table>` 等）

### 9.4 String（内建新类型）

- **`String` = 内建新类型（newtype）**：底层布局 = `Vec<u8>`（编译器内建实现）；功能与 `Vec<u8>` 一致，数组/切片规则全部适用
- **零成本互转**：`as_slice()` 无前缀内容视图 `&[u8]`；字面量 = `&[u8]` 静态只读切片；String 与 `&[u8]` 经 `as_slice` / `String.from` 显式互转
- **构造**：`String.from(&[u8], alloc)` / `String.from_slice(&buf, arena)`
- **方法**：`concat` / `split` / `join` / `find ?usize` / `substring` / `replace` / `to_upper` / `to_bytes()`（带 u64 前缀序列化）/ `==` 内容比较（经 ICompare，内建）
- 编译器内建实现 `ICompare`（内容序）；拼接走方法 `s.concat(other)`（无 `+` 重载）

### 9.5 Tree（递归/层级）

```hc
interface ITreeLeaf{
    parent:?owned *mut Tree<ITreeLeaf>
    children:Vec<Tree<ITreeLeaf>>
}
Tree<ITreeLeaf>   // 递归/层级：节点含子节点集合，父 owned 拥有子
```

- 成员：字段（标量/连续类型/元组/其它复杂类型，可带 `owned`）+ 方法（函数成员）+ 显式接口实现
- 构造 = `new()` 样板（脚本生成）或普通函数返回实例；无特殊构造语法
- **无隐式析构**：资源清理走 `defer`
- 无继承，组合优于继承
- **无限大小类型拒绝（2026-08-22 定案）**：所有类型必须有限大小且可计算——值内嵌自引用/互递归（无间接层）= 编译错误（报类型名 + 循环链位置）；合法间接层（打破循环）= 指针/装箱/堆容器（Vec/Map/Table/String）/`?T`；`tree`/`LinkedList` 既有递归不受影响

---

## 10. 接口与标量接口族

### 10.1 接口声明与实现

```hc
interface IShape {
    fn area(self: *Self) f32;   // self = 实现类型的实例；Self = 实现类型
}

class Rect: IShape { ... }              // implements 标注 = 冒号后缀
class Foo: IShape, IDrawable { ... }    // 接口列表逗号分隔
class Fib: IIterable<i32> { ... }       // 泛型接口在 implements 中实例化（尖括号类型参数）
```

- **显式声明实现**（冒号后缀）；可描述复杂类型、标量、内建类型、用户定义类型
- **存储形态（连续/堆上）由特性标注决定**，不参与接口标注
- **三用途**：① 标记 class 功能；② 标记参数类型（`where T: IShape` 约束）；③ 类型参数编译可验证
- 不提供运算符重载；闭包「调用契约」= 内置调用接口类型 `FnN<参数> 返回`

### 10.2 接口指针（胖指针）

- `*IShape` = **胖指针**（**三字宽 = data + 虚表 + alloc 引用**）
- 装箱时携带分配器，销毁  `owned *I` 时用携带的 alloc 释放 data
- `box(rect, alloc)`（ `owned *mut Rect`）赋给接口指针时**编译期自动收窄**（接口实现检查通过即合法）
- data 部分参与 Debug 悬垂标记，虚表指针不参与（编译期静态）
- **接口 = 类型标注**：`*INumber` = 只读引用、`*mut INumber` = 可写引用（标量可 `box` 装箱）

### 10.3 接口参数

- 接口类型传参 = **带约束的虚拟类型 T**，约束放签名末尾 **where 子句**：`fn add(a: *T) void where T: INumber`
- 形态映射：`&T`→`*T`（只读）/ `&mut T`→`*mut T`（可写）/ `move T`→ `owned T`（拥有）
- 调用点显式：`add(&a)` / `add(&mut a)` / `add(move a)`
- **静态分发**（单态化、无虚表）为主路径；动态分发（`*IShape` 胖指针装箱）保留给异构集合

### 10.4 接口工厂

- `Io.threaded() -> ThreadedIo` / `Io.evented() -> EventedIo`——返回具体实现类型（实现 Io），可参与 `T: Io` 单态化
- 入口 `fn main(io: Io)` 编译器注入并自动收窄为接口句柄
- 具体类型值传给接口句柄参数时自动装箱

### 10.5 标量接口族

```hc
interface ICompare {
    fn eq(self: *Self, other: Self) bool;   // a == b
    fn lt(self: *Self, other: Self) bool;   // a < b
}   // ne/le/gt/ge 由编译器派生

interface INumber: ICompare {
    fn add(self: *Self, other: Self) Self;  // a + b ≡ a.add(b)
    fn sub(self: *Self, other: Self) Self;
    fn mul(self: *Self, other: Self) Self;
    fn div(self: *Self, other: Self) Self;
    fn neg(self: *Self) Self;
}

interface IABS {
    fn abs(self: *Self) Self;
}

interface IMod {
    fn mod(self: *Self, other: Self) Self;
}

interface IPow {
    fn pow(self: *Self, exp: Self) Self;
}
```

- 内建标量**编译器内建实现**对应接口：`i8–i128`/`isize` → `IMod`；`u8–u128`/`usize` → `IABS`；`f16–f128` → `IPow`（不可用户重载）
- String 编译器内建实现 `ICompare`（内容序）
- **相等比较 `==`/`!=` = 值比较**（H3 定案）：标量/枚举/元组/String/集合按值（经 `eq`）；**class 需实现 ICompare 才有 `==`，否则编译错误**；指针比较 = 指向对象地址（数组指针/切片含位置 + 长度）
- 运算符 `+ - * /`（及一元 `-`）绑定 `INumber` 族；`%`/`%%` 由编译器派生
- `bool`/`char`/`void`/指针不实现（非数字）
- 泛型约束示例：`fn sum(a: &[T]) T where T: INumber`

### 10.6 迭代契约

- 接口 **`IIterable`** 按元素访问形态三态（泛型实例化语法与 `Vec<i32>` 一致用尖括号）：

| 接口 | 形态 | 语法 |
|---|---|---|
| `IIterable<*T>` | 只读迭代 | `for (x) \|item\|` |
| `IIterable<*mut T>` | 可写迭代 | `for (x) \|mut item\|` |
| `IIterable<owned T>` | 拥有迭代 | `for (x) \|move item\|`（消耗元素/转移所有权） |

- 元素类型 T 与形态由接口方法 `next(self: *mut Self) ?T` 按对应形态推断
- 内建类型（数组/切片/Vec/Map/Table/String）编译器内建实现三态
- **拥有迭代语义**：`for (x) |move item|` = 迭代器持有容器所有权——x 被 move 进迭代器，next 逐元素转移所有权，迭代后容器不可再用
- 用户类型实现迭代接口即可参与 `for`；`arr.iter()` 迭代器为**显式数据对象**（可传递/组合）
- 一次性迭代器 1.0 即可，惰性/组合子迭代留 1.x
- **迭代器对象 API（2026-08-22 定案）**：`iter()` 返回迭代器方法签名 = `next(self: *mut Self) ?T` + `filter(fn)` / `map(fn)` 组合子（返回**新的显式迭代器对象**，链式可组合）；**惰性求值（`next()` 按需求值、链式延迟计算）真实现仍留 1.x（A7 不动）**——迭代器/组合子为显式数据对象，非隐式求值机制

### 10.7 序列化内建契约

- **连续类型 ↔ bytes**：`to_bytes` / `from_bytes` 内存直映射（内建，仅连续类型；`packed`/`align(N)` 布局尊重）
- **堆类型 ↔ JSON**：`to_json` / `from_json` 内建 + 脚本生成可定制
- **集合 → 字节**：Vec/Map/Table/切片 → byte 数组（内建，长度前缀 u64 LE + 元素字节）
- 序列化能力为**内建接口契约**（编译器实现，用户不可重载）

---

## 11. 所有权与内存模型

### 11.1 核心模型

- **作用域** = 无名字的函数；函数 = 有名字的作用域
- 所有权注册在作用域上，退出自动销毁；**销毁顺序 = 声明逆序（LIFO）**
- **所有权 = 销毁责任的唯一归属**（Zig 式）：同一时间只有一个对象（作用域或分配器）对「变量所在内存退出时销毁」负责——**不是访问权限控制**
- 非 Arena 分配器创建的复杂类型 → 销毁责任**默认注册当前作用域**（退出自动销毁；显式 `o` 冗余标注）
- Arena 分配 → 由 Arena 统一负责销毁（归 Arena）
- 总原则：凡由分配器分配的对象都有所有权；global 与 Arena 分配的对象所有权归固定对象（根作用域/Arena），不可 move

### 11.2 所有权情况全录

| 类别 | 情况 |
|---|---|
| **A. 无所有权** | ① 值类型（标量/连续类型）——栈内存随作用域退出天然回收；② 指针/切片自身——栈上地址值，销毁责任在指向的目标；③ Arena 分配的对象——归 Arena 统一回收 |
| **B. 有所有权（责任人）** | ① 非 Arena 分配 → 当前作用域；② Arena → Arena；③ global → 根作用域；④ 复杂类型字段带 `o` → 父对象；⑤ `copy()` 新对象 → 当前作用域 |
| **C. 转移（move）** | ① 函数参数/返回值（变量本身不变——move 后原绑定仍可访问，悬垂/冲突由用户负责）；② 字段带 `o` 赋值 → 父对象；③ 线程捕获；④ **禁止 move**：Arena、global、无所有权对象 |
| **D. 销毁时机** | ① 作用域退出递归销毁一切有所有权子对象；② 根作用域退出（程序结束）；③ Arena 统一回收；④ 显式销毁仅限资源（文件句柄等）→ `defer` |

### 11.3 引用与指针

- **指针自由**：变量可有多个读写指针（`*mut`）与多个只读指针（`*T`）；指针问题（悬垂/别名）由用户负责；`*mut` 可复制
- **引用类型赋值 = 编译错误**：`var s2 = s1;`（数组/集合/String 等引用类型）**不合法**
  - 共享数据走显式指针 `var p = &s1;`（指针问题用户负责）
  - 复制走显式 `copy(&s1)`（新建内存、有所有权）；`copy` 默认深复制，浅复制需显式标注
  - 标量/连续类型赋值即复制（值语义）
- **move 语义**：把有所有权的变量「销毁的权力」在作用域之间转移——**变量本身不会移动**（内存地址不变、原绑定仍存在）；**复制内容必须是显式的**（`copy(&x)`，赋值不隐含复制）；仅对**拥有所有权**的变量合法；调用点显式 `move`；转移后目标作用域负责销毁；原绑定继续访问造成的悬垂/冲突由用户负责

### 11.4 内存机制

| Allocator 方法 | 语义 |
|---|---|
| `alloc.alloc(n) &[u8]` | **字节分配** |
| `alloc.init(T)` | 按类型创建实例（无参形态） |
| `alloc.init(T{字段 = 值, ...})` | 按类型创建实例（带参形态） |
| `alloc.deinit()` | 释放——Arena 统一回收场景 |

- **Arena**：批量分配器——一次分配大量内存、统一回收（`Arena.init(alloc)` / `arena.alloc(n)` / `arena.init(T)` / 作用域退出自动统一回收或显式 `deinit`）
- Arena 分配的对象**无所有权**（归 Arena，禁止 move——move 须对整个 Arena 进行）；适用请求级生命周期
- **o 默认规则**：非 Arena 分配器创建的复杂类型默认由作用域负责销毁（退出自动销毁，无需显式 o/deinit）；Arena 例外；`defer` 管非内存资源
- **defer/errdefer**：资源清理（不用隐式析构）；多 defer 按 LIFO
- **无 GC**：双模式同一套模型（Avoid: 引用计数）

---

## 12. 错误处理

### 12.1 error union

```hc
fn parse(data: &[u8]) ParseError!Value { ... }   // 显式错误集 E!T
fn f(x: &[u8]) !i32 { ... }                      // 推断错误集 !T
var v = try parse(data);                          // try 传播
var x = expr catch 默认值;                        // catch 处理
var x = expr catch |err| { ... };
if (e!) |v| else |err| { ... }                    // 双向捕获（必须成对，错误显式处理）
```

- `E!T` / `!T` 表达「可能出错」：成功时为 T、失败时为错误值；与 optional（`?T`「可能没值」）**正交**
- **错误值引用**：`error.Name` 在任何上下文可引用（**错误名全局唯一**）；可 return/比较/switch 分支匹配；返回显式错误集的函数中 `return error.X` 未在集合内 → 编译报错；`anyerror` 上下文任意错误名可用
- **`!T` = 推断错误集**：错误集由编译器从函数体收集；与显式错误集语义一致、调用方可穷举；递归/泛型无法收集时退化 `anyerror`
- 错误集联合：`A || B`
- **忽略错误仅 `catch |_| {}`**（不提供 `catch {}` 简写，忽略必须显式）

### 12.2 错误值运行时表示

- 错误 = **全局唯一整数错误码**（编译器维护「错误名 ↔ 码」表，跨包统一）
- **编码 = 「包 ID + 包内码」**（高位 = 编译单元包 ID，低位 = 包内错误序——静态链接与动态库/插件场景均无冲突）
- error union 运行时表示 = 错误码 + 成功标记（**Zig 式——成功路径零额外负载**）
- Debug 附带错误源位置（返回点）；`anyerror` = 任意码（64 位空间）

### 12.3 不可恢复终止

- **`@panic("消息", 位置)`**：不可恢复运行时错误——打印消息 + 位置（Debug 带堆栈），**abort 终止**
- **不执行 defer 清理**、**无 unwind/recover**（回卷是隐式控制流，不引入——与「没有隐藏控制」一致）
- 测试环境：测试函数内 panic → 该测试记 FAIL（不终止整个 `hc test`）；Release 同样 abort
- 与 error union（可恢复）正交

### 12.4 退出映射

- **`ExitType` = 语言内建枚举**：`enum ExitType { Exit, Error }` + `io.exit(t: ExitType, code: u8) !void`
  - `Exit` 正常静默 / `Error` 错误退出（打印错误标记/位置）
- `main` 返回 error → 等效 `io.exit(ExitType.Error, 1)`；正常返回 → `io.exit(ExitType.Exit, 0)`；测试失败 → 非零退出码

### 12.5 测试断言

| 断言 | 语义 |
|---|---|
| `expect(cond)` | 断言条件为真 |
| `expect_eq(a, b)` | 失败输出期望 vs 实际；支持 `==` 可比较类型（含 String 内容比较） |
| `expect_neq(a, b)` | 不等断言 |
| `expect_error(error.e, expr)` | 断言表达式抛出指定错误 |
| `expect_eq_slices(a, b)` | 失败输出长度 + 首个差异位置 |

- 全部返回 `anyerror!void`（`try` 传播即失败）；测试函数内隐式可用

---

## 13. 模块与包

### 13.1 命名空间

```hc
namespace Math { ... }   // 块式分组——可跨文件、一文件多组
```

> 命名空间是一组用点分隔的名称，每个名称下面都包括一些类型和方法。它是一组相关联的类型方法的集合。


- 同包跨命名空间访问经 `import`；`pub` 管**包边界**：跨包可见需 `pub` + 依赖声明
- 命名空间名 `PascalCase`（首字母大写，缩写词全大写，如 `namespace TCP`）
- **`src/Modules/` 目录定义模块**（ADR-0026）：`src/Modules/` 下的每个子目录 = 一个模块。子目录名即模块名，编译器自动发现。模块内非 `pub` 符号对外不可见。模块必须定义 `context.hc` 实现 `IContext` 接口。

### 13.2 模块系统（ADR-0026，2026-08-25 定案）

> 模块是 `src/Modules/` 目录下的子目录，每个模块是一个独立的 IoC 容器域。模块只知接口，不知具体实现，通过 `IContext` 接口注册和获取依赖。

- **物理结构**：`src/Modules/X/` 目录 = 模块 X，命名空间 = `project.X`。每个模块必须包含 `context.hc`（定义 `IContext` 实现）。
- **边界**：模块 owns 数据（领域类型 = 模块内 `class`，无 `pub` = 模块私有）；对外只暴露 **`pub` API**（接口定义和 context 结构体）
- **模块是库包的基本组成单位**（库 = 1+ 模块，`Kind::lib`）
- **`IContext` 接口**（`H.std.ioc` 提供）：`register<T>(impl)` 深拷贝到 Arena / `register<T>(name, impl)` / `registerFactory<T>(name, factory)` / `get<T>() -> *T`（Arena 引用，不 defer） / `get<T>(name) -> *T` / `make<T>(name) -> owned T`（调用者拥有，必须 defer）
- **内存管理**：`get` 返回 Arena 引用（无所有权，不需要 `defer`）；`make` 返回 `owned T`（必须 `defer` 或 `move`）；`register` 深拷贝到 Arena，调用者管理原实例
- **Context 层级委托**：子 context 持有父 context 引用，解析不到时向上委托。每个 context 背靠 Arena 分配器，context 销毁时所有通过它创建的对象一并销毁。
- **模块面向接口编程**：模块只知接口，不知具体实现。注册什么就用什么。接口定义在提供该模块中，使用方通过 `import` 引入。
- **模块间连接**：`import` = 符号引用（类型/函数，API 面）；`context` = 数据/依赖注入——两者正交。
- **`[module]` 特性标记已移除**（由 `src/Modules/` 目录结构替代）

#### 引导流程示例

```hc
// src/main.hc
import H.std.{io};
import H.std.ioc.{IContext, AppContext};
import myapp.Auth.{AuthContext, IUserService};

fn main() !void {
    var app_ctx = AppContext.init(alloc);
    defer app_ctx.deinit();
    app_ctx.register(IUserService, UserService{});
    var auth = AuthContext.init(app_ctx);
    run(app_ctx);
}
```

#### 测试

```hc
// tests/test_auth.hc
import myapp.Auth.{AuthContext, IUserService};

[Test] fn test_auth_service() !void {
    var ctx = AuthContext.init(alloc);
    defer ctx.deinit();
    ctx.register(IUserService, MockUserService{});
}
```

### 13.3 导入语句 import

```hc
import H.std.{io as my};        // 符号选择 + as 别名（重名重命名）
import H.std.net.{http, tcp};   // 多符号选择
import pkg.mod;                 // 整模块导入
```

- **`import` 是文件级导入语句**（声明可放文件任意顶层位置，导入符号作用域 = 文件）
- **导入对象 = 模块**（`src/Modules/` 目录定义的模块或包）
- **import 与上下文分工**：`import` = **符号引用**（类型/函数，API 面）；模块间**数据连接走上下文**（init 参数注入）
- 路径形态：`包名.模块名`——`H.std` = 内置标准库根路径；用户库经 build.zon 声明后按包名引用
- **库符号访问规则**：库函数可直接调用；库类型需创建（`alloc.init(T)` 堆上 / 值字面量栈上）
- **冲突规则**：通配/整模块导入遇同名冲突 → 编译错误；显式 `import pkg.mod.{name}`（非通配）优先级高于通配；`as 别名` 显式改名

### 13.4 可见性

- **默认私有**（`pub` 显式导出——显式优于隐式）；同模块内可访问
- `pub` 是包（模块）边界——跨包只暴露 `pub` 项

### 13.5 编译单元 / 文件模型

- **目录 = 包（package）**——包内全部 `.hc` 文件**共享命名空间**（同包跨文件直接可见）
- 跨包访问：`import pkg.mod` + `build.zon` 依赖声明
- `hc build` 编译包内全部文件；`hc run file.hc` 单文件脚本运行（隐式单文件包）；`hc run <目录>` 目录包运行（入口 = `main.hc` 或首个 `.hc`）
- **包形态**：
  - **应用** = 含 `main` 的包（`Kind::exe`，产出可运行 exe）
  - **库** = **不含 main** 的包（`Kind::lib`，代码集合 = 1+ 模块，不单独运行；产出 **lib 静态库**（`hc build`，编译时链接进 exe）或 **dll 动态库**（`hc build --dll`，exe 运行时加载））

### 13.6 包管理

- 包管理器内置编译器
- **依赖清单 = H 数据字面量**：`const build = Build{ name = ..., deps = [...], ... }`（构建时校验版本与指纹）
- **build 文件 = `build.zon`**：内容为 H 数据字面量——包名/版本/作者 + 依赖列表（名称/来源/版本/哈希指纹）+ 构建选项；`hc pkg add ns/pkg@ver` 写依赖
- **供应链安全**：依赖包 `script` 块执行前指纹校验 + 来源审计
- 官方注册中心（自托管 MVP → 治理规则；第三块 E5）

---

## 14. 元编程（script / comptime）

### 14.1 双轨

```hc
script { ... }            // 脚本生成：编译前执行（无运行时环境，纯模板生成）
comptime { ... }          // 编译时执行（语义分析阶段，完整类型系统，可执行更多操作）
fn List(T: type) type     // comptime 式泛型：编译期函数、类型即值、anytype、惰性实例化
```

- **分工**：comptime 管类型级计算（泛型）；脚本生成管样板（数据定义驱动：序列化/校验/存储）——样板场景下脚本功能**可选**
- **执行位置与可见数据**：
  - `script` 块 = **编译前**执行（解释器），无运行时环境（io/alloc/argv 不可用）；类型信息可见 = 所在作用域（script 块在 class 内 → 该 class 类型数据可用；在命名空间下 → 整个命名空间类型信息可用，`types` 元数据对象 `types.fields/type/all` 可见范围随块位置）
  - `comptime` 块 = **编译时**执行（语义分析阶段），能获取的数据多得多（完整类型系统，`types` 元数据可见全部类型）
- **生成物不得与当前环境冲突**（同名 = 编译错误）
- **脚本输入机制**：script 块产物 = 生成的代码字符串就地替换本块；脚本用 H 字符串操作遍历类型定义拼接生成代码，无第二语言
- **comptime 块与错误机制**：作用域/函数/script 块/comptime 块均为可返回错误的执行单元（error union）——script/comptime 块执行失败 = **编译错误**（带块内位置 + 所属块位置）
- **降级闸门**：脚本生成保持 1.0 必达；若成本超预期 → 降级为「脚本生成 = 编译期执行 H 子集函数（comptime 内联）」，`script{}` 语法保留
- **序列化定制**：脚本生成序列化/校验/存储样板（数据定义 → 样板）——`types.fields` 驱动校验（String 非空 / i32 ≥ 0 / ?String null 守卫）与 to_json（String 带引号 / i32 裸值 / ?String→`null`）
- **无宏**（与「没有隐藏控制」一致）

### 14.2 comptime 类型函数（已实现）

- `fn List(T: type) type`：参数含 `type`/`anytype` 即触发编译期执行的普通函数
- `anytype` 完整语义：调用点按实参具体类型实例化——返回 `anytype` 解析为体 return 表达式在具体绑定下的类型
- `comptime_int` / `comptime_float` 类型识别与惰性宽度
- `comptime` 块：装载期受限 Interp 求值、结果丢弃、失败 = 编译错误
- **嵌套/递归实例化**：`PairPair<i32>` / `LinkedList<T>` 自引用
- **comptime 值函数**：参数含 `T: type`、非返回 `type` 的普通函数调用点编译期求值（如 `array_len(i32)` = 4 折叠）

---

## 15. 并发与异步

### 15.1 基础语法

```hc
var shared: owned Hub<i32> = ...;        // 四模式共享容器
var t: owned Thread<i32> = spawn(af, ...);       // spawn = 函数 + 显式参数
var r = try t.join();                        // 消耗所有权（await 同源）
t.cancel() / t.is_done() / t.detach()
var f: Future<R> = af(...); var v = await f;  // await 任何函数可用
```

### 15.2 四模式类型 ✅

| 类型 | 语义 |
|---|---|
| `Pipe<T>` | 单读单写 |
| `Tee<T>` | 单读多写 |
| `Funnel<T>` | 多读单写 |
| `Hub<T>` | 多读多写 |

- 内建泛型共享内存容器；写者数量由类型名保证（单写者无锁、多写者互斥）
- **协作式透明实现**：单线程确定性模型下四变体运行时行为一致（读者/写者数量为类型层契约，不引入真锁/真并发；真 OS 并行归 1.x）

### 15.3 四模式类型方法集

| 方法 | 语义 |
|---|---|
| `init(alloc)` | 构造（共享内存容器） |
| `init(alloc, cap)` | 通道有界构造 |
| `write(v)` | 队尾追加；close 后 → `error.Closed` |
| `read() T` | 队首弹出；空 → `error.Empty` |
| `try_read() ?T` | 队首弹出或 null |
| `close()` | 置结束标志 |
| `send(v)` | 通道方法：有界写，满 → `error.ChannelFull` |
| `recv() T` | 通道方法：≡ `read` |

- 全部方法取 `*Self`（并发安全由类型保证，用户类型不可模拟）

### 15.4 缓冲与阻塞（协作式映射）

- **共享内存容器**（write/read）：无容量概念——write 不阻塞、read 空 → `error.Empty`
- **通道**（send/recv）：有界队列——容量构造时指定，send 满 → `error.ChannelFull`（无真阻塞）、recv 空 ≡ read
- close 后 write/send 报 `error.Closed`、try_read 返回 null

### 15.5 线程所有权 ✅

- spawn 归当前作用域；退出时已完成→销毁、运行中→移交根作用域（**无隐式阻塞**）
- 根作用域 = 程序最后退出场所，负责最终资源回收
- 显式 `join() !T` 消耗所有权，错误以 error union 跨线程传播（join 透传 / cancel→`error.Cancelled` / detach 立即运行）

### 15.6 线程捕获 ✅

- 值类型复制值；引用类型 move 或 global
- **作用域例外**：作用域绑定的执行（join 后回到当前作用域）可捕获引用；逃逸线程引用捕获禁用（编译期检查）

**静态检查**：

- **绑定/逃逸判定**：句柄（Thread/Future）在声明作用域内被 join/await → 绑定（引用捕获合法）；被 detach 或作用域退出未 join → 逃逸（引用捕获编译错误）
- **冻结窗口（借用期）**：绑定场景下，被捕获引用的目标从 spawn 到 await/join 之间主线程不可写（不可 `var mut` 写入、不可取 `&mut`）——编译期检查，await/join 后恢复；并发写共享数据 → 显式用四模式类型

**Send/Sync 编译期诊断（2026-08-22 定案）**：

- **`Send` / `Sync` = 内建标记接口**（编译器内建实现，不可自定义/重载）；用户类型显式标注：`class Foo: Send` / `class Bar: Send, Sync`（implements 冒号后缀）
- **可推导性（组合性验证）**：标量/值类型（Continuous/元组/枚举）自动 `Send`+`Sync`；指针/切片看指向类型；`Vec`/`Map`/`Table`/`String` 等内建容器看元素/负载；用户标注 class 由编译器验证字段全满足才合法（含 `*mut`/可变共享 → 非 `Sync`），验证失败编译错误
- **诊断模式（协作式，编译期）**：`spawn`/`await` 边界捕获非 `Send` 引用 → **编译错误带位置**（`captured value of type X is not Send at spawn boundary`）；非 `Send` 值不可跨线程捕获；**与 Q19 正交**（Q19 冻结窗口管借用期，Send/Sync 管类型层跨线程可传递性，不替代 Q18/Q19）；**真并行检查（运行时）1.x 启用**——协作式下仅编译期诊断，零运行时开销

### 15.7 async/await ✅

- `async fn` 返回 `Future<R>`（R = 完整返回类型含错误联合 `Future<!R>`）
- **await ≡ join()** 且**任何函数可用**（无 async 传染）
- 执行模型 = **协作式延迟执行**（非 Go 式协程/M/N）：async fn 调用点返回**惰性** `Future` 值，体延迟到 await 才执行
- 协作式取消（cancel → `error.Cancelled`）、is_done 状态转移、await 幂等缓存

### 15.8 Thread 方法集

| 方法 | 语义 |
|---|---|
| `join() !T` | 消耗所有权，等待完成 |
| `cancel() !void` | 协作式取消（延迟模型下运行点 = join/detach/程序结束，cancel 置协作标志） |
| `is_done() bool` | 完成状态查询 |
| `detach()` | 分离（独立运行到完成） |

- `await f` ≡ `f.join()`

### 15.9 Io 执行模型 ✅

- `Io.threaded()` / `Io.evented()` 构造器写 runtime 字段（默认 io = threaded）
- `io.poll()` 排空根回收队列（作用域退出提升的未 join 线程运行到完成并返回计数；threaded 恒 0）
- 设计保留：`Io.threaded()` = 阻塞 IO + 每操作线程（简单，默认），`Io.evented()` = **单线程事件循环**（select/epoll 式非阻塞 IO + async/await 协作调度）

### 15.10 原子操作 ✅

- `@atomicLoad(T, p, order)` / `@atomicStore(T, p, v, order)` / `@atomicRmw(T, p, op, v, order)`
- 内存序：`relaxed` / `acquire` / `release` / `acq_rel` / `seq_cst`（默认 `seq_cst`）
- **协作式透明实现**：单线程无竞争 → load = deref、store = 写穿指针、Rmw op = `.add/.sub/.exchange`（返回旧值），内存序五值求值后丢弃

---

## 16. 项目结构与代码约定

### 16.1 项目形态

- **目录 = 包（package）**：一个项目 = 一个目录，含 `build.zon` 清单 + `.hc` 源码文件
- **入口约定**：应用包入口 = `main.hc`（目录运行/构建优先取 `main.hc`，否则目录内排序后首个 `.hc`）
- **build.zon**：包清单 = `const build = Build{ ... }` 数据字面量——`name` / `version` / `kind` / `files` / `deps`

### 16.2 源码约定

- 源码 `.hc` 文件位于**包根**（与 build.zon 同目录）——同包文件**共享命名空间**（跨文件直接可见）
- 多文件按职责拆分（如 `main.hc` / `math.hc` / `io.hc`）；命名空间（`namespace X`）组织符号；`src/Modules/` 目录定义模块（ADR-0026）
- 目录参数形态：`hc run <目录>` / `hc build <目录>` 把目录当包加载；单文件 `hc run file.hc` = 隐式单文件包

### 16.3 测试约定

- 测试 = `[test("名称")] fn`，可**与源码同文件**（无独立 `test/` 目录），也可放在项目根目录的 `tests/` 目录下
- `tests/` 目录不参与命名空间系统，仅由 `hc test` 发现和执行
- `[test]` 函数可被普通代码调用/复用
- `hc test <dir>` 递归收集 `.hc`（`study/` 设计草图目录除外）；按父目录分组（同目录 = 同包）
- 断言五件套测试函数内隐式可用；`[test]` 函数内隐式 `test_io` + `alloc`
- `hc test --mode=compile` 原生交叉验证（需 zig cc）

### 16.4 依赖约定

- `build.zon` `deps = [ Pkg{ ... } ]`（Pkg 数组）；**本地依赖带 `path`**（相对路径，指向依赖包根）；无 path = 注册中心依赖
- `hc pkg add <name> --path <dir>` 写入依赖声明；缺失本地依赖在装载时**响亮诊断**（不静默跳过）
- 依赖包 pub 符号以包名前缀登记，`import pkg.{sym}` 选择导入 / `pkg.sym(...)` 限定访问

### 16.5 hc init 脚手架

`hc init <name>` 在**当前目录**生成最小项目骨架：

```
<name>/
├── build.zon     # 清单：name/version/kind=Kind.exe/files=["main.hc"]/deps=[]
└── main.hc       # 入口 fn main() !void + [test] 冒烟测试
```

- **名称校验**：`[A-Za-z0-9_-]`（目录名合法；非空、非 `.`/`..`）
- **安全**：目录已存在且非空 → 拒绝覆盖（报错退出，不触碰现有文件）
- 脚手架即**最小可运行示例**：`hc run <name>` / `hc test <name>` 全绿
- 骨架注释内嵌源码/测试/依赖约定

---

## 17. 双模式执行与入口

### 17.1 架构

- 共享前端 → 共享 IR → 双后端（字节码 VM 脚本模式 + LLVM 原生编译模式）
- 入口：`fn main() !void`——`args` 由运行时注入（0 号 = 程序名）
- 程序环境：`io.env(name) ?&[u8]` / `io.stdin` / `io.stdout` / `io.stderr`；io/alloc 为标准库模块与预导入环境（`import H.std.{io}` 显式引用）；**`io.args()` 取消**

### 17.2 错误检测策略（独立维度）

| 模式 | 检测策略 |
|---|---|
| Debug | 全检测 |
| Release | 裸路径 |
| 脚本模式 | Debug 语义 |

### 17.3 退出

- **语言内建枚举** `enum ExitType { Exit, Error }` + `io.exit(t: ExitType, code: u8) !void`
  - `Exit` 正常静默
  - `Error` 错误退出（打印错误标记/位置）
- `main` 返回 error → `Error`/1；正常 → `Exit`/0；测试失败 → 非零退出码

---

## 18. 测试

### 18.1 测试函数

```hc
[test] fn add_basic() !void {
    try expect(add(1, 2) == 3);
}

[test("custom name")] fn named_test() !void { ... }
```

- **`[test("名称")] fn 名称() !void`**：标记为测试的函数——`hc test` 收集运行；可被普通代码调用/复用
- 参数可省：`[test]` 省略显示名 / `[test("名称")]` 指定显示名；显示名 = 名称 ?? 函数名
- **测试失败 = error**（`try expect(...)` 传播即失败，报告带位置）

### 18.2 断言 API（归 std.debug，测试函数内隐式可用）

| 断言 | 语义 |
|---|---|
| `expect(cond)` | 断言条件为真 |
| `expect_eq(a, b)` | 失败输出期望 vs 实际；支持 `==` 可比较类型（含 String 内容比较） |
| `expect_neq(a, b)` | 不等断言 |
| `expect_error(error.e, expr)` | 断言表达式抛出指定错误 |
| `expect_eq_slices(a, b)` | 失败输出长度 + 首个差异位置 |

- 全部返回 `anyerror!void`

### 18.3 输出与统计

- `[PASS]/[FAIL]/[SKIP] 文件名::测试名`；FAIL 附错误类型 + 断言位置
- 汇总 `N passed, M failed, K skipped (总耗时)`；失败数 > 0 → 退出码非零；失败不中止

### 18.4 隔离 / 执行 / 跳过

- 每个 test = 独立块作用域（退出自动销毁）
- 默认串行（并发测试留 1.x）
- `return error.SkipTest;` → 统计为 SKIP

### 18.5 环境注入

- test 内隐式 `test_io`（独立 `Io.threaded()` 实例）与 `alloc`
- IO 测试默认真实执行

### 18.6 双模式

- `hc test` 默认脚本 Debug
- `--mode=compile` 编译 **Debug**（双模式一致性验证——含错误路径用例）
- `--release` 编译 Release（零开销验证——仅跑**正常路径子集**，不含越界/溢出/悬垂错误路径用例）

### 18.7 示例测试形态（四层）

| 层级 | 内容 |
|---|---|
| S1 | 纯逻辑断言 |
| S2 | main smoke |
| S3 | 局部逻辑 |
| S4 | 演示标注（输出不捕获） |

---

## 附：语法速查

```hc
[continuous]
class Point { x: f32, y: f32, fn dist(a: *Point, b: *Point) f32 }  // 连续内存值类型
class Person { name: String, age: i32 }                             // 未标注 → 堆上
var p = alloc.init(Person{ name = ..., age = 30 });                 // 带参构造
var q = alloc.init(Person);                                         // 无参构造

enum Kind { player, enemy }                          // 合一式枚举
interface INumber: ICompare { fn add(self: *Self, other: Self) Self }  // 接口（冒号标注，可继承）
var t = (1, "a");                                    // 元组：访问 t.0 / 解构 var (a, b) = t;
var tbl = Table<i32>.init(alloc, 4, 8, 0);           // Table（方法构造 + t[i, j]）
fn f(a: i32, b: i32 = 0) i32 { ... }                 // 重载 + 可选参数（尾部、编译期常量默认值）
[test] fn check() !void { try expect_eq(add(1, 2), 3); }  // 测试函数
script { ... }  /  comptime { ... }                   // 元编程双轨
```

---

## 文档索引

| 主题 | 对应规范文件 |
|---|---|
| 词法、声明、运算符、语句与控制流、测试 | `06-01-syntax.md` |
| 基础类型（标量、切片、可选、错误联合、指针） | `06-02-types.md` |
| 扩展类型（class、枚举、元组、Table、String、tree） | `06-03-extended-types.md` |
| 函数与闭包、内建函数（box/copy/@） | `06-04-functions.md` |
| 接口、标量接口族、迭代契约、序列化内建 | `06-05-interfaces.md` |
| 所有权与内存模型 | `06-06-ownership.md` |
| 错误处理（error union、错误码、@panic） | `06-07-errors.md` |
| 模块与包（namespace/import/pub/build.zon） | `06-08-modules.md` |
| 元编程（script / comptime） | `06-09-meta.md` |
| 并发与异步（四模式、线程、Future） | `06-10-concurrency.md` |
| 项目结构与代码管理约定 | `06-13-project-structure.md` |
| 规范总纲 | `06-language-spec.md` |
