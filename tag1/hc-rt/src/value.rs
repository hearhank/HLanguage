//! Value 值模型：H 语言运行时值类型系统
//!
//! 定义：枚举：Value, LazyOp, ArenaAllocErr, AllocErr, AllocatorImpl
//! 定义：结构体：ClassData, ChanState, ChanInner, ClosureData, LazyIterData, ArenaState, BoxedData, VecData, MapData, LeakRecord, AllocBlock, PoolState
//!
//! ADR-0028 拆分：类型定义见 `models/`（一类型一文件）；函数按功能拆分——
//! 构造入口保留在本文件，显示 `display`、比较 `compare`、类型查询 `query`、字节提取 `bytes`。

mod bytes;
mod compare;
mod display;
mod models;
mod query;

pub use models::*;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::StringData;

impl Value {
    pub fn int(v: i128) -> Value {
        Value::Int(v)
    }
    pub fn bool(v: bool) -> Value {
        Value::Bool(v)
    }
    pub fn str_bytes(b: Vec<u8>) -> Value {
        Value::String(StringData::from_bytes(b))
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
}
