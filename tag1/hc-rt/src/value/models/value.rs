use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex as StdMutex};

use crate::StringData;

use super::allocator_impl::AllocatorImpl;
use super::arena_state::ArenaState;
use super::boxed_data::BoxedData;
use super::chan_state::ChanState;
use super::class_data::ClassData;
use super::closure_data::ClosureData;
use super::context_state::ContextState;
use super::lazy_iter_data::LazyIterData;
use super::map_data::MapData;
use super::vec_data::VecData;

/// 运行时值
#[derive(Debug, Clone)]
pub enum Value {
    /// 统一整数（宽度检查 tag1 简化，后续 M2.2 补）
    Int(i128),
    Float(f64),
    Bool(bool),
    /// String 值类型（拥有所有权的字节数组，值语义，克隆即深拷贝）
    String(StringData),
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
    /// IoC 容器上下文（ADR-0026：AppContext / 模块 Context，背靠 Arena）
    Context(Rc<RefCell<ContextState>>),
    /// 空值 / void
    Void,
    /// M2.5/M4.7 悬垂标记：目标已销毁（Debug 下指针访问抛错带位置）
    Dangling,
}

/// # Safety
/// 每个 Value 实例在任一时刻只被一个线程访问。spawn 时深复制值到新线程，
/// 原始线程和子线程操作各自副本，无数据竞争。
unsafe impl Send for Value {}
