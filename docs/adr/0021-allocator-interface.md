# 分配器接口扩展设计（Zig 式可扩展分配器）

> 2026-08-23 定案（grill-with-docs 访谈，22 子项全推荐）。关联：SPEC [08-mem-allocator-design.md](../SPEC/phase1/08-mem-allocator-design.md)（基础设计）、[00-feature-inventory.md](../SPEC/00-feature-inventory.md)（功能清单）、[02-1x-delayed-items.md](../SPEC/phase4/02-1x-delayed-items.md)（Phase 4 延迟项）。

## 背景

当前的分配器设计（ADR-0003 + `08-mem-allocator-design.md`）使用 `Value::Alloc`（无状态哨兵）和 `Value::Arena(Rc<RefCell<ArenaState>>)`（硬编码 bump 分配器）两种内建值表示。这种方式限制了扩展性：新增分配器后端（如 Pool、Stack、Page 等）需要修改 `Value` 枚举，无法让用户自定义分配器。

Zig 的分配器设计以**接口**（`std.mem.Allocator`）为核心，包含 `alloc`/`realloc`/`free` 三个方法指针，后端可以是 `page_allocator`、`ArenaAllocator`、`StackFallbackAllocator`、`PoolAllocator`、`c_allocator` 等，用户可以自定义实现。

本 ADR 将 H 的分配器系统从「内建枚举」迁移到「接口 + 枚举调度」模式，在保持向后兼容的前提下实现 Zig 式的可扩展性。

## 设计决策

### D1 — 分配器接口形态（接口方案）

**决策**：定义 `Allocator` 接口，所有分配器后端（Arena、Page、Pool、Stack 等）都实现该接口。`Value::Alloc` 和 `Value::Arena` 从枚举中移除，统一为 `Value::Allocator(Rc<RefCell<AllocatorImpl>>)`。

**理由**：接口方案提供了最大的扩展性，用户可以实现自定义分配器（后续开放 H 侧实现），不需要修改 `Value` 枚举。

### D2 — 分配器接口方法集（对齐 Zig）

**决策**：`Allocator` 接口包含三个方法：
- `alloc(n) -> []u8` — 分配 `n` 字节零初始化内存
- `realloc(block, n) -> []u8` — 调整内存块大小
- `free(block)` — 释放内存块

**理由**：对齐 Zig 的 `std.mem.Allocator` 接口，有 realloc 的后端可以覆盖默认实现提升性能。

### D3 — H 语言侧分配器 API（标准库 `H.std.heap`）

**决策**：分配器通过标准库暴露，`import H.std.heap.{Arena, page_allocator}`。`with_arena` 内建函数移除。

用户代码示例：
```hc
import H.std.heap.{Arena, page_allocator}

var arena = Arena.init(page_allocator);
var buf = arena.alloc(256);
// 使用 buf...
arena.deinit();
```

**理由**：标准库方式更一致，用户通过 import 使用，不需要学习内建函数。`Arena.init(backing_allocator)` 对齐 Zig 设计。

### D4 — Rust 侧 `AllocatorImpl` 枚举表示

**决策**：使用枚举式 `AllocatorImpl` 包含三种变体：
```rust
pub enum AllocatorImpl {
    Page,                                    // 无状态全局分配器
    Arena(ArenaState),                       // bump 分配器
    Custom(Box<dyn AllocatorTrait>),         // 自定义分配器
}
```

`deinit()` 方法通过 match 分发：
```rust
impl AllocatorImpl {
    fn alloc(&mut self, n: usize) -> Result<AllocBlock, AllocErr> { ... }
    fn free(&mut self, block: &AllocBlock) { ... }
    fn realloc(&mut self, block: &AllocBlock, n: usize) -> Result<AllocBlock, AllocErr> { ... }
    fn deinit(&mut self) {
        match self {
            Self::Arena(a) => a.deinit(),
            Self::Page => {},
            Self::Custom(c) => c.deinit(),
        }
    }
}
```

**理由**：枚举式避免 Arena 的 bump 分配走虚函数调用，Page 零大小（无状态），Custom 为自定义分配器兜底。

### D5 — `alloc` 环境变量彻底替换

**决策**：`alloc` 环境变量从 `Value::Alloc` 变为 `Value::Allocator(AllocatorImpl::Page)`。所有持分配器引用的地方（`BoxedData.alloc`, `VecData.alloc`, `MapData.alloc`）类型不变，因为 `Value::Allocator(...)` 是统一的值。

**理由**：一次性替换，干净无遗留。

### D6 — `deinit` 在 `AllocatorImpl` 枚举上

**决策**：`deinit()` 是 `AllocatorImpl` 枚举的方法，不在 `AllocatorTrait` 上。Arena 释放块链表，Page 空操作，Custom 通过 trait 的 `deinit()` 实现。

**理由**：`deinit` 是分配器实例的生命周期操作，不是分配接口的一部分。枚举式 match 分发更高效。

### D7 — `Value::Bytes` 新变体

**决策**：新增 `Value::Bytes(Rc<RefCell<Vec<u8>>>)` 作为分配器返回的原始内存块。与 `Value::Str` 区分开（Str 参与字符串操作，Bytes 是原始内存）。

支持的操作：
- `block.len` → 长度
- `block[i]` → 读取字节
- `block[i] = val` → 写入字节
- `block[start..end]` → 切片视图

**理由**：`Str` 和 `Bytes` 语义不同，分开避免混淆。`Bytes` 是可变原始内存，不参与字符串操作。

### D8 — `realloc` 默认实现

**决策**：`realloc` 在 `AllocatorTrait` 上有默认实现：`alloc(new_n)` → `copy(min(old_n, new_n) bytes)` → `free(old)`。Arena 等不支持 realloc 的后端不需要重复实现。

**理由**：减少分配器实现者的负担，同时允许 Page 等支持 realloc 的后端覆盖默认实现以提升性能。

### D9 — 第一期 Rust 实现

**决策**：第一期所有分配器后端用 Rust 实现，标准库暴露 `Arena`, `page_allocator` 等。后续开放 H 侧自定义分配器。

**理由**：分配器与运行时深度绑定，第一期先确保接口稳定，再开放 H 侧实现。

### D10 — 标准库 `H.std.heap` 首发分配器

**决策**：第一期标准库包含：
- `Arena` — bump 分配器，有状态，临时分配
- `page_allocator` — 全局无状态分配器，对应当前 `Alloc`
- `Pool(T)` — 固定大小对象池，空闲链表 + 后备分配器

**理由**：覆盖最常用的分配模式，`Pool(T)` 适合高频创建同类型对象的场景（如解析器节点）。

### D11 — 分阶段迁移计划

**决策**：三阶段迁移：

| 阶段 | 内容 | 测试 |
|------|------|------|
| **Phase 1** | 添加 `Value::Allocator` + `Value::Bytes`。`alloc` 环境变量改为 `AllocatorImpl::Page`。`AllocatorImpl` 实现 alloc/free/realloc/deinit。保留旧的 `Value::Alloc` / `Value::Arena` 兼容 | 现有测试全通过 |
| **Phase 2** | 迁移 `box(v, alloc)`、`Vec(T).init(alloc)`、`Map(K,V).init(alloc)`、`spawn()` 内部分配器。`Value::Alloc` / `Value::Arena` 标记 deprecated | 组件级测试 |
| **Phase 3** | 移除 `with_arena`。移除 `Value::Alloc` / `Value::Arena`。添加标准库 `H.std.heap`。清理旧代码 | 全量测试 |

**理由**：分阶段可独立测试，每阶段可部署，降低风险。

### D12 — `AllocatorTrait` Rust 定义

```rust
pub trait AllocatorTrait {
    fn alloc(&mut self, n: usize) -> Result<AllocBlock, AllocErr>;
    fn free(&mut self, block: &AllocBlock);
    fn realloc(&mut self, block: &AllocBlock, n: usize) -> Result<AllocBlock, AllocErr>;
    fn deinit(&mut self) {}  // 默认空实现
}

pub struct AllocBlock {
    pub data: Rc<RefCell<Vec<u8>>>,
    pub offset: usize,
    pub len: usize,
}
```

### D13 — `AllocErr` 错误类型

```rust
pub enum AllocErr {
    OutOfMemory,
    InvalidSize,
}
```

H 侧映射为 `error.OutOfMemory` / `error.InvalidSize`，用户可以用 `catch` / `?` 处理分配失败。

### D14 — `page_allocator` 实现

无状态分配器：`alloc(n)` 创建独立 `Vec<u8>`（`try_reserve_exact` + `resize`），包装为 `AllocBlock`。`free` 空操作（Rc 引用归零时自动释放）。`realloc` 直接调整 Vec 大小。

### D15 — `Pool(T)` 分配器设计

每个 Pool 实例管理一个固定大小的空闲链表，`alloc` 返回空闲块或新建，`free` 归还到空闲链表。Pool 持有后备分配器用于在池耗尽时申请新页。

```rust
pub struct PoolAllocator {
    item_size: usize,
    free_list: Vec<AllocBlock>,
    backing: AllocatorImpl,  // 后备分配器
}
```

### D16 — H 侧 `Allocator` 接口定义

标准库 `H.std.heap` 中定义：
```hc
interface Allocator {
    fn alloc(self, n: u64) -> []u8;
    fn free(self, block: []u8);
    fn realloc(self, block: []u8, n: u64) -> []u8;
}
```

`Arena` 和 `page_allocator` 都是实现该接口的类型。第一期接口定义在标准库中，由 Rust 后端提供实现。

## 迁移路线图

### 当前状态（迁移前）
```
Value::Alloc          → 全局分配器哨兵
Value::Arena(state)   → Arena bump 分配器（硬编码）
with_arena(fn)        → 内建函数
alloc 环境变量        → Value::Alloc
box(v, alloc)         → alloc 可为 Value::Alloc 或 Value::Arena
Vec(T).init(alloc)    → 同上
Map(K,V).init(alloc)  → 同上
```

### 迁移后状态
```
Value::Allocator(impl) → 统一分配器接口值
Value::Bytes(data)     → 原始内存块
alloc 环境变量         → Value::Allocator(Page)
box(v, alloc)          → alloc 为 Value::Allocator(...)
Vec(T).init(alloc)     → 同上
Map(K,V).init(alloc)   → 同上
H.std.heap.Arena       → 标准库 Arena 类
H.std.heap.page_allocator → 标准库全局分配器实例
H.std.heap.Pool(T)     → 标准库对象池类
```

## 关联文件

- `hc-rt/src/value.rs` — `Value` 枚举（新增 `Allocator` / `Bytes` 变体）
- `hc-rt/src/interp/call.rs` — `call_builtin`（移除 `with_arena`，更新 `box`/`spawn`）
- `hc-rt/src/interp/eval.rs` — 表达式求值（Bytes 索引/切片操作）
- `hc-rt/src/interp/expr.rs` — 表达式求值（添加 Bytes 支持）
- `hc-rt/src/interp/layout.rs` — 测试运行器（更新分配器创建）
- `hc-rt/src/interp/loader.rs` — 环境变量注入（`alloc` 改为 Page）
- `hc-rt/src/interp/io.rs` — IO 模块（线程分配器创建）
- `hc-rt/src/interp/mod.rs` — 模块导出
- `docs/SPEC/phase1/08-mem-allocator-design.md` — 更新设计文档

---

**关联**：[ADR-0003 内存模型](0003-memory-model.md)｜[08-mem-allocator-design.md](../SPEC/phase1/08-mem-allocator-design.md)｜[00-feature-inventory.md](../SPEC/00-feature-inventory.md)