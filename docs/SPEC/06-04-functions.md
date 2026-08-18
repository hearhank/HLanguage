# H 语言规范：函数与闭包、内建函数

> 对应实现模块：07 第一块语言系统 M2 语义（函数）/ M4 运行时（内建）。

## 函数

```hc
fn main(args: o Vec<String>) !void {}   // 入口：args 注入（0 号 = 程序名）；!void 入口错误运行时报告；环境经 `import H.std.{io}`（io.env(n)/io.stdin/io.stdout/io.stderr）；退出 io.exit(ExitType.Exit|Error, code)（2026-08-14 定案；2026-08-17 修订入口形态，ADR-0010）
fn fun(y: o *mut T) void {}          // o T：参数拥有（Q22b 类型标注制；退出销毁）
fn add(a: *T) void where T: INumber {}  // 接口约束：where 子句在签名末尾（Q22b）
fn f(x: &[u8]) !i32                   // 返回 error union
async fn af() T                       // 返回 Future(T)（第三块 E2）
var hp: o *mut Point = box(p, alloc); // 装箱：值 → 堆引用（Q8，显式分配器）
```

- **重载与可选参数（2026-08-14 定案；M1/M2 修订）**：允许重载——签名 = 函数名 + 参数类型列表 + 返回类型（共同决定）；**解析顺序（M1 定案）**：① 精确参数数量匹配（忽略默认值）→ ② **具体非泛型候选优先于泛型候选** → ③ 泛型候选按 where 约束编译时验证（接口限制运行时拆除，单态化零开销）→ ④ 多个同等匹配 → 歧义编译错误（要求显式标注）；**期望类型传播（M2 定案）**——表达式在已知目标类型时优先选择返回类型匹配的重载（`var x: f64 = get();`），无目标类型且多候选 → 歧义报错；可选参数 `fn f(a: i32, b: i32 = 0)`（默认值须编译期常量、只能尾部）；接口方法为成员契约不参与全局重载解析
- **参数**：数据栈对象（标量/连续类型）默认复制；其它对象只能 `*T`（只读引用）/ `*mut T`（可写引用）/ `o T`（拥有，函数作用域接管，退出销毁）
- **接口约束参数（Q22b 定案）**：虚拟类型 T + `where T: I1, I2` 约束子句（签名末尾）；调用点按形态显式：`add(&a)` / `add(&mut a)` / `add(move a)`
- **io 参数（Q22c 定案）**：库函数统一虚拟类型制 `io: *T where T: Io`（调用点 `&io`）；**入口特例**：`fn main(io: Io)` 由编译器注入默认 IO 实现句柄（唯一例外）
- **move 规则**：仅本作用域拥有（非 Arena/global）的变量可 move；拥有参数用 `o T` 标注（函数内隐含拥有，退出销毁）；`o` 与 `*`/`*mut` 正交（`o *mut T` 允许）；**调用点显式 `move`**（Q23）：`take(move s);` / `return move s;`
- **权限标注模型**（评审 D2）：类型标注 = 类型 + 读写权限（`*`/`*mut`）+ 所有权（o）；move 基于所有权判定，与读写权限同属统一权限体系；comptime 函数返回类型不适用运行时 move 规则
- **返回值**：`fn() oT` 返回拥有（所有权移出）；`fn() *T` / `*mut T` 返回引用；函数内新建的值必须 move 返回（无所有权的除外）；返回引用必须指向函数参数，不得返回局部变量引用；**元组多值返回** `fn f() (T1, T2)`（2026-08-14）
- **作用域退出销毁顺序 = 声明逆序（LIFO）**（Q26 定案）
- 方法 = 类型声明内的函数成员；方法调用双语：`p.dist(q)` ≡ `Point.dist(p, q)`（Q5，接收者自动取引用：首参 `*Self` 取 `&p`、`*mut Self` 取 `&mut p`）

## 闭包

- `|x| expr` 默认只读捕获；`mut |x| ...` 可写捕获（Rust 标杆）；`move |x| ...` 转移捕获（粒度 = 整个闭包）；调用 `f(x)`；**捕获精确化（2026-08-17，Phase 8 落地）**——捕获集合 = 自由变量精确分析（只捕获 body 实际引用、未被体内绑定遮蔽的外部变量，含嵌套闭包传递；未引用变量不捕获、闭包不可见）；只读捕获内重绑定被捕获变量（含复合赋值展开）→ `error.ReadonlyCapture`（写穿指针/字段/索引仍允许）；`move` 捕获深拷贝独立副本（Str/Closure 值递归复制——原绑定/原闭包捕获变量后续变更不影响闭包）
- 闭包是**数据对象**（捕获为字段的结构体 + 调用约定）；遵循所有权模型（可 `o`、可 move、捕获随闭包销毁）
- **捕获登记（Q26 定案；Q-S11 修订）**：捕获变量与闭包建立 Debug 可选悬垂标记（编译时选项，非安全保证）；闭包存容器合法
- **返回值规则（2026-08-14 定案）**：闭包可按值返回——捕获全为值类型（栈上数据对象随返回值复制，无悬垂）；捕获引用/堆数据的闭包返回时：捕获对象须可 move（`move` 捕获）或为 global，否则编译错误
- 单变量粒度捕获留 1.x

## 内建函数（Q12 定案，编译器内建，不可用户定义/重载；非关键字）

- **`box(value, alloc) -> o *mut T`**：装箱——分配堆内存、值写入堆、返回带所有权的可写指针；标量可装箱为接口指针（`*INumber` 等，2026-08-14）
- **`copy(&value, mode) -> T`**：按类型复制——标量 = 复制、引用类型 = 深拷贝、class = 递归复制（含内存树）；**默认深复制**，浅复制需显式标注——**内建枚举 `enum CopyMode { deep, shallow }`（L1 定案）**：`copy(&x)` ≡ `copy(&x, .deep)`、`copy(&x, .shallow)`（`.name` 推断枚举值字面量；浅复制引用字段共享，内存问题用户负责，Q1' 定案）

## @ 内建函数（Q-S1 定案，Zig 式）

| 类别 | 内建 | 语义 |
|---|---|---|
| 类型查询 | `@sizeOf(T)` / `@alignOf(T)` / `@offsetOf(T, "字段")` / `@typeOf(expr)` | 编译期常量（序列化/FFI 布局依赖） |
| 整数转换 | `@intCast(T, x)` | 宽度/符号转换；超范围 Debug 检测（Release 裸） |
| 枚举转换 | `@intFromEnum(e) usize` / `@enumFromInt(E, i)` | 变体序索引（0 起）/ 反向；越界 Debug 检测；仅纯常量枚举；**返回 `usize`（平台宽度，L6 定案）——序列化/固定宽度场景显式 `@intCast`**（2026-08-14 定案） |
| 指针转换 | `@ptrCast(T, p)` | 指针类型转换——**显式放弃类型安全的唯一逃生舱**（替代 Rust unsafe / C 强转） |
| 对齐 | `@alignCast(T, p)` | 对齐提升断言（Debug 检查） |
| 内存访问（volatile） | `@volatileLoad(p) T` / `@volatileStore(p, v)` | 防优化掉的读穿/写穿（LLVM `load volatile`/`store volatile`，MMIO 场景）；K2（ADR-0014，2026-08-18 落地） |
| 溢出 | `@addWithOverflow(a, b)` / `@subWithOverflow` / `@mulWithOverflow` | 返回元组 `(T, bool)`（value, overflow）；不受模式影响 |
| 编译期 | `@compileError("msg")` | 显式编译失败（comptime/脚本用） |
| FFI | `@cImport("header.h")` | 编译期解析 C 头文件生成 H 声明（Q-S4 定案；第三块 E3） |
| 原子操作 | `@atomicLoad(T, p, order)` / `@atomicStore(T, p, v, order)` / `@atomicRmw(T, p, op, v, order)` | 无锁原语（Q-S3 定案；第三块 E2）；`op` = `.add`/`.sub`/`.exchange`/`.cmpxchg` 等 |

- **内存序（Q-S3 定案，C11 五序子集）**：`relaxed` / `acquire` / `release` / `acq_rel` / `seq_cst`——**默认 `seq_cst`**（弱序需显式写）；四模式类型内部实现基于这些原语
- `@` 前缀不与用户标识符冲突；转换显式可见（「没有隐藏控制」）
- 其余 Zig 内建（`@bitCast`/`@mulAdd` 等）按需在 1.x 扩展（K4/K10 等系统编程缺口见 `05-open-questions`）
