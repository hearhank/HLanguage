# H 语言 (H Language)

一门以数据为中心、同时支持系统编程与脚本编程的编程语言，源代码以 `.hc` 为后缀。设计的核心哲学是：语言的首要职责是「定义数据、修改数据、传输数据、保存数据」，一切语言特性围绕这四项第一性原则组织，并提供「编译为原生二进制」与「脚本直接运行」两种执行方式。

> 裁定编号索引：**Q**（语言设计裁定 Q1–Q28）、**Q-T**（测试功能 Q-T1~Q-T6）、**R**（对比 review R-1~R-12）、**Q-S**（系统级 review 缺失定案 Q-S1~Q-S10：@ 内建/panic/原子/FFI/对齐打包/溢出/reborrow/!T 推断集/推断优先/1.0 范围）——全部记录于 `docs/review/2026-08-13-spec-examples-review.md`；各章节括号内的里程碑（M1–M8）见 `docs/SPEC/02-milestones.md`。

## 0. 语言设计规则 (Design rules)

语言的顶层设计要求——一切语言特性与标准库设计必须满足以下 7 条规则；具体展开见对应章节。

1. **以数据为中心**：语言的首要职责是「定义数据、修改数据、传输数据、保存数据」（四大支柱，见 §2）；一切语言特性围绕数据处理组织。
2. **系统级编程、性能为先**：可编译为贴近硬件的原生二进制；零开销路径优先（Release 裸读、无 GC）；性能敏感场景不因语言抽象妥协（见 §1 系统编程 / §5 内存模型）。
3. **测试友好**：所有问题（语法、语义、内存、并发）都可（且应当）通过测试发现——测试是语言一等公民：原生 test 块、断言 API、测试环境（见 §8）。
4. **没有隐藏控制**：无异常、无宏、无隐式内存分配、无隐式控制流；唯一允许的隐式行为是作用域退出时的自动销毁（词汇可见、可预测）；一切控制流、资源与错误路径显式可见（见 §3）。
5. **双模式执行**：同一套基本语法同时支持「编译为原生二进制」与「脚本直接运行」，两种模式语义一致——值语义、所有权语义、控制流一致（错误检测策略是独立维度，见 §1 双模式执行）。
6. **数据为处理的基本单元**：语言设计围绕定义/编辑/传输/存储数据展开；**处理数据的基本单元 = 函数**（唯一处理逻辑，见 §3）；数据形态（class/连续内存/集合/字节）决定其能力与操作方式。
7. **数据类型天生可序列化**：一切数据类型内建序列化能力——**Continuous 类型（连续内存）可 byte 化**（`to_bytes`/`from_bytes`，直映射）、**class 可 json 化**（`to_json`/`from_json`，脚本可定制）、集合 → 字节（长度前缀 u64 LE）（见 §2 数据序列化分层）。
8. **命名规范**：类型与命名空间 `PascalCase`（缩写词全大写，如 `HTTPRequest`）；标识符（变量/函数/方法/字段/参数）`snake_case`；常量 `SCREAMING_SNAKE`（2026-08-17 补定，见 §1 命名规范）。

## 1. 语言与定位

**H语言 (H)**:
项目与语言的正式名称。
_Avoid_: H-Lang, HC 语言

**H源文件 (.hc)**:
以 `.hc` 为后缀的 H 语言源代码文件，是 H 程序的唯一源码载体。

**双模式执行 (Dual-mode execution)**:
同一份 H 源代码既可编译为原生二进制，也可作为脚本被解释执行；两种模式共享同一套语言语义，不存在「编译专用」或「脚本专用」的子语言。**语义一致 = 值语义、所有权语义、控制流一致**；错误检测策略是独立维度（Debug 全检测、Release 裸路径、脚本模式 Debug 语义，2026-08-13 收紧边界）。
_Avoid_: 编译模式与脚本模式割裂成两门语言

**系统编程 (System programming)**:
H 支持的一种使用场景：编写贴近硬件、性能敏感、可独立部署为原生二进制的程序。

**脚本编程 (Scripting)**:
H 支持的一种使用场景：编写可即时运行、无需显式构建步骤的小程序或自动化任务。

**命名规范 (Naming convention, 2026-08-17 补定)**:
**类型与命名空间 `PascalCase`**（首字母大写；缩写词全大写：`HTTPRequest`/`TCPSocket`）；**标识符（变量/函数/方法/字段/参数）`snake_case`**；**常量 `SCREAMING_SNAKE`**（`FLAG_EXEC`）。既有内建类型名（`Vec`/`Map`/`Deque`/`String`/`Table`/`Io`/`ExitType`）维持现状；标准库模块名（`io`/`fs`/`net`/`json`/`csv`/`math`）保持小写（import 路径标识符）。
_Avoid_: 类型/命名空间用 snake_case；缩写词半大写（`HttpRequest`）

## 2. 数据为中心 (Data-centric)

**数据 (Data, 2026-08-14 定案)**:
数据是**内存的一种表现形式**——一切数据最终落在内存中：一块连续的内存是数据（如数组/表格/Continuous 类型）；多块内存按树形关联在一起也是数据（父拥有子，如 class 的内存树）；相互引用的内存块也是数据（指针网络）。「数据」是四大支柱的操作对象：定义、修改、传输、保存都是对内存形态的变换。
_Avoid_: 把「数据」仅理解为标量或记录（内存块、内存树、引用网络同样是数据）

**定义数据 (Define)**:
声明数据类型、数据结构的形状与约束。数据为中心的第一支柱。**数据组织（2026-08-14 定案）**：数组、集合、字典、表格等都是数据的组织形式——连续内存形态（数组、表格）与关联结构形态（集合、字典）皆可；类型定义 = 描述数据的组织方式与访问规则。

**修改数据 (Mutate)**:
对数据进行增、删、改等变换。数据为中心的第二支柱。

**传输数据 (Transmit)**:
数据跨进程、线程或网络边界的传递，包括序列化与 FFI。数据为中心的第三支柱。**传输机制（2026-08-14 定案）**：把数据用指针表示，再把指针在不同处理模块之间**复制/移动**即可完成传输——指针复制（共享视图，用户负责指针问题）与 `move`（转移销毁责任）是两种传输形态；跨进程/跨网络边界则走序列化（见数据序列化分层）。

**保存数据 (Persist)**:
数据的持久化存储，如文件与数据库。数据为中心的第四支柱。**存储方式（2026-08-14 定案）**：数据序列化为字节后可保存在三处——**内存**（缓冲区/共享内存）、**文件系统**（文件）、**数据库**（记录/表）——序列化是存储的前提（见数据序列化分层）。

**跨语言调用 (FFI, 2026-08-13 Q-S4 定案；2026-08-22 设计定案 ADR-0020)**:
C ABI 互操作：`extern fn` 声明外部 C 函数（**纯声明无 body，链接期解析**；反向由 `export fn` K5 覆盖）；`@cImport("header.h")` 编译期解析 C 头生成 H 声明（**顶层 `const c = @cImport(...);` 导入对象，限定名引用 `c.xxx`；MVP 只解析直接声明体，不展开 include/宏；自动生成 `[continuous] class`/H enum**）；**C 指针 = `*T` 但不参与 Debug 悬垂标记**（外部内存、用户负责——悬垂检测不适用；**外部标记 = 上下文推导**：extern fn 签名 / @cImport 声明中的指针自动外置；**进入引用体系需显式 `box` = 复制进托管堆**）；**C 错误码 → error union 手动映射**（无隐式转换，`if (ret != 0) return error.X;`，不加辅助内建）；C struct ↔ H Continuous 类型 POD 直映射 + `@offsetOf`/`@alignOf` 布局验证；**`hc cc` = zig cc 薄封装 + build.zon C 源声明（B5 并入 A1）**；**FFI 原生-only**（interp/IR 响亮拒绝 `error.NotCallable` 风格，测试走 `hc test --mode=compile`，不进 interp 一致性套件）；回调（传 H 回调）/ C 字符串专用类型 = 1.x。
_Avoid_: 隐式错误码翻译 / C 指针裸入引用体系

**数据序列化分层 (Serialization tiers, 2026-08-13 定案)**:
一切数据最终可表示为字节：**Continuous 类型（连续内存值类型）** ↔ byte 数组（内存直映射，内建 `to_bytes`/`from_bytes`——仅 Continuous 可直映射）；class（默认堆上类型）↔ JSON 字符串（内建 `to_json`/`from_json` + 脚本可定制）；Vec/Map/Table/切片等集合 → byte 数组（内建，长度前缀 u64 LE + 元素字节）。「传输」「保存」支柱的底层统一机制；序列化是数据类型的固有属性。
_Avoid_: 把序列化当事后补丁

## 3. 设计原则

**唯一处理逻辑 (Functions as sole processing)**:
函数是数据为中心的**唯一处理逻辑**：一切数据处理通过函数表达；接口描述功能（处理能力的契约）；复杂类型 = 数据字段 + 函数的组合；无其它处理机制（无运算符重载、无宏）。**允许函数重载（2026-08-14 定案）**：签名 = 函数名 + 参数类型列表 + 返回类型；可选参数（尾部、编译期常量默认值）；泛型约束编译时验证、接口限制运行时拆除（单态化零开销）。
_Avoid_: 运算符重载, 宏

**无隐藏控制 (No hidden control flow)**:
语言不提供任何隐式控制流或隐式资源行为——无异常、无宏黑魔法、无隐式内存分配。唯一允许的隐式行为是作用域退出时的自动销毁（词汇可见、可预测）。一切控制流、资源与错误路径显式可见。
_Avoid_: 异常, 宏, 隐式分配

## 4. 类型系统（M2）

**显式初始化 (Explicit initialization)**:
变量必须显式初始化，无默认零值；未初始化即编译错误。
_Avoid_: 零值默认, undefined

**标量类型体系 (Scalar types, 2026-08-14 定案；修订：比较接口化)**:
内建标量类型构成**数字接口族**：**`ICompare`**（比较契约：`eq`/`lt`，派生 `ne`/`le`/`gt`/`ge`）→ **`INumber : ICompare`**（数字基本契约：`add`/`sub`/`mul`/`div`/`neg`）→ 三个子接口——**`IInt`**（有符号整数 `i8`–`i128`/`isize`，追加 `mod`/`abs`）、**`IUint`**（无符号整数 `u8`–`u128`/`usize`，追加 `mod`）、**`IFloat`**（浮点 `f16`–`f128`，追加 `abs`/`pow`）。内建标量自动实现对应接口（编译器内建实现，不可用户自定义/重载）。**相等比较 `==`/`!=` = 值比较，内部调用 ICompare 的比较方法（2026-08-14 H3 修订）**：标量/枚举/元组/String/集合按值比较（经 `ICompare.eq`，编译器内建实现）；**class 身份比较删除**（class 需实现 ICompare 才有 `==`，否则编译错误）；**指针比较 = 指向对象地址**（数组指针/切片含位置 + 长度——比较（地址, 长度）对）；**序比较 `< <= > >=` 绑定 `ICompare`**（未实现则编译错误）。**运算符绑定 INumber 族**：`+ - * /`（及一元 `-`）为内建契约——`a + b` ≡ `a.add(b)`（方法 = 运算符显式形式，双向一致）；`%`/`%%` 由编译器派生。**泛型约束价值**：`where T: INumber` 可约束标量（如 `fn sum(a: &[T]) T where T: INumber`）。`bool`/`char`/`void`/指针不实现（非数字）。
_Avoid_: 用户自定义数字类型冒充内建标量（运算符只绑定 INumber 族）

**类型推断 (Type inference, 2026-08-13 Q-S9 定案)**:
**推断优先、显式兜底**：有信息来源的类型位置默认推断——变量绑定（`var x = init`）、字面量惰性宽度、泛型参数（anytype / where T）、指针形态（`var x = &mut t`）、函数返回类型（单路径/一致类型）；无法推断才显式——函数参数类型（含 `*`/`*mut`/`o` 形态）、class 字段类型、接口实现标注；`o` 不参与推断（按分配来源判定，Q1）。
_Avoid_: 全显式标注的样板噪音（能推断就推断）

**字面量惰性宽度 (Lazy literal width)**:
数字字面量采用 comptime_int/comptime_float：无固定宽度，在使用处定型，超范围编译期报错（Zig 式；2026-08-13 随 comptime 保留恢复完整语义）。
_Avoid_: 默认 i32/f64 假设

**整数溢出 (Integer overflow, 2026-08-13 Q-S6 定案)**:
算术溢出按模式：Debug/脚本模式检测并 `@panic`（带位置）、Release 裸 wrap（两补码回绕，零开销）；显式原语 `@addWithOverflow` / `@subWithOverflow` / `@mulWithOverflow` 返回 `{ value, overflow: bool }`（不受模式影响）。与「错误检测策略独立维度」（Debug 全检/Release 裸）一致。
_Avoid_: 依赖 Release 裸 wrap 做确定性算术（用 `@*WithOverflow`）

**可选值 (Optional)**:
`?T` 类型表达「可能没值」，使用前必须显式解包处理空态。与 error union（`E!T`「可能出错」）正交。**指针不可空**（2026-08-13 Q16 定案）：`*T`/`*mut T` 无空态，空指针用 `?*T`/`?*mut T`；悬垂 ≠ null（悬垂是「曾指向有效对象后被销毁」的运行时错误状态，Debug 抛错带位置）。
_Avoid_: null/undefined 陷阱

**类型定义 (Type definition, 2026-08-14 定案：struct/class 合并；H1 修订：特性标注)**:
统一的类型定义关键字 **`class`**（`struct` 删除）。**存储形态由特性标注决定（H1 修订，取代自动判定）**：标注 **`[continuous]`** → 连续内存值类型（值语义全集：无分配器/栈上/可内嵌、赋值即复制、`to_bytes`/`from_bytes` 直映射、`[pad]`/`[align(T)]` 布局控制、字面量构造 `X{...}`——编译器验证字段全为值类型，否则编译错误）；未标注 → **堆上**（需分配器、接口描述功能、字段可带 `o`、可 move、构造 = `alloc.init(T)` / `new()` 样板 / 普通函数返回）。**特性标注语法（H1 定案）**：类型声明上方中括号 `[name]` / `[name(参数)]`——`[continuous]`（连续内存）、`[pad]`（紧凑打包，原 packed）、`[align(T)]`（对齐到类型 T 的对齐值，如 `[align(u64)]` = 8 字节）。**成员可见性（Q3 定案）**：方法默认公开（`pub` 标注）；属性默认私有（`private` 标注，显式可选项）——`pub mut A: owned T`（pub 可见性 + mut 字段可写 +  `owned T` 字段类型形态分离）；**属性无所有权标注**（即使字段类型为  `owned T`，所有权由类型实例管理，随实例销毁）。**构造**：`alloc.init(T)`——分配器按类型自动获取大小并创建实例（返回拥有实例）；`box()` 装箱值。字段默认只读，`mut` 修饰可写；方法 = 函数成员（Zig 式，无 impl）；无继承（组合优于继承）。
_Avoid_: 继承；未标注连续内存的类型臆测为栈上

**枚举类 (Enum)**:
「定义数据」的类型构造：标签 + 可选负载的合一声明（`enum Value { int: i32, none }`）。纯常量枚举 = 变体均不带负载的特例。匹配用 switch 穷举 + 负载捕获（`|x|` 惯例）；数据栈对象判定与结构体一致。
_Avoid_: 分离的 enum 与 union

**复杂类型 (Complex type, 2026-08-14 修订；2026-08-22 补：无限大小拒绝)**:
堆上分配的类型（默认 class 语义；`tree` 递归/层级组合——父 `o` 拥有子，家族可扩展）；底层为连续或不连续内存；组合形式：字段可含标量、Continuous 类型、其它复杂类型，字段可带 `o`（负责其字段的销毁，随类型销毁）。构造 = `new()` 样板 / 普通函数返回实例；无隐式析构（资源清理走 defer）。含引用值或子项销毁责任在别处的复杂类型实例**可 move**（move 唯一约束 = 拥有所有权，见可移动性）。**无限大小类型拒绝（2026-08-22 定案，ADR-0018 C5-2）**：所有类型必须**有限大小且可计算**——值内嵌自引用/互递归（无间接层）= 编译错误（报类型名 + 循环链位置）；合法间接层（打破循环）= 指针/装箱/堆容器（Vec/Map/Table/String）/`?T`；`tree`（子节点走堆容器）与 `LinkedList`（`?` 自引用）既有递归不受影响。
_Avoid_: 把所有堆类型都叫 struct

**接口 (Interface)**:
描述类型功能的契约集合（方法签名），**显式声明实现**（Rust 式）。可描述复杂类型、标量类型、内建类型、用户定义类型（class/Continuous/元组）。不提供运算符重载。**三用途（2026-08-13 Q-R9 定案）**：① 标记 class 功能（implements 标注）；② 标记参数类型（`where T: Shape` 约束）；③ 类型参数编译可验证。**内建特性 Continuous（2026-08-14 定案）**：编译器识别的布局标记（非行为契约，见连续内存特性条目）。**接口参数**（2026-08-13 Q22/Q22b 定案）：带约束的虚拟类型 `T` + 签名末尾 `where` 子句（`fn add(a: *T) void where T: INumber`）；形态映射 `&T`→`*T`（只读）/ `&mut T`→`*mut T`（可写）/ `move T`→ `owned T`（拥有）。**接口指针**（Q17 定案；M5 修订）：`*Shape` 为**胖指针**（**三字宽 = data + 虚表 + alloc 引用**——装箱时携带分配器，销毁  `owned *I` 时用携带的 alloc 释放 data）；具体类型指针到接口指针**编译期自动收窄**；data 部分 Debug 可选悬垂标记（指针问题用户负责），虚表静态不参与。
_Avoid_: 把接口当抽象基类

**数据栈对象 (Data-stack object)**:
自包含的连续内存值类型（基础类型、**Continuous 类型**——字段全为值类型），传参默认复制语义。数组/集合为引用类型，不属数据栈对象（评审 B3）。
_Avoid_: 把含引用或堆数据的类型当作数据栈对象

**数组与集合 (Arrays and collections)**:
定长数组、变长数组、集合（Vec/Map/Set 等）为**引用类型**：传参走引用，复制需显式（`copy`）——2026-08-13 定案（评审 B3）。**赋值 = 编译错误**（2026-08-14 修订，取代 Q3c 别名共享）：引用类型绑定级赋值不合法——共享数据走显式指针（`var p = &s1;`，指针问题用户负责），复制走显式 `copy`（`var s2 = copy(&s1);`，新建内存、有所有权）；**`copy` 默认深复制**，浅复制需显式标注（如 `copy(&s1, .shallow)`），浅复制引入的内存问题由用户负责。**String = 内建新类型（2026-08-13 Q3 裁定；2026-08-14 M3 修订）**：底层布局 = `Vec<u8>`（编译器内建实现），功能与 `Vec<u8>` 一致，数组规则全部适用（原 Q16「String 值复制」例外取消）；**零成本互转**——`as_slice()` 无前缀内容视图 `&[u8]`，字面量 = `&[u8]` 静态只读切片，经 `as_slice`/`String.from` 显式互转。标量类型保持值语义（赋值即复制）。
_Avoid_: 数组默认复制传参

**切片 (Slice, 2026-08-14 H4 修订)**:
切片 = **带索引位置和长度的指针**（`&[T]` 只读 / `&mut [T]` 可写）——`*T`/`*mut T` 指针 + 起始索引 + 长度，对连续内存（通常为定长数组）的只读或可写视图。**数组的引用默认就是切片（H4 定案）**：`&arr` 直接产生切片（起始 + 长度），无需显式取段；`&arr[start..end]` 指定范围。不拥有数据（无 `o`）；操作与指针一致（多切片自由、指针问题用户负责、Debug 悬垂标记可选）；move 只针对其底层数组。
_Avoid_: 把切片当作数据栈对象

**连续内存 (Continuous, 2026-08-14 H1 修订：特性标注)**:
**显式特性标注 `[continuous]`**（类型声明上方中括号；编译器验证字段全为值类型——标量/枚举/定长数组/元组/连续类型，验证失败编译错误）；未标注 → 堆上。连续类型获得：无分配器（栈/内嵌）、可内嵌、赋值即复制、`to_bytes`/`from_bytes` 直映射、`[pad]`/`[align(T)]` 布局控制、字面量构造 `X{...}`。
_Avoid_: 给含引用/堆字段的类型标 [continuous]（编译错误）

**元组 (Tuple, 2026-08-14 定案；补充：多值返回/比较/序列化)**:
无名称、**初始化后字段只读**的匿名值类型（天然连续内存）：字面量 `(1, "a", 2.5)`、类型标注 `(i32, &[u8], f64)`、元素访问 `t.0`/`t.1`；**支持解构**（`var (a, b, _) = t;`，`_` 占位符放弃值）；字段级只读（无 `mut`）。**多值返回**：`fn divmod(a: i32, b: i32) (i32, i32)`；调用 `var (q, r) = divmod(a, b);`；`E!(T1, T2)` 合法（元组负载）。**比较**：`==` 逐元素（通用相等延伸）；可作 `Map` 键（元组哈希）。**序列化**：天然 `to_bytes`/`from_bytes`；可内嵌进 class 字段。
_Avoid_: 用元组替代需要命名/文档的字段（命名类型用 class）

**表格 (Table, 2026-08-14 定案；2026-08-22 设计会话补全)**:
内建泛型二维结构 `Table<T>`，**代替二维数组**（`[M][N]T` 不再提供，一维数组保留）：构造 **`Table<T>.init(alloc, rows, cols, init)`**（M8：统一方法形态；显式分配器、行数、列数、填充初始值，定长；可变表路径 = `init` + `var mut` 逐格赋值）+ **密封构造 `Table<T>.init_with(alloc, rows, cols, cb)`**（2026-08-22 定案：回调 `|i, j, cell: *mut T|` 内可写单元格——`cell.* = v`，元素可为指针时 `cell.* = &mut obj`；返回**密封表**——编译期强制只读：直接 `t[i,j]=v`、复合赋值 `t[i,j]+=v`、`&mut t` 均编译错误（与绑定声明无关），只读操作全可用，**不可解除密封**）；访问 **`t[i,j]`** 单元格（仅 Table 合法多参索引）+ **`t[i]` 行视图**（2026-08-22：返回切片 `&[T]`/`&mut [T]`，`t[i][j]`≡`t[i,j]`，`.len()`=列数）；**写（2026-08-22 补全）**：单元格赋值 `t[i,j]=v` 与复合赋值 `t[i,j]+=v` 支持，**整行赋值 `t[i]=行` 不支持**；**迭代（2026-08-22）**：`for x in t` 产出**扁平单元格**（行主序，元素=T，IIterable 三态直接套用；行用 `t[i]`/嵌套 for）；**方法（2026-08-22）**：`t.len()`=行数、`t.cols()`=列数，行视图 `.len()`=列数，无 `.get/.set`（索引即语法）；**序列化（2026-08-22）**：to_bytes = u64 LE 行数 + u64 LE 列数 + 行主序元素字节（from_bytes 靠双维度前缀自描述恢复）；**空表合法**（0×N/N×0/0×0，与 `[0]T` 一致）；越界按 Q24 数组模式（Debug 报错带位置/Release 裸/编译期可证编译期报错）；索引无符号（负索引编译错误）；**引用类型**（传参走引用、复制需显式 `copy`）；**底层 = 每行一个连续 `T[]` 数组（行主序，逻辑连续非整表单缓冲）**（2026-08-22 澄清；可 `to_bytes`）；**泛型参数（2026-08-22）**：T 可为任意类型（含指针）——`Table<T>` 拥有元素（非值类型元素需所有权）/ `Table<*T>` 存只读引用 / `Table<*mut T>` 存读写引用。变长需求用 `Vec<Table>`。**指针元素（2026-08-22 修订）**：`t[i,j]` 返回元素指针；`Table<*T>` 经 `t[i,j].*` 只能读 pointee，`Table<*mut T>` 经 `t[i,j].*` 可写 pointee；**单元格指针替换**：密封表（`init_with`）一律不可替换（类型级强制）；普通表（`init`）当前允许（绑定级只读未实现，2026-08-22 确认），待 1.x 实现绑定级只读后按 `var mut` 门控；**只读指针表（不同指针）用 `init_with`**（回调内逐格赋 `&mut obj`）——比 `init` 单 seed 均匀网格干净；空指针表用 `Table<?*mut T>`+`null` 填充；**copy/所有权（2026-08-22）**：内建 `copy(t)` 深复制整表（所有行+元素、保留 alloc，`CopyMode.shallow` 可选）；作用域退出释放所有行；`move` 合法；**嵌套（2026-08-22）**：`Table<Table<T>>` 合法。
_Avoid_: 用 `[M][N]T` 二维数组（已移除）

**迭代契约 (Iteration contract, 2026-08-14 定案；M4 修订；2026-08-22 补定)**:
接口 **`IIterable`** 按**元素访问形态**三态（泛型实例化语法与 `Vec<i32>` 一致）：`IIterable<*T>`（只读迭代，`for (x) |item|`）/ `IIterable<*mut T>`（可写迭代，`for (x) |mut item|`）/ `IIterable<o T>`（拥有迭代，`for (x) |move item|`——**M4 定案：迭代器持有容器所有权——x 被 move 进迭代器、next 逐元素转移所有权、迭代后容器不可再用**）。内建类型（数组/切片/Vec/Map/Table/String）编译器内建实现三态；用户类型实现迭代接口即可参与 `for`（`next(self: *mut Self) ?T` 按对应形态）；`arr.iter()` 迭代器为**显式数据对象**（可传递/组合）；一次性迭代器 1.0 即可，惰性/组合子迭代 1.x。**迭代器对象 API（2026-08-22 定案，ADR-0017 C3-1）**：`iter()` 返回迭代器方法签名 = `next(self: *mut Self) ?T` + `filter(fn)` / `map(fn)` 组合子（返回**新的显式迭代器对象**，链式可组合）；**惰性求值（`next()` 按需求值、链式延迟计算）真实现仍留 1.x（A7 不动）**——迭代器/组合子为显式数据对象，非隐式求值机制，与「无隐藏控制」对齐。
_Avoid_: 把迭代器当隐藏机制（迭代器是显式对象）

**@ 内建函数 (@ builtins, 2026-08-13 Q-S1 定案)**:
Zig 式 `@` 前缀编译期内建：`@sizeOf` / `@alignOf` / `@offsetOf` / `@typeOf`（类型查询，序列化/FFI 布局依赖）、`@intCast`（整数转换，超范围 Debug 检测）、`@ptrCast`（指针转换——**唯一显式放弃类型安全的逃生舱**，替代 Rust unsafe / C 强转）、`@alignCast`（对齐断言）、`@compileError`（显式编译失败）。`@` 前缀不与用户标识符冲突；转换显式可见（「没有隐藏控制」）。
_Avoid_: 用指针强转替代类型安全（@ptrCast 是唯一的逃生舱）

## 5. 内存模型（M4）

**Allocator**:
显式的内存分配器实例。分配内存的代码显式传入 Allocator；未传入时回退到全局分配器。销毁由 `defer` 显式控制。
_Avoid_: 隐式作用域级分配器

**Arena**:
批量分配器：一次分配大量内存、统一回收（`Arena.init(alloc)` / `arena.alloc(...)` / 显式 `deinit`）。**Arena 分配的对象无所有权**（归 Arena，禁止 move）；适用请求级生命周期（**每请求一个 arena**：长运行脚本（服务/事件循环）每轮新建 Arena、处理完整体回收）。**标准库提供 `mem.with_arena(fn)` 包装**：进入时建 Arena、退出时统一回收，减少样板。
_Avoid_: 对 Arena 内的对象逐个 deinit

**所有权 (Ownership, 2026-08-25 修订：显式 defer/move 模型替代自动销毁)**:
所有权 = **销毁责任的唯一归属**——`owned` 标注的变量必须由 `defer` 显式释放或 `move` 转移所有权；**不是访问权限控制**。权限三要素正交：可读（默认）、可写（`mut`）、拥有（`owned`）。

**变量权限**：
| 标注 | 可读 | 可写 | 拥有 | 说明 |
|------|------|------|------|------|
| `T` | ✅ | ❌ | ❌ | 值类型或不可变引用 |
| `mut T` | ✅ | ✅ | ❌ | 允许修改，无所有权 |
| `owned T` | ✅ | ❌ | ✅ | 拥有所有权，必须 `defer` 或 `move` |
| `owned mut T` | ✅ | ✅ | ✅ | 拥有所有权且可修改 |

**规则**：
1. **作用域不自动释放**。凡堆创建的对象、文件句柄、数据连接等外部资源，统一用 `defer` 在退出作用域时显式释放。
2. **无 `defer` 则必须 `move`**。`owned` 变量必须匹配 `defer` 语句或将所有权 `move` 出当前作用域，否则编译错误。
3. **栈/Arena 分配无所有权**。标量/Continuous 类型（栈上）以及 Arena 分配的对象，均无 `owned`，不需要 `defer`，由栈帧或 Arena 统一管理。
4. **`owned` 只能 move 一次**（affine type）。move 后原变量不再拥有所有权。
5. **`move` 转移销毁责任**。`move` 将资源的销毁责任从当前作用域转移到目标作用域——不复制数据、不移动内存。因提前释放导致的悬垂指针问题由用户负责（后期加强静态检查）。

**指针自由（Zig 式）**：变量可有多个读写指针（`*mut`）与多个只读指针（`*T`）——**指针问题（悬垂/别名）由用户负责**；语言不保证唯一写者；Debug 悬垂标记为可选诊断工具（编译时选项）。
_Avoid_: 引用计数（所有权显式管理，无需引用计数）

**作用域 (Scope)**:
所有权与生命周期的基本单位。每个块是一个作用域（可视为没有名字的函数），函数是有名字的作用域。**作用域退出不自动销毁变量**——销毁由 `defer` 语句显式控制。**`defer` 执行顺序 = 声明逆序（LIFO）**。
_Avoid_: 用「函数」泛指所有作用域；隐式作用域销毁

**move (2026-08-25 修订)**:
将变量的**销毁责任**（所有权）从一个作用域显式转移到另一个作用域（函数参数或返回值）的操作——**不复制数据、不移动内存**（**变量本身不变**：内存地址不变、原绑定仍可访问；已有指针仍指向它，继续访问造成的悬垂/冲突由用户负责）。合法条件：变量拥有 `owned` 标注（非 Arena/global 分配）即可。**`move` 关键字仅出现在调用点**（`take(move s);` / `return move s;`）；参数侧拥有用 `owned T` 标注。转移后原作用域不负责释放，目标作用域负责。
_Avoid_: 把 move 当作复制

**可移动性 (Movability, 2026-08-25 修订)**：
move 的唯一约束 = **变量标注为 `owned`**（非 Arena/global 分配；Arena 归 Arena、global 归根作用域，均禁止 move）。指针别名自由（用户负责），不阻塞 move。
_Avoid_: 把「有指针」当作禁 move 的理由

**全局变量 (Global variable, 2026-08-25 修订)**：
用 `global` 声明的变量，位于根作用域上下文，静态生命周期（程序运行期间存活）。**不可 move**（移出即失去全局性）。**初始化时机**：程序启动时执行（根作用域构造阶段，`main` 前）——支持运行时初始化；**跨文件按初始化表达式依赖图拓扑排序**（无依赖按文件声明序；循环依赖 = 编译错误）。
_Avoid_: 把全局变量当作普通作用域变量

**只读引用 (Read-only reference)**：
对目标变量的只读访问视图，不授予修改权。**多个只读指针可同时存在**；指针问题（悬垂）由用户负责；Debug 悬垂标记为可选诊断工具（编译时选项）。
_Avoid_: 把引用当作安全保证（指针问题用户负责）

**读写引用 (Read-write reference)**：
对目标变量的可写访问视图（可写包含可读）。**可有多个读写指针同时存在**（唯一写者概念取消）；`*mut` **可复制**。**指针问题（悬垂/别名冲突）由用户负责**（Zig 式）；Debug 悬垂标记为**可选诊断工具**（编译时选项开启，非安全保证）。
_Avoid_: 把指针当作安全保证（指针问题用户负责）

**悬垂检测 (Dangling detection)**：
**指针问题由用户负责（Zig 式）**——悬垂属用户责任，语言不保证检测。**Debug 悬垂标记为可选诊断工具**（编译时选项开启：目标销毁时标记指向它的指针，访问已标记指针 → 提示带位置——帮助定位，非安全保证）。**切换粒度 = 编译单元（文件）级**：CLI `hc run`/`hc test`/`hc build` 增 `--dangle=on|off|auto`（`auto` = Debug 开 / Release 关，默认）。悬垂的唯一产生路径是引用逃逸到比目标更长寿的容器/全局（返回值引用被编译期禁止）。
_Avoid_: 把悬垂检测当安全保证（它是诊断工具）

**defer (2026-08-25 修订：主销毁机制)**：
**主销毁机制**——所有 `owned` 变量必须通过 `defer` 在退出作用域时显式释放。`defer` 在作用域退出时执行（正常路径和错误路径均执行）。`errdefer` 仅在错误返回路径执行。**多个 `defer` 按 LIFO 执行**（后声明先执行，与资源创建顺序成镜像）。保证清理动作在创建处可见（不用隐式析构）。
_Avoid_: 隐式 RAII 析构（H 不用）；把 defer 当少数资源的特例

## 6. 元编程（M3）

**脚本生成 (Script generation, 2026-08-14 H5 修订)**:
元编程机制之一：源码中用 `script` 作用域标注脚本代码，**编译前**由脚本解释器执行并生成代码（H5 定案：编译前执行，无运行时环境——io/alloc/argv 不可用，一般只执行模板生成）。脚本用 H 编写；解释器同时支持实时预览与校验。**类型信息可见范围（H5 定案）**：script 块在 class 内 → 该 class 的类型数据可用；在命名空间下 → 整个命名空间类型信息可用（`types` 元数据对象可见范围随块位置收窄）；**生成物（class/function/属性列表等）不得与当前环境冲突**（同名 = 编译错误）。**`comptime { ... }` 块（H5 定案）**：**编译时**执行（语义分析阶段）——能获取的数据多得多（完整类型系统），可执行更多操作；comptime 函数 = 参数含 `type`/`anytype` 即编译期执行的普通函数；`types` 元数据在 comptime 上下文可见全部类型。**错误机制统一（2026-08-14 定案）**：作用域/函数/script 块/comptime 块均为可返回错误的执行单元（error union）——script/comptime 块执行失败 = **编译错误**（解释器/编译器抛出，带块内位置 + 所属块位置）。
_Avoid_: 把脚本生成当作唯一元编程机制

**comptime 式泛型 (Comptime generics, 2026-08-22 补：嵌套具体化边界)**:
泛型 = 编译期函数：`fn List(T: type) type`（**类型即值**）、`anytype` 参数（调用点推断）、惰性实例化（Zig 式）。**分工（X1 定案）**：comptime 管类型级计算（泛型/类型即值/comptime_int）；脚本生成管样板与数据定义驱动代码生成（见脚本生成）。**内建泛型嵌套（2026-08-22 定案，ADR-0018 C5-1）**：`Vec<List<i32>>`（List = 类型函数）具体化后类型应 = `Vec<List<@i32>>`（内建泛型名 + 内层具体化键）——当前仍退化裸名 `Vec`，预期行为已定案、修复归 C5 条目待实施。
_Avoid_: 把泛型当脚本生成做

## 7. 并发模型（M5）

> **2026-08-24 模型重构（ADR-0024）**：从「OS 线程 + 四模式容器」迁移到「M:N 协程 + 单一通道 `chan<T>`」。

**协程调度**：M:N 模型——N 个协程多路复用到 M 个 OS 线程上。`spawn(f, args...)` 创建轻量协程（G），由运行时调度器分配到处理器（P）上执行。`GOMAXPROCS` 控制并行 OS 线程数（默认 = CPU 核数）。初始为**协作式调度**（通道操作或显式 `yield` 时让出），后续可加异步抢占。调度器在 Rust 运行时（`hc-rt`）中实现，解释器与 LLVM 后端共用。
_Avoid_: 裸 OS 线程创建（用协程替代）

**通道 `chan<T>`**：单一通道类型替代四种容器（`Pipe`/`Tee`/`Funnel`/`Hub`）。API：`send(v)` / `recv() T` / `try_send(v) bool` / `try_recv() ?T` / `close()`。有缓冲/无缓冲两种模式，容量在 `init(alloc, cap)` 时指定。通道操作是协程调度点——send 在缓冲满时挂起当前协程，recv 在空时挂起，就绪时唤醒等待协程。
_Avoid_: 四种容器类型的认知负担（用单一 `chan<T>`）

**Mutex**：`Mutex.init(v)` / `.lock() !T` / `.try_lock() ?T`。与协程配合使用，用于保护共享状态。

**线程所有权 (Thread ownership)**:
任何作用域可开启协程，所有权默认归当前作用域。作用域退出时：协程已完成 → 销毁后退出；仍在运行 → 所有权移交根作用域管理。
_Avoid_: 隐式 join 阻塞

**协程捕获 (Goroutine capture)**:
协程捕获参数的规则：值类型 → 复制值；引用类型 → 必须 move 所有权，或为根作用域上下文中的 global 变量。**冻结窗口**：绑定场景下，被捕获引用目标在 spawn 到 join 之间主协程不可写。**Send/Sync 编译期诊断**：`spawn` 边界捕获非 `Send` 引用 → 编译错误带位置。
_Avoid_: 闭包式只读捕获用于逃逸协程

**Future**:
异步任务的结果句柄。`async fn f(...) R` 返回 `Future<R>`；`await` 等待其完成。基于协程任务实现。

**原子操作 (@ atomics, 2026-08-13 Q-S3 定案)**:
无锁原语：`@atomicLoad(T, p, order)` / `@atomicStore(T, p, v, order)` / `@atomicRmw(T, p, op, v, order)`（op = `.add`/`.sub`/`.exchange`/`.cmpxchg` 等，限整数/指针/枚举等可原子类型）；**内存序 C11 五序子集**——`relaxed` / `acquire` / `release` / `acq_rel` / `seq_cst`（**默认 `seq_cst`**，弱序需显式）。
_Avoid_: 用普通读写模拟原子（数据竞争）

## 8. 测试（M8）

**测试函数 (Test function)**:
`test fn 名称() !void { ... }`——**标记为测试的函数**（2026-08-13 Q8 + Q-R11 定案）：`hc test` 收集全部测试函数运行；**可被普通代码调用/复用**（原「测试块非函数」限制解除）；**测试失败 = error**（`try` 传播断言即失败，报告带位置）；独立函数作用域（退出自动销毁）、默认串行（Q-T3）；`return error.SkipTest;` 标记跳过；输出逐项 `[PASS]/[FAIL]/[SKIP]` + 汇总统计、失败非零退出码（Q-T2）。
_Avoid_: 把测试函数与业务函数混写

**断言 API (Assertion API)**:
测试块内**隐式可用**的断言集（归 std.debug，2026-08-13 Q-T1 定案）：`expect(cond)`（布尔）/ `expect_eq(a, b)`（相等，失败输出期望 vs 实际）/ `expect_neq(a, b)`（不等）/ `expect_error(error.e, expr)`（期望返回指定错误）/ `expect_eq_slices(a, b)`（逐项相等，失败输出长度 + 首个差异位置）；均返回 `anyerror!void`（`try` 传播即失败）。
_Avoid_: 为测试造第二套断言语法

**测试环境 (Test environment)**:
test 块内**隐式可用**的测试环境（2026-08-13 Q-T4 定案；2026-08-17 修订）：`alloc`（默认分配器）与预导入环境（io 模块，ADR-0010）；**`test_io` 已取消**——测试直接调 `main()`，需要 io 的测试经 `import H.std.{io}` 使用环境；IO 测试默认真实执行。
_Avoid_: 测试里隐式依赖全局 IO

## 9. 错误处理（M6）

**错误联合 (Error union)**:
`E!T` / `!T` 类型表达「可能出错」：E 为错误集（`error{ NotFound, ... }`），成功时为 T、失败时为错误值。与 optional（`?T`「可能没值」）正交；错误值引用 `error.NotFound`（错误名全局唯一，2026-08-13 Q13 定案）。函数返回**显式错误集**；`!T` 为**推断错误集**（2026-08-13 Q-S8 定案：编译器从函数体收集 `return error.X` + `try` 传播的实际返回集——与显式集语义一致、调用方可穷举；无法收集时退化 `anyerror`，提示显式标注）。**错误值运行时表示（2026-08-14 定案）**：错误 = **全局唯一整数错误码**（编译器维护「错误名 ↔ 码」表，跨包统一）；error union 运行时表示 = 错误码 + 成功标记（Zig 式——成功路径零额外负载）；Debug 附带错误源位置（返回点）；`anyerror` = 任意码（64 位空间）。
_Avoid_: 异常

**try 传播 (try propagation)**:
前缀运算符：`try expr`——expr 为 error union，成功时解包为 T，失败时从当前函数返回该错误（错误路径显式可见，与「没有隐藏控制」一致）。
_Avoid_: 隐式异常传播

**catch 处理 (catch handling)**:
`expr catch 默认值` / `expr catch |err| { ... }`——处理 error union 的错误分支；**忽略错误仅 `catch |_| {}`**（2026-08-13 Q11 定案：不提供 `catch {}` 简写）；`if (e!) |v| else |err|` 双向捕获（Q9 定案：必须成对——错误必须显式处理）。
_Avoid_: 吞错

**anyerror**:
任意错误类型：仅用于**接口方法契约**（实现方决定具体错误集，2026-08-13 Q34 定案）；普通函数仍用显式错误集。
_Avoid_: 普通函数用 anyerror 偷懒

**@panic**:
内置终止原语（2026-08-13 Q-S2 定案）：不可恢复运行时错误（Debug 悬垂标记开启时访问已标记悬垂指针）调用 `@panic("消息", 位置)`——打印消息 + 位置（Debug 带堆栈）后 **abort 终止**；**不执行 defer 清理**、**无 unwind/recover**（回卷是隐式控制流，不引入）；测试环境内 panic → 该测试记 FAIL（不终止整个 hc test）。
_Avoid_: 用 panic 替代 error union（可恢复错误走 `E!T`）

## 10. 模块与包（M1/M8，2026-08-25 重构：ADR-0026）

### 目录结构

```
project/
├── src/
│   ├── main.hc              # 入口，命名空间 = 项目名
│   ├── Modules/
│   │   ├── Auth/             # 模块，命名空间 = project.Auth
│   │   │   ├── context.hc    # 模块 context（IContext 实现）
│   │   │   ├── interfaces.hc # 公开接口定义
│   │   │   └── services.hc   # 内部实现
│   │   └── Storage/          # 模块，命名空间 = project.Storage
│   │       ├── context.hc
│   │       ├── interfaces.hc
│   │       └── storage.hc
│   └── utils/                # 普通代码（非模块）
│       └── helpers.hc
tests/                        # 项目根目录，测试文件
├── test_auth.hc
└── test_storage.hc
```

### 命名空间 (Namespace)

**文件路径即命名空间**——不再使用 C# 式块式 `namespace { }` 声明。规则：
- 根入口文件 `src/main.hc` 的命名空间 = **项目名称**（build.zon 的 `name` 字段）
- 标准库的根命名空间 = `H.std`
- 子目录文件的命名空间 = `{上级命名空间}.{当前文件夹名称}`
- 示例：`src/main.hc` → `{项目名}`；`src/Modules/Auth/services.hc` → `{项目名}.Auth`
- 文件内不写 `namespace` 关键字时，自动归属上述路径命名空间
- 文件内写 `namespace abc { }` 时，**覆盖**默认路径命名空间，指定为 `abc`
- 编译时按命名空间将同空间文件编译在一起
- `namespace` 关键字仍可用于**显式覆盖**默认路径命名空间
_Avoid_: 一文件多命名空间

### 引入 (using, 已移除)

`using Math;` 引入命名空间（2026-08-13 Q21 定案）——**2026-08-17 被 `import` 取代**（ADR-0010），**2026-08-23 决定移除，2026-08-28 实施**（解析到 `using` 直接报错，提示改用 `import`；实现层 `Decl::Using` 变体、语义/运行时 `apply_usings` 已删除）。同包跨命名空间改用 `import`：限定访问 `Math.square(5)` 或符号选择 `import Math.{square};` 后平铺调用 `square(5)`。
_Avoid_: 通配符式隐式引入一切

### 导入 (import)

文件级导入语句（2026-08-17 定案，ADR-0010——取代 using，推翻「无文件级 import」）：
- `import pkg.mod.{sym as 别名}` — 符号选择 + `as` 重名重命名
- `import pkg.mod;` — 整模块导入
- `import pkg.mod as m;` — 整模块 + 别名

`H.std` = 内置标准库根路径，用户库经 build.zon 声明后按依赖名引用。**依赖解析顺序**：(1) 系统 SDK 目录（`$H_HOME/sdk/<name>/`，未设置则回退 `~/.hc/sdk/<name>/`），(2) 当前项目目录。**重名冲突规则**：同名导入符号冲突 → 编译错误，用户必须用 `as` 显式消歧。**库符号访问规则**：库函数可直接调用；库类型需创建（`alloc.init(T)` 堆上 / 值字面量栈上）。
_Avoid_: 多套导入机制并存

### 模块系统（ADR-0026，2026-08-25 定案）

`[module]` 特性标记已移除，模块由 `src/Modules/` 目录结构定义。

**模块定义规则**：
- `src/Modules/` 下的每个子目录 = 一个模块。子目录名即模块名，编译器自动发现，无需手动声明。
- 模块目录仅支持扁平结构，不支持嵌套子模块。嵌套应通过独立包实现。
- 每个模块必须定义 context（`src/Modules/X/context.hc`），实现 `IContext` 接口。
- 纯工具函数应放在 `src/` 下的非 `Modules/` 目录中，不放在模块目录下。
- 模块内非 `pub` 符号对外不可见。模块的公开 API = context 结构体 + 接口定义。

**模块与标准库**：
- 标准库（`H.std`）可直接 `import` 使用。
- 模块与标准库以外的对象交流，必须通过 context。

### IContext 接口与 IoC 容器

`IContext` 接口定义在 `H.std.ioc` 中，提供 IoC 容器能力：

```h
interface IContext {
    fn register<T>(self, impl: T);                             // 注册单例（深拷贝到 Arena）
    fn register<T>(self, name: &[u8], impl: T);                // 命名单例
    fn registerFactory<T>(self, name: &[u8], factory: fn(ctx: &IContext) -> T);
    fn get<T>(self) -> *T;                                     // 获取 Arena 引用（无所有权，不 defer）
    fn get<T>(self, name: &[u8]) -> *T;                        // 按名获取 Arena 引用
    fn make<T>(self, name: &[u8]) -> owned T;                  // 创建新实例（调用者拥有，必须 defer）
}

**内存管理规则**：
- `get<T>()` 返回 `*T`（Arena 引用，无所有权，不需要 `defer`）
- `make<T>(name)` 返回 `owned T`（调用者拥有，必须 `defer` 或 `move`）
- `register<T>(impl)` 在 Arena 中深拷贝一份，原实例由调用者自己管理
- `registerFactory<T>(name, fn)` 工厂首次调用结果缓存到 Arena，后续 `get` 返回缓存引用；`make<T>(name)` 每次调用工厂创建新实例

**Context 层级委托**：
- `AppContext`（应用域）→ `ModuleContext`（模块子域），子 context 持有父 context 引用
- 子 context 解析不到时向上委托给父 context
- 每个 context 背靠 Arena 分配器，context 销毁时所有通过它创建的对象一并销毁

**模块面向接口编程**：
- 模块只知接口，不知具体实现。注册什么就用什么。
- 接口定义在提供该接口的模块中（如 `src/Modules/Auth/interfaces.hc`），使用方通过 `import project.Auth.{IUserService}` 引入。
- 模块内类型直接创建外部类型 → 编译错误。
- 模块间连接：`import` = 符号引用（类型/函数，API 面）；`context` = 数据/依赖注入——两者正交。

**引导流程示例**：

```h
// src/main.hc
import H.std.{io};
import H.std.ioc.{IContext, AppContext};
import myapp.Auth.{AuthContext, IUserService};
import myapp.Storage.{StorageContext, IFileService};

fn main() !void {
    var app_ctx = AppContext.init(alloc);
    defer app_ctx.deinit();

    // 注册全局服务
    app_ctx.register(IUserService, UserService{});
    app_ctx.register(IFileService, FileService{});

    // 初始化模块（注册到父 context 的子域）
    var auth = AuthContext.init(app_ctx);
    var storage = StorageContext.init(app_ctx);

    run(app_ctx);
}
```

**初始化与生命周期**：
- 初始化即注册：`AuthContext.init(app_ctx)` 将模块注册到父 context
- 懒加载实例化：`get<T>()` 按需创建对象，对象随 context 销毁
- 同一接口可注册多个实现，通过 `name` 区分
- 工厂方法接收 context 引用，可在工厂内部解析依赖

### 程序环境 (io 模块)

标准库模块形态的程序环境句柄（2026-08-17 定案，ADR-0010）：`io.print`/`io.fs.*`/`io.net.*`/`io.time.*`/`io.text.*`/`io.rng.*`/`io.storage.*`/`io.archive.*`/`io.ipc.*`/`io.env(n)`/`io.stdin`/`stdout`/`stderr`/`io.exit(ExitType, code)` 均为模块函数 + 模块内环境状态；经 `import H.std.{io}` 引入。`H.std` 是标准库的统一导入路径。`Io` 接口保留（并发 E2 的 `Io.threaded()/evented()`）。
_Avoid_: 把程序环境当全局可变状态泄漏

### 应用程序 (Application)

含 `main` 入口函数的包（build.zon `Kind::exe`）——编译产出可运行的 exe（平台原生形态）。与库相对：库不含 main（见库）。
_Avoid_: 把库当应用运行

### 库 (Library)

不含 `main` 入口的包（build.zon `Kind::lib`）——代码集合（1+ 模块），**不单独运行**；产出形态构建参数选择：**lib 静态库**（编译时链接进 exe）或 **dll 动态库**（exe 运行时加载）。
_Avoid_: 库内写 main 入口

### 包与依赖 (Package & deps)

包管理器内置编译器；**包形态 = 应用（`Kind::exe`，含 main）/ 库（`Kind::lib`，无 main，1+ 模块，产出 lib/dll）**；依赖清单 = **H 数据字面量**（`const build = Build{ ... }`，build.zon 式）。**依赖解析**：`import <name>.<sym>` 中 `<name>` 对应 build.zon 依赖声明的 `name` 字段。解析顺序：(1) 系统 SDK 目录（`$H_HOME/sdk/<name>/`，未设置则回退 `~/.hc/sdk/<name>/`），(2) 当前项目目录。官方注册中心；`hc build` / `hc cc`（M8 工具链，系统库自带、静态链接默认）。
_Avoid_: 隐藏系统依赖

### `.hs` 脚本导入

`.hs` 文件使用 `import "path/to/file.hs"` 引用其他 `.hs` 文件。Parser 扩展：`import` 后跟字符串字面量 → 文件引用（AST 新增 `Decl::ImportFile` 变体）；跟标识符路径 → 模块引用（既有 `Decl::Import`）。文件引用与标准库引用是同一 `import` 语句的两种形态，parser 按引号检测区分。脚本项目不需要 `build.zon`。
_Avoid_: 混用 `.hs` 文件引用与 `.hc` 模块引用

### 测试目录

项目根目录的 `tests/` 目录用于存放测试文件：
- `tests/` 不参与命名空间系统，仅由 `hc test` 发现和执行
- 测试文件通过 `import` 引入被测模块的接口
- 测试中可注入 mock 实现：

```h
// tests/test_auth.hc
import myapp.Auth.{AuthContext, IUserService};

[Test] fn test_auth_service() !void {
    var ctx = AuthContext.init(alloc);
    defer ctx.deinit();
    ctx.register(IUserService, MockUserService{});
    // ...
}
```

## 12. 容器与初始化器（2026-08-26 定案，ADR-0027）

**容器 (Container)**:
持有元素所有权的动态集合类型（Vec / Deque / Map / Table）。容器默认 owning——元素所有权归容器，容器销毁时一并释放元素内存。容器不持有元素时（借用语义）通过切片 `&[T]` / `&mut [T]` 表达，不是容器类型。
_避免_: 把切片当作容器

**容器初始化器统一规则 (Container init convention, ADR-0027)**:
所有容器使用 `init` 方法创建，遵循以下规则：
1. 分配器永远是**最后一个参数**（可省略，回退全局 alloc）
2. `var mut` 可变绑定 → 可空构造，后续追加元素
3. `var` 只读绑定 → 建议提供初值（空容器产生编译警告）
4. 所有数据通过 `move` 进入容器（值类型自动拷贝）

| 容器 | init 签名 | 说明 |
|------|-----------|------|
| `Vec<T>` | `.init()` | 空 Vec，默认 allocator |
| | `.init(alloc)` | 空 Vec，显式 allocator |
| | `.init(cap)` | 预分配容量，默认 allocator |
| | `.init(cap, alloc)` | 预分配容量，显式 allocator |
| | `.init([items])` | 从数组字面量，默认 allocator |
| | `.init([items], alloc)` | 从数组字面量，显式 allocator |
| `Deque<T>` | 同 Vec | 方法集额外有 push_front/pop_front |
| `Map<K,V>` | `.init()` | 空 Map，默认 allocator |
| | `.init(alloc)` | 空 Map，显式 allocator |
| | `.init({k = v, ...})` | 花括号 KV 字面量，默认 allocator |
| | `.init({k = v, ...}, alloc)` | 花括号 KV 字面量，显式 allocator |
| `Table<T>` | `.init(rows, cols, val)` | 统一初始值，默认 allocator |
| | `.init(rows, cols, val, alloc)` | 统一初始值，显式 allocator |
| | `.init([[items]])` | 从二维数组，默认 allocator |
| | `.init([[items]], alloc)` | 从二维数组，显式 allocator |
| | `.init_with(rows, cols, fn)` | 回调构造，默认 allocator |
| | `.init_with(rows, cols, fn, alloc)` | 回调构造，显式 allocator |

_避免_: 分配器放在第一个参数；容器类型参数中加 `owned`/`mut`

**元素读写权限 (Element mutability)**:
容器元素的读写权限与容器变量本身的 `var`/`var mut` 绑定：
- `var v = Vec<i32>.init(alloc, [1, 2, 3])` — 容器只读，元素只读（`v[0] = 5` 编译错误）
- `var mut v = Vec<i32>.init(alloc)` — 容器可变，元素可读写（`v.append(5)` 允许）
此为简化设计：不再需要类型参数级别的 `owned`/`mut` 标注。
_避免_: 在容器类型参数中加 `owned mut T`（容器统一默认 owning）

**alloc.init 三形态 (Three forms of alloc.init, ADR-0027)**:
堆分配原语 `alloc.init` 有三种形态，覆盖所有堆分配场景：

```h
// 形态 1：类型实例，零初始化
var mut p: *T = alloc.init(T);

// 形态 2：类型实例，带字段初始化
var mut p: *T = alloc.init(T{ field = "value" });

// 形态 3：数组，n 个元素
var mut a: *[T, n] = alloc.init(T, n);
```

三者与容器 `init` 方法正交：`alloc.init` 是底层分配原语，容器 `init` 是高级构造器（内部使用 allocator 管理存储）。
_避免_: 用 `alloc.init` 创建容器（容器有自己的 `init` 方法）

**容器字面量 (Container literals)**:
- `Vec<T>[1, 2, 3]` — 方括号字面量（IR 降级器待实现）
- `Map<K,V>{"k" = v}` — 花括号 KV 字面量，`=` 分隔键值
- 数组字面量 `[1, 2, 3]` 保持现有语法，始终是 owning 引用类型

**A6 数据结构 (A6 data structures)**:
RingBuf / PageMem / IntrList / TreeMap / Bitmap 保持 `io.*` 命名空间访问，不纳入统一容器初始化设计。

## 11. 工具链（M8）

**LSP服务器 (Language Server Protocol Server)**:
提供语言智能服务的服务器，遵循LSP协议。H语言的LSP服务器提供诊断、定义跳转、悬停提示、自动补全等功能。独立二进制 `hc-lsp` 或子命令 `hc lsp` 启动。支持Zed、VSCode、Neovim等编辑器。
_Avoid_: 将LSP服务器与编译器混淆

**诊断 (Diagnostic)**:
编译器或LSP服务器提供的错误和警告信息。包括语法错误、类型错误、语义错误等。LSP诊断包含位置（行列）、级别（错误/警告）、消息。实时诊断当前文件，保存时诊断整个项目。
_Avoid_: 将诊断与运行时错误混淆

**定义跳转 (Go to Definition)**:
LSP功能，跳转到符号的定义位置。支持函数、类、枚举、接口、变量等符号。支持跨文件跳转（依赖包中的符号）。
_Avoid_: 将定义跳转与引用查找混淆

**悬停提示 (Hover)**:
LSP功能，鼠标悬停时显示符号的类型信息和文档注释。类型信息由编译器类型推断提供，文档注释为 `///` 格式的Markdown文本。
_Avoid_: 悬停提示显示过多信息

**自动补全 (Auto Completion)**:
LSP功能，提供代码补全建议。包括关键字、标识符（变量、函数、类型）、字段和方法（基于类型推断）、导入建议。根据上下文过滤补全项。
_Avoid_: 补全项过多，影响选择

**Tree-sitter语法 (Tree-sitter Grammar)**:
用于编辑器语法高亮、缩进、大纲等的语法定义。H语言的Tree-sitter语法从现有lexer/parser转换，逐步迁移。核心语法包括表达式、函数、类、枚举、接口、命名空间、控制流。
_Avoid_: 将Tree-sitter语法与编译器语法混淆

**Zed扩展 (Zed Extension)**:
为Zed编辑器提供的语言扩展。包含Tree-sitter语法、LSP服务器配置、语言元数据（config.toml）。集成到H2仓库的 `extensions/zed/` 目录。
_Avoid_: 将Zed扩展与LSP服务器混淆

**文档注释 (Doc Comment)**:
`///` 格式的注释，用于文档生成和LSP悬停提示。支持Markdown格式。类似Rust的文档注释格式。
_Avoid_: 使用其他格式的文档注释
