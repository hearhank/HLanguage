# mem 标准库内存分配器设计

> 状态：设计定稿（2026-08-17）。对应 `04-stdlib-scope.md` 基础层 `mem` 行（Allocator 抽象、全局回退分配器、Arena），参考 Zig `std.mem`。实现落点：第一块 M4（内存模型）+ 第二块 M5.1（标准库最小 mem 子集）。

## 1. 目标与范围

`mem` 提供 H 的全部内存分配设施，由三部分组成：

1. **`Allocator` 抽象** —— 分配接口，显式传递，可替换
2. **全局回退分配器 `alloc`** —— 未显式传入分配器时的默认值（每线程独立实例，Q8 定案）
3. **`Arena`** —— 批量分配、统一回收的分配器（请求级生命周期惯例）

设计约束（来自既有定案）：

- **无隐式内存分配**（CONTEXT §4「没有隐藏控制」）——唯一隐式行为是作用域退出自动销毁，其余分配必须显式
- **分配经显式 Allocator 进行**，未显式传入时回退全局分配器（[ADR-0003](0003-memory-model.md)）
- **所有权 = 销毁责任的唯一归属**（ADR-0003 Q-S11 修订）——非 Arena 分配默认当前作用域负责，Arena 由 Arena 统一负责
- 分配器实例是显式类型（类似 `Io`），无全局/隐式状态泄漏

## 2. Allocator 抽象

### 2.1 接口形态

`Allocator` 是一个接口类型（接口三用途，见 `06-05-interfaces.md`）：函数需要分配时，以显式参数接收分配器。

```hc
fn parse(io: *T, alloc: Allocator) !Vec(Node) where T: Io { ... }
```

方法集（**对齐 2026-08-14 phase1 审计 C2 定案**，取代早前 `arena.alloc(T{...})` 单形态）：

| 方法 | 签名 | 语义 |
|---|---|---|
| `alloc` | `alloc(n: usize) &[u8]` | 字节分配：返回 `n` 字节零初始化连续内存（`&[u8]` 视图，不拥有数据，数据归属分配器或作用域，见 §5） |
| `init` | `alloc.init(T)` / `alloc.init(T{...})` | 类型实例构造（双形态）：无参形态按类型字段默认值创建（definite assignment M2.5）；带参形态为类型字面量求值即实例 |
| `deinit` | `alloc.deinit() void` | 释放分配器自身持有的资源；全局分配器为 no-op，Arena 回收全部 |

字节分配返回 `&[u8]`（切片视图，无所有权）而非拥有数组——调用方自行决定归属（绑到作用域拥有变量，或作为 arena 内无所有权数据）。类型实例构造 `init(T)` 返回**拥有实例**（归调用方作用域或 arena，见 §5）。

### 2.2 失败语义

分配失败返回 **`error.OutOfMemory`**（错误码表注册，M2.6 错误名↔码机制）：`alloc(n) error.OutOfMemory!&[u8]`。与 H 的 error union 正交——分配失败是可恢复运行时错误，调用方 `try`/`catch` 处理；不是 panic。

### 2.3 对齐

所有实现须保证返回地址至少满足**平台最大标量对齐**（H 值为 i128/f64 承载，故对齐 ≥ 16 字节，与 tag1 `%Value` 盒一致）。`@alignOf(T)` 对内建标量正确；聚合类型对齐由布局规则（`[align(T)]` / `[pad]`）决定，分配器只保证自身下限。

## 3. 全局回退分配器 `alloc`

### 3.1 定义

`alloc` 是默认分配器，**每线程独立实例**（Q8 定案：避免跨线程分配/释放竞争，也消除锁开销）。生命周期 = 程序/线程生命周期。

- 分配默认零初始化（确定性，配合「无隐式行为」哲学）
- `deinit()` 为 no-op；程序退出由 OS 回收。**Debug 模式下可做泄漏检测**（§8.3）
- 程序环境入口同时以 `io.alloc` 暴露（M5.4），与全局 `alloc` 同一实例

### 3.2 实现后端

全局分配器背后是平台内存后端，抽象为一个薄层（tag1 后续可替换）：

| 后端 | 场景 |
|---|---|
| libc `malloc`/`free`（`hc build` 原生、`hc run` 默认） | 桌面/服务端 |
| Win32 `HeapAlloc` / `VirtualAlloc` | Windows 原生 |
| 无 OS（freestanding K6） | 自定义后端（1.x，`05-open-questions` K6） |

后端点仅在 `mem` 内部一处隔离；`Allocator` 接口对上层不暴露后端细节。

### 3.3 与所有权

`alloc` 分配的对象**默认注册当前作用域**（拥有所有权，ADR-0003 Q-S11）：作用域退出 LIFO 自动销毁；`o` 标注为冗余（不参与判定）。`move` 合法（非 Arena 来源）。典型：

```hc
var mut v = alloc.init(Vec<i32>);   // 拥有，退出自动销毁
```

## 4. Arena

### 4.1 定义

`Arena` 是**批量分配、统一回收**的分配器：以 backing allocator 为后盾，内部管理若干内存块，分配从块内切出（bump 或空闲复用），`deinit` 时一次性把全部块归还 backing。

```hc
var arena = Arena.init(alloc);       // backing = 全局分配器
var buf = arena.alloc(1024);         // 字节分配
var node = arena.init(Node{ ... });  // 类型实例（归 arena）
// 作用域退出：arena 默认拥有 → 自动统一回收；显式提前可 arena.deinit()
```

方法集与 `Allocator` 一致（`alloc(n)` / `init(T)` / `init(T{...})` / `deinit()`），语义上所有结果**归 arena**。

### 4.2 实现形态

- **bump allocation + 块链表**：arena 维护块列表，`alloc(n)` 在当前块剩余空间内切（不足则向 backing 申请新块）；`deinit` 遍历块链表归还 backing
- 不做对象级 free（arena 语义是「统一回收」，无逐对象释放）
- `arena.init(T)` = 按类型大小对齐后 bump + 字段默认值填充（与 `alloc.init(T)` 同一构造逻辑，仅内存来源不同）

### 4.3 与所有权模型（ADR-0003 核心交互）

- **arena 分配的对象无所有权**（归 arena，Q16 定案）——对象自身退出**不**各自销毁
- **禁止 move**（move 须对整个 Arena 进行）；`o` 标注在此无意义
- **引用逃逸（Q18）**：arena 内对象的引用不得比 arena 长寿（编译期检查 + Debug 悬垂标记兜底）
- arena 实例默认拥有（Q16），作用域退出自动 `deinit`；显式提前回收可调用 `arena.deinit()`

适用惯例：**请求级生命周期**——每请求一个 arena，处理完整体回收（`examples/02-idioms/49-arena-pool.hc`）。

### 4.4 与集合/String

集合与 String 提供 arena 形态构造，避免中间复制：

- `String.from_slice(&buf, arena)` —— 内容视图入 arena（不复制）
- 集合实例本身由 `Vec.init(alloc)` 等构造时持有分配器引用（§7），传 arena 即归 arena

## 5. 分配结果的内存归属（决定性规则）

一个分配结果的内存归属由**分配器来源 + 绑定方式**共同决定，规则如下（对齐 ADR-0003 所有权全录）：

| 分配器 | 绑定到 `o` 变量 | 绑定到无 `o` 变量 | move |
|---|---|---|---|
| 全局 `alloc` | 拥有（当前作用域负责销毁） | 拥有（来源判定，`o` 冗余） | 允许 |
| `Arena` | 无所有权（归 arena） | 无所有权（归 arena） | 禁止 |

`alloc.init(T)` / `alloc.alloc(n)` 返回值的归属：**调用方作用域**（默认拥有）或 **arena**（调 `arena.init`/`arena.alloc` 时归 arena）。字节 `alloc(n)` 返回 `&[u8]` 切片视图——数据归分配器，切片自身是栈上值，无所有权。

## 6. 与装箱 / 胖指针交互

- **`box(v, alloc)`**（Q12 定案）：把值装箱到堆，返回  `owned *mut T`（拥有，作用域负责销毁）
- **接口指针 `*I` = 三字宽胖指针**（Q17 + feasibility M5 定案）：`data + 虚表 + alloc 引用`——装箱时携带显式 allocator，销毁  `owned *I` 时用携带的 alloc 释放 data。**选择携带 alloc 而非回退全局**（显式分配器哲学，feasibility 建议前者）
- `box` 的 alloc 参数显式传入；未传时回退 `alloc`

```hc
var hp: owned *mut Point = box(p, alloc);   // hp 拥有，退出自动销毁（含 data 释放）
var sp: owned *INumber = box(a, alloc);     // 胖指针：data + 虚表 + alloc 引用
```

## 7. 与集合 / 序列化的交互

- **集合持有分配器引用**：`Vec(T).init(alloc)` / `Map(K,V).init(alloc)` / `Table(T).init(alloc, rows, cols, init)`——容器把 alloc 存为内部字段，扩容与子对象分配均走它。这使集合能在作用域退出时递归销毁其拥有内容
- **序列化**：`to_bytes` / `from_bytes` 输出目标缓冲需分配 → 接收 alloc 参数（或由调用方预分配传入）
- **String**：`String.from(&[u8], alloc)`（复制构造，拥有）；`String.from_slice(&buf, arena)`（arena 形态视图）

## 8. 错误与调试

### 8.1 分配失败

`error.OutOfMemory` 走 error union 传播（§2.2）。Arena 从 backing 申请失败时同样返回该错误，arena 保持可用（部分块已提交，deinit 统一回收）。

### 8.2 Debug 悬垂标记

沿 [ADR-0003](0003-memory-model.md) Q-R7/Q-R8：Debug 默认开启——目标销毁时标记指向它的指针，访问带位置提示；Release 默认关闭（裸路径）。分配器不承担该机制（由运行时引用登记负责），但 arena 统一回收后，arena 内对象的存活引用应立即全部标记悬垂（回收是批量销毁，登记链必须随之一批注销）。

### 8.3 Debug 泄漏检测

全局分配器 Debug 模式跟踪未释放分配（分配记录表），程序/线程退出时报告泄漏清单（大小 + 分配点）。Release 关闭（零簿记）。这是 M4 验收「Debug 泄漏检测生效」的落点。

## 9. 线程模型

- 全局 `alloc` 每线程独立实例（§3.1），线程内分配/释放无锁
- **Arena 线程本地**使用，不跨线程共享；跨线程共享数据走四种模式类型（`OneToOne` 等，内部自行管理存储，构造时接收 alloc）
- `Allocator` 接口不承诺线程安全；需要共享分配器时用互斥包装（1.x，标准库提供 `mem.MutexAllocator` 候选）

## 10. tag1 现状与正式设计的差距

tag1（第一块 M4 + 第二块 M5.1）已实现 alloc/arena 内建的 Value 模型形态（`interp.rs`）：`alloc` 为内建值、`alloc.init(T)`/`alloc.alloc(n)`/`arena.alloc(n)` 可用，`deinit` 为 no-op，`arena.alloc(T{...})` 旧形态兼容。与正式设计的关键差距：

| # | 差距 | 正式落点 |
|---|---|---|
| G1 | ~~Arena 无真实内存管理（tag1 用 `Value` 模拟），`deinit` no-op~~ **✅ 已落地**（tag1）：`Value::Arena`/`IrValue::Arena` 为真实状态句柄——bump + 块链表（默认块 1024，超限按实际大小开新块）、`alloc(n)` 从当前块切出零初始化区域（不足向 backing 申请）、`deinit` 批量归还（清空块链表 + 归零统计 + 标记不可用）、deinit 后 alloc 抛 `error.ArenaDeinitialized`；OOM 仍返回可 catch 的 `error.OutOfMemory`。**`arena.init(T)`/`arena.init(T{...})` typed 构造已落地**（E1/E2）：按类型大小对齐后 bump 记账 + 字段默认值填充（对齐 `alloc.init(T)` 双形态；连续 class 按布局总大小，堆上 class = 指针宽 8；未知类型 `UnknownType`；deinit 后 init 同抛 `ArenaDeinitialized`）。跨两个后端（tree-walking interp + IR）各带测试 | bump + 块链表 + backing 归还 |
| G2 | ~~无 `error.OutOfMemory` 错误码~~ **✅ 已落地**（tag1）：`hc::error_code_table` 追加内建 `BUILTIN_ERRORS` 注册（用户码序不变）；`alloc.alloc(n)` / `arena.alloc(n)` 分配失败返回 `error.OutOfMemory`（可 catch 的 error union 值，非进程 panic——`Vec::try_reserve_exact` 优雅失败） | 错误码表注册（M2.6 机制） |
| G3 | ~~胖指针/装箱未携带 alloc 引用（tag1 `Value::Interface` 结构待扩为三字宽）~~ **✅ 已落地**（tag1）：`box(v)` 返回三字宽胖指针——`Value::Boxed`/`IrValue::Boxed` 承载 `data + vtbl + alloc`（interp `BoxedData`，IR `Cell::Boxed`）。`box` 取 1-2 参：显式传分配器则携带，未传回退全局 `alloc`（旧 1 参形态的 ArityMismatch 缺陷一并修复）；`p.alloc()` 返回携带的分配器引用；`p.*` 解引用读/写穿透 pointee；装箱 class 经 `*I` 接口分派鸭子类型达具体实现（Rect/Circle `s.area()`）。编译器内建类型对齐  `owned *mut T`（`SType::Ptr(.., true)`）。跨两个后端（tree-walking interp + IR）各带 6 测试 | §6 定案落地 |
| G4 | ~~集合未持有分配器引用（tag1 集合为 Value 内建）~~ **✅ 已落地**（tag1）：`Vec(T).init(alloc)` / `Map(K,V).init(alloc)` / `Table(T).init(alloc, rows, cols, init)` 把 alloc 存为内部字段（interp `Value::Vec`/`Value::Map` 句柄，IR `Cell::Vec { arr, alloc }`/`Cell::Map { fields, alloc }`），`collection.alloc()` 返回携带的分配器引用；未显式传 alloc（含裸类型表达式 `Vec<i32>` 实例化）回退全局 `alloc`。集合句柄 `deref_value` 剥为共享底层 Arr（20 余个既有 Arr 方法/索引/遍历复用），`&Vec` 传参、`sort(&mut v)` 写穿、`for` 遍历均正常；Map 句柄方法与 `Class("Map")` 共用实现（put/get/contains/remove/len/iter/from_json/to_json）。跨两个后端（tree-walking interp + IR）各带 8 测试（`hc-rt/tests/collection.rs`、`hc/tests/ir.rs`） | §7 定案落地 |
| G5 | ~~无对齐保证与泄漏检测~~ **✅ 已落地**（tag1）：对齐——`ALLOC_ALIGN = 16`（H 值承载 i128/f64），bump 切出前游标圆整到 16 倍数（interp `align_up`，IR `align_up_ir`），对齐填充计入 `arena.bytes()`（真实 bump 语义：alloc(1)+alloc(1)+alloc(16) → 游标 0→1→16→17→32→48）；泄漏检测——全局 `alloc.alloc(n)` 登记分配记录（interp `LeakRecord{size,line,weak}` 弱引用自动释放：值销毁 → weak 失效自动注销，作用域退出即释放；IR `Ctx::alloc_tracker` 纯计数无行号），`alloc.leaks()` 活跃数、`alloc.leak_report()` 清单文本（`"leak: line {line}: {size} bytes\n"`）；CLI `hc run`/`hc test`（interp + IR）程序/线程退出时打印 `[LEAK]` 清单到 stderr，**不改变退出码**（§11 已裁决：退出码留给将来）。跨两个后端（tree-walking interp + IR）各带测试（`hc-rt/tests/leak.rs`、`hc/tests/ir.rs`） | §2.3 / §8.3 定案落地 |
| G6 | 分配零初始化（tag1 `vec![0u8; n]` 已零初始化 ✅） | 保持 |

## 11. 开放问题

- **对齐上限的具体值**：定 16 字节（i128/f64 承载）还是平台 `max_align_t` 动态？
- **allocator 转发链**：arena 的 backing 可否再是 arena（嵌套 arena）？允许（仅 bump 记帐），但需明确 deinit 顺序（内层先于外层）
- ~~**Debug 泄漏检测的报告形式**：退出打印清单 vs 错误退出？~~ ✅ **已裁决**（G5 落地）：打印清单到 stderr + **退出码保持绿**（不因泄漏失败——保留现有测试/示例退出语义；非零退出留给更晚阶段明确界定）
- **`alloc` 每线程实例**的实现：thread-local 还是显式 per-thread 传递？（tag1 单线程，不阻塞；并发落地时定）

---

## 12. Zig 式可扩展分配器（ADR-0021 定案落地）

> 2026-08-23 定案。详见 [ADR-0021 分配器接口扩展设计](../../adr/0021-allocator-interface.md)。

### 12.1 设计目标

将分配器系统从「内建枚举」迁移到「接口 + 枚举调度」模式，实现 Zig 式的可扩展性：

- 所有分配器后端（Arena、Page、Pool、Stack 等）实现统一的 `Allocator` 接口
- 用户后续可自定义分配器（H 侧实现接口）
- 新增分配器后端不需要修改 `Value` 枚举

### 12.2 核心变更

| 当前 | 迁移后 |
|------|--------|
| `Value::Alloc`（无状态哨兵） | `Value::Allocator(AllocatorImpl::Page)` |
| `Value::Arena(ArenaState)`（硬编码） | `Value::Allocator(AllocatorImpl::Arena(ArenaState))` |
| `with_arena(fn)` 内建函数 | **移除**，改用 `H.std.heap.Arena.init(backing)` |
| `alloc` 环境变量 = `Value::Alloc` | `alloc` 环境变量 = `Value::Allocator(Page)` |
| 无 `Value::Bytes` | 新增 `Value::Bytes(Rc<RefCell<Vec<u8>>>)` 表示原始内存块 |

### 12.3 `AllocatorImpl` 枚举（Rust 侧）

```rust
pub enum AllocatorImpl {
    Page,                                    // 无状态全局分配器
    Arena(ArenaState),                       // bump 分配器（现有 ArenaState 复用）
    Custom(Box<dyn AllocatorTrait>),         // 自定义分配器（后续开放 H 侧）
}
```

方法集：`alloc(n)`, `realloc(block, n)`, `free(block)`, `deinit()`。其中 `deinit` 是枚举方法（非 trait 方法），通过 match 分发。

### 12.4 `AllocatorTrait`（自定义分配器接口）

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

pub enum AllocErr {
    OutOfMemory,
    InvalidSize,
}
```

### 12.5 标准库 `H.std.heap`（首发分配器）

| 类型 | 说明 | 构造 |
|------|------|------|
| `page_allocator` | 全局无状态分配器（每个 `alloc` 创建独立 Vec） | 预定义实例 |
| `Arena` | bump 分配器，接收后备分配器 | `Arena.init(backing_allocator)` |
| `Pool(T)` | 固定大小对象池，空闲链表复用 | `Pool.init(backing_allocator, item_size)` |

H 侧 `Allocator` 接口定义：
```hc
interface Allocator {
    fn alloc(self, n: u64) -> []u8;
    fn free(self, block: []u8);
    fn realloc(self, block: []u8, n: u64) -> []u8;
}
```

### 12.6 分阶段迁移

| 阶段 | 内容 | 测试 |
|------|------|------|
| **Phase 1** | 添加 `Value::Allocator` + `Value::Bytes`。`alloc` 环境变量改为 `AllocatorImpl::Page`。`AllocatorImpl` 实现 alloc/free/realloc/deinit。保留旧的 `Value::Alloc` / `Value::Arena` 兼容 | 现有测试全通过 |
| **Phase 2** | 迁移 `box(v, alloc)`、`Vec(T).init(alloc)`、`Map(K,V).init(alloc)`、`spawn()` 内部分配器。`Value::Alloc` / `Value::Arena` 标记 deprecated | 组件级测试 |
| **Phase 3** | 移除 `with_arena`。移除 `Value::Alloc` / `Value::Arena`。添加标准库 `H.std.heap`。清理旧代码 | 全量测试 |

### 12.7 用法示例

```hc
import H.std.heap.{Arena, page_allocator, Pool}

// 使用 page_allocator
var buf = page_allocator.alloc(256);
// ...
page_allocator.free(buf);

// 使用 Arena（bump 分配器，统一回收）
var arena = Arena.init(page_allocator);
var b1 = arena.alloc(64);
var b2 = arena.alloc(128);
// 使用 b1, b2...
arena.deinit();  // 一次性释放所有内存

// 使用 Pool（固定大小对象池）
var pool = Pool.init(page_allocator, @sizeOf(MyNode));
var node = pool.alloc();
// ...
pool.free(node);
```

---

**关联**：[ADR-0021 分配器接口扩展设计](../../adr/0021-allocator-interface.md)｜[ADR-0003 内存模型](0003-memory-model.md)｜[ADR-0005 所有权语法](0005-ownership-syntax.md)｜[04-stdlib-scope.md](04-stdlib-scope.md)｜[07-bootstrap-plan.md](07-bootstrap-plan.md)｜[06-04-functions.md](06-04-functions.md)（构造/内建）｜示例：`02-idioms/49-arena-pool.hc`、`01-syntax/04-memory/27-ownership.hc`、`01-syntax/04-memory/29-globals.hc`
