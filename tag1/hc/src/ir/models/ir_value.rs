//! IR 值模型（ADR-0028：自 ir/mod.rs 拆分）

use super::*;

#[derive(Debug, Clone)]
pub enum IrValue {
    Int(i128),
    Float(f64),
    Bool(bool),
    /// String 值类型：堆分配的字节数组
    String(StringDataIr),
    /// 可选值（`null` = `Opt(None)`，对齐 tree-walking `Value::Opt`）
    Opt(Option<Box<IrValue>>),
    /// 错误值（M4.2：码 + 名字；码 = M2.6 编译期错误码表，全局唯一）
    Err {
        name: String,
        code: u32,
    },
    /// 指针：共享堆 cell 索引（别名装置——对齐 tree-walking `Value::Ptr(Rc<RefCell>)`）
    Ptr(usize),
    /// 装箱/接口胖指针（G3：三字宽 data + vtbl + alloc；指向 `Cell::Boxed`）
    Boxed(usize),
    /// 数组：`Cell::Elems` 的 cell 索引（元素为共享 cell——切片/写索引别名）
    Arr(usize),
    /// 集合 Vec（G4：持分配器引用的集合；指向 `Cell::Vec`。deref peel 到 Arr）
    Vec(usize),
    /// 集合 Map（G4：持分配器引用的 Map；指向 `Cell::Map`）
    Map(usize),
    /// 切片视图：共享底层 `Cell::Elems` + 窗口；`data` 为数组 cell 索引
    Slice {
        data: usize,
        start: usize,
        len: usize,
    },
    /// 类实例：`Cell::Class` 的 cell 索引（字段为普通值——无字段级别名）
    Class(usize),
    /// Arena 分配器句柄（G1：真实 bump + 块链表；指向 `Cell::Arena`）
    Arena(usize),
    /// 互斥锁（E4：真 OS 并行——Mutex.init(v) 构造，.lock()/.try_lock() 访问）
    Mutex(Arc<std::sync::Mutex<IrValue>>),
    /// 通道（E4：M:N 协程通信——chan<T>）
    Chan(Arc<ChanStateIr>),
    /// 枚举值（`Type.variant` 常量 或 `Type{variant = payload}`）
    Enum {
        name: String,
        variant: String,
        payload: Option<Box<IrValue>>,
    },
    /// 开区间切片 `arr[a..]` 的上界哨兵
    End,
    /// 迭代器值（Phase 3）：指向 `Cell::Iter` 的 cell 索引
    Iter(usize),
    /// 函数引用（Phase 4）：名字在调用点经 `pick_func` 按 arity/类型分派
    Fn(String),
    /// 闭包值（Phase 4）：func = [`IrModule::closures`] 索引；
    /// captures = 捕获变量 cell 索引（共享读/mut → 原 cell；move → 深拷贝新 cell）。
    /// 别名语义：闭包帧捕获参数槽直接绑定 captures[i] cell → 写穿对齐 oracle Rc<RefCell>。
    Closure {
        func: usize,
        captures: Vec<usize>,
        is_mut: bool,
    },
    Void,
}

impl PartialEq for IrValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (IrValue::Int(a), IrValue::Int(b)) => a == b,
            (IrValue::Float(a), IrValue::Float(b)) => a == b,
            (IrValue::Bool(a), IrValue::Bool(b)) => a == b,
            (IrValue::String(a), IrValue::String(b)) => a == b,
            (IrValue::Opt(a), IrValue::Opt(b)) => a == b,
            (IrValue::Err { name: an, code: ac }, IrValue::Err { name: bn, code: bc }) => {
                an == bn && ac == bc
            }
            (IrValue::Ptr(a), IrValue::Ptr(b)) => a == b,
            (IrValue::Boxed(a), IrValue::Boxed(b)) => a == b,
            (IrValue::Arr(a), IrValue::Arr(b)) => a == b,
            (IrValue::Vec(a), IrValue::Vec(b)) => a == b,
            (IrValue::Map(a), IrValue::Map(b)) => a == b,
            (
                IrValue::Slice {
                    data: da,
                    start: sa,
                    len: la,
                },
                IrValue::Slice {
                    data: db,
                    start: sb,
                    len: lb,
                },
            ) => da == db && sa == sb && la == lb,
            (IrValue::Class(a), IrValue::Class(b)) => a == b,
            (IrValue::Arena(a), IrValue::Arena(b)) => a == b,
            (IrValue::Mutex(a), IrValue::Mutex(b)) => Arc::ptr_eq(a, b),
            (IrValue::Chan(a), IrValue::Chan(b)) => Arc::ptr_eq(a, b),
            (
                IrValue::Enum {
                    name: an,
                    variant: av,
                    payload: ap,
                },
                IrValue::Enum {
                    name: bn,
                    variant: bv,
                    payload: bp,
                },
            ) => an == bn && av == bv && ap == bp,
            (IrValue::End, IrValue::End) => true,
            (IrValue::Iter(a), IrValue::Iter(b)) => a == b,
            (IrValue::Fn(a), IrValue::Fn(b)) => a == b,
            (
                IrValue::Closure {
                    func: af,
                    captures: ac,
                    is_mut: am,
                },
                IrValue::Closure {
                    func: bf,
                    captures: bc,
                    is_mut: bm,
                },
            ) => af == bf && ac == bc && am == bm,
            (IrValue::Void, IrValue::Void) => true,
            _ => false,
        }
    }
}

impl IrValue {
    pub(in crate::ir) fn as_bool(&self) -> bool {
        match self {
            IrValue::Bool(b) => *b,
            IrValue::Int(i) => *i != 0,
            IrValue::Float(f) => *f != 0.0,
            IrValue::String(s) => !s.is_empty(),
            IrValue::Opt(Some(v)) => v.as_bool(),
            // 指针恒真（对齐 tree-walking `Value::Ptr(_) => true`）
            IrValue::Ptr(_) => true,
            IrValue::Boxed(_) => true,
            _ => true,
        }
    }
    pub(in crate::ir) fn is_err(&self) -> bool {
        matches!(self, IrValue::Err { .. })
    }
    pub(in crate::ir) fn display(&self, ctx: &Ctx) -> String {
        match self {
            IrValue::Int(i) => i.to_string(),
            IrValue::Float(f) => {
                if f.fract() == 0.0 && f.is_finite() && f.abs() < 1e15 {
                    format!("{f:.1}")
                } else {
                    f.to_string()
                }
            }
            IrValue::Bool(b) => b.to_string(),
            IrValue::String(s) => s.to_string(),
            IrValue::Opt(Some(v)) => format!("?{}", v.display(ctx)),
            IrValue::Opt(None) => "null".into(),
            IrValue::Err { name, .. } => format!("error.{name}"),
            // 指针显示 pointee（对齐 tree-walking `Value::Ptr(p) => p.borrow().display()`）
            IrValue::Ptr(c) => ctx.cell_value(*c).display(ctx),
            IrValue::Boxed(c) => match &ctx.cells[*c] {
                Cell::Boxed { data, .. } => ctx.cell_value(*data).display(ctx),
                _ => "boxed".into(),
            },
            IrValue::Arr(c) => match &ctx.cells[*c] {
                Cell::Elems(e) => {
                    let items: Vec<String> = e
                        .iter()
                        .map(|ec| ctx.cell_value(*ec).display(ctx))
                        .collect();
                    format!("[{}]", items.join(", "))
                }
                _ => "[]".into(),
            },
            // 集合（G4）：Vec 委托 Arr 显示；Map 显示 `Map { k = v, ... }`
            IrValue::Vec(c) => match &ctx.cells[*c] {
                Cell::Vec { arr, .. } => arr.display(ctx),
                _ => "[]".into(),
            },
            IrValue::Map(c) => match &ctx.cells[*c] {
                Cell::Map { fields, .. } => {
                    let items: Vec<String> = fields
                        .iter()
                        .map(|(k, fc)| format!("{k} = {}", ctx.cell_value(*fc).display(ctx)))
                        .collect();
                    format!("Map {{ {} }}", items.join(", "))
                }
                _ => "Map {  }".into(),
            },
            IrValue::Slice { data, start, len } => match &ctx.cells[*data] {
                Cell::Elems(e) => {
                    let items: Vec<String> = e[*start..*start + *len]
                        .iter()
                        .map(|ec| ctx.cell_value(*ec).display(ctx))
                        .collect();
                    format!("[{}]", items.join(", "))
                }
                _ => "[]".into(),
            },
            IrValue::Class(c) => match &ctx.cells[*c] {
                Cell::Class { name, fields } => {
                    let items: Vec<String> = fields
                        .iter()
                        .map(|(k, fc)| format!("{k} = {}", ctx.cell_value(*fc).display(ctx)))
                        .collect();
                    format!("{name} {{ {} }}", items.join(", "))
                }
                _ => "void".into(),
            },
            IrValue::Arena(c) => match &ctx.cells[*c] {
                Cell::Arena(st) => {
                    format!("Arena(bytes={}, blocks={})", st.total, st.blocks.len())
                }
                _ => "Arena".into(),
            },
            IrValue::Enum {
                name,
                variant,
                payload,
            } => match payload {
                Some(p) => format!("{name}.{variant} = {}", p.display(ctx)),
                None => format!("{name}.{variant}"),
            },
            IrValue::End => "end".into(),
            IrValue::Iter(_) => "<iter>".into(),
            IrValue::Fn(name) => name.clone(),
            IrValue::Closure { .. } => "<closure>".into(),
            IrValue::Mutex(m) => match m.lock() {
                Ok(v) => format!("Mutex({})", v.display(ctx)),
                Err(_) => "Mutex(<poisoned>)".to_string(),
            },
            IrValue::Chan(ch) => format!(
                "Chan({}/{})",
                ch.inner.lock().unwrap().queue.len(),
                ch.capacity
            ),
            IrValue::Void => "void".into(),
        }
    }
    pub(in crate::ir) fn value_eq(&self, ctx: &Ctx, other: &IrValue) -> bool {
        match (self, other) {
            (IrValue::Int(a), IrValue::Int(b)) => a == b,
            (IrValue::Int(a), IrValue::Float(b)) => *a as f64 == *b,
            (IrValue::Float(a), IrValue::Int(b)) => *a == *b as f64,
            (IrValue::Float(a), IrValue::Float(b)) => a == b,
            (IrValue::Bool(a), IrValue::Bool(b)) => a == b,
            (IrValue::String(a), IrValue::String(b)) => a == b,
            (IrValue::Opt(a), IrValue::Opt(b)) => match (a, b) {
                (Some(x), Some(y)) => x.value_eq(ctx, y),
                (None, None) => true,
                _ => false,
            },
            // M4.2：错误按「码」比较（全局唯一），非名字
            (IrValue::Err { code: a, .. }, IrValue::Err { code: b, .. }) => a == b,
            // 指针：同 cell = 同一目标（身份——对齐 tree-walking `Rc::ptr_eq`）；
            // Ptr 与普通值比较时解引用后比较（对齐 `(Ptr, b) => deref(a).value_eq(b)`）
            (IrValue::Ptr(a), IrValue::Ptr(b)) => a == b,
            (IrValue::Ptr(a), b) => ctx.cell_value(*a).value_eq(ctx, b),
            (a, IrValue::Ptr(b)) => a.value_eq(ctx, ctx.cell_value(*b)),
            // 装箱胖指针：同 cell 索引 = 同一目标（身份）；与普通值比较时解引用 pointee
            (IrValue::Boxed(a), IrValue::Boxed(b)) => a == b,
            (IrValue::Boxed(a), b) => match &ctx.cells[*a] {
                Cell::Boxed { data, .. } => ctx.cell_value(*data).value_eq(ctx, b),
                _ => false,
            },
            (a, IrValue::Boxed(b)) => match &ctx.cells[*b] {
                Cell::Boxed { data, .. } => a.value_eq(ctx, ctx.cell_value(*data)),
                _ => false,
            },
            // 集合（G4）：Vec 按内容比较（委托 Arr）；Map 按键值表比较（含 Class("Map")）
            (IrValue::Vec(a), IrValue::Vec(b)) => match (&ctx.cells[*a], &ctx.cells[*b]) {
                (
                    Cell::Vec {
                        arr: IrValue::Arr(aa),
                        ..
                    },
                    Cell::Vec {
                        arr: IrValue::Arr(bb),
                        ..
                    },
                ) => arr_eq(ctx, *aa, *bb),
                _ => a == b,
            },
            (IrValue::Vec(a), b) => match &ctx.cells[*a] {
                Cell::Vec { arr, .. } => arr.value_eq(ctx, b),
                _ => false,
            },
            (a, IrValue::Vec(b)) => match &ctx.cells[*b] {
                Cell::Vec { arr, .. } => a.value_eq(ctx, arr),
                _ => false,
            },
            (IrValue::Map(a), IrValue::Map(b)) => map_eq(ctx, *a, *b),
            (IrValue::Map(a), IrValue::Class(b)) if class_name(ctx, *b) == "Map" => {
                map_class_eq(ctx, *a, *b)
            }
            (IrValue::Class(a), IrValue::Map(b)) if class_name(ctx, *a) == "Map" => {
                map_class_eq(ctx, *b, *a)
            }
            // ---- Phase 2 聚合 ----
            (IrValue::Arr(a), IrValue::Arr(b)) => arr_eq(ctx, *a, *b),
            (
                IrValue::Slice {
                    data: da,
                    start: sa,
                    len: la,
                },
                IrValue::Slice {
                    data: db,
                    start: sb,
                    len: lb,
                },
            ) => {
                if la != lb {
                    return false;
                }
                let (da_e, db_e) = match (&ctx.cells[*da], &ctx.cells[*db]) {
                    (Cell::Elems(x), Cell::Elems(y)) => (x.clone(), y.clone()),
                    _ => return false,
                };
                (0..*la).all(|i| {
                    ctx.cell_value(da_e[*sa + i])
                        .value_eq(ctx, ctx.cell_value(db_e[*sb + i]))
                })
            }
            (IrValue::Slice { data, start, len }, IrValue::Arr(b)) => {
                let d = match &ctx.cells[*data] {
                    Cell::Elems(x) => x.clone(),
                    _ => return false,
                };
                let (a_e, b_e) = (
                    d,
                    match &ctx.cells[*b] {
                        Cell::Elems(x) => x.clone(),
                        _ => return false,
                    },
                );
                *len == b_e.len()
                    && (0..*len).all(|i| {
                        ctx.cell_value(a_e[*start + i])
                            .value_eq(ctx, ctx.cell_value(b_e[i]))
                    })
            }
            (IrValue::Arr(a), IrValue::Slice { data, start, len }) => {
                let d = match &ctx.cells[*data] {
                    Cell::Elems(x) => x.clone(),
                    _ => return false,
                };
                let (a_e, d_e) = (
                    match &ctx.cells[*a] {
                        Cell::Elems(x) => x.clone(),
                        _ => return false,
                    },
                    d,
                );
                a_e.len() == *len
                    && (0..*len).all(|i| {
                        ctx.cell_value(a_e[i])
                            .value_eq(ctx, ctx.cell_value(d_e[*start + i]))
                    })
            }
            (IrValue::Class(a), IrValue::Class(b)) => class_eq(ctx, *a, *b),
            (
                IrValue::Enum {
                    name: an,
                    variant: av,
                    payload: ap,
                },
                IrValue::Enum {
                    name: bn,
                    variant: bv,
                    payload: bp,
                },
            ) => {
                an == bn
                    && av == bv
                    && match (ap, bp) {
                        (Some(x), Some(y)) => x.value_eq(ctx, y),
                        (None, None) => true,
                        _ => false,
                    }
            }
            (IrValue::Fn(a), IrValue::Fn(b)) => a == b,
            (IrValue::Closure { .. }, _) | (_, IrValue::Closure { .. }) => false,
            (IrValue::End, IrValue::End) => true,
            (IrValue::Void, IrValue::Void) => true,
            (IrValue::Mutex(a), IrValue::Mutex(b)) => match (a.lock(), b.lock()) {
                (Ok(av), Ok(bv)) => av.value_eq(ctx, &bv),
                _ => false,
            },
            (IrValue::Chan(a), IrValue::Chan(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}
