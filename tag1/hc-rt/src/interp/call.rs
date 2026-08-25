//! 解释器函数调用：普通函数调用、spawn 协程启动与闭包调用

use super::*;

impl Interp {
    // ---------- 内建 ----------

    pub(crate) fn call_builtin(
        &mut self,
        name: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        match name {
            "box" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let v = self.eval(&args[0])?;
                // G3：分配器引用——显式传入或回退全局 alloc（`box(v)` 单参形态）
                let alloc_v = if args.len() > 1 {
                    let a = self.eval(&args[1])?;
                    self.deref_value(a)
                } else {
                    Value::Allocator(Rc::new(RefCell::new(AllocatorImpl::Page)))
                };
                let vtbl = v.type_name();
                Ok(Some(Value::Boxed(Rc::new(RefCell::new(BoxedData {
                    data: Rc::new(RefCell::new(v)),
                    vtbl,
                    alloc: alloc_v,
                })))))
            }
            "unbox" => {
                if args.len() != 1 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let v = self.eval(&args[0])?;
                // 不用 deref_value——后者会解引用 Boxed 到内部值。
                // 需要直接匹配 Boxed 或 Ptr(Boxed)。
                let boxed = match v {
                    Value::Boxed(b) => Some(b),
                    Value::Ptr(c) => match &*c.borrow() {
                        Value::Boxed(b) => Some(b.clone()),
                        _ => None,
                    },
                    _ => None,
                };
                match boxed {
                    Some(b) => {
                        let inner = b.borrow().data.borrow().clone();
                        // 消费 Box，清空 data 防止双重释放
                        *b.borrow_mut().data.borrow_mut() = Value::Void;
                        Ok(Some(inner))
                    }
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            "copy" => {
                if args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let v = self.eval(&args[0])?;
                // copy(&x, .shallow)（L1：CopyMode 内建枚举，.shallow 推断）
                let shallow = if args.len() > 1 {
                    let mode = self.eval(&args[1])?;
                    match self.deref_value(mode) {
                        Value::Enum { variant, .. } => variant == "shallow",
                        _ => false,
                    }
                } else {
                    false
                };
                Ok(Some(if shallow {
                    self.shallow_copy(v)
                } else {
                    self.deep_copy(v)
                }))
            }
            // @ 内建（M4.3 子集）：@intFromEnum / @enumFromInt
            "@intFromEnum" => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                match v {
                    Value::Enum { name, variant, .. } => {
                        // 内建枚举（L3）：ExitType = [Exit, Error]
                        let idx = if name == "ExitType" {
                            match variant.as_str() {
                                "Exit" => 0,
                                "Error" => 1,
                                _ => 0,
                            }
                        } else {
                            match self.types.get(&name) {
                                Some(TypeDef::Enum { variants }) => {
                                    variants.iter().position(|v| v.name == variant).unwrap_or(0)
                                        as i128
                                }
                                _ => 0,
                            }
                        };
                        Ok(Some(Value::Int(idx)))
                    }
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            "@enumFromInt" => {
                if args.len() != 2 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let ty = match &args[0] {
                    Expr::Ident(n, _) => n.clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let i = self.eval(&args[1])?;
                let i = match self.deref_value(i) {
                    Value::Int(i) => i,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                match self.types.get(&ty) {
                    Some(TypeDef::Enum { variants }) => {
                        let variant = variants.get(i as usize).map(|v| v.name.clone());
                        match variant {
                            Some(v) => Ok(Some(Value::Enum {
                                name: ty.clone(),
                                variant: v,
                                payload: None,
                            })),
                            None => Err(RtError::new("IndexOutOfBounds", Some(span.clone()))),
                        }
                    }
                    _ => Err(RtError::new("UnknownType", Some(span.clone()))),
                }
            }
            "@panic" => {
                // Q-S2：@panic("消息", 位置) abort
                let msg = if args.is_empty() {
                    "panic".to_string()
                } else {
                    let v = self.eval(&args[0])?;
                    self.deref_value(v).display()
                };
                Err(RtError::msg("Panic", msg))
            }
            // ---------- M4.3 @ 内建基础集 ----------
            "@sizeOf" => {
                // @sizeOf(T)：类型字节大小（连续类型与 to_bytes 布局一致）
                let ty = match &args[0] {
                    Expr::Ident(n, _) => n.clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                match self.type_size_of(&ty) {
                    Some(s) => Ok(Some(Value::Int(s as i128))),
                    None => Err(RtError::msg(
                        "UnknownType",
                        format!("@sizeOf: unknown type `{ty}`"),
                    )),
                }
            }
            "@alignOf" => {
                // @alignOf(T)：自然对齐（标量 = 宽度；连续 class = pad/align 尊重；其余 8）
                let ty = match &args[0] {
                    Expr::Ident(n, _) => n.clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let align = match ty.as_str() {
                    "i8" | "u8" | "bool" => 1,
                    "i16" | "u16" | "f16" => 2,
                    "i32" | "u32" | "f32" => 4,
                    "i128" | "u128" | "f128" => 16,
                    _ => self.continuous_align(&ty).unwrap_or(8),
                };
                Ok(Some(Value::Int(align as i128)))
            }
            "@offsetOf" => {
                // @offsetOf(T, field)：连续 class 字段偏移（与 to_bytes 填充一致）
                let ty = match &args[0] {
                    Expr::Ident(n, _) => n.clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let field = match &args[1] {
                    Expr::Ident(f, _) => f.clone(),
                    Expr::StrLit { value, .. } => value.clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                match self.continuous_layout(&ty) {
                    Some((layout, _)) => match layout.iter().find(|(n, _, _)| *n == field) {
                        Some((_, off, _)) => Ok(Some(Value::Int(*off as i128))),
                        None => Err(RtError::msg(
                            "UnknownField",
                            format!("@offsetOf: `{ty}` has no field `{field}`"),
                        )),
                    },
                    None => Err(RtError::msg(
                        "NotContinuous",
                        format!("@offsetOf: `{ty}` is not a continuous type"),
                    )),
                }
            }
            "@typeOf" => {
                // @typeOf(expr)：表达式运行时类型名（tag1 简化：type_name）
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                Ok(Some(Value::str(&v.type_name())))
            }
            "@intCast" => {
                // @intCast(T, x)：整数转换（Debug 范围检查，溢出抛错带位置）
                let ty = match &args[0] {
                    Expr::Ident(n, _) => n.clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let v = self.eval(&args[1])?;
                let v = self.deref_value(v);
                let i = match v {
                    Value::Int(i) => i,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                if let Some((min, max)) = Self::int_width_bounds(&ty) {
                    if i < min || i > max {
                        return Err(RtError::new("IntCastOverflow", Some(span.clone())));
                    }
                }
                Ok(Some(Value::Int(i)))
            }
            "@ptrCast" | "@alignCast" => {
                // @ptrCast(T, p) / @alignCast(T, p)：tag1 指针无类型化——透传
                let v = self.eval(
                    args.last()
                        .ok_or_else(|| RtError::new("ArityMismatch", Some(span.clone())))?,
                )?;
                Ok(Some(v))
            }
            "@volatileLoad" => {
                // K2：@volatileLoad(ptr)——读穿指针。interp 无优化器，volatile 透明 =
                // 常规解引用（对齐 `p.*`）；原生在 LLVM volatile 指令层体现语义。
                if args.len() != 1 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let p = self.eval(&args[0])?;
                Ok(Some(self.deref_checked(p, span)?))
            }
            "@volatileStore" => {
                // K2：@volatileStore(ptr, v)——写穿指针（对齐 `p.* = v` 赋值路径）
                if args.len() != 2 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let p = self.eval(&args[0])?;
                self.check_dangling(&p, span)?;
                let v = self.eval(&args[1])?;
                match p {
                    Value::Ptr(cell) => *cell.borrow_mut() = v,
                    Value::Boxed(b) => *b.borrow_mut().data.borrow_mut() = v,
                    _ => return Err(RtError::new("BadAssign", Some(span.clone()))),
                }
                Ok(Some(Value::Void))
            }
            "@ptrFromInt" => {
                // K4：@ptrFromInt(addr)——整数地址 → 虚拟指针。登记过（@intFromPtr）→ 重建
                // 原指针（round-trip 保真）；未登记 → 合成匿名槽（同地址幂等）。取 Rc 堆地址
                // 与 Debug 悬垂跟踪同源，槽经 Rc 存活（interp 无真实物理内存，MMIO 地址 = 匿名槽）。
                if args.len() != 1 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let n = self.eval(&args[0])?;
                match n {
                    Value::Int(i) => {
                        let addr = i as usize;
                        if let Some(v) = self.addr_registry.get(&addr) {
                            return Ok(Some(v.clone()));
                        }
                        let cell = Rc::new(RefCell::new(Value::Void));
                        self.addr_registry.insert(addr, Value::Ptr(cell.clone()));
                        Ok(Some(Value::Ptr(cell)))
                    }
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            "@intFromPtr" => {
                // K4：@intFromPtr(p)——指针 → 整数地址。取 cell/Boxed 的 Rc 堆地址（与悬垂
                // 跟踪同源），登记进 addr_registry 供 @ptrFromInt 重建（round-trip 保真）。
                if args.len() != 1 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let p = self.eval(&args[0])?;
                match &p {
                    Value::Ptr(cell) => {
                        let addr = Rc::as_ptr(cell) as usize;
                        self.addr_registry.insert(addr, p.clone());
                        Ok(Some(Value::Int(addr as i128)))
                    }
                    Value::Boxed(b) => {
                        let addr = Rc::as_ptr(b) as usize;
                        self.addr_registry.insert(addr, p.clone());
                        Ok(Some(Value::Int(addr as i128)))
                    }
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            "@atomicLoad" => {
                // Q-S3：@atomicLoad(T, p, order)——原子读。协作式单线程下无并发竞争，
                // atomic 透明 = 常规解引用（对齐 @volatileLoad）。T 为类型名参数——直接
                // 读 Ident 名不求值（对齐 @sizeOf）；order 内存序求值后丢弃。
                if args.len() != 3 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let _ty = match &args[0] {
                    Expr::Ident(n, _) => n.clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let _ = self.eval(&args[2])?;
                let p = self.eval(&args[1])?;
                Ok(Some(self.deref_checked(p, span)?))
            }
            "@atomicStore" => {
                // Q-S3：@atomicStore(T, p, v, order)——原子写（对齐 @volatileStore 写穿）。
                if args.len() != 4 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let _ty = match &args[0] {
                    Expr::Ident(n, _) => n.clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let _ = self.eval(&args[3])?;
                let p = self.eval(&args[1])?;
                self.check_dangling(&p, span)?;
                let v = self.eval(&args[2])?;
                match p {
                    Value::Ptr(cell) => *cell.borrow_mut() = v,
                    Value::Boxed(b) => *b.borrow_mut().data.borrow_mut() = v,
                    _ => return Err(RtError::new("BadAssign", Some(span.clone()))),
                }
                Ok(Some(Value::Void))
            }
            "@atomicRmw" => {
                // Q-S3：@atomicRmw(T, p, op, v, order)——读改写，返回旧值。op 为内建枚举
                // 变体（.add/.sub/.exchange/.cmpxchg；tag1 支持前三）。协作式下无竞争，
                // 直接读改写（对齐常规 binop_values 语义）。T 为类型名参数——直接读
                // Ident 名不求值（对齐 @sizeOf）。
                if args.len() != 5 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let _ty = match &args[0] {
                    Expr::Ident(n, _) => n.clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let _ = self.eval(&args[4])?;
                let p = self.eval(&args[1])?;
                let op = self.eval(&args[2])?;
                let v = self.eval(&args[3])?;
                let op_name = match self.deref_value(op) {
                    Value::Enum { variant, .. } => variant,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let v = self.deref_value(v);
                match p {
                    Value::Ptr(cell) => {
                        let old = cell.borrow().clone();
                        let new = match op_name.as_str() {
                            "add" | "sub" => match (&old, &v) {
                                (Value::Int(a), Value::Int(b)) => {
                                    Value::Int(if op_name == "add" { a + b } else { a - b })
                                }
                                _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                            },
                            "exchange" => v.clone(),
                            _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                        };
                        *cell.borrow_mut() = new;
                        Ok(Some(old))
                    }
                    _ => Err(RtError::new("BadAssign", Some(span.clone()))),
                }
            }
            "@compileError" => {
                // 语义层应已拦截（编译期错误）；运行时到达 = 未拦截路径
                let msg = if args.is_empty() {
                    "compileError".to_string()
                } else {
                    let v = self.eval(&args[0])?;
                    self.deref_value(v).display()
                };
                Err(RtError::msg(
                    "CompileError",
                    format!("@compileError: {msg}"),
                ))
            }
            "@addWithOverflow" | "@subWithOverflow" | "@mulWithOverflow" => {
                // 返回 (T, bool) 元组；tag1 Int = i128 无溢出（标志恒 false）
                let a = self.eval(&args[0])?;
                let b = self.eval(&args[1])?;
                let a = match self.deref_value(a) {
                    Value::Int(i) => i,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let b = match self.deref_value(b) {
                    Value::Int(i) => i,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let r = match name {
                    "@addWithOverflow" => a.wrapping_add(b),
                    "@subWithOverflow" => a.wrapping_sub(b),
                    _ => a.wrapping_mul(b),
                };
                Ok(Some(Value::arr(vec![Value::Int(r), Value::Bool(false)])))
            }
            "sqrt" => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                match v {
                    Value::Int(i) => Ok(Some(Value::Float((i as f64).sqrt()))),
                    Value::Float(f) => Ok(Some(Value::Float(f.sqrt()))),
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            // min/max（M5.5 工具：i32/f64 数值比较，73-rate-limit 令牌桶）
            "min" | "max" => {
                if args.len() != 2 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let a0 = self.eval(&args[0])?;
                let a = self.deref_value(a0);
                let b0 = self.eval(&args[1])?;
                let b = self.deref_value(b0);
                let take_a = match (&a, &b) {
                    (Value::Int(x), Value::Int(y)) => {
                        if name == "min" {
                            x <= y
                        } else {
                            x >= y
                        }
                    }
                    (Value::Float(x), Value::Float(y)) => {
                        if name == "min" {
                            x <= y
                        } else {
                            x >= y
                        }
                    }
                    (Value::Int(x), Value::Float(y)) => {
                        if name == "min" {
                            (*x as f64) <= *y
                        } else {
                            (*x as f64) >= *y
                        }
                    }
                    (Value::Float(x), Value::Int(y)) => {
                        if name == "min" {
                            *x <= (*y as f64)
                        } else {
                            *x >= (*y as f64)
                        }
                    }
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                Ok(Some(if take_a { a } else { b }))
            }
            // 格式辅助（M5.3 serialize）：fmt_int/fmt_float → String
            // （63-template-render 占位符替换；float 用 display 格式——整值补 `.0`）
            "fmt_int" => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                match v {
                    Value::Int(i) => Ok(Some(Value::str(&i.to_string()))),
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            "fmt_float" => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                match v {
                    Value::Float(f) => Ok(Some(Value::str(&Value::Float(f).display()))),
                    Value::Int(i) => Ok(Some(Value::str(&Value::Float(i as f64).display()))),
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            // read_u64_le(slice)：8 字节小端 → i64（57-protocol-parse 长度前缀帧）
            "read_u64_le" => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                let b = match self.value_bytes(&v) {
                    Some(b) => b,
                    None => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                if b.len() < 8 {
                    return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                }
                let n = u64::from_le_bytes(b[0..8].try_into().unwrap());
                Ok(Some(Value::Int(n as i128)))
            }
            // std 算法（M5.2 最小集）：sort / binary_search
            "sort" => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                let has_cmp = args.len() > 1;
                let cmp_f = if has_cmp {
                    let f = self.eval(&args[1])?;
                    let f = self.deref_value(f);
                    match f {
                        Value::Closure(c) => Some(c),
                        _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                    }
                } else {
                    None
                };
                match v {
                    Value::Arr(a) => {
                        let mut items: Vec<Value> =
                            a.borrow().iter().map(|c| c.borrow().clone()).collect();
                        items.sort_by(|x, y| {
                            match &cmp_f {
                                Some(c) => {
                                    // 比较器闭包返回序（负数/零/正数）
                                    let r = self.call_closure(c, &[x.clone(), y.clone()], span);
                                    match r {
                                        Ok(Value::Int(i)) if i < 0 => std::cmp::Ordering::Less,
                                        Ok(Value::Int(i)) if i > 0 => std::cmp::Ordering::Greater,
                                        Ok(Value::Float(f)) if f < 0.0 => std::cmp::Ordering::Less,
                                        Ok(Value::Float(f)) if f > 0.0 => {
                                            std::cmp::Ordering::Greater
                                        }
                                        _ => std::cmp::Ordering::Equal,
                                    }
                                }
                                None => x.value_lt(y).map_or(std::cmp::Ordering::Equal, |lt| {
                                    if lt {
                                        std::cmp::Ordering::Less
                                    } else if x.value_eq(y) {
                                        std::cmp::Ordering::Equal
                                    } else {
                                        std::cmp::Ordering::Greater
                                    }
                                }),
                            }
                        });
                        for (c, v) in a.borrow().iter().zip(items.iter()) {
                            *c.borrow_mut() = v.clone();
                        }
                        Ok(Some(Value::Void))
                    }
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            // 解析器辅助（71-recursive-parser；操作 &[u8] 与 *usize）
            "skip_space" | "peek" | "advance" | "is_digit" | "parse_number" => {
                return self.call_parser_builtin(name, args, span);
            }
            "parse_int" => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                let s = match v {
                    Value::Str(s) => s.borrow().clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let text = String::from_utf8_lossy(&s).trim().to_string();
                let parsed = if text.is_empty() {
                    None
                } else {
                    text.parse::<i128>().ok()
                };
                Ok(Some(match parsed {
                    Some(n) => Value::Opt(Some(Rc::new(Value::Int(n)))),
                    None => Value::Opt(None),
                }))
            }
            "parse_float" => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                let s = match v {
                    Value::Str(s) => s.borrow().clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let text = String::from_utf8_lossy(&s).trim().to_string();
                let parsed = if text.is_empty() {
                    None
                } else {
                    text.parse::<f64>().ok()
                };
                Ok(Some(match parsed {
                    Some(n) => Value::Opt(Some(Rc::new(Value::Float(n)))),
                    None => Value::Opt(None),
                }))
            }
            "binary_search" => {
                let v = self.eval(&args[0])?;
                let target = self.eval(&args[1])?;
                let v = self.deref_value(v);
                let target = self.deref_value(target);
                let items: Vec<Value> = match &v {
                    Value::Arr(a) => a.borrow().iter().map(|c| c.borrow().clone()).collect(),
                    Value::Slice { data, start, len } => data
                        .borrow()
                        .iter()
                        .skip(*start)
                        .take(*len)
                        .map(|c| c.borrow().clone())
                        .collect(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let mut lo = 0usize;
                let mut hi = items.len();
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    let cmp = items[mid].value_lt(&target);
                    match cmp {
                        Some(true) => lo = mid + 1,
                        Some(false) if items[mid].value_eq(&target) => {
                            return Ok(Some(Value::Opt(Some(Rc::new(Value::Int(mid as i128))))))
                        }
                        _ => hi = mid,
                    }
                }
                Ok(Some(Value::Opt(None)))
            }
            // 断言五件套（Q-T1）：测试函数内隐式可用；3 参形式 = 解析器 expect（71）
            "expect" => {
                if args.len() == 3 {
                    return self.call_parser_builtin(name, args, span);
                }
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                if !v.as_bool() {
                    self.fail_info = Some(format!("expect failed at {}:{}", span.line, span.col));
                    return Err(RtError::new("AssertFailed", Some(span.clone())));
                }
                Ok(Some(Value::Void))
            }
            "expect_eq" | "expect_neq" => {
                if args.len() != 2 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let a = self.eval(&args[0])?;
                let b = self.eval(&args[1])?;
                let a = self.deref_value(a);
                let b = self.deref_value(b);
                let eq = a.value_eq(&b);
                let want_eq = name == "expect_eq";
                if eq != want_eq {
                    self.fail_info = Some(format!(
                        "{} failed at {}:{}: expected {} {}, got {}",
                        name,
                        span.line,
                        span.col,
                        if want_eq { "=" } else { "!=" },
                        b.display(),
                        a.display()
                    ));
                    return Err(RtError::new("AssertFailed", Some(span.clone())));
                }
                Ok(Some(Value::Void))
            }
            "expect_error" => {
                if args.len() != 2 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let want = self.eval(&args[0])?;
                let got = self.eval(&args[1])?;
                let want = self.deref_value(want);
                let got = self.deref_value(got);
                match (want, got) {
                    // M4.2：错误码比较（码全局唯一）
                    (Value::Err { name: w, .. }, Value::Err { name: g, .. }) if w == g => {
                        Ok(Some(Value::Void))
                    }
                    (Value::Err { name: w, .. }, Value::Err { name: g, .. }) => {
                        self.fail_info = Some(format!(
                            "expect_error failed at {}:{}: expected error.{w}, got error.{g}",
                            span.line, span.col
                        ));
                        Err(RtError::new("AssertFailed", Some(span.clone())))
                    }
                    (_, g) => {
                        self.fail_info = Some(format!(
                            "expect_error failed at {}:{}: expected error, got {}",
                            span.line,
                            span.col,
                            g.type_name()
                        ));
                        Err(RtError::new("AssertFailed", Some(span.clone())))
                    }
                }
            }
            "expect_eq_slices" => {
                if args.len() != 2 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let a = self.eval(&args[0])?;
                let b = self.eval(&args[1])?;
                let a = self.deref_value(a);
                let b = self.deref_value(b);
                if !a.value_eq(&b) {
                    self.fail_info = Some(format!(
                        "expect_eq_slices failed at {}:{}: {} != {}",
                        span.line,
                        span.col,
                        a.display(),
                        b.display()
                    ));
                    return Err(RtError::new("AssertFailed", Some(span.clone())));
                }
                Ok(Some(Value::Void))
            }
            // E4 true-OMP：spawn(f, args...) 创建协程 G 提交到调度器，返回 Thread 句柄
            "spawn" => {
                if args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let callee = self.eval(&args[0])?;
                let callee = self.deref_value(callee);
                match &callee {
                    Value::Fn(_) | Value::Closure(_) => {}
                    _ => return Err(RtError::new("NotCallable", Some(span.clone()))),
                }
                let mut arg_vals = Vec::new();
                for a in &args[1..] {
                    arg_vals.push(self.eval(a)?);
                }
                // Q8：每线程独立 alloc 实例（全新 Arena；子任务执行时绑定为 alloc）
                let alloc_v = Value::Allocator(Rc::new(RefCell::new(AllocatorImpl::Arena(
                    Rc::new(RefCell::new(ArenaState::new())),
                ))));

                // E4：创建协程共享状态
                let result = Arc::new(Mutex::new(None::<ThreadResult>));
                let cancel = Arc::new(AtomicBool::new(false));
                let done = Arc::new(AtomicBool::new(false));

                let source = self.source.clone();
                let tid = self.next_tid;
                self.next_tid += 1;

                let result_tx = result.clone();
                let cancel_tx = cancel.clone();
                let done_tx = done.clone();

                let callee_clone = callee.clone();
                let arg_vals_clone = arg_vals.clone();

                // 确保调度器已启动
                self.scheduler.start();

                // 创建协程任务闭包
                let task: Box<dyn FnOnce() + Send> = Box::new(move || {
                    let mut interp = Interp::new(&source);
                    let program =
                        hc::parse_source(&source).unwrap_or_else(|_| panic!("spawn: parse failed"));
                    interp
                        .load(&program)
                        .unwrap_or_else(|e| panic!("spawn: load failed: {} {}", e.name, e.message));

                    // 检查取消标志
                    if cancel_tx.load(Ordering::SeqCst) {
                        let err_v = interp.err_val("Cancelled");
                        let mut r = result_tx.lock().unwrap();
                        *r = Some(ThreadResult::Ok(err_v));
                        done_tx.store(true, Ordering::SeqCst);
                        return;
                    }

                    // 绑定每线程 alloc
                    interp.push_scope();
                    interp.bind("alloc", alloc_v);

                    let r = match callee_clone {
                        Value::Fn(ref fname) => match interp.pick_fn(fname, &arg_vals_clone) {
                            Ok(fdef) => {
                                interp.call_fn(&fdef, &arg_vals_clone, &Span::new(0, 0, 0, 0))
                            }
                            Err(e) => Err(e),
                        },
                        Value::Closure(ref cl) => {
                            interp.call_closure(cl, &arg_vals_clone, &Span::new(0, 0, 0, 0))
                        }
                        _ => Err(RtError::new("NotCallable", None)),
                    };
                    let _ = interp.pop_scope(true);

                    let mut rv = result_tx.lock().unwrap();
                    *rv = Some(match r {
                        Ok(v) => ThreadResult::Ok(v),
                        Err(e) => ThreadResult::Err(e),
                    });
                    done_tx.store(true, Ordering::SeqCst);
                });

                // 提交协程到调度器
                let gid = self.scheduler.submit("spawn".to_string(), task);

                self.thread_handles.insert(
                    tid,
                    ThreadState {
                        join_handle: None,
                        result,
                        cancel,
                        done,
                    },
                );

                let mut f = HashMap::new();
                f.insert("_tid".to_string(), Value::Int(tid as i128));
                f.insert("done".to_string(), Value::Bool(false));
                f.insert("cancel".to_string(), Value::Bool(false));
                f.insert("detached".to_string(), Value::Bool(false));
                Ok(Some(Value::class("Thread", f)))
            }
            // Phase 3 移除：with_arena 已弃用，使用 Arena.init(alloc) 替代
            _ => Ok(None),
        }
    }

    /// 标量方法（ICompare/INumber 族内建：add/sub/mul/div/neg/mod/abs/eq/lt）
    pub(crate) fn call_scalar_method(
        &mut self,
        self_v: &Value,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        // 整数操作保持整数语义（div 截断、mod 取余）
        if args.len() == 1 {
            let raw = self.eval(&args[0])?;
            let arg_v = self.deref_value(raw);
            if let (Value::Int(a), Value::Int(b)) = (self_v, &arg_v) {
                let i_op = match field {
                    "add" => {
                        Some(Value::Int(a.checked_add(*b).ok_or_else(|| {
                            RtError::new("Overflow", Some(span.clone()))
                        })?))
                    }
                    "sub" => {
                        Some(Value::Int(a.checked_sub(*b).ok_or_else(|| {
                            RtError::new("Overflow", Some(span.clone()))
                        })?))
                    }
                    "mul" => {
                        Some(Value::Int(a.checked_mul(*b).ok_or_else(|| {
                            RtError::new("Overflow", Some(span.clone()))
                        })?))
                    }
                    "div" => {
                        if *b == 0 {
                            return Err(RtError::new("DivisionByZero", Some(span.clone())));
                        }
                        Some(Value::Int(a / b))
                    }
                    "mod" => {
                        if *b == 0 {
                            return Err(RtError::new("DivisionByZero", Some(span.clone())));
                        }
                        Some(Value::Int(a % b))
                    }
                    "eq" => Some(Value::Bool(a == b)),
                    "lt" => Some(Value::Bool(a < b)),
                    _ => None,
                };
                if let Some(v) = i_op {
                    return Ok(Some(v));
                }
            }
        }
        // 一元整数操作
        if args.is_empty() {
            if let Value::Int(a) = self_v {
                let i_op = match field {
                    "neg" => Some(Value::Int(-*a)),
                    "abs" => Some(Value::Int(a.abs())),
                    _ => None,
                };
                if let Some(v) = i_op {
                    return Ok(Some(v));
                }
            }
        }
        let v = match self_v {
            Value::Int(i) => *i as f64,
            Value::Float(f) => *f,
            _ => return Ok(None),
        };
        let mut one_arg = |ix: &[Expr]| -> std::result::Result<f64, RtError> {
            let a = self.eval(&ix[0])?;
            let a = self.deref_value(a);
            match a {
                Value::Int(i) => Ok(i as f64),
                Value::Float(f) => Ok(f),
                _ => Err(RtError::new("TypeError", Some(span.clone()))),
            }
        };
        let r = match field {
            "add" => v + one_arg(args)?,
            "sub" => v - one_arg(args)?,
            "mul" => v * one_arg(args)?,
            "div" => v / one_arg(args)?,
            "mod" => v % one_arg(args)?,
            "neg" => -v,
            "abs" => v.abs(),
            "pow" => v.powf(one_arg(args)?),
            "eq" | "lt" => {
                let other = one_arg(args)?;
                let b = match field {
                    "eq" => v == other,
                    _ => v < other,
                };
                return Ok(Some(Value::Bool(b)));
            }
            _ => return Ok(None),
        };
        // 整数保持整数（无小数部分时）
        if r.fract() == 0.0 && r.is_finite() && r.abs() < 9e18 {
            Ok(Some(Value::Int(r as i128)))
        } else {
            Ok(Some(Value::Float(r)))
        }
    }

    pub(crate) fn call_builtin_method(
        &mut self,
        self_v: &Value,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        // 标量方法（INumber/ICompare 族：a.add(b) ≡ a + b）
        if matches!(self_v, Value::Int(_) | Value::Float(_)) {
            if let Some(v) = self.call_scalar_method(self_v, field, args, span)? {
                return Ok(Some(v));
            }
        }
        match (self_v, field) {
            (Value::Str(s), "concat") => {
                let other = self.eval(&args[0])?;
                let other = self.deref_value(other);
                let other_bytes = other
                    .extract_bytes()
                    .ok_or_else(|| RtError::new("TypeError", Some(span.clone())))?;
                let mut bytes = s.borrow().clone();
                bytes.extend_from_slice(&other_bytes);
                return Ok(Some(Value::str_bytes(bytes)));
            }
            (Value::String(s), "concat") => {
                let other = self.eval(&args[0])?;
                let other = self.deref_value(other);
                let other_bytes = other
                    .extract_bytes()
                    .ok_or_else(|| RtError::new("TypeError", Some(span.clone())))?;
                let mut bytes = s.as_slice().to_vec();
                bytes.extend_from_slice(&other_bytes);
                return Ok(Some(Value::str_bytes(bytes)));
            }
            (Value::Str(s), "as_slice") => Ok(Some(Value::Str(s.clone()))),
            (Value::String(s), "as_slice") => {
                let bytes = s.as_slice().to_vec();
                Ok(Some(Value::Str(Rc::new(RefCell::new(bytes)))))
            }
            (Value::String(s), "into_array") => {
                let mut s = s.clone();
                let (ptr, len, cap) = s.take_ptr();
                if !ptr.is_null() {
                    let layout = std::alloc::Layout::from_size_align(cap, 1).expect("valid layout");
                    let vec = unsafe {
                        let b = std::slice::from_raw_parts_mut(ptr, len);
                        Vec::from_raw_parts(b.as_mut_ptr(), len, cap)
                    };
                    Ok(Some(Value::Bytes(Rc::new(RefCell::new(vec)))))
                } else {
                    Ok(Some(Value::Bytes(Rc::new(RefCell::new(Vec::new())))))
                }
            }
            (Value::Str(s), "split") => {
                // 按分隔符切分（返回 Vec of String）
                let sep_v = self.eval(&args[0])?;
                let sep_v = self.deref_value(sep_v);
                let sep = match sep_v {
                    Value::Int(i) => vec![i as u8],
                    _ => sep_v
                        .extract_bytes()
                        .ok_or_else(|| RtError::new("TypeError", Some(span.clone())))?,
                };
                let data = s.borrow().clone();
                let mut out = Vec::new();
                if sep.is_empty() {
                    return Ok(Some(Value::arr(vec![Value::str_bytes(data)])));
                }
                let mut start = 0usize;
                let mut i = 0usize;
                while i + sep.len() <= data.len() {
                    if &data[i..i + sep.len()] == sep.as_slice() {
                        out.push(Value::str_bytes(data[start..i].to_vec()));
                        i += sep.len();
                        start = i;
                    } else {
                        i += 1;
                    }
                }
                out.push(Value::str_bytes(data[start..].to_vec()));
                Ok(Some(Value::arr(out)))
            }
            (Value::String(s), "split") => {
                let sep_v = self.eval(&args[0])?;
                let sep_v = self.deref_value(sep_v);
                let sep = match sep_v {
                    Value::Int(i) => vec![i as u8],
                    _ => sep_v
                        .extract_bytes()
                        .ok_or_else(|| RtError::new("TypeError", Some(span.clone())))?,
                };
                let data = s.as_slice().to_vec();
                let mut out = Vec::new();
                if sep.is_empty() {
                    return Ok(Some(Value::arr(vec![Value::str_bytes(data)])));
                }
                let mut start = 0usize;
                let mut i = 0usize;
                while i + sep.len() <= data.len() {
                    if &data[i..i + sep.len()] == sep.as_slice() {
                        out.push(Value::str_bytes(data[start..i].to_vec()));
                        i += sep.len();
                        start = i;
                    } else {
                        i += 1;
                    }
                }
                out.push(Value::str_bytes(data[start..].to_vec()));
                Ok(Some(Value::arr(out)))
            }
            (Value::Str(s), "to_bytes") => {
                // 序列化格式：[u64 LE 长度][utf8]
                let b = s.borrow();
                let mut out = (b.len() as u64).to_le_bytes().to_vec();
                out.extend_from_slice(&b);
                Ok(Some(Value::str_bytes(out)))
            }
            (Value::String(s), "to_bytes") => {
                let b = s.as_slice();
                let mut out = (b.len() as u64).to_le_bytes().to_vec();
                out.extend_from_slice(b);
                Ok(Some(Value::str_bytes(out)))
            }
            (Value::Str(s), "find") => {
                let needle = self.eval(&args[0])?;
                let needle = self.deref_value(needle);
                let needle_bytes: Vec<u8> = match &needle {
                    Value::Int(i) => vec![*i as u8],
                    _ => needle
                        .extract_bytes()
                        .ok_or_else(|| RtError::new("TypeError", Some(span.clone())))?,
                };
                let data = s.borrow().clone();
                let pos = if needle_bytes.is_empty() {
                    Some(0usize)
                } else {
                    data.windows(needle_bytes.len())
                        .position(|w| w == needle_bytes.as_slice())
                };
                Ok(Some(match pos {
                    Some(p) => Value::Opt(Some(Rc::new(Value::Int(p as i128)))),
                    None => Value::Opt(None),
                }))
            }
            (Value::String(s), "find") => {
                let needle = self.eval(&args[0])?;
                let needle = self.deref_value(needle);
                let needle_bytes: Vec<u8> = match &needle {
                    Value::Int(i) => vec![*i as u8],
                    _ => needle
                        .extract_bytes()
                        .ok_or_else(|| RtError::new("TypeError", Some(span.clone())))?,
                };
                let data = s.as_slice().to_vec();
                let pos = if needle_bytes.is_empty() {
                    Some(0usize)
                } else {
                    data.windows(needle_bytes.len())
                        .position(|w| w == needle_bytes.as_slice())
                };
                Ok(Some(match pos {
                    Some(p) => Value::Opt(Some(Rc::new(Value::Int(p as i128)))),
                    None => Value::Opt(None),
                }))
            }
            (Value::Str(s), "substring") => {
                let lo = self.eval(&args[0])?;
                let hi = self.eval(&args[1])?;
                let lo = self.deref_value(lo);
                let hi = self.deref_value(hi);
                let (lo, hi) = match (lo, hi) {
                    (Value::Int(a), Value::Int(b)) => (a.max(0) as usize, b.max(0) as usize),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let data = s.borrow();
                let hi = hi.min(data.len());
                let sub = data[lo.min(hi)..hi].to_vec();
                Ok(Some(Value::str_bytes(sub)))
            }
            (Value::String(s), "substring") => {
                let lo = self.eval(&args[0])?;
                let hi = self.eval(&args[1])?;
                let lo = self.deref_value(lo);
                let hi = self.deref_value(hi);
                let (lo, hi) = match (lo, hi) {
                    (Value::Int(a), Value::Int(b)) => (a.max(0) as usize, b.max(0) as usize),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let data = s.as_slice();
                let hi = hi.min(data.len());
                let sub = data[lo.min(hi)..hi].to_vec();
                Ok(Some(Value::str_bytes(sub)))
            }
            (Value::Str(s), "replace") => {
                let from = self.eval(&args[0])?;
                let to = self.eval(&args[1])?;
                let from = self.deref_value(from);
                let to = self.deref_value(to);
                let from_b = from
                    .extract_bytes()
                    .ok_or_else(|| RtError::new("TypeError", Some(span.clone())))?;
                let to_b = to
                    .extract_bytes()
                    .ok_or_else(|| RtError::new("TypeError", Some(span.clone())))?;
                let data = s.borrow().clone();
                let mut out = Vec::new();
                let mut i = 0usize;
                while i < data.len() {
                    if from_b.is_empty() {
                        out.push(data[i]);
                        i += 1;
                    } else if i + from_b.len() <= data.len()
                        && &data[i..i + from_b.len()] == from_b.as_slice()
                    {
                        out.extend_from_slice(&to_b);
                        i += from_b.len();
                    } else {
                        out.push(data[i]);
                        i += 1;
                    }
                }
                Ok(Some(Value::str_bytes(out)))
            }
            (Value::String(s), "replace") => {
                let from = self.eval(&args[0])?;
                let to = self.eval(&args[1])?;
                let from = self.deref_value(from);
                let to = self.deref_value(to);
                let from_b = from
                    .extract_bytes()
                    .ok_or_else(|| RtError::new("TypeError", Some(span.clone())))?;
                let to_b = to
                    .extract_bytes()
                    .ok_or_else(|| RtError::new("TypeError", Some(span.clone())))?;
                let data = s.as_slice().to_vec();
                let mut out = Vec::new();
                let mut i = 0usize;
                while i < data.len() {
                    if from_b.is_empty() {
                        out.push(data[i]);
                        i += 1;
                    } else if i + from_b.len() <= data.len()
                        && &data[i..i + from_b.len()] == from_b.as_slice()
                    {
                        out.extend_from_slice(&to_b);
                        i += from_b.len();
                    } else {
                        out.push(data[i]);
                        i += 1;
                    }
                }
                Ok(Some(Value::str_bytes(out)))
            }
            (Value::Str(s), "len") => Ok(Some(Value::Int(s.borrow().len() as i128))),
            (Value::String(s), "len") => Ok(Some(Value::Int(s.len() as i128))),
            // G2（io 差异项）：to_upper/to_lower——ASCII 大小写转换（非 ASCII 字节不变）
            (Value::Str(s), "to_upper") | (Value::Str(s), "to_lower") => {
                let upper = field == "to_upper";
                let data = s.borrow();
                let out: Vec<u8> = data
                    .iter()
                    .map(|&b| {
                        if upper {
                            b.to_ascii_uppercase()
                        } else {
                            b.to_ascii_lowercase()
                        }
                    })
                    .collect();
                Ok(Some(Value::str_bytes(out)))
            }
            (Value::String(s), "to_upper") | (Value::String(s), "to_lower") => {
                let upper = field == "to_upper";
                let data = s.as_slice();
                let out: Vec<u8> = data
                    .iter()
                    .map(|&b| {
                        if upper {
                            b.to_ascii_uppercase()
                        } else {
                            b.to_ascii_lowercase()
                        }
                    })
                    .collect();
                Ok(Some(Value::str_bytes(out)))
            }
            (Value::Arr(_a), "len") => {
                if let Value::Arr(a) = self_v {
                    Ok(Some(Value::Int(a.borrow().len() as i128)))
                } else {
                    unreachable!()
                }
            }
            (Value::Arr(a), "append") => {
                let v = self.eval(&args[0])?;
                a.borrow_mut().push(Rc::new(RefCell::new(v)));
                Ok(Some(Value::Void))
            }
            // Deque 双端队列操作（M5.2：Vec/Deque 共享 Arr 值模型；方法按名分派）
            (Value::Arr(a), "push_back") => {
                let v = self.eval(&args[0])?;
                a.borrow_mut().push(Rc::new(RefCell::new(v)));
                Ok(Some(Value::Void))
            }
            (Value::Arr(a), "push_front") => {
                let v = self.eval(&args[0])?;
                a.borrow_mut().insert(0, Rc::new(RefCell::new(v)));
                Ok(Some(Value::Void))
            }
            (Value::Arr(a), "pop_back") => {
                let v = a.borrow_mut().pop().map(|cell| cell.borrow().clone());
                Ok(Some(match v {
                    Some(x) => Value::Opt(Some(Rc::new(x))),
                    None => Value::Opt(None),
                }))
            }
            (Value::Arr(a), "pop_front") => {
                let mut arr = a.borrow_mut();
                let v = if arr.is_empty() {
                    None
                } else {
                    Some(arr.remove(0).borrow().clone())
                };
                Ok(Some(match v {
                    Some(x) => Value::Opt(Some(Rc::new(x))),
                    None => Value::Opt(None),
                }))
            }
            (Value::Arr(a), "front") => {
                let v = a.borrow().first().map(|cell| cell.borrow().clone());
                Ok(Some(match v {
                    Some(x) => Value::Opt(Some(Rc::new(x))),
                    None => Value::Opt(None),
                }))
            }
            (Value::Arr(a), "back") => {
                let v = a.borrow().last().map(|cell| cell.borrow().clone());
                Ok(Some(match v {
                    Some(x) => Value::Opt(Some(Rc::new(x))),
                    None => Value::Opt(None),
                }))
            }
            // Deque 最小方法集：get ?T / put / remove（Map 同款语义）
            (Value::Arr(a), "get") => {
                let idx = self.eval(&args[0])?;
                let i = self.as_index(&idx, span)?;
                let v = a.borrow().get(i).map(|cell| cell.borrow().clone());
                Ok(Some(match v {
                    Some(x) => Value::Opt(Some(Rc::new(x))),
                    None => Value::Opt(None),
                }))
            }
            (Value::Arr(a), "put") => {
                let idx = self.eval(&args[0])?;
                let i = self.as_index(&idx, span)?;
                let v = self.eval(&args[1])?;
                let mut arr = a.borrow_mut();
                if i >= arr.len() {
                    return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                }
                arr[i] = Rc::new(RefCell::new(v));
                Ok(Some(Value::Void))
            }
            (Value::Arr(a), "remove") => {
                let idx = self.eval(&args[0])?;
                let i = self.as_index(&idx, span)?;
                let mut arr = a.borrow_mut();
                if i >= arr.len() {
                    return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                }
                Ok(Some(arr.remove(i).borrow().clone()))
            }
            // extend(other)：追加另一集合/字节串的全部元素（57-protocol-parse 帧拼接）
            (Value::Arr(a), "extend") => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                match v {
                    Value::Arr(src) => {
                        for c in src.borrow().iter() {
                            a.borrow_mut().push(c.clone());
                        }
                        Ok(Some(Value::Void))
                    }
                    Value::Str(b) => {
                        for byte in b.borrow().iter() {
                            a.borrow_mut()
                                .push(Rc::new(RefCell::new(Value::Int(*byte as i128))));
                        }
                        Ok(Some(Value::Void))
                    }
                    Value::String(s) => {
                        for byte in s.as_slice().iter() {
                            a.borrow_mut()
                                .push(Rc::new(RefCell::new(Value::Int(*byte as i128))));
                        }
                        Ok(Some(Value::Void))
                    }
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            // append_u64(v)：u64 LE 8 字节追加为元素（57-protocol-parse 长度前缀）
            (Value::Arr(a), "append_u64") => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                let n = match v {
                    Value::Int(i) => i as u64,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                for byte in n.to_le_bytes() {
                    a.borrow_mut()
                        .push(Rc::new(RefCell::new(Value::Int(byte as i128))));
                }
                Ok(Some(Value::Void))
            }
            // Vec<i32>.init(alloc)：集合空容器（G4：捕获分配器引用，缺省回退全局）
            (Value::Arr(_), "init") => {
                let alloc_v = if !args.is_empty() {
                    let a = self.eval(&args[0])?;
                    self.deref_value(a)
                } else {
                    Value::Alloc
                };
                Ok(Some(Value::vec(vec![], alloc_v)))
            }
            // Vec<i32>.from_bytes 集合反序列化（u64 长度前缀 + i32 元素）
            (Value::Arr(_), "from_bytes") => {
                let bytes = self.eval(&args[0])?;
                let bytes = self.deref_value(bytes);
                let b = match bytes {
                    Value::Str(s) => s.borrow().clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                if b.len() < 8 {
                    return Err(RtError::new("InvalidBytes", Some(span.clone())));
                }
                let n = u64::from_le_bytes(b[0..8].try_into().unwrap()) as usize;
                let mut items = Vec::new();
                let mut pos = 8usize;
                for _ in 0..n {
                    let v = if b.len() >= pos + 4 {
                        let i = i32::from_le_bytes(b[pos..pos + 4].try_into().unwrap());
                        pos += 4;
                        Value::Int(i as i128)
                    } else {
                        break;
                    };
                    items.push(v);
                }
                Ok(Some(Value::vec(items, Value::Alloc)))
            }
            // 惰性迭代器链（A7：iter/filter/map 返回 LazyIter，next() 按需求值）
            (_, "iter") => {
                if matches!(self_v, Value::LazyIter(_)) {
                    // 已是 LazyIter，直接返回自身
                    return Ok(Some(self_v.clone()));
                }
                let data = self.make_lazy_iter_data(self_v)?;
                Ok(Some(Value::LazyIter(Rc::new(RefCell::new(data)))))
            }
            (_, "filter") => {
                let f = self.eval(&args[0])?;
                let f = self.deref_value(f);
                if !matches!(f, Value::Closure(_)) {
                    return Err(RtError::new("TypeError", Some(span.clone())));
                }
                let li = match self_v {
                    Value::LazyIter(existing) => existing.clone(),
                    _ => Rc::new(RefCell::new(self.make_lazy_iter_data(self_v)?)),
                };
                li.borrow_mut().ops.push(LazyOp::Filter(f));
                Ok(Some(Value::LazyIter(li)))
            }
            (_, "map") => {
                let f = self.eval(&args[0])?;
                let f = self.deref_value(f);
                if !matches!(f, Value::Closure(_)) {
                    return Err(RtError::new("TypeError", Some(span.clone())));
                }
                let li = match self_v {
                    Value::LazyIter(existing) => existing.clone(),
                    _ => Rc::new(RefCell::new(self.make_lazy_iter_data(self_v)?)),
                };
                li.borrow_mut().ops.push(LazyOp::Map(f));
                Ok(Some(Value::LazyIter(li)))
            }
            // Map 方法（Map = class 实例，字段即键值）
            (Value::Class(c), "put") if c.borrow().name == "Map" => {
                let k = self.eval(&args[0])?;
                let v = self.eval(&args[1])?;
                let key = k.display();
                c.borrow_mut().fields.insert(key, v);
                Ok(Some(Value::Void))
            }
            (Value::Class(c), "get") if c.borrow().name == "Map" => {
                let k = self.eval(&args[0])?;
                let key = k.display();
                let v = c.borrow().fields.get(&key).cloned();
                Ok(Some(match v {
                    Some(x) => Value::Opt(Some(Rc::new(x))),
                    None => Value::Opt(None),
                }))
            }
            (Value::Class(c), "contains") if c.borrow().name == "Map" => {
                let k = self.eval(&args[0])?;
                let key = k.display();
                Ok(Some(Value::Bool(c.borrow().fields.contains_key(&key))))
            }
            (Value::Class(c), "remove") if c.borrow().name == "Map" => {
                let k = self.eval(&args[0])?;
                let key = k.display();
                c.borrow_mut().fields.remove(&key);
                Ok(Some(Value::Void))
            }
            (Value::Class(c), "len") if c.borrow().name == "Map" => {
                Ok(Some(Value::Int(c.borrow().fields.len() as i128)))
            }
            // 集合（G4）：Map 句柄方法（字段即键值，逻辑同 Class("Map")）
            (Value::Map(m), "put") => {
                let k = self.eval(&args[0])?;
                let v = self.eval(&args[1])?;
                let key = k.display();
                m.borrow_mut().fields.insert(key, v);
                Ok(Some(Value::Void))
            }
            (Value::Map(m), "get") => {
                let k = self.eval(&args[0])?;
                let key = k.display();
                let v = m.borrow().fields.get(&key).cloned();
                Ok(Some(match v {
                    Some(x) => Value::Opt(Some(Rc::new(x))),
                    None => Value::Opt(None),
                }))
            }
            (Value::Map(m), "contains") => {
                let k = self.eval(&args[0])?;
                let key = k.display();
                Ok(Some(Value::Bool(m.borrow().fields.contains_key(&key))))
            }
            (Value::Map(m), "remove") => {
                let k = self.eval(&args[0])?;
                let key = k.display();
                m.borrow_mut().fields.remove(&key);
                Ok(Some(Value::Void))
            }
            (Value::Map(m), "len") => Ok(Some(Value::Int(m.borrow().fields.len() as i128))),
            (
                Value::Slice {
                    data: _,
                    start: _,
                    len,
                },
                "len",
            ) => Ok(Some(Value::Int(*len as i128))),
            // 分配器方法
            (Value::Alloc, "init") => {
                // alloc.init(T) / alloc.init(T{...})
                if args.len() != 1 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                // 无参构造 alloc.init(T)：按类型创建空实例（字段逐赋值，definite assignment M2.5）
                if let Expr::Ident(tname, _) = &args[0] {
                    if let Some(TypeDef::Class { fields, .. }) = self.types.get(tname) {
                        let mut f = HashMap::new();
                        // 先克隆字段类型/默认值：default_value(&mut self) 具体化会重新借用 self
                        let ftypes: Vec<(String, Type, Option<Expr>)> = fields
                            .iter()
                            .map(|fd| (fd.name.clone(), fd.ty.clone(), fd.default.clone()))
                            .collect();
                        for (fname, fty, default) in &ftypes {
                            let val = if let Some(de) = default {
                                self.eval(de)?
                            } else {
                                self.default_value(Some(fty))?
                            };
                            f.insert(fname.clone(), val);
                        }
                        return Ok(Some(Value::class(tname, f)));
                    }
                    if self.types.contains_key(tname) {
                        // 枚举等：空变体
                        return Ok(Some(Value::Enum {
                            name: tname.clone(),
                            variant: "__none__".into(),
                            payload: None,
                        }));
                    }
                }
                // 带参构造 alloc.init(T{...})：字面量求值即实例
                let v = self.eval(&args[0])?;
                Ok(Some(v))
            }
            (Value::Alloc, "alloc") => {
                let n = self.eval(&args[0])?;
                let n = self.deref_value(n);
                if let Value::Int(i) = n {
                    match alloc_zeroed_bytes(i) {
                        Some(b) => {
                            // G5/§8.3 Debug 泄漏检测：登记分配（弱引用随值销毁失效）
                            let rc = Rc::new(RefCell::new(b));
                            self.alloc_tracker.borrow_mut().push(LeakRecord {
                                size: rc.borrow().len(),
                                line: span.line as u32,
                                weak: Rc::downgrade(&rc),
                            });
                            Ok(Some(Value::Str(rc)))
                        }
                        None => Ok(Some(self.err_val("OutOfMemory"))),
                    }
                } else {
                    Err(RtError::new("TypeError", Some(span.clone())))
                }
            }
            // G5/§8.3 Debug 泄漏检测：当前活跃（未释放）分配数
            (Value::Alloc, "leaks") => {
                let n = self
                    .alloc_tracker
                    .borrow()
                    .iter()
                    .filter(|r| r.weak.upgrade().is_some())
                    .count();
                Ok(Some(Value::int(n as i128)))
            }
            // G5/§8.3 Debug 泄漏检测：泄漏清单文本（`leak: line L: N bytes` 每行）
            (Value::Alloc, "leak_report") => {
                let mut out = Vec::new();
                for r in self.alloc_tracker.borrow().iter() {
                    if r.weak.upgrade().is_some() {
                        out.extend_from_slice(
                            format!("leak: line {}: {} bytes\n", r.line, r.size).as_bytes(),
                        );
                    }
                }
                Ok(Some(Value::str_bytes(out)))
            }
            (Value::Alloc, "deinit") => Ok(Some(Value::Void)),
            // G5/§8.3 Debug 泄漏检测：断言无泄漏——有活跃分配则返回错误
            (Value::Alloc, "assert_no_leaks") => {
                let leaks: Vec<String> = self
                    .alloc_tracker
                    .borrow()
                    .iter()
                    .filter(|r| r.weak.upgrade().is_some())
                    .map(|r| format!("leak: line {}: {} bytes", r.line, r.size))
                    .collect();
                if leaks.is_empty() {
                    Ok(Some(Value::Void))
                } else {
                    Err(RtError::msg(
                        "LeakDetected",
                        format!(
                            "{} allocation(s) not freed:\n{}",
                            leaks.len(),
                            leaks.join("\n")
                        ),
                    ))
                }
            }
            // Arena 方法（G1：bump + 块链表 + deinit 批量归还 + 统计）
            (Value::Arena(a), m) => self.call_arena_method(a.clone(), m, args, span),
            // Allocator 方法（Phase 1：统一分配器接口，替代 Value::Alloc / Value::Arena）
            (Value::Allocator(a), "alloc") => {
                let n = if args.is_empty() {
                    // 无参 alloc → Pool 使用 item_size；否则 ArityMismatch
                    match &*a.borrow() {
                        AllocatorImpl::Pool(p) => Value::Int(p.borrow().item_size as i128),
                        _ => return Err(RtError::new("ArityMismatch", Some(span.clone()))),
                    }
                } else {
                    self.eval(&args[0])?
                };
                let n = self.deref_value(n);
                if let Value::Int(i) = n {
                    if i < 0 {
                        return Err(RtError::new("TypeError", Some(span.clone())));
                    }
                    if i as u128 > usize::MAX as u128 {
                        return Ok(Some(self.err_val("OutOfMemory")));
                    }
                    let n = i as usize;
                    match a.borrow_mut().alloc(n) {
                        Ok(block) => {
                            let data = {
                                let b = block.data.borrow();
                                b[block.offset..block.offset + block.len].to_vec()
                            };
                            let rc = Rc::new(RefCell::new(data));
                            // G5/§8.3 Debug 泄漏检测：登记分配
                            self.alloc_tracker.borrow_mut().push(LeakRecord {
                                size: rc.borrow().len(),
                                line: span.line as u32,
                                weak: Rc::downgrade(&rc),
                            });
                            Ok(Some(Value::Bytes(rc)))
                        }
                        Err(AllocErr::OutOfMemory) => Ok(Some(self.err_val("OutOfMemory"))),
                        Err(AllocErr::InvalidSize) => {
                            Err(RtError::new("TypeError", Some(span.clone())))
                        }
                    }
                } else {
                    Err(RtError::new("TypeError", Some(span.clone())))
                }
            }
            (Value::Allocator(a), "free") => {
                if args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let ptr = self.eval(&args[0])?;
                let ptr = self.deref_value(ptr);
                match ptr {
                    Value::Bytes(data) => {
                        let len = data.borrow().len();
                        let block = AllocBlock {
                            data,
                            offset: 0,
                            len,
                        };
                        a.borrow_mut().free(&block);
                        Ok(Some(Value::Void))
                    }
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            (Value::Allocator(a), "bytes") => {
                if !args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let total = match &*a.borrow() {
                    AllocatorImpl::Arena(arena) => arena.borrow().total as i128,
                    _ => 0,
                };
                Ok(Some(Value::int(total)))
            }
            (Value::Allocator(_), "leaks") => {
                let n = self
                    .alloc_tracker
                    .borrow()
                    .iter()
                    .filter(|r| r.weak.upgrade().is_some())
                    .count();
                Ok(Some(Value::int(n as i128)))
            }
            (Value::Allocator(_), "leak_report") => {
                let mut out = Vec::new();
                for r in self.alloc_tracker.borrow().iter() {
                    if r.weak.upgrade().is_some() {
                        out.extend_from_slice(
                            format!("leak: line {}: {} bytes\n", r.line, r.size).as_bytes(),
                        );
                    }
                }
                Ok(Some(Value::str_bytes(out)))
            }
            (Value::Allocator(a), "deinit") => {
                a.borrow_mut().deinit();
                Ok(Some(Value::Void))
            }
            // G5/§8.3 Debug 泄漏检测：断言无泄漏
            (Value::Allocator(_), "assert_no_leaks") => {
                let leaks: Vec<String> = self
                    .alloc_tracker
                    .borrow()
                    .iter()
                    .filter(|r| r.weak.upgrade().is_some())
                    .map(|r| format!("leak: line {}: {} bytes", r.line, r.size))
                    .collect();
                if leaks.is_empty() {
                    Ok(Some(Value::Void))
                } else {
                    Err(RtError::msg(
                        "LeakDetected",
                        format!(
                            "{} allocation(s) not freed:\n{}",
                            leaks.len(),
                            leaks.join("\n")
                        ),
                    ))
                }
            }
            (Value::Allocator(_), "init") => {
                // allocator.init(T) / allocator.init(T{...})
                if args.len() != 1 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                if let Expr::Ident(tname, _) = &args[0] {
                    if let Some(TypeDef::Class { fields, .. }) = self.types.get(tname) {
                        let mut f = HashMap::new();
                        let ftypes: Vec<(String, Type, Option<Expr>)> = fields
                            .iter()
                            .map(|fd| (fd.name.clone(), fd.ty.clone(), fd.default.clone()))
                            .collect();
                        for (fname, fty, default) in &ftypes {
                            let val = if let Some(de) = default {
                                self.eval(de)?
                            } else {
                                self.default_value(Some(fty))?
                            };
                            f.insert(fname.clone(), val);
                        }
                        return Ok(Some(Value::class(tname, f)));
                    }
                    if self.types.contains_key(tname) {
                        return Ok(Some(Value::Enum {
                            name: tname.clone(),
                            variant: "__none__".into(),
                            payload: None,
                        }));
                    }
                }
                let v = self.eval(&args[0])?;
                Ok(Some(v))
            }
            (Value::Allocator(a), "blocks") => {
                if !args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let n = match &*a.borrow() {
                    AllocatorImpl::Arena(arena) => arena.borrow().blocks.len() as i128,
                    _ => 0,
                };
                Ok(Some(Value::int(n)))
            }
            // Io 方法
            (Value::Class(c), "print") if c.borrow().name == "Io" => {
                self.call_io_print(args, span)?;
                Ok(Some(Value::Void))
            }
            // 组 E E3：io.poll()——evented 事件循环一轮（运行待处理延迟任务，返回事件数）
            (Value::Class(c), "poll") if c.borrow().name == "Io" => {
                let v = Value::Class(c.clone());
                let n = self.io_poll(&v, args, span)?;
                Ok(Some(Value::Int(n as i128)))
            }
            // io.exit(ExitType, code)（M4.2：Exit 静默正常 / Error 错误退出打印）
            (Value::Class(c), "exit") if c.borrow().name == "Io" => {
                if args.len() != 2 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let t = self.eval(&args[0])?;
                let code = self.eval(&args[1])?;
                let t = self.deref_value(t);
                let code = match self.deref_value(code) {
                    Value::Int(i) => i.clamp(0, 255) as u8,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let is_error = matches!(t, Value::Enum { variant, .. } if variant == "Error");
                if is_error {
                    eprintln!("error: program exited with code {code}");
                }
                self.exit_code = Some(code);
                // 中止执行（正常退出信号）
                Err(RtError::msg("ExitRequested", format!("code {code}")))
            }
            // 序列化内建（M4.4；所有数据类型天生可序列化）
            (Value::Class(c), "to_bytes") => {
                let d = c.borrow();
                let bytes = self.class_to_bytes(&d.name, &d.fields)?;
                Ok(Some(Value::str_bytes(bytes)))
            }
            (Value::Class(c), "to_json") => {
                let _d = c.borrow();
                let json = self.value_to_json(&Value::Class(c.clone()));
                Ok(Some(Value::str(&json)))
            }
            (Value::Arr(a), "to_bytes") => {
                // 集合 → 字节（u64 LE 前缀 + 元素，M4.4）
                let items = a.borrow().clone();
                let mut out = Vec::new();
                out.extend_from_slice(&(items.len() as u64).to_le_bytes());
                for cell in items {
                    let v = cell.borrow().clone();
                    out.extend(self.value_to_bytes(&v));
                }
                Ok(Some(Value::str_bytes(out)))
            }
            (Value::Class(c), "from_json") if c.borrow().name == "Map" => {
                let json = self.eval(&args[0])?;
                let json = self.deref_value(json);
                if let Value::Str(s) = json {
                    let s = s.borrow().clone();
                    let obj = self.parse_json_obj(&String::from_utf8_lossy(&s))?;
                    let mut f = HashMap::new();
                    for (k, v) in obj {
                        f.insert(k, v);
                    }
                    Ok(Some(Value::class("Map", f)))
                } else {
                    Err(RtError::new("TypeError", Some(span.clone())))
                }
            }
            // 集合（G4）：Value::Map 形态的 init（捕获分配器引用）
            (Value::Map(m), "init") => {
                let alloc_v = if !args.is_empty() {
                    let a = self.eval(&args[0])?;
                    self.deref_value(a)
                } else {
                    Value::Alloc
                };
                let _ = m;
                Ok(Some(Value::map(HashMap::new(), alloc_v)))
            }
            // 集合（G4）：Value::Map 形态的 from_json / to_json
            (Value::Map(m), "from_json") => {
                let alloc = m.borrow().alloc.clone();
                let json = self.eval(&args[0])?;
                let json = self.deref_value(json);
                if let Value::Str(s) = json {
                    let s = s.borrow().clone();
                    let obj = self.parse_json_obj(&String::from_utf8_lossy(&s))?;
                    Ok(Some(Value::map(obj, alloc)))
                } else {
                    Err(RtError::new("TypeError", Some(span.clone())))
                }
            }
            (Value::Map(m), "to_json") => {
                let json = self.value_to_json(&Value::Map(m.clone()));
                Ok(Some(Value::str(&json)))
            }
            // M5.4 真实 IO：io.fs 模块函数 / io.time / File 句柄方法 / io.net
            (Value::Class(c), m) if c.borrow().name == "Fs" => self.call_fs_method(m, args, span),
            (Value::Class(c), m) if c.borrow().name == "Time" => {
                self.call_time_method(m, args, span)
            }
            (Value::Class(c), m) if c.borrow().name == "Net" => self.call_net_method(m, args, span),
            // G1（E3.1）：UDP——`io.net.udp` 命名空间（bind）与 UdpSocket 实例方法
            (Value::Class(c), m) if c.borrow().name == "Udp" => {
                self.call_udp_ns_method(m, args, span)
            }
            (Value::Class(c), m) if c.borrow().name == "UdpSocket" => {
                let v = Value::Class(c.clone());
                self.call_udp_socket_method(m, &v, args, span)
            }
            (Value::Class(c), m) if c.borrow().name == "TcpConn" => {
                let v = Value::Class(c.clone());
                self.call_conn_method(m, &v, args, span)
            }
            (Value::Class(c), m) if c.borrow().name == "TcpListener" => {
                let v = Value::Class(c.clone());
                self.call_listener_method(m, &v, args, span)
            }
            (Value::Class(c), m) if c.borrow().name == "File" => {
                let v = Value::Class(c.clone());
                self.call_file_method(m, &v, args, span)
            }
            // G2（io 差异项）：Dir 类方法 list_dir(alloc) / close()（DirEntry 仅持字段，
            // .name/.is_dir 走 eval_field，无需方法分派）
            (Value::Class(c), m) if c.borrow().name == "Dir" => {
                let v = Value::Class(c.clone());
                self.call_dir_method(m, &v, args, span)
            }
            // G3（E3.2 ipc）：io.ipc 命名空间 pipe()/shm()；Pipe 两端/Shm 实例方法
            (Value::Class(c), m) if c.borrow().name == "Ipc" => self.call_ipc_method(m, args, span),
            (Value::Class(c), m) if c.borrow().name == "PipeReader" => {
                let v = Value::Class(c.clone());
                self.call_pipe_method(true, m, &v, args, span)
            }
            (Value::Class(c), m) if c.borrow().name == "PipeWriter" => {
                let v = Value::Class(c.clone());
                self.call_pipe_method(false, m, &v, args, span)
            }
            (Value::Class(c), m) if c.borrow().name == "Shm" => {
                let v = Value::Class(c.clone());
                self.call_shm_method(m, &v, args, span)
            }
            // G4（E3.3 storage）：io.storage 命名空间 open；KvStore 实例方法
            (Value::Class(c), m) if c.borrow().name == "Storage" => {
                self.call_storage_method(m, args, span)
            }
            (Value::Class(c), m) if c.borrow().name == "KvStore" => {
                let v = Value::Class(c.clone());
                self.call_store_method(m, &v, args, span)
            }
            // G4（E3.3 archive）：io.archive 命名空间 compress/decompress
            (Value::Class(c), m) if c.borrow().name == "Archive" => {
                self.call_archive_method(m, args, span)
            }
            // G5（E3.3 text/rng）：io.text 命名空间正则；io.rng 命名空间伪随机数
            (Value::Class(c), m) if c.borrow().name == "Text" => {
                self.call_text_method(m, args, span)
            }
            (Value::Class(c), m) if c.borrow().name == "RngNs" => {
                self.call_rng_method(m, args, span)
            }
            // A6：标准库数据结构——Bitmap 位图命名空间
            (Value::Class(c), m) if c.borrow().name == "BitmapNs" => {
                self.call_bitmap_ns_method(m, args, span)
            }
            (Value::Class(c), m) if c.borrow().name == "Bitmap" => {
                let v = Value::Class(c.clone());
                self.call_bitmap_method(m, &v, args, span)
            }
            // A6：标准库数据结构——RingBuf 环形缓冲命名空间
            (Value::Class(c), m) if c.borrow().name == "RingBufNs" => {
                self.call_ringbuf_ns_method(m, args, span)
            }
            (Value::Class(c), m) if c.borrow().name == "RingBuf" => {
                let v = Value::Class(c.clone());
                self.call_ringbuf_method(m, &v, args, span)
            }
            // A6：标准库数据结构——PageMem 页内存池命名空间
            (Value::Class(c), m) if c.borrow().name == "PageMemNs" => {
                self.call_pagemem_ns_method(m, args, span)
            }
            (Value::Class(c), m) if c.borrow().name == "PageMem" => {
                let v = Value::Class(c.clone());
                self.call_pagemem_method(m, &v, args, span)
            }
            // A6：标准库数据结构——IntrList 侵入式链表命名空间
            (Value::Class(c), m) if c.borrow().name == "IntrListNs" => {
                self.call_intrlist_ns_method(m, args, span)
            }
            (Value::Class(c), m) if c.borrow().name == "IntrList" => {
                let v = Value::Class(c.clone());
                self.call_intrlist_method(m, &v, args, span)
            }
            // A6：标准库数据结构——TreeMap 有序映射命名空间
            (Value::Class(c), m) if c.borrow().name == "TreeMapNs" => {
                self.call_treemap_ns_method(m, args, span)
            }
            (Value::Class(c), m) if c.borrow().name == "TreeMap" => {
                let v = Value::Class(c.clone());
                self.call_treemap_method(m, &v, args, span)
            }
            // io.stdin()：从标准输入读一行（无缓冲换行去除）
            (Value::Class(c), "stdin") if c.borrow().name == "Io" => {
                let mut line = String::new();
                match std::io::stdin().read_line(&mut line) {
                    Ok(0) | Err(_) => Ok(Some(Value::str(""))),
                    Ok(_) => {
                        let trimmed = line.trim_end_matches(['\n', '\r']);
                        Ok(Some(Value::str(trimmed)))
                    }
                }
            }
            // G2（io 差异项）：io.stdout.write_all(data) / io.stderr.write_all(data)
            // ——写真实标准输出/错误流（stdout/stderr 为独立字节流，Q 程序环境形态）
            (Value::Class(c), "write_all") if c.borrow().name == "Stdout" => {
                let data = self.eval_str_arg(args, 0, span)?;
                use std::io::Write;
                match std::io::stdout().lock().write_all(&data) {
                    Ok(_) => Ok(Some(Value::Void)),
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            (Value::Class(c), "write_all") if c.borrow().name == "Stderr" => {
                let data = self.eval_str_arg(args, 0, span)?;
                use std::io::Write;
                match std::io::stderr().lock().write_all(&data) {
                    Ok(_) => Ok(Some(Value::Void)),
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            // M5.4 程序环境：io.env(name)
            (Value::Class(c), "env") if c.borrow().name == "Io" => {
                let name = self.eval_str_arg(args, 0, span)?;
                match std::env::var(String::from_utf8_lossy(&name).as_ref()) {
                    Ok(v) => Ok(Some(Value::Opt(Some(Rc::new(Value::str(&v)))))),
                    Err(_) => Ok(Some(Value::Opt(None))),
                }
            }
            // E2.2 线程生命周期（组 G）：Thread 类方法 join/cancel/is_done/detach
            (Value::Class(c), m) if c.borrow().name == "Thread" => {
                let v = Value::Class(c.clone());
                self.call_thread_method(&v, m, args, span)
            }
            // 组 E E2：Future 类方法 cancel/is_done（协作式取消，对齐 Thread）
            (Value::Class(c), m) if c.borrow().name == "Future" => {
                let v = Value::Class(c.clone());
                self.call_future_method(&v, m, args, span)
            }
            // 组 F：四模式容器 init(alloc[, cap]) 构造（base 类型实例化标记 → 真实容器）
            (Value::Class(c), "init") if is_four_mode_type(&c.borrow().name) => {
                let name = c.borrow().name.clone();
                Ok(Some(self.make_four_mode_container(&name, args, span)?))
            }
            // 组 F：四模式容器方法 write/read/try_read/close/send/recv
            (Value::Class(c), m) if is_four_mode_type(&c.borrow().name) => {
                let v = Value::Class(c.clone());
                self.call_four_mode_method(&v, m, args, span)
            }
            // LazyIter 方法：next() 按需求值，to_array() 解析全部剩余项
            (Value::LazyIter(li), "next") => {
                let v = self.lazy_iter_next(&mut li.borrow_mut(), span)?;
                Ok(Some(v))
            }
            (Value::LazyIter(li), "to_array") => {
                let mut items = Vec::new();
                loop {
                    let v = self.lazy_iter_next(&mut li.borrow_mut(), span)?;
                    match v {
                        Value::Opt(Some(val)) => items.push((*val).clone()),
                        Value::Opt(None) => break,
                        _ => break,
                    }
                }
                Ok(Some(Value::arr(items)))
            }
            // E4：Mutex 方法 lock/try_lock
            (Value::Mutex(m), "lock") => match m.lock() {
                Ok(v) => Ok(Some(v.clone())),
                Err(_) => Err(RtError::new("MutexPoisoned", Some(span.clone()))),
            },
            (Value::Mutex(m), "try_lock") => match m.try_lock() {
                Ok(v) => Ok(Some(Value::Opt(Some(Rc::new(v.clone()))))),
                Err(_) => Ok(Some(Value::Opt(None))),
            },
            // E4：chan<T> 方法（委托到 chan.rs 模块）
            (Value::Chan(ch), _) => {
                return self.call_chan_method(ch, field, args, span);
            }
            _ => Ok(None),
        }
    }

    // ---------- LazyIter 辅助方法 ----------

    /// 根据值创建 LazyIterData（非 LazyIter 可迭代值 → LazyIterData）
    fn make_lazy_iter_data(&self, v: &Value) -> Result<LazyIterData> {
        match v {
            Value::Arr(_) => Ok(LazyIterData {
                source: v.clone(),
                index: 0,
                source_type: "arr".to_string(),
                ops: Vec::new(),
                keys_cache: Vec::new(),
            }),
            Value::Slice { .. } => Ok(LazyIterData {
                source: v.clone(),
                index: 0,
                source_type: "slice".to_string(),
                ops: Vec::new(),
                keys_cache: Vec::new(),
            }),
            Value::Str(_) => Ok(LazyIterData {
                source: v.clone(),
                index: 0,
                source_type: "str".to_string(),
                ops: Vec::new(),
                keys_cache: Vec::new(),
            }),
            Value::Class(c) if c.borrow().name == "Map" => {
                let keys_cache = c.borrow().fields.keys().cloned().collect();
                Ok(LazyIterData {
                    source: v.clone(),
                    index: 0,
                    source_type: "map".to_string(),
                    ops: Vec::new(),
                    keys_cache,
                })
            }
            Value::Map(m) => {
                let keys_cache = m.borrow().fields.keys().cloned().collect();
                Ok(LazyIterData {
                    source: v.clone(),
                    index: 0,
                    source_type: "map".to_string(),
                    ops: Vec::new(),
                    keys_cache,
                })
            }
            Value::Class(_) => Ok(LazyIterData {
                source: v.clone(),
                index: 0,
                source_type: "class".to_string(),
                ops: Vec::new(),
                keys_cache: Vec::new(),
            }),
            _ => Err(RtError::msg(
                "NotIterable",
                format!("value of type `{}` is not iterable", v.type_name()),
            )),
        }
    }

    /// LazyIter 按需求值：从源取下一元素，经 filter/map 链后返回 `Opt(T)`
    pub(crate) fn lazy_iter_next(&mut self, data: &mut LazyIterData, span: &Span) -> Result<Value> {
        loop {
            // 克隆源以避免与 data.index 的借用冲突
            let source = data.source.clone();
            let element = match data.source_type.as_str() {
                "arr" => {
                    let Value::Arr(a) = &source else {
                        return Err(RtError::msg(
                            "InternalError",
                            "lazy_iter: arr source_type mismatch",
                        ));
                    };
                    let items = a.borrow();
                    if data.index >= items.len() {
                        return Ok(Value::Opt(None));
                    }
                    let v = items[data.index].borrow().clone();
                    data.index += 1;
                    v
                }
                "slice" => {
                    let Value::Slice {
                        data: d,
                        start,
                        len,
                    } = &source
                    else {
                        return Err(RtError::msg(
                            "InternalError",
                            "lazy_iter: slice source_type mismatch",
                        ));
                    };
                    let items = d.borrow();
                    if data.index >= *len {
                        return Ok(Value::Opt(None));
                    }
                    let v = items[*start + data.index].borrow().clone();
                    data.index += 1;
                    v
                }
                "str" => {
                    let Value::Str(s) = &source else {
                        return Err(RtError::msg(
                            "InternalError",
                            "lazy_iter: str source_type mismatch",
                        ));
                    };
                    let bytes = s.borrow();
                    if data.index >= bytes.len() {
                        return Ok(Value::Opt(None));
                    }
                    let v = Value::Int(bytes[data.index] as i128);
                    data.index += 1;
                    v
                }
                "map" => {
                    if data.index >= data.keys_cache.len() {
                        return Ok(Value::Opt(None));
                    }
                    let key = &data.keys_cache[data.index];
                    data.index += 1;
                    let val = match &source {
                        Value::Class(c) => c.borrow().fields.get(key).cloned(),
                        Value::Map(m) => m.borrow().fields.get(key).cloned(),
                        _ => None,
                    };
                    let val = val.unwrap_or(Value::Void);
                    let mut f = HashMap::new();
                    f.insert("key".to_string(), Value::str(key));
                    f.insert("value".to_string(), val);
                    Value::class("KV", f)
                }
                "class" => {
                    // 用户类型迭代：委托给源自身的 next() 方法
                    let next_v = self.eval_next_method(&source)?;
                    match next_v {
                        Value::Opt(Some(v)) => (*v).clone(),
                        Value::Opt(None) | Value::Void => return Ok(Value::Opt(None)),
                        other => other,
                    }
                }
                other => {
                    return Err(RtError::msg(
                        "NotIterable",
                        format!("lazy_iter: unknown source_type `{other}`"),
                    ));
                }
            };

            // 按链式调用顺序应用所有操作（filter/map 交错）
            let mut result = element;
            let mut skipped = false;
            for op in &data.ops {
                match op {
                    LazyOp::Filter(v) => {
                        if let Value::Closure(c) = v {
                            if !self.call_closure_bool(c, &[result.clone()], span)? {
                                skipped = true;
                                break;
                            }
                        }
                    }
                    LazyOp::Map(v) => {
                        if let Value::Closure(c) = v {
                            result = self.call_closure_value(c, &[result], span)?;
                        }
                    }
                }
            }
            if skipped {
                continue;
            }

            return Ok(Value::Opt(Some(Rc::new(result))));
        }
    }

    // ---------- E4 true-OMP：线程生命周期（OS 线程） ----------

    /// Thread 类方法分派：`join() !T`（等待 OS 线程结束返回结果）/ `cancel() !void`（设置取消标志）/
    /// `is_done() bool` / `detach()`（标记分离，线程继续运行，程序结束时不等待）。
    pub(crate) fn call_thread_method(
        &mut self,
        self_v: &Value,
        m: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        if !args.is_empty() {
            return Err(RtError::new("ArityMismatch", Some(span.clone())));
        }
        let tid = self.get_thread_tid(self_v);
        match m {
            "join" => {
                let result = self.thread_join(tid, span)?;
                self.thread_set_bool(self_v, "done", true);
                Ok(Some(result))
            }
            "detach" => {
                // OS 线程已运行，仅标记分离（程序结束时不等待）
                self.thread_set_bool(self_v, "detached", true);
                Ok(Some(Value::Void))
            }
            "is_done" => {
                // 先查 thread_handles（运行中线程的实时状态）
                if let Some(th) = self.thread_handles.get(&tid) {
                    return Ok(Some(Value::Bool(th.done.load(Ordering::SeqCst))));
                }
                // 已从 thread_handles 移除（joined），查 Thread 类字段
                let done = self.thread_field_bool(self_v, "done");
                Ok(Some(Value::Bool(done)))
            }
            "cancel" => {
                if let Some(th) = self.thread_handles.get(&tid) {
                    th.cancel.store(true, Ordering::SeqCst);
                }
                self.thread_set_bool(self_v, "cancel", true);
                Ok(Some(Value::Void))
            }
            _ => Ok(None),
        }
    }

    /// 提取 Thread 类中的 `_tid` 字段
    pub(crate) fn get_thread_tid(&self, thread: &Value) -> i64 {
        let tv = self.deref_value(thread.clone());
        if let Value::Class(c) = tv {
            let d = c.borrow();
            if let Some(Value::Int(tid)) = d.fields.get("_tid") {
                return *tid as i64;
            }
        }
        0
    }

    /// 等待协程结束并返回结果
    fn thread_join(&mut self, tid: i64, span: &Span) -> Result<Value> {
        let mut handle = self
            .thread_handles
            .remove(&tid)
            .ok_or_else(|| RtError::new("TypeError", Some(span.clone())))?;

        // 等待协程完成（轮询 done 标志，worker 线程执行任务）
        while !handle.done.load(Ordering::SeqCst) {
            thread::yield_now();
        }

        // 获取结果
        let mut r = handle.result.lock().unwrap();
        match r.take() {
            Some(ThreadResult::Ok(v)) => Ok(v),
            Some(ThreadResult::Err(e)) => Err(e),
            None => Err(RtError::msg(
                "ThreadNoResult",
                format!("thread {tid} produced no result"),
            )),
        }
    }

    /// 非失败版 thread_join（用于 drain_root_threads，丢弃错误）
    pub(crate) fn thread_join_impl(&mut self, tid: i64) {
        if let Some(handle) = self.thread_handles.remove(&tid) {
            // 等待协程完成
            while !handle.done.load(Ordering::SeqCst) {
                thread::yield_now();
            }
        }
    }

    /// 读取线程缓存结果（Future/await 用；done 或已取消时有效）
    pub(crate) fn thread_result(&self, thread: &Value, span: &Span) -> Result<Value> {
        let tv = self.deref_value(thread.clone());
        if let Value::Class(c) = tv {
            let d = c.borrow();
            if let Some(v) = d.fields.get("result") {
                return Ok(v.clone());
            }
        }
        Err(RtError::new("TypeError", Some(span.clone())))
    }

    pub(crate) fn thread_set_bool(&self, thread: &Value, key: &str, v: bool) {
        if let Value::Class(c) = thread {
            c.borrow_mut()
                .fields
                .insert(key.to_string(), Value::Bool(v));
        }
    }

    pub(crate) fn thread_field_bool(&self, thread: &Value, key: &str) -> bool {
        if let Value::Class(c) = thread {
            if let Some(Value::Bool(b)) = c.borrow().fields.get(key) {
                return *b;
            }
        }
        false
    }

    pub(crate) fn thread_set_value(&self, thread: &Value, key: &str, v: Value) {
        if let Value::Class(c) = thread {
            c.borrow_mut().fields.insert(key.to_string(), v);
        }
    }

    // ---------- 组 E E2：协作式 Future（await ≡ join()，复用 G 组 Thread 机制） ----------

    /// async fn 调用点：普通函数同步调用；`async fn` 延迟执行——返回 `Future(R)` 值
    /// （捕获 callee + 已求值实参 + 每 Future 独立 alloc，await 时运行体到完成）。
    pub(crate) fn call_or_defer(
        &mut self,
        fdef: &FnDef,
        arg_vals: &[Value],
        span: &Span,
    ) -> Result<Value> {
        if fdef.is_async {
            Ok(self.make_future(fdef, arg_vals.to_vec()))
        } else {
            self.call_fn(fdef, arg_vals, span)
        }
    }

    /// 构造 `Future(R)` 值：fields 布局对齐 Thread（fn/args/alloc/done/result/cancel）。
    /// 实参已在调用点求值（Zig 语义：参数值随调用捕获，体延迟到 await）。
    pub(crate) fn make_future(&self, fdef: &FnDef, arg_vals: Vec<Value>) -> Value {
        // Q8 对齐：每 Future 独立 alloc 实例（体执行时绑定为 alloc）
        let alloc_v = Value::Arena(Rc::new(RefCell::new(ArenaState::new())));
        let mut f = HashMap::new();
        f.insert("fn".to_string(), Value::Fn(fdef.name.clone()));
        f.insert("args".to_string(), Value::arr(arg_vals));
        f.insert("alloc".to_string(), alloc_v);
        f.insert("cancel".to_string(), Value::Bool(false));
        f.insert("done".to_string(), Value::Bool(false));
        f.insert("result".to_string(), Value::Void);
        Value::class("Future", f)
    }

    /// Future 类方法分派：`cancel() !void`（协作标志，await 时检查后跳过执行 →
    /// `error.Cancelled`）/ `is_done() bool`（await 后 true）。await 本体经 `Expr::Await`
    /// 直接走 `future_run`（不依赖方法分派）。
    pub(crate) fn call_future_method(
        &mut self,
        self_v: &Value,
        m: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        if !args.is_empty() {
            return Err(RtError::new("ArityMismatch", Some(span.clone())));
        }
        match m {
            "cancel" => {
                self.thread_set_bool(self_v, "cancel", true);
                Ok(Some(Value::Void))
            }
            "is_done" => {
                let done = self.thread_field_bool(self_v, "done");
                Ok(Some(Value::Bool(done)))
            }
            _ => Ok(None),
        }
    }

    // ---------- 组 F：四模式共享容器（Pipe/Tee/Funnel/Hub） ----------

    /// 四模式容器构造：`Pipe<T>.init(alloc[, cap])`。fields 布局：
    /// `queue`（FIFO 元素数组）/ `closed`（结束标志）/ `alloc`（分配器引用）/
    /// `cap`（仅通道形态 init(alloc, cap)——send/recv 有界）。
    pub(crate) fn make_four_mode_container(
        &mut self,
        name: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Value> {
        if args.is_empty() || args.len() > 2 {
            return Err(RtError::new("ArityMismatch", Some(span.clone())));
        }
        let alloc_v = {
            let a = self.eval(&args[0])?;
            self.deref_value(a)
        };
        let mut f = HashMap::new();
        f.insert("closed".to_string(), Value::Bool(false));
        f.insert("alloc".to_string(), alloc_v);
        if args.len() == 2 {
            let cap = self.eval(&args[1])?;
            let cap = self.deref_value(cap);
            let cap_i = match cap {
                Value::Int(i) => i.max(0),
                _ => return Err(RtError::new("TypeError", Some(span.clone()))),
            };
            f.insert("cap".to_string(), Value::Int(cap_i));
        }
        // E4：Pipe 使用 mpsc 通道（无锁 SPSC）
        if name == "Pipe" {
            let (tx, rx) = mpsc::channel();
            let cid = self.next_channel_id;
            self.next_channel_id += 1;
            self.channels.insert(
                cid,
                ChannelState::Pipe {
                    sender: tx,
                    receiver: rx,
                },
            );
            f.insert("_chan_id".to_string(), Value::Int(cid as i128));
        } else if name == "Tee" || name == "Funnel" || name == "Hub" {
            // E4：Tee/Funnel/Hub 使用 Mutex+Condvar 队列
            let queue = Arc::new(Mutex::new(VecDeque::new()));
            let condvar = Arc::new(Condvar::new());
            let cid = self.next_channel_id;
            self.next_channel_id += 1;
            let state = match name {
                "Tee" => ChannelState::Tee {
                    queue: queue.clone(),
                    condvar: condvar.clone(),
                },
                "Funnel" => ChannelState::Funnel {
                    queue: queue.clone(),
                    condvar: condvar.clone(),
                },
                "Hub" => ChannelState::Hub {
                    queue: queue.clone(),
                    condvar: condvar.clone(),
                },
                _ => unreachable!(),
            };
            self.channels.insert(cid, state);
            f.insert("_chan_id".to_string(), Value::Int(cid as i128));
        } else {
            // 其他模式保持 Vec 实现（后续升级）
            f.insert("queue".to_string(), Value::arr(vec![]));
        }
        Ok(Value::class(name, f))
    }

    /// 四模式容器方法分派：write/read/try_read/close（共享内存形态，write/read 无容量
    /// 概念）+ send/recv（通道形态，有界队列）。协作式单线程下四变体运行时行为相同——
    /// 读者/写者数量是类型层契约，不引入真锁/真并发（ADR-0011 协作式模型）。
    pub(crate) fn call_four_mode_method(
        &mut self,
        self_v: &Value,
        m: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        let tv = self.deref_value(self_v.clone());
        let (name, closed, ccell) = match &tv {
            Value::Class(c) => {
                let d = c.borrow();
                let closed = matches!(d.fields.get("closed"), Some(Value::Bool(true)));
                (d.name.clone(), closed, c.clone())
            }
            _ => return Err(RtError::new("TypeError", Some(span.clone()))),
        };
        // E4：Pipe 使用 mpsc 通道
        if name == "Pipe" {
            let cid = match ccell.borrow().fields.get("_chan_id") {
                Some(Value::Int(id)) => *id,
                _ => return Err(RtError::new("TypeError", Some(span.clone()))),
            };
            return self.call_one_to_one_method(cid, &name, m, args, span, closed, ccell);
        }
        // E4：Tee/Funnel/Hub 使用 Mutex+Condvar 队列
        if name == "Tee" || name == "Funnel" || name == "Hub" {
            let cid = match ccell.borrow().fields.get("_chan_id") {
                Some(Value::Int(id)) => *id,
                _ => return Err(RtError::new("TypeError", Some(span.clone()))),
            };
            return self.call_mutex_condvar_method(cid, &name, m, args, span, closed, ccell);
        }
        // 原有 Vec 实现（其他模式，如 Deque 等）
        let (queue, cap) = match &tv {
            Value::Class(c) => {
                let d = c.borrow();
                let queue = d
                    .fields
                    .get("queue")
                    .cloned()
                    .unwrap_or_else(|| Value::arr(vec![]));
                let cap = match d.fields.get("cap") {
                    Some(Value::Int(i)) => Some(*i),
                    _ => None,
                };
                (queue, cap)
            }
            _ => return Err(RtError::new("TypeError", Some(span.clone()))),
        };
        match m {
            "write" => {
                if args.len() != 1 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                if closed {
                    return Ok(Some(self.err_val("Closed")));
                }
                let v = self.eval(&args[0])?;
                if let Value::Arr(items) = &queue {
                    items.borrow_mut().push(Rc::new(RefCell::new(v)));
                }
                Ok(Some(Value::Void))
            }
            "read" | "recv" => {
                if !args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                if let Value::Arr(items) = &queue {
                    let popped = {
                        let mut it = items.borrow_mut();
                        if it.is_empty() {
                            return Ok(Some(self.err_val("Empty")));
                        }
                        it.remove(0)
                    };
                    let val = popped.borrow().clone();
                    Ok(Some(val))
                } else {
                    Err(RtError::new("TypeError", Some(span.clone())))
                }
            }
            "try_read" => {
                if !args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                if let Value::Arr(items) = &queue {
                    let popped = {
                        let mut it = items.borrow_mut();
                        if it.is_empty() {
                            return Ok(Some(Value::Opt(None)));
                        }
                        it.remove(0)
                    };
                    let val = popped.borrow().clone();
                    Ok(Some(Value::Opt(Some(Rc::new(val)))))
                } else {
                    Err(RtError::new("TypeError", Some(span.clone())))
                }
            }
            "send" => {
                if args.len() != 1 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                if closed {
                    return Ok(Some(self.err_val("Closed")));
                }
                let v = self.eval(&args[0])?;
                if let Value::Arr(items) = &queue {
                    let mut it = items.borrow_mut();
                    if let Some(cap) = cap {
                        if it.len() as i128 >= cap {
                            return Ok(Some(self.err_val("ChannelFull")));
                        }
                    }
                    it.push(Rc::new(RefCell::new(v)));
                }
                Ok(Some(Value::Void))
            }
            "close" => {
                if !args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                ccell
                    .borrow_mut()
                    .fields
                    .insert("closed".to_string(), Value::Bool(true));
                Ok(Some(Value::Void))
            }
            _ => Ok(None),
        }
    }

    /// E4：Pipe 通道方法（使用 mpsc 无锁 SPSC 队列）
    fn call_one_to_one_method(
        &mut self,
        cid: i128,
        _name: &str,
        m: &str,
        args: &[Expr],
        span: &Span,
        closed: bool,
        ccell: Rc<RefCell<ClassData>>,
    ) -> Result<Option<Value>> {
        // Map i128 channel ID to i64 key for HashMap<i64, ChannelState>
        let cid_i64 = cid as i64;
        match m {
            "write" => {
                if args.len() != 1 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                if closed {
                    return Ok(Some(self.err_val("Closed")));
                }
                let v = self.eval(&args[0])?;
                let state = self
                    .channels
                    .get_mut(&cid_i64)
                    .ok_or_else(|| RtError::new("TypeError", Some(span.clone())))?;
                match state {
                    ChannelState::Pipe { sender, .. } => {
                        let _ = sender.send(v);
                    }
                    _ => unreachable!(),
                }
                Ok(Some(Value::Void))
            }
            "read" | "recv" => {
                if !args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let state = self
                    .channels
                    .get_mut(&cid_i64)
                    .ok_or_else(|| RtError::new("TypeError", Some(span.clone())))?;
                match state {
                    ChannelState::Pipe { receiver, .. } => match receiver.recv() {
                        Ok(v) => Ok(Some(v)),
                        Err(_) => Ok(Some(self.err_val("Closed"))),
                    },
                    _ => unreachable!(),
                }
            }
            "try_read" => {
                if !args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let state = self
                    .channels
                    .get_mut(&cid_i64)
                    .ok_or_else(|| RtError::new("TypeError", Some(span.clone())))?;
                match state {
                    ChannelState::Pipe { receiver, .. } => match receiver.try_recv() {
                        Ok(v) => Ok(Some(Value::Opt(Some(Rc::new(v))))),
                        Err(_) => Ok(Some(Value::Opt(None))),
                    },
                    _ => unreachable!(),
                }
            }
            "close" => {
                if !args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                // Drop the sender to signal close
                self.channels.remove(&cid_i64);
                ccell
                    .borrow_mut()
                    .fields
                    .insert("closed".to_string(), Value::Bool(true));
                Ok(Some(Value::Void))
            }
            _ => Ok(None),
        }
    }

    /// E4：Mutex+Condvar 队列方法（Tee/Funnel/Hub 共享实现）
    fn call_mutex_condvar_method(
        &mut self,
        cid: i128,
        _name: &str,
        m: &str,
        args: &[Expr],
        span: &Span,
        closed: bool,
        _ccell: Rc<RefCell<ClassData>>,
    ) -> Result<Option<Value>> {
        let cid_i64 = cid as i64;
        match m {
            "write" | "send" => {
                if args.len() != 1 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                if closed {
                    return Ok(Some(self.err_val("Closed")));
                }
                let v = self.eval(&args[0])?;
                let state = self
                    .channels
                    .get(&cid_i64)
                    .ok_or_else(|| RtError::new("TypeError", Some(span.clone())))?;
                let (queue, condvar) = match state {
                    ChannelState::Tee { queue, condvar }
                    | ChannelState::Funnel { queue, condvar }
                    | ChannelState::Hub { queue, condvar } => (queue, condvar),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let mut q = queue.lock().unwrap();
                q.push_back(v);
                condvar.notify_all();
                Ok(Some(Value::Void))
            }
            "read" | "recv" => {
                if !args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let state = self
                    .channels
                    .get(&cid_i64)
                    .ok_or_else(|| RtError::new("TypeError", Some(span.clone())))?;
                let (queue, condvar) = match state {
                    ChannelState::Tee { queue, condvar }
                    | ChannelState::Funnel { queue, condvar }
                    | ChannelState::Hub { queue, condvar } => (queue, condvar),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let mut q = queue.lock().unwrap();
                while q.is_empty() {
                    q = condvar.wait(q).unwrap();
                }
                let v = q.pop_front().unwrap();
                Ok(Some(v))
            }
            "try_read" => {
                if !args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let state = self
                    .channels
                    .get(&cid_i64)
                    .ok_or_else(|| RtError::new("TypeError", Some(span.clone())))?;
                let queue = match state {
                    ChannelState::Tee { queue, .. }
                    | ChannelState::Funnel { queue, .. }
                    | ChannelState::Hub { queue, .. } => queue,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let mut q = queue.lock().unwrap();
                match q.pop_front() {
                    Some(v) => Ok(Some(Value::Opt(Some(Rc::new(v))))),
                    None => Ok(Some(Value::Opt(None))),
                }
            }
            "close" => {
                if !args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                // Remove the channel to signal close
                self.channels.remove(&cid_i64);
                Ok(Some(Value::Void))
            }
            _ => Ok(None),
        }
    }

    /// await 运行 Future 到完成（≡ join）：
    /// - 已运行（done）→ 返回缓存 result；
    /// - 已取消（cancel 且未运行）→ 置 done、缓存 `error.Cancelled`、返回 Cancelled；
    /// - 否则在子任务作用域（alloc 绑定独立实例 Q8）中调用 fn，缓存 result 并置 done。
    pub(crate) fn future_run(&mut self, fut: &Value, span: &Span) -> Result<Value> {
        let tv = self.deref_value(fut.clone());
        let (callee, args, alloc_v, cancelled, done) = match tv {
            Value::Class(c) => {
                let d = c.borrow();
                let callee = d.fields.get("fn").cloned().unwrap_or(Value::Void);
                let args = match d.fields.get("args") {
                    Some(Value::Arr(a)) => a
                        .borrow()
                        .iter()
                        .map(|c| c.borrow().clone())
                        .collect::<Vec<_>>(),
                    _ => vec![],
                };
                let alloc_v = d.fields.get("alloc").cloned().unwrap_or(Value::Alloc);
                let cancelled = matches!(d.fields.get("cancel"), Some(Value::Bool(true)));
                let done = matches!(d.fields.get("done"), Some(Value::Bool(true)));
                (callee, args, alloc_v, cancelled, done)
            }
            _ => return Err(RtError::new("TypeError", Some(span.clone()))),
        };
        if done {
            return self.thread_result(fut, span);
        }
        if cancelled {
            let err_v = self.err_val("Cancelled");
            self.thread_set_bool(fut, "done", true);
            self.thread_set_value(fut, "result", err_v);
            return self.thread_result(fut, span);
        }
        self.push_scope();
        self.bind("alloc", alloc_v);
        let r = (|| -> Result<Value> {
            match callee {
                Value::Fn(fname) => {
                    let fdef = self.pick_fn(&fname, &args)?;
                    self.call_fn(&fdef, &args, span)
                }
                Value::Closure(cl) => self.call_closure(&cl, &args, span),
                _ => Err(RtError::new("NotCallable", Some(span.clone()))),
            }
        })();
        let _ = self.pop_scope(Self::is_err_path(&r.clone().map(Flow::Value)));
        let result = match r {
            Ok(v) => v,
            Err(e) => return Err(e),
        };
        self.thread_set_bool(fut, "done", true);
        self.thread_set_value(fut, "result", result.clone());
        Ok(result)
    }

    /// Arena 内建方法（G1）：`alloc` 字节 bump 分配 / `init` 类型构造 / `deinit` 批量归还 /
    /// `bytes`/`blocks` 统计诊断
    pub(crate) fn call_arena_method(
        &mut self,
        arena: Rc<RefCell<ArenaState>>,
        method: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        match method {
            "alloc" => {
                if args.len() != 1 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                match v {
                    Value::Int(i) => {
                        // 字节分配：bump 切块；超容量/失败 → error.OutOfMemory（可 catch）
                        if i < 0 {
                            return Err(RtError::new("TypeError", Some(span.clone())));
                        }
                        if i as u128 > usize::MAX as u128 {
                            return Ok(Some(self.err_val("OutOfMemory")));
                        }
                        let n = i as usize;
                        match arena.borrow_mut().bump(n) {
                            Ok((block, off)) => {
                                let bytes = block.borrow();
                                Ok(Some(Value::str_bytes(bytes[off..off + n].to_vec())))
                            }
                            Err(ArenaAllocErr::Deinit) => {
                                Err(RtError::new("ArenaDeinitialized", Some(span.clone())))
                            }
                            Err(ArenaAllocErr::Oom) => Ok(Some(self.err_val("OutOfMemory"))),
                        }
                    }
                    // 非整数实参：类型字面量构造（arena.alloc(Node{...}) 兼容形态）
                    _ => Ok(Some(v)),
                }
            }
            "init" => {
                // arena.init(T) / arena.init(T{...})（E1：typed 构造，对齐 alloc.init(T)
                // 双形态；内存来源 arena——按类型大小对齐后 bump 记账 + 字段默认值填充）
                if args.len() != 1 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                // 无参构造 arena.init(T)：按类型建空实例（字段逐默认值，definite assignment M2.5）
                if let Expr::Ident(tname, _) = &args[0] {
                    let inst = if let Some(TypeDef::Class { fields, .. }) = self.types.get(tname) {
                        let mut f = HashMap::new();
                        // 先克隆字段类型/默认值：default_value(&mut self) 具体化会重新借用 self
                        let ftypes: Vec<(String, Type, Option<Expr>)> = fields
                            .iter()
                            .map(|fd| (fd.name.clone(), fd.ty.clone(), fd.default.clone()))
                            .collect();
                        for (fname, fty, default) in &ftypes {
                            let val = if let Some(de) = default {
                                self.eval(de)?
                            } else {
                                self.default_value(Some(fty))?
                            };
                            f.insert(fname.clone(), val);
                        }
                        Value::class(tname, f)
                    } else if self.types.contains_key(tname) {
                        // 枚举等：空变体
                        Value::Enum {
                            name: tname.clone(),
                            variant: "__none__".into(),
                            payload: None,
                        }
                    } else {
                        return Err(RtError::msg(
                            "UnknownType",
                            format!("unknown type `{tname}`"),
                        ));
                    };
                    let size = self.type_size_of(tname).unwrap_or(8);
                    match arena.borrow_mut().bump(size) {
                        Ok(_) => Ok(Some(inst)),
                        Err(ArenaAllocErr::Deinit) => {
                            Err(RtError::new("ArenaDeinitialized", Some(span.clone())))
                        }
                        Err(ArenaAllocErr::Oom) => Ok(Some(self.err_val("OutOfMemory"))),
                    }
                } else {
                    // 带参构造 arena.init(T{...})：字面量求值即实例；按实例类型 bump 记账
                    let v = self.eval(&args[0])?;
                    let ty = match &v {
                        Value::Class(c) => c.borrow().name.clone(),
                        Value::Enum { name, .. } => name.clone(),
                        _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                    };
                    let size = self.type_size_of(&ty).unwrap_or(8);
                    match arena.borrow_mut().bump(size) {
                        Ok(_) => Ok(Some(v)),
                        Err(ArenaAllocErr::Deinit) => {
                            Err(RtError::new("ArenaDeinitialized", Some(span.clone())))
                        }
                        Err(ArenaAllocErr::Oom) => Ok(Some(self.err_val("OutOfMemory"))),
                    }
                }
            }
            "deinit" => {
                if !args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                arena.borrow_mut().deinit();
                Ok(Some(Value::Void))
            }
            "bytes" => {
                if !args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                Ok(Some(Value::int(arena.borrow().total as i128)))
            }
            "blocks" => {
                if !args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                Ok(Some(Value::int(arena.borrow().blocks.len() as i128)))
            }
            _ => Ok(None),
        }
    }
}
