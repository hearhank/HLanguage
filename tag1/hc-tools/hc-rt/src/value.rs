//! 运行时值模型（M4 运行时与语言内建——tag1 子集）
//!
//! tag1 采用引用计数值模型：变量槽 = `Rc<RefCell<Value>>`，指针 = 槽的共享引用。
//! 完整所有权（作用域销毁/唯一写者/悬垂标记）归 M2.4/M2.5/M4.1 后续里程碑。

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::{Rc, Weak};
use std::sync::{Arc, Condvar, Mutex as StdMutex};

/// 运行时值
#[derive(Debug, Clone)]
pub enum Value {
    /// 统一整数（宽度检查 tag1 简化，后续 M2.2 补）
    Int(i128),
    Float(f64),
    Bool(bool),
    /// 字节串（String / &[u8] / 静态切片）
    Str(Rc<RefCell<Vec<u8>>>),
    /// 数组/集合（共享可变；元素为共享槽以支持 for 可写捕获与索引写回）
    Arr(Rc<RefCell<Vec<Rc<RefCell<Value>>>>>),
    /// 切片视图（带位置和长度的指针，H4 定案）：data[start..start+len]
    Slice {
        data: Rc<RefCell<Vec<Rc<RefCell<Value>>>>>,
        start: usize,
        len: usize,
    },
    /// class 实例
    Class(Rc<RefCell<ClassData>>),
    /// 枚举变体（负载可选）
    Enum {
        name: String,
        variant: String,
        payload: Option<Rc<Value>>,
    },
    /// 可选值
    Opt(Option<Rc<Value>>),
    /// 错误值（M4.2：码 + 名字——码 = M2.6 编译期错误码表「包 ID + 包内码」，
    /// 全局唯一；运行时未登记错误名动态分配）
    Err {
        name: String,
        code: u32,
    },
    /// 指针（共享槽）
    Ptr(Rc<RefCell<Value>>),
    /// 装箱/接口胖指针（G3：data + vtbl + alloc 三字宽，设计文档 §6 定案落地）。
    /// tag1：data = 被装箱值的共享槽（拥有）；vtbl = 具体类型名（真实接口虚表归编译期，
    /// tag1 方法分派鸭子类型——deref 即达 pointee）；alloc = 装箱时显式传入的分配器
    /// 引用（`box(v)` 未传回退全局 `alloc`）——销毁  `owned *I` 时用携带的 alloc 释放 data。
    Boxed(Rc<RefCell<BoxedData>>),
    /// 集合句柄（G4：Vec/Deque 持有分配器引用，设计文档 §7 定案落地）。
    /// tag1：items = Arr 同款共享槽存储（外部形态即数组），alloc = 构造 `init(alloc)`
    /// 时携带的分配器引用——扩容/子对象分配概念上走它（tag1 无真实 backing 分配）。
    /// 方法分派经 deref 剥为 `Value::Arr` 复用全部 Arr 方法。
    Vec(Rc<RefCell<VecData>>),
    /// Map 句柄（G4：持有分配器引用，设计文档 §7）。字段即键值；alloc 同 Vec。
    Map(Rc<RefCell<MapData>>),
    /// 函数引用（tag1：仅命名函数）
    Fn(String),
    /// 闭包（捕获环境 = 共享槽快照；tag1：捕获整个当前作用域链）
    Closure(ClosureData),
    /// 分配器句柄（tag1：无状态哨兵；Phase 1 向后兼容，Phase 3 移除）
    Alloc,
    /// Arena 分配器句柄（G1：真实 bump + 块链表；deinit 批量归还 backing；Phase 1 向后兼容，Phase 3 移除）
    Arena(Rc<RefCell<ArenaState>>),
    /// 统一分配器接口值（Phase 1 新增，替代 Value::Alloc / Value::Arena）
    Allocator(Rc<RefCell<AllocatorImpl>>),
    /// 原始内存块（Phase 1 新增；分配器返回的原始内存，与 Str 区分）
    Bytes(Rc<RefCell<Vec<u8>>>),
    /// 惰性迭代器（A7 惰性/组合子迭代器，2026-08-23）
    /// 包装一个可迭代源 + 位置 + 可选的 filter/map 变换。
    /// `next()` 按需求值，链式延迟计算。
    LazyIter(Rc<RefCell<LazyIterData>>),
    /// 互斥锁（E4：真 OS 并行——Mutex.init(v) 构造，.lock()/.try_lock() 访问）
    Mutex(Arc<StdMutex<Value>>),
    /// 通道（E4：M:N 协程通信——chan<T> 替代 Pipe/Tee/Funnel/Hub）
    Chan(Arc<ChanState>),
    /// 空值 / void
    Void,
    /// M2.5/M4.7 悬垂标记：目标已销毁（Debug 下指针访问抛错带位置）
    Dangling,
}

#[derive(Debug, Clone)]
pub struct ClassData {
    pub name: String,
    pub fields: HashMap<String, Value>,
}

/// 通道状态（M:N 协程通信）
#[derive(Debug)]
pub struct ChanState {
    pub inner: StdMutex<ChanInner>,
    pub send_cond: Condvar,
    pub recv_cond: Condvar,
    pub capacity: usize,
}

#[derive(Debug)]
pub struct ChanInner {
    pub queue: VecDeque<Value>,
    pub closed: bool,
}

/// 每个 Value 实例在任一时刻只被一个线程访问。spawn 时深复制值到新线程，原始线程和子线程操作各自副本，无数据竞争。
unsafe impl Send for ClassData {}
unsafe impl Send for Value {}

/// 闭包数据（运行时表示；AST 部分由解释器填充）
#[derive(Debug, Clone)]
/// 每个 Value 实例在任一时刻只被一个线程访问。spawn 时深复制值到新线程，原始线程和子线程操作各自副本，无数据竞争。
unsafe impl Send for ClosureData {}

pub struct ClosureData {
    pub params: Vec<String>,
    pub body: hc::ast::Block,
    pub is_mut: bool,
    pub is_move: bool,
    pub env: Vec<std::collections::HashMap<String, Rc<RefCell<Value>>>>,
}

/// Arena 默认块大小（首块及新块下限；单块申请大于此值时按实际大小开块）
pub const ARENA_BLOCK_SIZE: usize = 1024;

/// 惰性迭代器操作类型（filter/map 按链式调用顺序存储）
#[derive(Debug, Clone)]
/// 每个 Value 实例在任一时刻只被一个线程访问。spawn 时深复制值到新线程，原始线程和子线程操作各自副本，无数据竞争。
unsafe impl Send for LazyOp {}

pub enum LazyOp {
    /// 筛选闭包：返回 false 则跳过该元素
    Filter(Value),
    /// 变换闭包：变换元素值
    Map(Value),
}

/// 惰性迭代器数据（A7：`next()` 按需求值，filter/map 链式延迟计算）
/// 操作按链式调用顺序存储在 `ops` 中，`lazy_iter_next` 按序应用。
/// 例如 `arr.map(g).filter(f)` → ops = [Map(g), Filter(f)]，
/// 对每个源元素：先 Map(g) 变换，再 Filter(f) 筛选。
#[derive(Debug, Clone)]
/// 每个 Value 实例在任一时刻只被一个线程访问。spawn 时深复制值到新线程，原始线程和子线程操作各自副本，无数据竞争。
unsafe impl Send for LazyIterData {}

pub struct LazyIterData {
    /// 源数据（原始可迭代值：Arr/Slice/Str/Map/Vec/Class）
    pub source: Value,
    /// 当前位置（源的迭代索引）
    pub index: usize,
    /// 源类型名（"arr"/"slice"/"str"/"map"/"vec"/"class"）
    pub source_type: String,
    /// 操作列表（按链式调用顺序存储：filter/map 交错，按序应用）
    pub ops: Vec<LazyOp>,
    /// Map 遍历键缓存（非 Map 源时为空；构造时固定顺序保证确定性遍历）
    pub keys_cache: Vec<String>,
}

/// 分配器对齐下限（§2.3：H 值为 i128/f64 承载，对齐 ≥ 16 字节，与 tag1 `%Value` 盒一致）。
/// bump 游标按此圆整，返回区域起始相对块起点恒为 16 的倍数。
pub const ALLOC_ALIGN: usize = 16;

/// 对齐到 `ALLOC_ALIGN` 倍数（向上圆整；`align_up(x)` = `(x + A - 1) & !(A - 1)`）
fn align_up(x: usize, a: usize) -> usize {
    (x + a - 1) & !(a - 1)
}

/// Arena 分配器状态（G1：真实 bump + 块链表；`deinit` 批量归还 backing）
#[derive(Debug, Clone)]
pub struct ArenaState {
    /// 已提交块（真实 backing 内存；每块独立 Vec，bump 从当前块切，不足时申请新块）
    pub blocks: Vec<Rc<RefCell<Vec<u8>>>>,
    /// 当前块内游标（下一分配起点）
    pub cursor: usize,
    /// 累计分配字节（统计；`arena.bytes()`）
    pub total: usize,
    /// 可用标志（`deinit` 后 false → `alloc` 抛 `ArenaDeinitialized`）
    pub live: bool,
    /// G5/§8.3 Debug 泄漏检测：Arena 块分配登记表（`Arena.init(alloc)` 时从 Interp 共享；
    /// deinit 清 block 后弱引用失效自动视为释放；退出时仍存活者 = 泄漏）
    pub alloc_tracker: Option<Rc<RefCell<Vec<LeakRecord>>>>,
}

/// 每个 Value 实例在任一时刻只被一个线程访问。spawn 时深复制值到新线程，原始线程和子线程操作各自副本，无数据竞争。
unsafe impl Send for ArenaState {}

/// 装箱状态（G3：data + vtbl + alloc 三字宽胖指针；对齐设计文档 §6）
#[derive(Debug, Clone)]
pub struct BoxedData {
    /// data 字：被装箱值（拥有；deref/方法分派经它达 pointee）
    pub data: Rc<RefCell<Value>>,
    /// vtbl 字：具体类型名（tag1 编译期静态标注；真实接口虚表归编译期）
    pub vtbl: String,
    /// alloc 字：创建时携带的分配器引用（销毁  `owned *I` 时用它释放 data）
    pub alloc: Value,
}

/// 每个 Value 实例在任一时刻只被一个线程访问。spawn 时深复制值到新线程，原始线程和子线程操作各自副本，无数据竞争。
unsafe impl Send for BoxedData {}

/// 集合状态（G4：Vec/Deque 共用；对齐设计文档 §7）
#[derive(Debug, Clone)]
pub struct VecData {
    /// items：Arr 同款共享槽存储（方法分派经 deref 剥为 `Value::Arr` 共享此存储）
    pub items: Rc<RefCell<Vec<Rc<RefCell<Value>>>>>,
    /// alloc：构造 `Vec(T).init(alloc)` 时携带的分配器引用
    pub alloc: Value,
}

/// 每个 Value 实例在任一时刻只被一个线程访问。spawn 时深复制值到新线程，原始线程和子线程操作各自副本，无数据竞争。
unsafe impl Send for VecData {}

/// Map 状态（G4：对齐设计文档 §7；字段即键值）
#[derive(Debug, Clone)]
pub struct MapData {
    /// fields：键值存储（键 = 键的 display；与既有 `Class("Map")` 表示一致）
    pub fields: HashMap<String, Value>,
    /// alloc：构造 `Map(K,V).init(alloc)` 时携带的分配器引用
    pub alloc: Value,
}

/// 每个 Value 实例在任一时刻只被一个线程访问。spawn 时深复制值到新线程，原始线程和子线程操作各自副本，无数据竞争。
unsafe impl Send for MapData {}

/// 全局分配器 Debug 泄漏登记（§8.3：分配记录表；`weak` 持分配数据的弱引用——
/// 值被销毁（作用域退出自动销毁）后升级失败，即视为已释放。退出时仍可升级者 = 泄漏）。
#[derive(Debug)]
pub struct LeakRecord {
    /// 分配大小（字节）
    pub size: usize,
    /// 分配点行号（调用 `alloc.alloc(n)` 处；IR 侧无行号 → 0）
    pub line: u32,
    /// 分配数据弱引用（存活判定）
    pub weak: Weak<RefCell<Vec<u8>>>,
}

/// bump 分配失败原因（调用方映射为 H 可处理错误）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArenaAllocErr {
    /// arena 已 deinit，不可再分配
    Deinit,
    /// backing 分配失败 / 超出可表示容量
    Oom,
}

impl ArenaState {
    pub fn new() -> Self {
        Self {
            blocks: vec![],
            cursor: 0,
            total: 0,
            live: true,
            alloc_tracker: None,
        }
    }

    /// bump 分配 `n` 字节零初始化内存：当前块剩余空间足够则切出，
    /// 不足则向 backing 申请新块（大小 = `max(ARENA_BLOCK_SIZE, n)`）。
    /// 返回（块引用, 块内偏移）——调用方按 `[off..off+n]` 读出区域。
    ///
    /// **对齐（G5/§2.3）**：切出前把游标圆整到 `ALLOC_ALIGN`（16）的倍数，保证
    /// 返回区域起始相对块起点 16 对齐；对齐填充计入 `total`（真实 bump 语义——
    /// 分配器消耗对齐后的空间）。新块游标从 0 起，起点天然对齐。
    pub fn bump(&mut self, n: usize) -> Result<(Rc<RefCell<Vec<u8>>>, usize), ArenaAllocErr> {
        if !self.live {
            return Err(ArenaAllocErr::Deinit);
        }
        let aligned = align_up(self.cursor, ALLOC_ALIGN);
        let need_new =
            self.blocks.is_empty() || aligned + n > self.blocks.last().unwrap().borrow().len();
        if need_new {
            let size = n.max(ARENA_BLOCK_SIZE);
            let mut block = Vec::new();
            // 优雅失败（`vec![0u8; size]` 对超大 size 会中止进程）
            block
                .try_reserve_exact(size)
                .map_err(|_| ArenaAllocErr::Oom)?;
            block.resize(size, 0u8);
            let block_rc = Rc::new(RefCell::new(block));
            // G5/§8.3 Debug 泄漏检测：登记 Arena 块分配（弱引用随 deinit 失效）
            if let Some(ref tracker) = self.alloc_tracker {
                tracker.borrow_mut().push(LeakRecord {
                    size: block_rc.borrow().len(),
                    line: 0,
                    weak: Rc::downgrade(&block_rc),
                });
            }
            self.blocks.push(block_rc);
            self.cursor = 0;
        }
        let block = self.blocks.last().unwrap().clone();
        let off = align_up(self.cursor, ALLOC_ALIGN);
        self.total += off + n - self.cursor;
        self.cursor = off + n;
        Ok((block, off))
    }

    /// deinit：清空全部块（归还 backing）、重置统计、标记不可用
    pub fn deinit(&mut self) {
        self.blocks.clear();
        self.cursor = 0;
        self.total = 0;
        self.live = false;
    }
}

/// 分配器返回的内存块
#[derive(Debug, Clone)]
pub struct AllocBlock {
    pub data: Rc<RefCell<Vec<u8>>>,
    pub offset: usize,
    pub len: usize,
}

/// 分配失败错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocErr {
    OutOfMemory,
    InvalidSize,
}

/// 自定义分配器接口（Rust 侧实现，供 Custom 变体使用）
pub trait AllocatorTrait {
    fn alloc(&mut self, n: usize) -> Result<AllocBlock, AllocErr>;
    fn free(&mut self, block: &AllocBlock);
    fn realloc(&mut self, block: &AllocBlock, n: usize) -> Result<AllocBlock, AllocErr> {
        // 默认实现：alloc + copy + free（对不支持 realloc 的后端兜底）
        let new_block = self.alloc(n)?;
        let copy_len = block.len.min(n);
        copy_alloc_block(block, &new_block, copy_len);
        self.free(block);
        Ok(new_block)
    }
    fn deinit(&mut self) {}
    /// 克隆自身（用于 AllocatorImpl::Clone）
    fn clone_box(&self) -> Box<dyn AllocatorTrait>;
}

/// 复制源数据到目标 AllocBlock（realloc 辅助，避免同时借用两个 RefCell）
fn copy_alloc_block(src: &AllocBlock, dst: &AllocBlock, len: usize) {
    let src_data = {
        let s = src.data.borrow();
        s[src.offset..src.offset + len].to_vec()
    };
    let mut d = dst.data.borrow_mut();
    d[dst.offset..dst.offset + len].copy_from_slice(&src_data);
}

/// 固定大小对象池状态（空闲链表 + 后备分配器）
pub struct PoolState {
    pub item_size: usize,
    pub free_list: Vec<AllocBlock>,
    pub backing: Box<AllocatorImpl>,
}

/// 每个 Value 实例在任一时刻只被一个线程访问。spawn 时深复制值到新线程，原始线程和子线程操作各自副本，无数据竞争。
unsafe impl Send for PoolState {}

impl PoolState {
    pub fn new(backing: AllocatorImpl, item_size: usize) -> Self {
        Self {
            item_size,
            free_list: Vec::new(),
            backing: Box::new(backing),
        }
    }
}

/// 分配器实现枚举（Phase 1：统一分配器接口，四变体）
pub enum AllocatorImpl {
    /// 无状态全局分配器（每 alloc 创建独立 Vec）
    Page,
    /// Arena bump 分配器（复用现有 ArenaState）
    Arena(Rc<RefCell<ArenaState>>),
    /// 固定大小对象池（空闲链表复用 + 后备分配器）
    Pool(Rc<RefCell<PoolState>>),
    /// 自定义分配器（Rust 侧实现，后续开放 H 侧）
    Custom(Box<dyn AllocatorTrait>),
}

/// 每个 Value 实例在任一时刻只被一个线程访问。spawn 时深复制值到新线程，原始线程和子线程操作各自副本，无数据竞争。
unsafe impl Send for AllocatorImpl {}

impl std::fmt::Debug for AllocatorImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Page => write!(f, "PageAllocator"),
            Self::Arena(a) => {
                let d = a.borrow();
                write!(
                    f,
                    "ArenaAllocator(bytes={}, blocks={})",
                    d.total,
                    d.blocks.len()
                )
            }
            Self::Pool(p) => {
                let d = p.borrow();
                write!(
                    f,
                    "PoolAllocator(item_size={}, free={})",
                    d.item_size,
                    d.free_list.len()
                )
            }
            Self::Custom(_) => write!(f, "CustomAllocator(...)"),
        }
    }
}

impl Clone for AllocatorImpl {
    fn clone(&self) -> Self {
        match self {
            Self::Page => Self::Page,
            Self::Arena(a) => Self::Arena(a.clone()),
            Self::Pool(p) => {
                let d = p.borrow();
                Self::Pool(Rc::new(RefCell::new(PoolState {
                    item_size: d.item_size,
                    free_list: d.free_list.clone(),
                    backing: d.backing.clone(),
                })))
            }
            Self::Custom(c) => Self::Custom(c.clone_box()),
        }
    }
}

impl AllocatorImpl {
    /// 分配 n 字节零初始化内存
    pub fn alloc(&mut self, n: usize) -> Result<AllocBlock, AllocErr> {
        match self {
            Self::Page => {
                if n == 0 {
                    return Ok(AllocBlock {
                        data: Rc::new(RefCell::new(Vec::new())),
                        offset: 0,
                        len: 0,
                    });
                }
                let mut v = Vec::new();
                v.try_reserve_exact(n).map_err(|_| AllocErr::OutOfMemory)?;
                v.resize(n, 0u8);
                Ok(AllocBlock {
                    data: Rc::new(RefCell::new(v)),
                    offset: 0,
                    len: n,
                })
            }
            Self::Arena(arena) => {
                let mut a = arena.borrow_mut();
                let (data, offset) = a.bump(n).map_err(|e| match e {
                    ArenaAllocErr::Deinit => AllocErr::InvalidSize,
                    ArenaAllocErr::Oom => AllocErr::OutOfMemory,
                })?;
                Ok(AllocBlock {
                    data,
                    offset,
                    len: n,
                })
            }
            Self::Pool(pool) => {
                let mut p = pool.borrow_mut();
                if n > p.item_size {
                    return Err(AllocErr::InvalidSize);
                }
                // 先从空闲链表取，没有再分配
                if let Some(block) = p.free_list.pop() {
                    Ok(block)
                } else {
                    let item_size = p.item_size;
                    p.backing.alloc(item_size)
                }
            }
            Self::Custom(impl_) => impl_.alloc(n),
        }
    }

    /// 释放内存块
    pub fn free(&mut self, block: &AllocBlock) {
        match self {
            Self::Page => {
                // Page 分配器：Rc 引用归零时自动释放，此处空操作
            }
            Self::Arena(_) => {
                // Arena：不逐对象 free，deinit 统一释放
            }
            Self::Pool(pool) => {
                let mut p = pool.borrow_mut();
                let item_size = p.item_size;
                // 重置偏移并放回空闲链表复用
                p.free_list.push(AllocBlock {
                    data: block.data.clone(),
                    offset: 0,
                    len: item_size,
                });
            }
            Self::Custom(impl_) => impl_.free(block),
        }
    }

    /// 调整内存块大小
    pub fn realloc(&mut self, block: &AllocBlock, n: usize) -> Result<AllocBlock, AllocErr> {
        match self {
            Self::Page => {
                if n == 0 {
                    return Ok(AllocBlock {
                        data: Rc::new(RefCell::new(Vec::new())),
                        offset: 0,
                        len: 0,
                    });
                }
                let mut v = block.data.borrow_mut();
                let old_len = v.len();
                if n <= old_len {
                    v.truncate(n);
                    return Ok(AllocBlock {
                        data: block.data.clone(),
                        offset: 0,
                        len: n,
                    });
                }
                v.try_reserve_exact(n - old_len)
                    .map_err(|_| AllocErr::OutOfMemory)?;
                v.resize(n, 0u8);
                Ok(AllocBlock {
                    data: block.data.clone(),
                    offset: 0,
                    len: n,
                })
            }
            Self::Arena(_) => {
                // Arena 不支持 realloc，走 alloc + copy + free
                let new_block = self.alloc(n)?;
                let copy_len = block.len.min(n);
                copy_alloc_block(block, &new_block, copy_len);
                self.free(block);
                Ok(new_block)
            }
            Self::Pool(pool) => {
                let p = pool.borrow_mut();
                if n > p.item_size {
                    return Err(AllocErr::InvalidSize);
                }
                let item_size = p.item_size;
                // 相同大小直接返回原块
                Ok(AllocBlock {
                    data: block.data.clone(),
                    offset: 0,
                    len: item_size,
                })
            }
            Self::Custom(impl_) => impl_.realloc(block, n),
        }
    }

    /// 释放分配器持有的资源
    pub fn deinit(&mut self) {
        match self {
            Self::Page => {}
            Self::Arena(arena) => {
                arena.borrow_mut().deinit();
            }
            Self::Pool(pool) => {
                let mut p = pool.borrow_mut();
                // 归还空闲链表所有块到后备分配器
                let blocks: Vec<AllocBlock> = p.free_list.drain(..).collect();
                for block in &blocks {
                    p.backing.free(block);
                }
                p.backing.deinit();
            }
            Self::Custom(impl_) => impl_.deinit(),
        }
    }
}

impl Value {
    pub fn int(v: i128) -> Value {
        Value::Int(v)
    }
    pub fn bool(v: bool) -> Value {
        Value::Bool(v)
    }
    pub fn str_bytes(b: Vec<u8>) -> Value {
        Value::Str(Rc::new(RefCell::new(b)))
    }
    pub fn str(s: &str) -> Value {
        Value::str_bytes(s.as_bytes().to_vec())
    }
    pub fn arr(items: Vec<Value>) -> Value {
        let items = items
            .into_iter()
            .map(|v| Rc::new(RefCell::new(v)))
            .collect();
        Value::Arr(Rc::new(RefCell::new(items)))
    }
    /// 集合（G4）：携带分配器引用的 Vec/Deque 句柄
    pub fn vec(items: Vec<Value>, alloc: Value) -> Value {
        let items = items
            .into_iter()
            .map(|v| Rc::new(RefCell::new(v)))
            .collect();
        Value::Vec(Rc::new(RefCell::new(VecData {
            items: Rc::new(RefCell::new(items)),
            alloc,
        })))
    }
    /// 集合（G4）：携带分配器引用的 Map 句柄
    pub fn map(fields: HashMap<String, Value>, alloc: Value) -> Value {
        Value::Map(Rc::new(RefCell::new(MapData { fields, alloc })))
    }
    pub fn class(name: &str, fields: HashMap<String, Value>) -> Value {
        Value::Class(Rc::new(RefCell::new(ClassData {
            name: name.to_string(),
            fields,
        })))
    }

    /// 显示（io.print `{}`）
    pub fn display(&self) -> String {
        match self {
            Value::Int(i) => i.to_string(),
            Value::Float(f) => {
                if f.fract() == 0.0 && f.is_finite() && f.abs() < 1e15 {
                    format!("{f:.1}")
                } else {
                    f.to_string()
                }
            }
            Value::Bool(b) => b.to_string(),
            Value::Str(s) => String::from_utf8_lossy(&s.borrow()).to_string(),
            Value::Arr(a) => {
                let items: Vec<String> = a.borrow().iter().map(|v| v.borrow().display()).collect();
                format!("[{}]", items.join(", "))
            }
            Value::Slice { data, start, len } => {
                let d = data.borrow();
                let items: Vec<String> = d[*start..*start + *len]
                    .iter()
                    .map(|v| v.borrow().display())
                    .collect();
                format!("[{}]", items.join(", "))
            }
            Value::Class(c) => {
                let d = c.borrow();
                let items: Vec<String> = d
                    .fields
                    .iter()
                    .map(|(k, v)| format!("{k} = {}", v.display()))
                    .collect();
                format!("{} {{ {} }}", d.name, items.join(", "))
            }
            Value::Enum {
                name,
                variant,
                payload,
            } => match payload {
                Some(p) => format!("{name}.{variant} = {}", p.display()),
                None => format!("{name}.{variant}"),
            },
            Value::Opt(Some(v)) => format!("?{}", v.display()),
            Value::Opt(None) => "null".to_string(),
            Value::Err { name, .. } => format!("error.{name}"),
            Value::Ptr(p) => p.borrow().display(),
            Value::Boxed(b) => b.borrow().data.borrow().display(),
            Value::Vec(v) => {
                let d = v.borrow();
                let items: Vec<String> = d
                    .items
                    .borrow()
                    .iter()
                    .map(|c| c.borrow().display())
                    .collect();
                format!("[{}]", items.join(", "))
            }
            Value::Map(m) => {
                let d = m.borrow();
                let items: Vec<String> = d
                    .fields
                    .iter()
                    .map(|(k, v)| format!("{k} = {}", v.display()))
                    .collect();
                format!("Map {{ {} }}", items.join(", "))
            }
            Value::Fn(f) => format!("fn {f}"),
            Value::Closure(_) => "closure".to_string(),
            Value::Alloc => "alloc".to_string(),
            Value::Arena(a) => {
                let d = a.borrow();
                format!("Arena(bytes={}, blocks={})", d.total, d.blocks.len())
            }
            Value::Allocator(a) => match &*a.borrow() {
                AllocatorImpl::Page => "allocator(page)".to_string(),
                AllocatorImpl::Arena(ar) => {
                    let d = ar.borrow();
                    format!(
                        "allocator(Arena(bytes={}, blocks={}))",
                        d.total,
                        d.blocks.len()
                    )
                }
                AllocatorImpl::Pool(p) => {
                    let d = p.borrow();
                    format!(
                        "allocator(Pool(item_size={}, free={}))",
                        d.item_size,
                        d.free_list.len()
                    )
                }
                AllocatorImpl::Custom(_) => "allocator(custom)".to_string(),
            },
            Value::Bytes(b) => {
                let d = b.borrow();
                format!("Bytes({} bytes)", d.len())
            }
            Value::LazyIter(li) => {
                let d = li.borrow();
                format!(
                    "LazyIter({} @{})({} ops)",
                    d.source_type,
                    d.index,
                    d.ops.len(),
                )
            }
            Value::Mutex(m) => match m.lock() {
                Ok(v) => format!("Mutex({})", v.display()),
                Err(_) => "Mutex(<poisoned>)".to_string(),
            },
            Value::Chan(ch) => format!(
                "Chan({}/{})",
                ch.inner.lock().unwrap().queue.len(),
                ch.capacity
            ),
            Value::Void => "void".to_string(),
            Value::Dangling => "<dangling>".to_string(),
        }
    }

    /// 深比较（== 值比较，H3 定案：内部调用 ICompare；tag1 直接按值）
    pub fn value_eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Int(a), Value::Float(b)) => *a as f64 == *b,
            (Value::Float(a), Value::Int(b)) => *a == *b as f64,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => *a.borrow() == *b.borrow(),
            (Value::Arr(a), Value::Arr(b)) => {
                let (a, b) = (a.borrow(), b.borrow());
                a.len() == b.len()
                    && a.iter()
                        .zip(b.iter())
                        .all(|(x, y)| x.borrow().value_eq(&y.borrow()))
            }
            (
                Value::Slice {
                    data: da,
                    start: sa,
                    len: la,
                },
                Value::Slice {
                    data: db,
                    start: sb,
                    len: lb,
                },
            ) => {
                if la != lb {
                    return false;
                }
                let (da, db) = (da.borrow(), db.borrow());
                (0..*la).all(|i| da[*sa + i].borrow().value_eq(&db[*sb + i].borrow()))
            }
            (Value::Slice { data, start, len }, Value::Arr(b)) => {
                let d = data.borrow();
                let b = b.borrow();
                *len == b.len()
                    && (0..*len).all(|i| d[*start + i].borrow().value_eq(&b[i].borrow()))
            }
            (Value::Arr(a), Value::Slice { data, start, len }) => {
                let d = data.borrow();
                let a = a.borrow();
                a.len() == *len
                    && (0..*len).all(|i| a[i].borrow().value_eq(&d[*start + i].borrow()))
            }
            (Value::Class(a), Value::Class(b)) => {
                let (a, b) = (a.borrow(), b.borrow());
                if a.name != b.name || a.fields.len() != b.fields.len() {
                    return false;
                }
                a.fields
                    .iter()
                    .all(|(k, v)| b.fields.get(k).map_or(false, |w| v.value_eq(w)))
            }
            (
                Value::Enum {
                    name: an,
                    variant: av,
                    payload: ap,
                },
                Value::Enum {
                    name: bn,
                    variant: bv,
                    payload: bp,
                },
            ) => {
                an == bn
                    && av == bv
                    && match (ap, bp) {
                        (Some(x), Some(y)) => x.value_eq(y),
                        (None, None) => true,
                        _ => false,
                    }
            }
            (Value::Opt(a), Value::Opt(b)) => match (a, b) {
                (Some(x), Some(y)) => x.value_eq(y),
                (None, None) => true,
                _ => false,
            },
            (Value::Err { code: a, .. }, Value::Err { code: b, .. }) => a == b,
            (Value::Arena(a), Value::Arena(b)) => Rc::ptr_eq(a, b),
            (Value::Allocator(a), Value::Allocator(b)) => Rc::ptr_eq(a, b),
            (Value::Bytes(a), Value::Bytes(b)) => *a.borrow() == *b.borrow(),
            (Value::Ptr(a), Value::Ptr(b)) => Rc::ptr_eq(a, b),
            (Value::Ptr(a), b) => a.borrow().value_eq(b),
            (a, Value::Ptr(b)) => a.value_eq(&b.borrow()),
            // 装箱胖指针：身份同 cell；与普通值比较时解引用后比较（对齐 Ptr 语义）
            (Value::Boxed(a), Value::Boxed(b)) => Rc::ptr_eq(a, b),
            (Value::Boxed(a), b) => a.borrow().data.borrow().value_eq(b),
            (a, Value::Boxed(b)) => a.value_eq(&b.borrow().data.borrow()),
            // 集合（G4）：剥为共享 Arr 后按内容比较（Arr/Slice/Vec 三者互通）
            (Value::Vec(a), b) => Value::Arr(a.borrow().items.clone()).value_eq(b),
            (a, Value::Vec(b)) => a.value_eq(&Value::Arr(b.borrow().items.clone())),
            (Value::Map(a), Value::Map(b)) => {
                let (a, b) = (a.borrow(), b.borrow());
                if a.fields.len() != b.fields.len() {
                    return false;
                }
                a.fields
                    .iter()
                    .all(|(k, v)| b.fields.get(k).map_or(false, |w| v.value_eq(w)))
            }
            (Value::Map(a), Value::Class(b)) if b.borrow().name == "Map" => {
                let (a, b) = (a.borrow(), b.borrow());
                if a.fields.len() != b.fields.len() {
                    return false;
                }
                a.fields
                    .iter()
                    .all(|(k, v)| b.fields.get(k).map_or(false, |w| v.value_eq(w)))
            }
            (Value::Class(a), Value::Map(b)) if a.borrow().name == "Map" => {
                let (a, b) = (a.borrow(), b.borrow());
                if a.fields.len() != b.fields.len() {
                    return false;
                }
                a.fields
                    .iter()
                    .all(|(k, v)| b.fields.get(k).map_or(false, |w| v.value_eq(w)))
            }
            (Value::Void, Value::Void) => true,
            (Value::Mutex(a), Value::Mutex(b)) => match (a.lock(), b.lock()) {
                (Ok(av), Ok(bv)) => av.value_eq(&bv),
                _ => false,
            },
            (Value::Chan(a), Value::Chan(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// 序比较（ICompare；tag1 仅数值/字符串/布尔）
    pub fn value_lt(&self, other: &Value) -> Option<bool> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Some(a < b),
            (Value::Int(a), Value::Float(b)) => Some((*a as f64) < *b),
            (Value::Float(a), Value::Int(b)) => Some(*a < *b as f64),
            (Value::Float(a), Value::Float(b)) => Some(a < b),
            (Value::Str(a), Value::Str(b)) => Some(*a.borrow() < *b.borrow()),
            (Value::Bool(a), Value::Bool(b)) => Some(a < b),
            (Value::Ptr(a), Value::Ptr(b)) => Some(Rc::as_ptr(a) < Rc::as_ptr(b)),
            (Value::Allocator(a), Value::Allocator(b)) => Some(Rc::as_ptr(a) < Rc::as_ptr(b)),
            (Value::Bytes(a), Value::Bytes(b)) => Some(*a.borrow() < *b.borrow()),
            _ => None,
        }
    }

    /// 转为 bool（条件上下文）
    pub fn as_bool(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::Opt(Some(v)) => v.as_bool(),
            Value::Ptr(_) => true,
            Value::Boxed(_) => true,
            Value::Vec(_) => true,
            Value::Map(_) => true,
            Value::Str(s) => !s.borrow().is_empty(),
            Value::Bytes(b) => !b.borrow().is_empty(),
            Value::Allocator(_) => true,
            _ => true,
        }
    }

    pub fn type_name(&self) -> String {
        match self {
            Value::Int(_) => "i128".into(),
            Value::Float(_) => "f64".into(),
            Value::Bool(_) => "bool".into(),
            Value::Str(_) => "&[u8]".into(),
            Value::Arr(_) => "array".into(),
            Value::Slice { .. } => "slice".into(),
            Value::Class(c) => c.borrow().name.clone(),
            Value::Enum { name, .. } => name.clone(),
            Value::Opt(_) => "optional".into(),
            Value::Err { .. } => "error".into(),
            Value::Ptr(_) => "pointer".into(),
            Value::Boxed(_) => "pointer".into(),
            Value::Vec(_) => "array".into(),
            Value::Map(_) => "Map".into(),
            Value::Fn(_) => "fn".into(),
            Value::Closure(_) => "closure".into(),
            Value::Alloc => "alloc".into(),
            Value::Arena(_) => "Arena".into(),
            Value::Allocator(_) => "allocator".into(),
            Value::Bytes(_) => "Bytes".into(),
            Value::LazyIter(_) => "LazyIter".into(),
            Value::Mutex(_) => "Mutex".into(),
            Value::Chan(_) => "Chan".into(),
            Value::Void => "void".into(),
            Value::Dangling => "dangling".into(),
        }
    }
}
