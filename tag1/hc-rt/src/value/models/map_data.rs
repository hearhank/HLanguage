use std::collections::HashMap;

use super::value::Value;

/// Map 状态（G4：对齐设计文档 §7；字段即键值）
#[derive(Debug, Clone)]
pub struct MapData {
    /// fields：键值存储（键 = 键的 display；与既有 `Class("Map")` 表示一致）
    pub fields: HashMap<String, Value>,
    /// alloc：构造 `Map(K,V).init(alloc)` 时携带的分配器引用
    pub alloc: Value,
}

unsafe impl Send for MapData {}
