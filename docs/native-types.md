# H 语言类型系统文档

<!-- 生成日期: 2026-08-26 -->
<!-- 基于: grilling session + 代码库探索 -->

---

## 1. 类型系统概览

### 1.1 AST 类型节点 (`Type` 枚举)

| 类型 | 语法 | 示例 | 说明 |
|------|------|------|------|
| `Named` | `T` / `T<A,B>` | `i32`, `Vec<i32>`, `Map<String,i32>` | 命名类型，可带泛型参数 |
| `Ptr` | `*T` / `*mut T` | `*i32`, `*mut u8` | 只读/可写指针 |
| `Slice` | `&[T]` / `&mut [T]` | `&[u8]`, `&mut [i32]` | 只读/可写切片视图 |
| `Optional` | `?T` | `?i32`, `?String` | 可选值 |
| `ErrorUnion` | `E!T` | `!void`, `NotFound!i32` | 错误联合类型 |
| `Tuple` | `(T1, T2)` | `(i32, f64)` | 元组 |
| `Array` | `[T, N]` | `[i32, 10]` | 定长数组（编译期常量长度） |
| `ComptimeInt` | 编译期整数字面量 | `3` (在类型位置) | 编译期任意精度 |
| `Infer` | `_` | `var x: _ = 42` | 类型推断占位 |
| `Owned` | `o T` | `o String` | 所有权标注 |

### 1.2 运行时值类型 (`Value` 枚举, 28 变体)

参见 §2-§5。

---

## 2. 标量类型

H 语言使用 **统一整数类型**：所有整数在运行时由 `i128` 表示，宽度检查在语义分析阶段完成。

### 2.1 整数

| 类型 | 位宽 | 有符号 | 说明 |
|------|------|--------|------|
| `i8` | 8 | 是 | |
| `u8` | 8 | 否 | 常用于字节 |
| `i16` | 16 | 是 | |
| `u16` | 16 | 否 | |
| `i32` | 32 | 是 | 默认整数 |
| `u32` | 32 | 否 | |
| `i64` | 64 | 是 | |
| `u64` | 64 | 否 | |
| `i128` | 128 | 是 | 运行时表示 |
| `u128` | 128 | 否 | |
| `isize` | 指针宽度 | 是 | |
| `usize` | 指针宽度 | 否 | 长度/索引 |

**运行时**: `Value::Int(i128)`  
**LLVM 原生**: 全内联指令（add/sub/mul/div/mod/bitwise/shift/cmp），无 helper 调用开销  
**字面量**: `42`, `0xFF`, `0b1010`, `0o777`, `1_000_000`  
**后缀**: `42i32`, `255u8`

### 2.2 浮点数

| 类型 | 位宽 | 说明 |
|------|------|------|
| `f16` | 16 | 半精度 |
| `f32` | 32 | 单精度 |
| `f64` | 64 | 双精度（默认，运行时表示） |
| `f128` | 128 | 四精度 |

**运行时**: `Value::Float(f64)`  
**LLVM 原生**: 全内联 fadd/fsub/fmul/fdiv/frem/fcmp + math 内建（sqrt/abs/floor/ceil/round/pow/nan/inf）  
**字面量**: `3.14`, `1.0e10`, `3.0f32`

### 2.3 布尔

**运行时**: `Value::Bool(bool)`  
**LLVM 原生**: 全内联  
**字面量**: `true`, `false`

### 2.4 Void / Null

| 类型 | 运行时 | 说明 |
|------|--------|------|
| `void` | `Value::Void` | 空返回类型 |
| `null` | `Value::Opt(None)` | 可选值的空状态 |

---

## 3. 字符串类型

### 3.1 当前实现 (v0.1.5)

```
StringData = { buf: [u8; 64], len: usize }  // 72 字节, 栈分配
```

- 值类型（Copy 语义），赋值即 memcpy
- 字面量 > 64 字节 → 编译错误
- 不需要 `deinit()`

### 3.2 目标设计 (Q11 方案 A)

```
String = { ptr: *const u8, len: u16 }  // 栈上胖指针
```

- 短字符串（≤ 某个阈值）: `ptr` 指向栈上 inline buffer
- 长字符串: `ptr` 指向堆分配
- 统一 `Str` 和 `String` 为单一类型
- 值语义，复制即复制指针和长度

### 3.3 String 方法

| 方法 | 签名 | 说明 |
|------|------|------|
| `.len()` | `usize` | 字节长度 |
| `.as_slice()` | `&[u8]` | 只读字节视图 |
| `.concat(s2)` | `String` | 拼接 |
| `.substring(lo, hi)` | `String` | 子串 |
| `.find(needle)` | `?usize` | 查找子串位置 |
| `.split(delim)` | 迭代器 | 分割 |
| `.replace(old, new)` | `String` | 替换 |
| `.to_upper()` | `String` | 转大写 |
| `.to_lower()` | `String` | 转小写 |

**LLVM 原生**: 通过 helper 函数支持（`@hc_str_concat` 等）

---

## 4. 复合类型

### 4.1 定长数组 `[T, N]`

```hc
var a: [i32, 3] = [1, 2, 3];          // 类型标注
var b = [10, 20, 30];                  // 类型推断 → [i32, 3]
var mut m: [i32, 10] = alloc.init(i32, 10);  // 分配器初始化
```

- `N` 是编译期常量，`[i32, 3]` 和 `[i32, 5]` 是不同类型
- 元素通过索引访问：`a[0]`, `a[1]`
- 可写需要 `var mut`
- `alloc.init(T, N)` 创建指定长度的默认值数组

**运行时**: `Value::Arr(Rc<RefCell<Vec<Rc<RefCell<Value>>>>>)`  
**LLVM 原生**: helper 函数（`@hc_make_arr`, `@hc_index`, `@hc_store_index`）

### 4.2 动态数组 `Vec<T>`

```hc
var mut v: Vec<i32> = alloc.init(Vec<i32>);   // 空 Vec
var mut v2 = Vec<i32>[1, 2, 3];               // 字面量初始化
v.append(42);
var s = v.as_slice();
```

- 动态可变长度
- 持有分配器引用，扩容时使用
- deref 到 `Arr`，共享所有 `Arr` 方法

**运行时**: `Value::Vec(Rc<RefCell<VecData>>)`  
**LLVM 原生**: 部分支持（`init`/`append`/`len`/`as_slice` helper 函数）

| 方法 | 说明 |
|------|------|
| `.init(alloc)` | 创建空 Vec |
| `.append(item)` | 追加元素 |
| `.len()` | 元素数量 |
| `.as_slice()` | 只读切片视图 |
| `.front()` / `.back()` | 首/尾元素 |
| `.get(i)` / `.put(i, v)` | 索引读写 |
| `.extend(other)` | 合并另一个集合 |

### 4.3 切片 `&[T]` / `&mut [T]`

```hc
var sub = arr[1..4];       // 只读切片
var mut sub = arr[1..4];   // 可写切片（写穿到源数组）
```

- 非拥有视图，指向源数组的 `[start..start+len]` 范围
- 可写切片写入会写穿到源数组
- 可以从 `Arr`、`Vec`、`String` 创建

**运行时**: `Value::Slice { data, start, len }`  
**LLVM 原生**: helper 函数（`@hc_slice`, `@hc_store_slice`）

### 4.4 元组 `(T1, T2, ...)`

```hc
fn divmod(a: i32, b: i32) (i32, i32) {
    return (a / b, a % b);
}
var (q, r) = divmod(10, 3);  // 解构
```

**运行时**: `Value::Arr`（元素为元组各字段）  
**LLVM 原生**: 通过 `Arr` 的 `Destructure` 支持

### 4.5 指针 `*T` / `*mut T`

```hc
var mut x: i32 = 5;
var p = &mut x;     // *mut i32
p.* = 10;           // 写穿
var q = &x;         // *i32（只读）
```

**运行时**: `Value::Ptr(Rc<RefCell<Value>>)`  
**LLVM 原生**: `@hc_deref` / `@hc_store_ptr` helper

### 4.6 装箱 `box(T)` / 接口胖指针

```hc
var p = box(42);           // 堆分配
var iface: *I = box(obj);  // 接口类型擦除
```

**运行时**: `Value::Boxed(Rc<RefCell<BoxedData>>)` — 三字宽：`{data, vtbl, alloc}`

---

## 5. 集合类型

### 5.1 Map `Map<K, V>` — 字典

```hc
var mut m: Map<String, i32> = alloc.init(Map<String, i32>);
m.put("key", 42);
var v = m.get("key");  // ?i32
```

**运行时**: `Value::Map(Rc<RefCell<MapData>>)` — `HashMap<String, Value>` + allocator  
**LLVM 原生**: 待补齐

| 方法 | 说明 |
|------|------|
| `.init(alloc)` | 创建空 Map |
| `.put(key, val)` | 插入/更新 |
| `.get(key)` | 查找（返回 `?V`） |
| `.remove(key)` | 删除 |
| `.len()` | 键值对数量 |
| `.keys()` | 键迭代器 |
| `.values()` | 值迭代器 |

### 5.2 Table `Table<T>` — 二维表

```hc
var mut t: Table<i32> = alloc.init(Table<i32>, cols);
t.add_row([1, 2, 3]);
var cell = t.get(row, col);
```

- 固定列数，动态行数
- 内部 `Vec<Vec<T>>` 实现

**LLVM 原生**: 待补齐

### 5.3 Set `Set<T>` — 集合 (语法糖)

```hc
var mut s: Set<i32> = alloc.init(Set<i32>);
s.add(42);
```

- `Set<T>` 是 `Map<T, void>` 的语法糖
- 不需要单独运行时类型

---

## 6. 枚举与错误

### 6.1 Enum — 枚举

```hc
enum Color { Red, Green, Blue }
enum Maybe { some: i32, none }

var c = Color.Red;
var m = Maybe{some = 42};
switch (m) {
    some => |i| io.print("{}", i),
    none => io.print("none"),
}
```

**运行时**: `Value::Enum { name, variant, payload }`  
**LLVM 原生**: helper 函数（`@hc_make_enum`, `@hc_unwrap`, `@hc_match_test`）

### 6.2 Optional `?T`

```hc
var x: ?i32 = 42;
var y: ?i32 = null;
var z = x orelse 0;         // 默认值
if (x) |val| { ... }        // 捕获
switch (x) { ... }          // 穷尽匹配
```

**运行时**: `Value::Opt(Option<Rc<Value>>)`  
**LLVM 原生**: inline tag check

### 6.3 Error `E!T` / `!T`

```hc
fn open(path: String) NotFound!File { ... }
var f = try open("data.txt");  // 传播错误
var f = open("data.txt") catch |e| { ... };
```

**运行时**: `Value::Err { name, code }`  
**LLVM 原生**: inline tag check (`T_ERR`)

---

## 7. 函数与闭包

### 7.1 函数引用 `Fn`

```hc
fn add(a: i32, b: i32) i32 { return a + b; }
var f = add;          // Fn 值
var r = f(1, 2);      // 间接调用
```

**运行时**: `Value::Fn(String)`  
**LLVM 原生**: Phase 8 完整支持（`ptrtoint` + `CallIndirect`）

### 7.2 闭包

```hc
var mut x = 0;
var inc = || { x += 1; return x; };  // 可变捕获
var r = inc();  // 1
```

**运行时**: `Value::Closure(ClosureData)` — 捕获环境 = 共享槽快照  
**LLVM 原生**: Phase 8 完整支持（`malloc` + GEP 构造胖指针）

---

## 8. 类与结构体

### 8.1 Class — 类

```hc
class Counter {
    mut count: i32,

    fn inc(self: *mut Self) void {
        self.count += 1;
    }
}
var c = Counter{count = 0};
c.inc();
```

**运行时**: `Value::Class(Rc<RefCell<ClassData>>)` — `{name, fields: HashMap<String, Value>}`  
**LLVM 原生**: helper 函数（`@hc_make_class`, `@hc_field`, `@hc_store_field`）

### 8.2 Struct — 结构体 (值类型)

```hc
struct Point { x: i32, y: i32 }
var p = Point{x = 10, y = 20};
var q = p;  // 复制（值语义）
```

- 值类型，字段必须是值类型（标量、定长数组、嵌套 struct）
- 栈分配，复制即 memcpy

**LLVM 原生**: 连续内存布局 + `@hc_field` / `@hc_store_field`

---

## 9. 所有权与读写权

### 9.1 所有权模型

| 标注 | 含义 |
|------|------|
| `var x: T` | 只读绑定 |
| `var mut x: T` | 可读写绑定 |
| `o T` | 所有权标注（非值类型） |
| `owned T` | 拥有所有权，需 `defer` 或 `move` |

### 9.2 集合所有权传播

集合内部元素的所有权（非值类型）归集合所有：
- 集合可读 → 内部元素可读
- 集合可写 → 内部元素可写
- 替换元素 → 原元素内存释放

### 9.3 初始化器

```hc
// 定长数组
var mut a: [i32, 10] = alloc.init(i32, 10);
var mut a: [T, N] = alloc.init(T, N);

// 结构体
var mut s: Point = alloc.init(Point);
var mut s = alloc.init(Point{x = 10, y = 20});

// 动态集合
var mut v: Vec<i32> = alloc.init(Vec<i32>);
var mut m: Map<String, i32> = alloc.init(Map<String, i32>);
var mut t: Table<i32> = alloc.init(Table<i32>, cols);

// 字面量（类型推断）
var a = [1, 2, 3];               // [i32, 3]
var v = Vec<i32>[1, 2, 3];       // Vec<i32>
```

---

## 10. LLVM 原生 Codegen 支持状态

| 类型 | 支持级别 | 备注 |
|------|----------|------|
| Int / Float / Bool | ✅ 完整内联 | 直接 LLVM SSA 指令 |
| Void / Null | ✅ 完整内联 | |
| String | ✅ Helper 原生 | `@hc_str_*` 系列 |
| Arr (定长数组) | ✅ Helper 原生 | `@hc_make_arr` / `@hc_index` |
| Slice | ✅ Helper 原生 | `@hc_slice` / `@hc_store_slice` |
| Ptr | ✅ Helper 原生 | `@hc_deref` / `@hc_store_ptr` |
| Class | ✅ Helper 原生 | `@hc_make_class` / `@hc_field` |
| Enum | ✅ Helper 原生 | `@hc_make_enum` / `@hc_unwrap` |
| Error | ✅ 内联 | tag check |
| Fn / Closure | ✅ Phase 8 | `CallIndirect` |
| Vec (动态数组) | ⚠️ 部分 | `init`/`append`/`len` 已实现，其他待补齐 |
| Map | ❌ 待补齐 | 解释器已实现 |
| Table | ❌ 待补齐 | 新类型 |
| Set | ❌ 待补齐 | Map<T,void> 语法糖 |
| Union | ❌ 临时拒绝 | ADR-0014 |
| sort / binary_search | ❌ 待补齐 | 标准库功能 |
| JSON / CSV / FS / Net | ❌ 待补齐 | IO 功能，解释器已实现 |

---

## 11. 与 Rust 类型对比

| Rust | H 语言 | 说明 |
|------|--------|------|
| `i8..i128, u8..u128` | `i8..i128, u8..u128` | 相同，但 H 运行时统一 i128 |
| `f32, f64` | `f16, f32, f64, f128` | H 多 f16/f128 |
| `bool` | `bool` | 相同 |
| `()` | `void` | 相同 |
| `char` | 无 | H 用 `u8` 或 `i32` |
| `str` / `&str` | `String` | H 统一为值类型 |
| `String` | `String` | H 栈分配为主 |
| `[T; N]` | `[T, N]` | 相同 |
| `Vec<T>` | `Vec<T>` | 相同 |
| `&[T]` | `&[T]` | 相同 |
| `HashMap<K,V>` | `Map<K,V>` | 相同 |
| `BTreeMap<K,V>` | 无 | |
| `HashSet<T>` | `Set<T>` | 语法糖 Map<T,void> |
| `(T1, T2)` | `(T1, T2)` | 相同 |
| `Option<T>` | `?T` | 相同 |
| `Result<T,E>` | `E!T` | 相同 |
| `Box<T>` | `box(T)` | 相同 |
| `*const T` / `*mut T` | `*T` / `*mut T` | 相同 |
| `dyn Trait` | `*I` (box 后) | H 用接口胖指针 |
| `fn` | `Fn` | 相同 |
| 闭包 | 闭包 | 相同 |
| `Mutex<T>` | `Mutex<T>` | 相同 |
| `RwLock<T>` | 无 | 协作式单线程无需 |
| `Arc<T>` / `Rc<T>` | 无显式标注 | 解释器内部用 Rc |
| `VecDeque<T>` | `Deque<T>` | H 有 Deque |
| `LinkedList<T>` | 无 | |
| `BinaryHeap<T>` | 无 | |
| `BTreeSet<T>` | 无 | |
| `Cow<T>` | 无 | |
| `PhantomData<T>` | 无 | |
| `Pin<T>` | 无 | |
| `Cell<T>` / `RefCell<T>` | 无显式 | 解释器内部用 RefCell |
| mpsc::channel | `Chan<T>` | H 有 M:N 通道 |
| DataFrame | `Table<T>` | H 特有 |