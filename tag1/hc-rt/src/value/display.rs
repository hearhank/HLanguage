use super::models::{AllocatorImpl, Value};

impl Value {
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
            Value::String(s) => String::from_utf8_lossy(s.as_slice()).to_string(),
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
                String::from_utf8_lossy(&d).to_string()
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
            Value::Context(_) => "Context".to_string(),
            Value::Void => "void".to_string(),
            Value::Dangling => "<dangling>".to_string(),
        }
    }
}
