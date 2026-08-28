use super::models::Value;

impl Value {
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
            Value::String(s) => !s.is_empty(),
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
            Value::String(_) => "String".into(),
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
            Value::Context(_) => "Context".into(),
            Value::Void => "void".into(),
            Value::Dangling => "dangling".into(),
        }
    }
}
