use std::rc::Rc;
use std::sync::Arc;

use super::models::Value;

impl Value {
    /// 深比较（== 值比较，H3 定案：内部调用 ICompare；tag1 直接按值）
    pub fn value_eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Int(a), Value::Float(b)) => *a as f64 == *b,
            (Value::Float(a), Value::Int(b)) => *a == *b as f64,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
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
            (Value::Bytes(a), Value::String(b)) => *a.borrow() == b.as_slice(),
            (Value::String(a), Value::Bytes(b)) => a.as_slice() == *b.borrow(),
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
            (Value::Context(a), Value::Context(b)) => Rc::ptr_eq(a, b),
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
            (Value::String(a), Value::String(b)) => Some(a.as_slice() < b.as_slice()),
            (Value::Bool(a), Value::Bool(b)) => Some(a < b),
            (Value::Ptr(a), Value::Ptr(b)) => Some(Rc::as_ptr(a) < Rc::as_ptr(b)),
            (Value::Allocator(a), Value::Allocator(b)) => Some(Rc::as_ptr(a) < Rc::as_ptr(b)),
            (Value::Bytes(a), Value::Bytes(b)) => Some(*a.borrow() < *b.borrow()),
            _ => None,
        }
    }
}
