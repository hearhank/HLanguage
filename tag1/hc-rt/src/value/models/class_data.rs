use std::collections::HashMap;

use super::value::Value;

#[derive(Debug, Clone)]
pub struct ClassData {
    pub name: String,
    pub fields: HashMap<String, Value>,
}

/// # Safety
/// 每个 Value 实例在任一时刻只被一个线程访问。spawn 时深复制值到新线程，
/// 原始线程和子线程操作各自副本，无数据竞争。
unsafe impl Send for ClassData {}
