use std::cell::RefCell;
use std::rc::Rc;

use super::value::Value;

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

unsafe impl Send for BoxedData {}
