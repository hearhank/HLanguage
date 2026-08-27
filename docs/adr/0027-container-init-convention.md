# ADR-0027：容器初始化器统一约定

## 状态

2026-08-26 定案

## 背景

标准库容器（Vec / Deque / Map / Table / String 等）的初始化方式存在不一致：
- 分配器参数位置不统一（有些是第一个参数，有些是最后一个）
- 部分容器没有字面量语法
- 缺少一致的初始化器设计规则

同时，关于容器元素所有权和读写权限的语义也不明确——是否需要在类型参数中标注 `owned`/`mut`。

## 决策

### 1. 简化：容器默认 owning，去掉 `owned`/`mut` 类型参数

**不再在容器类型参数中使用 `owned`/`mut` 标注。** 所有容器（Vec / Deque / Map / Table）默认 owning 元素——容器销毁时一并释放元素内存。

```h
// 之前（设计讨论中的方案）
var v = Vec<owned mut i32>.init(alloc);

// 之后（定案）
var mut v = Vec<i32>.init(alloc);
```

### 2. 元素读写权限 = 容器变量绑定权限

容器元素的读写权限与容器变量本身的 `var`/`var mut` 绑定：

| 绑定 | 容器本身 | 元素访问 |
|------|----------|----------|
| `var` | 只读（不可 reassign，不可 append） | 只读 |
| `var mut` | 可变（可 reassign，可 append） | 可读写 |

```h
var mut v = Vec<i32>.init(alloc);  // 可变 → 元素可读写
v.append(5);                        // ✅
v[0] = 42;                          // ✅

var v = Vec<i32>.init(alloc, [1,2,3]);  // 只读 → 元素只读
v[0] = 42;                              // ❌ 编译错误
```

### 3. 借用语义通过切片实现

借用场景不使用容器类型，而是使用已有的切片语法：

```h
var mut v = Vec<i32>.init(alloc, [1,2,3]);
var s: &[i32] = &v;              // 只读借用
var ms: &mut [i32] = &mut v;      // 可变借用
```

### 4. 分配器永远是最后一个参数

所有容器的 `init` 方法中，分配器参数统一放在最后（业务参数优先，分配器是基础设施细节）：

```h
// ✅ 正确（分配器在最后）
Vec<i32>.init()                   // 空 Vec，默认 allocator
Vec<i32>.init(alloc)              // 空 Vec，显式 allocator（唯一参数时首尾等价）
Vec<i32>.init(100)                // 预分配容量，默认 allocator
Vec<i32>.init(100, alloc)         // 预分配容量，显式 allocator
Vec<i32>.init([1, 2, 3])          // 从数组字面量，默认 allocator
Vec<i32>.init([1, 2, 3], alloc)   // 从数组字面量，显式 allocator

// ❌ 错误（分配器在业务参数之前）
Vec<i32>.init(alloc, 100)
```

### 5. 各容器 init 签名

| 容器 | 签名 | 说明 |
|------|------|------|
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

### 6. `alloc.init` 三形态

堆分配原语 `alloc.init` 有三种形态，与容器 `init` 正交：

```h
// 形态 1：类型实例，零初始化
var mut p: *T = alloc.init(T);

// 形态 2：类型实例，带字段初始化
var mut p: *T = alloc.init(T{ field = "value" });

// 形态 3：数组，n 个元素
var mut a: *[T, n] = alloc.init(T, n);
```

### 7. 容器字面量语法

- `Vec<T>[1, 2, 3]` — 方括号字面量
- `Map<K,V>{"k" = v}` — 花括号 KV 字面量，`=` 分隔键值
- 数组字面量 `[1, 2, 3]` 保持现有语法

### 8. 排除项

- **String**：保持工厂方法（`String.from(...)`），不纳入 init 统一设计
- **A6 数据结构**（RingBuf / PageMem / IntrList / TreeMap / Bitmap）：保持 `io.*` 命名空间访问
- **数组（定长 `[N]T`）**：保持字面量 `[1, 2, 3]` 语法，无构造器

## 理由

1. **去掉 `owned`/`mut` 类型参数**大幅降低了认知负担——用户不需要在类型参数层面思考所有权策略，只需理解 `var`/`var mut` 的绑定语义。
2. **分配器在最后**与设计文档 `08-mem-allocator-design.md §7` 一致，且更自然（业务参数优先，分配器是基础设施细节）。
3. **`var`/`var mut` 决定元素读写权限**是 H 语言所有权模型的自然延伸，没有引入新概念。
4. **借用通过切片**保持了语言已有的 `&[T]` / `&mut [T]` 路径，不增加容器类型的复杂度。

## 替代方案

1. **类型参数 `owned`/`mut`**：在 `Vec<owned mut T>` 中标注元素所有权和读写权限。被否决，因为过于复杂——每个容器使用点都需要考虑类型参数标注，且与 `var`/`var mut` 绑定语义重叠。
2. **分配器在第一个参数**：当前实现的做法。被否决，与设计文档不一致，且分配器作为基础设施细节应放在最后。
3. **所有容器用 `alloc.init` 创建**：统一用 `alloc.init(Vec<i32>, alloc)` 创建容器。被否决，因为容器 `init` 方法可以有更多重载变体（容量、回调等），`alloc.init` 只做底层分配。

## 影响

1. 需修改容器 `init` 方法实现，将分配器参数从第一个移到最后一个。
2. 需实现 IR 降级器中的容器字面量（`Vec<T>[1, 2, 3]`、`Map<K,V>{"k" = v}`）。
3. 需修改示例代码中容器初始化使用方式。
4. 与现有 `alloc.init` 模式兼容，无冲突。