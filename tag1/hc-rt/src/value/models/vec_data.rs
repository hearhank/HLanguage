use std::cell::RefCell;
use std::rc::Rc;

use super::value::Value;

/// 集合状态（G4：Vec/Deque 共用；对齐设计文档 §7）
#[derive(Debug, Clone)]
pub struct VecData {
    /// items：Arr 同款共享槽存储（方法分派经 deref 剥为 `Value::Arr` 共享此存储）
    pub items: Rc<RefCell<Vec<Rc<RefCell<Value>>>>>,
    /// alloc：构造 `Vec(T).init(alloc)` 时携带的分配器引用
    pub alloc: Value,
}

unsafe impl Send for VecData {}
