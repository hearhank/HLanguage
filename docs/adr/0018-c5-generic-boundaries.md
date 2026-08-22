# C5 泛型边界定案：内建泛型嵌套具体化 + 无限大小类型语言层拒绝

> 2026-08-22 定案（C5 = 组 D 完成注记已知边界，grill-with-docs 访谈，C5-2 确认）。关联：SPEC [06-03-extended-types.md](../SPEC/06-03-extended-types.md)（tree/class 递归形态）、[06-02-types.md](../SPEC/06-02-types.md)（类型总表）、[01-unimplemented-features.md](../phase3/01-unimplemented-features.md)（C5 条目）、[02-syntax-rules.md](../phase3/02-syntax-rules.md)（§3 类型规则）。

## 背景

- C5 是组 D（E1.2 comptime 完整）完成注记登记的两项已知边界：
  - **C5-1**：内建泛型外层嵌套 `Vec<List<i32>>` 仍退化裸名 `Vec`（具体化时外层内建泛型的嵌套类型实参丢失）
  - **C5-2**：`?` 自引用外无限大小类型语言层非法未处理（值内嵌自引用/互递归不报错）
- 两项独立：C5-1 是 `hc/src/comptime.rs` 具体化实现 bug 的预期行为定案；C5-2 是类型安全规则设计

## 决策

### C5-1 内建泛型嵌套具体化 → 预期行为定案（实现修复）

1. **预期行为**：`Vec<List<i32>>`（List = 类型函数）具体化后类型 = **内建泛型名 + 具体化键**，即 `Vec<List<@i32>>`——外层内建泛型保留嵌套类型实参，**不退化裸名 `Vec`**
2. 内层类型函数先具体化登记（`List(i32)` → `List<@i32>`），外层内建泛型以「内层具体化键」为实参继续实例化
3. 三后端共享（interp / IR / LLVM）经既有 `map_type_apps` 深度遍历 + 内建泛型 resolve 通道——修复点 = 内建泛型 resolve 不丢弃嵌套实参（返回 `Named(内建名, [具体化键])` 而非 `Named(内建名, [])`）
4. **归实现修复**（C5 条目待实施），本 ADR 只定预期行为

### C5-2 无限大小类型 → **语言层拒绝（编译错误，带类型名 + 循环链位置）**

1. **规则**：所有类型必须**有限大小且可计算**。**值内嵌自引用/互递归（无间接层）= 编译错误**——报类型名 + 循环链位置（`class Foo { foo: Foo }` 之类）
2. **合法间接层（打破循环）**：指针 `*T`/`*mut T`、装箱（`box`/`o`）、堆容器 `Vec`/`Map`/`Table`/`String`（指针尺寸）、`?T`（Optional——既有 `LinkedList` 自引用终止形态）
3. **非法形态（须拒绝）**：直接值内嵌 `class Foo { foo: Foo }`；定长数组内嵌 `class Foo { foos: [N]Foo }`（定长数组 = 值内嵌）；`[continuous] class A { a: A }`（值类型内嵌自身）；互递归值内嵌 `class A { b: B } class B { a: A }`
4. **实现落点**：语义阶段**尺寸可计算性检查**——类型图「按值内嵌」边 + 环检测；有环且无间接层断环 → 编译错误。`tree`（层级组合，子节点走堆容器）与 `LinkedList`（`?` 自引用）既有合法递归不受影响

## 理由

- C5-1：内建泛型（Vec/Map/Table 等）是语言基座，嵌套具体化退化裸名会导致类型混淆/无法区分不同元素类型实例——保留嵌套实参是类型正确性底线；实现点单一（comptime resolve 通道），与既有 D3 深度遍历复用
- C5-2：无限大小类型违反「类型 = 可计算内存布局」前提，会破坏序列化/布局控制/分配器取大小等全部下层机制；显式拒绝（Zig/Rust 同精神——Rust "recursive type has infinite size"）比静默错误布局更安全；间接层清单与既有递归形态（tree/LinkedList/堆容器）一致，零新概念
