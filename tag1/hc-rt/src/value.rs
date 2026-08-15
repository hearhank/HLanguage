//! 运行时值模型（M4 运行时与语言内建——tag1 子集）
//!
//! tag1 采用引用计数值模型：变量槽 = `Rc<RefCell<Value>>`，指针 = 槽的共享引用。
//! 完整所有权（作用域销毁/唯一写者/悬垂标记）归 M2.4/M2.5/M4.1 后续里程碑。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

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
    /// 函数引用（tag1：仅命名函数）
    Fn(String),
    /// 闭包（捕获环境 = 共享槽快照；tag1：捕获整个当前作用域链）
    Closure(ClosureData),
    /// 分配器句柄（tag1：无状态哨兵）
    Alloc,
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

/// 闭包数据（运行时表示；AST 部分由解释器填充）
#[derive(Debug, Clone)]
pub struct ClosureData {
    pub params: Vec<String>,
    pub body: hc::ast::Block,
    pub is_mut: bool,
    pub is_move: bool,
    pub env: Vec<std::collections::HashMap<String, Rc<RefCell<Value>>>>,
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
            Value::Fn(f) => format!("fn {f}"),
            Value::Closure(_) => "closure".to_string(),
            Value::Alloc => "alloc".to_string(),
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
            (Value::Ptr(a), Value::Ptr(b)) => Rc::ptr_eq(a, b),
            (Value::Ptr(a), b) => a.borrow().value_eq(b),
            (a, Value::Ptr(b)) => a.value_eq(&b.borrow()),
            (Value::Void, Value::Void) => true,
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
            Value::Str(s) => !s.borrow().is_empty(),
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
            Value::Fn(_) => "fn".into(),
            Value::Closure(_) => "closure".into(),
            Value::Alloc => "alloc".into(),
            Value::Void => "void".into(),
            Value::Dangling => "dangling".into(),
        }
    }
}
