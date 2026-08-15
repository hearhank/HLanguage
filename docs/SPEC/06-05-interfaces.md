# H 语言规范：接口、标量接口族、迭代契约、序列化内建

> 对应实现模块：07 第一块语言系统 M2 语义 / M4 内建。

## 接口

接口 = 类型的**特性标**（评审 A1）：声明类型必须实现的方法契约。

```hc
interface Shape {
    fn area(self: *Self) f32;   // self = 实现类型的实例；Self = 实现类型（Q14）
}

class Rect: Shape { ... }   // implements 标注 = 冒号后缀（Q14 正式定案，2026-08-14）
class Foo: Shape, Drawable { ... }
```

- **显式声明实现**（冒号后缀，2026-08-14 正式定案——接口列表逗号分隔）；可描述复杂类型、标量、内建类型、用户定义类型（class/元组）；**存储形态（连续/堆上）由特性标注决定**，不参与接口标注；**接口实例化标注（C4 定案）**：泛型接口在 implements 标注中可实例化——`class Fib: IIterable(i32)`（接口名 + 圆括号类型参数，与 `Vec(i32)` 一致）
- **三用途（Q-R9 定案）**：① 标记 class 功能（implements 标注）；② 标记参数类型（`where T: Shape` 约束）；③ 类型参数编译可验证
- 不提供运算符重载；闭包「调用契约」= 内置调用接口类型 `FnN(参数) 返回`（Q13）
- **接口指针（Q17 定案；M5 修订）**：`*Shape` = **胖指针**（**三字宽 = data + 虚表 + alloc 引用**——M5 定案：装箱时携带分配器，销毁 `o *I` 时用携带的 alloc 释放 data）；`box(rect, alloc)`（`o *mut Rect`）赋给接口指针时**编译期自动收窄**（接口实现检查通过即合法）；data 部分参与 Debug 悬垂标记，虚表指针不参与（编译期静态）；**接口 = 类型标注**——`*INumber` = 只读引用、`*mut INumber` = 可写引用（标量可 `box` 装箱）
- **接口参数（Q22/Q22b 定案）**：接口类型传参 = **带约束的虚拟类型 T**，约束放签名末尾 **where 子句**：`fn add(a: *T) void where T: INumber`；形态映射 `&T`→`*T`（只读）/ `&mut T`→`*mut T`（可写）/ `move T`→`o T`（拥有）；调用点显式：`add(&a)` / `add(&mut a)` / `add(move a)`；**静态分发**（单态化、无虚表）为主路径；动态分发（`*Shape` 胖指针装箱）保留给异构集合
- **接口工厂返回具体实现类型（R-4 定案）**：`Io.threaded() -> ThreadedIo`、`Io.evented() -> EventedIo`——返回具体实现类型（实现 Io），可参与 `T: Io` 单态化；入口 `fn main(io: Io)` 编译器注入并自动收窄为接口句柄；具体类型值传给接口句柄参数时自动装箱
- 接口方法（如 INumber 的 `add`）为**成员契约**，不参与全局重载解析（2026-08-14）

## 标量接口族（2026-08-14 定案；修订：比较接口化）

```hc
interface ICompare {
    fn eq(self: *Self, other: Self) bool;   // a == b（方法形式；== 本身语言内建通用）
    fn lt(self: *Self, other: Self) bool;   // a < b（序比较绑定 ICompare）
}   // ne/le/gt/ge 由编译器派生

interface INumber: ICompare {
    fn add(self: *Self, other: Self) Self;   // a + b ≡ a.add(b)
    fn sub(self: *Self, other: Self) Self;   // a - b
    fn mul(self: *Self, other: Self) Self;   // a * b
    fn div(self: *Self, other: Self) Self;   // a / b
    fn neg(self: *Self) Self;                // -a
}

interface IInt: INumber {
    fn mod(self: *Self, other: Self) Self;   // 取余
    fn abs(self: *Self) Self;
}

interface IUint: INumber {
    fn mod(self: *Self, other: Self) Self;
}

interface IFloat: INumber {
    fn abs(self: *Self) Self;
    fn pow(self: *Self, exp: Self) Self;
}
```

- 内建标量**编译器内建实现**对应接口：`i8–i128`/`isize` → `IInt`；`u8–u128`/`usize` → `IUint`；`f16–f128` → `IFloat`（不可用户重载）；String 编译器内建实现 `ICompare`（内容序）
- **相等比较 `==`/`!=` = 值比较，内部调用 ICompare 的比较方法（H3 定案）**：标量/枚举/元组/String/集合按值（经 `eq`，编译器内建实现）；**class 身份比较删除**（class 需实现 ICompare 才有 `==`，否则编译错误）；**指针比较 = 指向对象地址**（数组指针/切片含位置 + 长度——比较（地址, 长度）对）；**序比较 `< <= > >=` 绑定 `ICompare`**（未实现则编译错误）
- 运算符 `+ - * /`（及一元 `-`）绑定 INumber 族：`a + b` ≡ `a.add(b)`；`%`/`%%` 由编译器派生
- 泛型约束示例：`fn sum(a: &[T]) T where T: INumber`（作用于任意标量；见 86-scalar-interfaces.hc）
- **接口 = 类型标注**：`*INumber` = 只读引用、`*mut INumber` = 可写引用（标量可 `box` 装箱，Q17 统一机制）
- `bool`/`char`/`void`/指针不实现（非数字）

## 迭代契约（2026-08-14 定案）

- 接口 **`IIterable`** 按**元素访问形态**三态（泛型实例化语法与 `Vec(i32)` 一致用圆括号）：
  - `IIterable(*T)` — 只读迭代（默认 `for (x) |item|`）
  - `IIterable(*mut T)` — 可写迭代（`for (x) |mut item|`，评审 B5）
  - `IIterable(o T)` — 拥有迭代（`for (x) |move item|`，消耗元素/转移所有权）
- 元素类型 T 与形态由接口方法（`next(self: *mut Self) ?T` 按对应形态）推断；内建类型（数组/切片/Vec/Map/Table/String）编译器内建实现三态
- **拥有迭代语义（M4 定案，2026-08-14）**：`for (x) |move item|`（`IIterable(o T)`）= **迭代器持有容器所有权**——x 被 move 进迭代器，next 逐元素转移所有权，迭代后容器不可再用；内建实现——Vec/String/Deque 逐个 pop + 转移、数组逐元素 move（引用类型元素移出）
- 用户类型实现迭代接口即可参与 `for`；`arr.iter()` 迭代器为**显式数据对象**（可传递/组合）；一次性迭代器 1.0 即可，惰性/组合子迭代留 1.x

## 序列化内建契约（2026-08-14 定案：序列化 = 默认接口）

- **连续类型 ↔ bytes**：`to_bytes`/`from_bytes` 内存直映射（内建，仅连续类型；`packed`/`align(N)` 布局尊重，`@offsetOf`/`@alignOf` 可验证）
- **堆类型 ↔ JSON**：`to_json`/`from_json` 内建 + 脚本生成可定制（第三块 E1 定制通道）
- **集合 → 字节**：Vec/Map/Table/切片 → byte 数组（内建，长度前缀 u64 LE + 元素字节）
- 序列化能力为**内建接口契约**（编译器实现，用户不可重载）；「传输」「保存」支柱的底层统一机制（CONTEXT §2 数据序列化分层）
