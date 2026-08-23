use super::*;

/// 重载/可选参数分派（对齐 oracle `pick_fn` `interp.rs:2665-2796`）：
/// ① 精确参数数（非空则用；空则全池）→ ② 按实参值类型匹配（具体优先泛型）→ ③ 尾参默认回退。
/// 返回类型匹配（`expected_ret`）IR 未跟踪，留待 Phase 7 期望类型传播补齐。
pub(crate) fn pick_func(
    ctx: &Ctx,
    module: &IrModule,
    name: &str,
    arg_vals: &[IrValue],
) -> Option<usize> {
    let candidates = module.func_index.get(name)?;
    // ① 精确参数数量匹配
    let exact: Vec<usize> = candidates
        .iter()
        .copied()
        .filter(|&i| module.funcs[i].params.len() == arg_vals.len())
        .collect();
    let pool: Vec<usize> = if exact.is_empty() {
        candidates.clone()
    } else {
        exact
    };
    if pool.len() == 1 {
        return Some(pool[0]);
    }
    // ② 按实参值类型匹配（具体优先于泛型；返回类型匹配留待 Phase 7）
    let mut best: Option<usize> = None;
    for &fi in &pool {
        let f = &module.funcs[fi];
        let mut ok = true;
        let mut is_generic = false;
        for (p, a) in f.param_ty.iter().zip(arg_vals.iter()) {
            let pt = p.strip();
            // 指针/装箱实参解引用后匹配
            let a = deref_value(ctx, a);
            match pt {
                Type::Named(n, _) => {
                    let want_float = matches!(n.as_str(), "f32" | "f64" | "f16" | "f128");
                    let want_int = matches!(
                        n.as_str(),
                        "i8" | "i16"
                            | "i32"
                            | "i64"
                            | "i128"
                            | "isize"
                            | "u8"
                            | "u16"
                            | "u32"
                            | "u64"
                            | "u128"
                            | "usize"
                    );
                    let want_bool = n == "bool";
                    match a {
                        IrValue::Int(_) if want_float => ok = false,
                        IrValue::Float(_) if want_int => ok = false,
                        IrValue::Str(_) if want_int || want_float || want_bool => ok = false,
                        IrValue::Bool(_) if !want_bool => ok = false,
                        IrValue::Class(c) if n != "String" && class_name(ctx, *c) != *n => {
                            ok = false;
                        }
                        // 泛型 T（where T: INumber 等）：不排除（编译时验证归 M2）
                        _ if n.chars().next().map_or(false, |c| c.is_uppercase())
                            && !n.starts_with("String")
                            && !n.starts_with("Vec")
                            && !n.starts_with("Map")
                            && !n.starts_with("Deque") =>
                        {
                            is_generic = true;
                        }
                        _ => {}
                    }
                }
                Type::Slice(inner, _) => {
                    // &[u8] / &[T]：Str 或数组；泛型元素 T 标记为泛型
                    match a {
                        IrValue::Str(_) => {}
                        IrValue::Arr(_) | IrValue::Slice { .. } => {}
                        _ => ok = false,
                    }
                    if let Type::Named(n, _) = inner.strip() {
                        if n.chars().next().map_or(false, |c| c.is_uppercase())
                            && !n.starts_with("String")
                            && !n.starts_with("Vec")
                            && !n.starts_with("Map")
                            && !n.starts_with("Deque")
                        {
                            is_generic = true;
                        }
                    }
                }
                Type::Infer => {}
                _ => {}
            }
        }
        if ok {
            match &best {
                None => best = Some(fi),
                Some(b) => {
                    let b_generic = module.funcs[*b].param_ty.iter().any(type_has_generic);
                    if !is_generic && b_generic {
                        // 具体优先于泛型
                        best = Some(fi);
                    } else if is_generic && !b_generic {
                        // 保留 best（泛型不替换具体）
                    }
                    // 同具体度：保留首个注册（稳定）
                }
            }
        }
    }
    if let Some(b) = best {
        return Some(b);
    }
    // ③ 带默认参数的回退（参数数 <= 声明数且尾部默认）
    for &fi in candidates {
        let f = &module.funcs[fi];
        if f.params.len() > arg_vals.len() {
            let missing = f.params.len() - arg_vals.len();
            let tail_has_default = f.param_defaults[f.params.len() - missing..]
                .iter()
                .all(|d| *d);
            if tail_has_default {
                return Some(fi);
            }
        }
    }
    None
}

/// 类型是否含泛型参数（重载分派：具体优先泛型；对齐 oracle `type_has_generic`）
pub(crate) fn type_has_generic(t: &Type) -> bool {
    match t.strip() {
        Type::Named(n, args) => {
            let n = n.as_str();
            (n.chars().next().map_or(false, |c| c.is_uppercase())
                && !n.starts_with("String")
                && !n.starts_with("Vec")
                && !n.starts_with("Map")
                && !n.starts_with("Deque"))
                || args.iter().any(type_has_generic)
        }
        Type::Ptr(inner, _)
        | Type::Slice(inner, _)
        | Type::Optional(inner)
        | Type::ErrorUnion(_, inner) => type_has_generic(inner),
        Type::Tuple(items) => items.iter().any(type_has_generic),
        Type::Array(_, inner) => type_has_generic(inner),
        _ => false,
    }
}

/// Class 单元的类名（pick_func 按类名匹配；oracle 用 `Value::Class(c).borrow().name`）
pub(crate) fn class_name(ctx: &Ctx, cell: usize) -> String {
    match &ctx.cells[cell] {
        Cell::Class { name, .. } => name.clone(),
        _ => "<not-a-class>".into(),
    }
}

/// 值是否为连续类（[`IrModule::continuous`] 运行时门；`DeepCopy` 指令判定）。
/// 非 Class 值（标量/数组/切片/枚举/指针等）恒 false——恒等 = 引用别名。
pub(crate) fn is_continuous_class(ctx: &Ctx, module: &IrModule, v: &IrValue) -> bool {
    match v {
        IrValue::Class(c) => module.continuous.contains(&class_name(ctx, *c)),
        _ => false,
    }
}

/// 深拷贝（move 捕获；对齐 oracle `deep_copy` `interp.rs:5539-5562`）：
/// Arr/Class/Ptr/Opt(Some) 递归拷贝，其余按值克隆（Str 本身是不可变字节串）。
pub(crate) fn deep_copy(ctx: &mut Ctx, v: IrValue) -> IrValue {
    match v {
        IrValue::Arr(c) => {
            let elems = match &ctx.cells[c] {
                Cell::Elems(e) => e.clone(),
                _ => return IrValue::Arr(c),
            };
            let new_elems: Vec<usize> = elems
                .iter()
                .map(|ec| {
                    let cv = ctx.cell_value(*ec).clone();
                    let copied = deep_copy(ctx, cv);
                    ctx.alloc(Cell::Value(copied))
                })
                .collect();
            IrValue::Arr(ctx.alloc(Cell::Elems(new_elems)))
        }
        IrValue::Class(c) => {
            let (name, fields) = match &ctx.cells[c] {
                Cell::Class { name, fields } => (name.clone(), fields.clone()),
                _ => return IrValue::Class(c),
            };
            let new_fields: HashMap<String, usize> = fields
                .iter()
                .map(|(k, vc)| {
                    let cv = ctx.cell_value(*vc).clone();
                    let copied = deep_copy(ctx, cv);
                    (k.clone(), ctx.alloc(Cell::Value(copied)))
                })
                .collect();
            IrValue::Class(ctx.alloc(Cell::Class {
                name,
                fields: new_fields,
            }))
        }
        IrValue::Ptr(c) => {
            let cv = ctx.cell_value(c).clone();
            let copied = deep_copy(ctx, cv);
            IrValue::Ptr(ctx.alloc(Cell::Value(copied)))
        }
        // 装箱胖指针：data 深拷贝（新 cell），vtbl/alloc 原样携带
        IrValue::Boxed(c) => {
            let (data, vtbl, alloc) = match &ctx.cells[c] {
                Cell::Boxed { data, vtbl, alloc } => (*data, vtbl.clone(), alloc.clone()),
                _ => return IrValue::Boxed(c),
            };
            let cv = ctx.cell_value(data).clone();
            let copied = deep_copy(ctx, cv);
            let new_data = ctx.alloc(Cell::Value(copied));
            IrValue::Boxed(ctx.alloc(Cell::Boxed {
                data: new_data,
                vtbl,
                alloc,
            }))
        }
        // 集合（G4）：Vec items 深拷贝（新 Elems），alloc 原样携带；Map 字段深拷贝
        IrValue::Vec(c) => {
            let (arr, alloc) = match &ctx.cells[c] {
                Cell::Vec { arr, alloc } => (arr.clone(), alloc.clone()),
                _ => return IrValue::Vec(c),
            };
            let copied = deep_copy(ctx, arr);
            IrValue::Vec(ctx.alloc(Cell::Vec { arr: copied, alloc }))
        }
        IrValue::Map(c) => {
            let (fields, alloc) = match &ctx.cells[c] {
                Cell::Map { fields, alloc } => (fields.clone(), alloc.clone()),
                _ => return IrValue::Map(c),
            };
            let new_fields: HashMap<String, usize> = fields
                .iter()
                .map(|(k, vc)| {
                    let cv = ctx.cell_value(*vc).clone();
                    let copied = deep_copy(ctx, cv);
                    (k.clone(), ctx.alloc(Cell::Value(copied)))
                })
                .collect();
            IrValue::Map(ctx.alloc(Cell::Map {
                fields: new_fields,
                alloc,
            }))
        }
        IrValue::Opt(Some(b)) => IrValue::Opt(Some(Box::new(deep_copy(ctx, *b)))),
        // move 捕获闭包值：捕获 cell 逐个深拷贝——闭包持有独立环境副本
        // （与原作用域/其他闭包脱离共享，对齐 oracle `deep_copy` Closure 臂）
        IrValue::Closure {
            func,
            captures,
            is_mut,
        } => {
            let new_caps: Vec<usize> = captures
                .iter()
                .map(|c| {
                    let cv = ctx.cell_value(*c).clone();
                    let copied = deep_copy(ctx, cv);
                    ctx.alloc(Cell::Value(copied))
                })
                .collect();
            IrValue::Closure {
                func,
                captures: new_caps,
                is_mut,
            }
        }
        other => other,
    }
}

/// 值类型名（方法分派 key：`"{type}.{method}"`；对齐 oracle `Value::type_name`）
pub(crate) fn ir_type_name(ctx: &Ctx, v: &IrValue) -> String {
    match v {
        IrValue::Int(_) => "i128".into(),
        IrValue::Float(_) => "f64".into(),
        IrValue::Bool(_) => "bool".into(),
        IrValue::Str(_) => "&[u8]".into(),
        IrValue::Arr(_) => "array".into(),
        IrValue::Vec(_) => "array".into(),
        IrValue::Map(_) => "Map".into(),
        IrValue::Slice { .. } => "slice".into(),
        IrValue::Class(c) => class_name(ctx, *c),
        IrValue::Arena(_) => "Arena".into(),
        IrValue::Enum { name, .. } => name.clone(),
        IrValue::Opt(_) => "optional".into(),
        IrValue::Err { .. } => "error".into(),
        IrValue::Ptr(_) => "pointer".into(),
        IrValue::Boxed(_) => "pointer".into(),
        IrValue::Fn(_) => "fn".into(),
        IrValue::Closure { .. } => "closure".into(),
        IrValue::End => "end".into(),
        IrValue::Iter(_) => "<iter>".into(),
        IrValue::Void => "void".into(),
    }
}

/// IrConst → IrValue（默认参数常量值；与 `IrInst::Const` 执行一致）
pub(crate) fn const_val(c: &IrConst) -> IrValue {
    match c {
        IrConst::Int(i) => IrValue::Int(*i),
        IrConst::Float(f) => IrValue::Float(*f),
        IrConst::Bool(b) => IrValue::Bool(*b),
        IrConst::Str(s) => IrValue::Str(s.clone().into_bytes()),
        IrConst::Void => IrValue::Void,
        IrConst::Null => IrValue::Opt(None),
        IrConst::Err { name, code } => IrValue::Err {
            name: name.clone(),
            code: *code,
        },
        IrConst::End => IrValue::End,
    }
}

/// 执行一个函数：堆/单元模型（Phase 1）。每槽分配共享 cell，`&x` = `Ptr(cell)`
/// 可跨帧存活（传入函数后写穿调用方槽——别名语义对齐 tree-walking `Rc<RefCell>`）。
pub(crate) fn exec_func(
    ctx: &mut Ctx,
    module: &IrModule,
    idx: usize,
    args: &[IrValue],
    depth: usize,
) -> R<IrValue> {
    if depth >= MAX_CALL_DEPTH {
        return Err(IrError::msg("StackOverflow", "maximum call depth exceeded"));
    }
    let func = &module.funcs[idx];
    let mut frame = Frame {
        cells: Vec::with_capacity(func.n_slots),
        defers: Vec::new(),
        readonly: Vec::new(),
    };
    for _ in 0..func.n_slots {
        frame.cells.push(ctx.alloc(Cell::Value(IrValue::Void)));
    }
    // 绑定实参；缺失尾参用编译期常量默认值补齐（ADR-0009 / 对齐 oracle `call_fn`）
    for (i, ps) in func.params.iter().enumerate() {
        if i < args.len() {
            ctx.set(&frame, *ps, args[i].clone());
        } else if i < func.defaults.len() {
            if let Some(d) = &func.defaults[i] {
                ctx.set(&frame, *ps, const_val(d));
            }
        }
    }
    exec_body(ctx, module, func, frame, depth)
}

/// 调用闭包（对齐 oracle `call_closure` `interp.rs:1444-1494`）：
/// 捕获参数槽直接绑定捕获 cell（写穿 = 共享读/mut 语义）；显式参数绑新值。
/// 单表达式闭包体 `|v| v+a` 已在 lower 阶段降级为 `Return { temp }`。
pub(crate) fn call_closure_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    func_idx: usize,
    captures: &[usize],
    args: &[IrValue],
    is_mut: bool,
    depth: usize,
) -> R<IrValue> {
    if depth >= MAX_CALL_DEPTH {
        return Err(IrError::msg("StackOverflow", "maximum call depth exceeded"));
    }
    let func = &module.closures[func_idx];
    let n_caps = captures.len();
    // M2.7 只读强制（Phase 8）：非 `mut` 闭包 → 捕获参数槽只读
    // （Store 写这些槽 → ReadonlyCapture；经指针/字段/索引写穿放行）
    let readonly: Vec<usize> = if is_mut {
        Vec::new()
    } else {
        func.params.iter().take(n_caps).copied().collect()
    };
    let mut frame = Frame {
        cells: Vec::with_capacity(func.n_slots),
        defers: Vec::new(),
        readonly,
    };
    for _ in 0..func.n_slots {
        frame.cells.push(ctx.alloc(Cell::Value(IrValue::Void)));
    }
    // 捕获参数（前 n_caps 个槽）→ 直接绑定捕获 cell（写穿调用方槽）
    for (i, cap_cell) in captures.iter().enumerate() {
        if i < func.params.len() {
            frame.cells[func.params[i]] = *cap_cell;
        }
    }
    // 显式参数（捕获参数之后）
    for (i, ps) in func.params.iter().enumerate().skip(n_caps) {
        let ai = i - n_caps;
        if ai < args.len() {
            ctx.set(&frame, *ps, args[ai].clone());
        }
    }
    exec_body(ctx, module, func, frame, depth)
}

/// 执行函数/闭包体（共享循环；当前函数体在 `func`，模块其余函数在 `module.funcs`）
pub(crate) fn exec_body(
    ctx: &mut Ctx,
    module: &IrModule,
    func: &IrFunc,
    frame: Frame,
    depth: usize,
) -> R<IrValue> {
    ctx.cur_depth = depth;
    let mut frame = frame;
    let mut pc = 0usize;
    let mut fail: Option<String> = None;
    loop {
        if pc >= func.body.len() {
            return Err(IrError::msg(
                "NoReturn",
                format!("function `{}` fell through", func.name),
            ));
        }
        match &func.body[pc] {
            IrInst::Const { temp, val } => {
                ctx.set(
                    &frame,
                    *temp,
                    match val {
                        IrConst::Int(i) => IrValue::Int(*i),
                        IrConst::Float(f) => IrValue::Float(*f),
                        IrConst::Bool(b) => IrValue::Bool(*b),
                        IrConst::Str(s) => IrValue::Str(s.clone().into_bytes()),
                        IrConst::Void => IrValue::Void,
                        IrConst::Null => IrValue::Opt(None),
                        IrConst::Err { name, code } => IrValue::Err {
                            name: name.clone(),
                            code: *code,
                        },
                        IrConst::End => IrValue::End,
                    },
                );
            }
            IrInst::Load { temp, slot } => {
                let v = ctx.get(&frame, *slot).clone();
                ctx.set(&frame, *temp, v);
            }
            IrInst::Store { slot, temp } => {
                // M2.7 只读捕获强制（Phase 8）：非 `mut` 闭包写捕获参数槽 → 错误
                if frame.readonly.contains(slot) {
                    return Err(IrError::msg(
                        "ReadonlyCapture",
                        "cannot assign to captured variable in non-mut closure \
                         (declare the closure `mut` to capture mutably)",
                    ));
                }
                let v = ctx.get(&frame, *temp).clone();
                ctx.set(&frame, *slot, v);
            }
            IrInst::Bin { op, temp, a, b } => {
                let (av, bv) = (ctx.get(&frame, *a).clone(), ctx.get(&frame, *b).clone());
                let v = binop(*op, ctx, &av, &bv)?;
                ctx.set(&frame, *temp, v);
            }
            IrInst::Un { op, temp, a } => {
                let av = ctx.get(&frame, *a).clone();
                ctx.set(
                    &frame,
                    *temp,
                    match op {
                        IrUnOp::Neg => match av {
                            IrValue::Int(i) => IrValue::Int(-i),
                            IrValue::Float(f) => IrValue::Float(-f),
                            _ => return Err(IrError::msg("TypeError", "unary -")),
                        },
                        IrUnOp::Not => IrValue::Bool(!av.as_bool()),
                        IrUnOp::BitNot => match av {
                            IrValue::Int(i) => IrValue::Int(!i),
                            _ => return Err(IrError::msg("TypeError", "~")),
                        },
                    },
                );
            }
            IrInst::Jump { label } => {
                pc = find_label(func, *label)?;
                continue;
            }
            IrInst::JumpIf { temp, label } => {
                if ctx.get(&frame, *temp).as_bool() {
                    pc = find_label(func, *label)?;
                    continue;
                }
            }
            IrInst::JumpIfNot { temp, label } => {
                if !ctx.get(&frame, *temp).as_bool() {
                    pc = find_label(func, *label)?;
                    continue;
                }
            }
            IrInst::JumpIfErr { temp, label } => {
                if ctx.get(&frame, *temp).is_err() {
                    pc = find_label(func, *label)?;
                    continue;
                }
            }
            IrInst::JumpIfNull { temp, label } => {
                if matches!(ctx.get(&frame, *temp), IrValue::Opt(None)) {
                    pc = find_label(func, *label)?;
                    continue;
                }
            }
            IrInst::Label { .. } => {}
            // ---- Phase 6：defer / errdefer ----
            IrInst::PushDefer { id } => frame.defers.push(*id),
            IrInst::JumpIfNotDefer { id, label } => {
                if !frame.defers.contains(id) {
                    pc = find_label(func, *label)?;
                    continue;
                }
            }
            IrInst::PopDefer { id } => {
                // 移除最近一次登记（rposition）；未登记（分支未达）→ 无操作
                if let Some(pos) = frame.defers.iter().rposition(|d| d == id) {
                    frame.defers.remove(pos);
                }
            }
            IrInst::Call { name, args, temp } => {
                let arg_vals: Vec<IrValue> =
                    args.iter().map(|a| ctx.get(&frame, *a).clone()).collect();
                // Phase 7：隐式环境限定名（io.print / io.fs.open / alloc.init…）与
                // 虚拟根（json.parse / csv.parse / String.from）——未登记为用户函数时按
                // 「根值 → 字段 → 方法」路由（对齐 oracle eval_call 隐式环境 + 方法分派）；
                // 登记了同名用户函数则优先用户函数。
                if !module.func_index.contains_key(name) {
                    let root = name.split('.').next().unwrap_or("");
                    if is_dotted_implicit_root(root) && name.contains('.') {
                        let v = call_dotted_implicit(ctx, module, name, &arg_vals)?;
                        ctx.set(&frame, *temp, v);
                        pc += 1;
                        continue;
                    }
                }
                let callee_idx = pick_func(ctx, module, name, &arg_vals)
                    .ok_or_else(|| IrError::msg("NoFunction", format!("no function `{name}`")))?;
                let v = exec_func(ctx, module, callee_idx, &arg_vals, depth + 1)?;
                ctx.set(&frame, *temp, v);
            }
            IrInst::CallBuiltin { name, args, temp } => {
                let arg_vals: Vec<IrValue> =
                    args.iter().map(|a| ctx.get(&frame, *a).clone()).collect();
                let v = call_builtin(ctx, module, name, &arg_vals, &mut fail)?;
                ctx.set(&frame, *temp, v);
            }
            IrInst::Return { temp } => {
                let v = ctx.get(&frame, *temp).clone();
                if let Some(f) = fail {
                    return Err(IrError::msg("AssertFailed", f));
                }
                return Ok(v);
            }
            IrInst::ReturnVoid => {
                if let Some(f) = fail {
                    return Err(IrError::msg("AssertFailed", f));
                }
                return Ok(IrValue::Void);
            }
            // ---- Phase 1 指针 ----
            IrInst::AddrSlot { temp, slot } => {
                let cell = frame.cells[*slot];
                ctx.set(&frame, *temp, IrValue::Ptr(cell));
            }
            IrInst::AddrValue { temp, value } => {
                // 非 lvalue 取址快照：求值结果复制进新 cell（对齐 tree-walking `&expr` 兜底）
                let v = ctx.get(&frame, *value).clone();
                let cell = ctx.alloc(Cell::Value(v));
                ctx.set(&frame, *temp, IrValue::Ptr(cell));
            }
            IrInst::Deref { temp, a } => {
                // 解引用：Ptr/Boxed → pointee；非 Ptr → 恒等（对齐 tree-walking `deref_value`）
                let v = match ctx.get(&frame, *a) {
                    IrValue::Ptr(cell) => ctx.cell_value(*cell).clone(),
                    IrValue::Boxed(cell) => match &ctx.cells[*cell] {
                        Cell::Boxed { data, .. } => ctx.cell_value(*data).clone(),
                        _ => IrValue::Void,
                    },
                    other => other.clone(),
                };
                ctx.set(&frame, *temp, v);
            }
            IrInst::StorePtr { target, value } => {
                let t = ctx.get(&frame, *target).clone();
                let v = ctx.get(&frame, *value).clone();
                match t {
                    IrValue::Ptr(cell) => ctx.set_cell(cell, v),
                    // 装箱胖指针：写穿 data cell
                    IrValue::Boxed(cell) => {
                        let data = match &ctx.cells[cell] {
                            Cell::Boxed { data, .. } => Some(*data),
                            _ => None,
                        };
                        match data {
                            Some(d) => ctx.set_cell(d, v),
                            None => return Err(IrError::msg("BadAssign", "store to non-pointer")),
                        }
                    }
                    _ => return Err(IrError::msg("BadAssign", "store to non-pointer")),
                }
            }
            // ---- Phase 2 聚合 ----
            IrInst::Field { temp, base, field } => {
                let bv = deref_value(ctx, ctx.get(&frame, *base)).clone();
                let v = field_value(ctx, &bv, field)?;
                ctx.set(&frame, *temp, v);
            }
            IrInst::StoreField { base, field, value } => {
                let bv = deref_value(ctx, ctx.get(&frame, *base)).clone();
                let v = ctx.get(&frame, *value).clone();
                store_field(ctx, &bv, field, v)?;
                // K1 union 写路径：字段写后字节重解释同步其余字段（对齐 interp assign_class_field）
                if let IrValue::Class(c) = bv {
                    if let Cell::Class { fields, .. } = &ctx.cells[c] {
                        if fields.contains_key("@union") {
                            union_sync_ir(ctx, module, c, field)?;
                        }
                    }
                }
            }
            IrInst::UnionSync { class, written } => {
                let c = match ctx.get(&frame, *class) {
                    IrValue::Class(c) => *c,
                    _ => return Err(IrError::msg("TypeError", "union sync on non-class")),
                };
                union_sync_ir(ctx, module, c, written)?;
            }
            IrInst::Index { temp, base, index } => {
                let bv = deref_value(ctx, ctx.get(&frame, *base)).clone();
                let iv = deref_value(ctx, ctx.get(&frame, *index)).clone();
                let i = as_index(ctx, &iv)?;
                let v = index_value(ctx, &bv, i)?;
                ctx.set(&frame, *temp, v);
            }
            IrInst::StoreIndex { base, index, value } => {
                let bv = deref_value(ctx, ctx.get(&frame, *base)).clone();
                let iv = deref_value(ctx, ctx.get(&frame, *index)).clone();
                let i = as_index(ctx, &iv)?;
                let v = ctx.get(&frame, *value).clone();
                store_index(ctx, &bv, i, v)?;
            }
            IrInst::SliceOf { temp, base, lo, hi } => {
                let bv = deref_value(ctx, ctx.get(&frame, *base)).clone();
                let lo_v = deref_value(ctx, ctx.get(&frame, *lo)).clone();
                let lo_i = as_index(ctx, &lo_v)?;
                let hi_v = ctx.get(&frame, *hi).clone();
                let (hi_i, open) = match hi_v {
                    IrValue::End => (0, true),
                    other => {
                        let d = deref_value(ctx, &other).clone();
                        (as_index(ctx, &d)?, false)
                    }
                };
                let v = slice_of(ctx, &bv, lo_i, hi_i, open)?;
                ctx.set(&frame, *temp, v);
            }
            IrInst::StoreSlice {
                base,
                lo,
                hi,
                value,
            } => {
                let bv = deref_value(ctx, ctx.get(&frame, *base)).clone();
                let lo_v = deref_value(ctx, ctx.get(&frame, *lo)).clone();
                let lo_i = as_index(ctx, &lo_v)?;
                let hi_v = ctx.get(&frame, *hi).clone();
                // 开区间 `arr[a..] = v`：对齐 oracle（eval(hi=`__end__`) 报错）→ BadIndex
                if matches!(hi_v, IrValue::End) {
                    return Err(IrError::msg("BadIndex", "open-end store slice"));
                }
                let hi_d = deref_value(ctx, &hi_v).clone();
                let hi_i = as_index(ctx, &hi_d)?;
                let v = ctx.get(&frame, *value).clone();
                store_slice(ctx, &bv, lo_i, hi_i, &v)?;
            }
            IrInst::MakeArr { temp, items } => {
                let mut cells = Vec::with_capacity(items.len());
                for it in items {
                    let v = ctx.get(&frame, *it).clone();
                    cells.push(ctx.alloc(Cell::Value(v)));
                }
                let c = ctx.alloc(Cell::Elems(cells));
                ctx.set(&frame, *temp, IrValue::Arr(c));
            }
            IrInst::MakeClass { temp, ty, fields } => {
                let mut fs = HashMap::new();
                for (k, vt) in fields {
                    let v = ctx.get(&frame, *vt).clone();
                    fs.insert(k.clone(), ctx.alloc(Cell::Value(v)));
                }
                let c = ctx.alloc(Cell::Class {
                    name: ty.clone(),
                    fields: fs,
                });
                ctx.set(&frame, *temp, IrValue::Class(c));
            }
            IrInst::MakeEnum {
                temp,
                name,
                variant,
                payload,
            } => {
                let p = match payload {
                    Some(pt) => Some(Box::new(ctx.get(&frame, *pt).clone())),
                    None => None,
                };
                ctx.set(
                    &frame,
                    *temp,
                    IrValue::Enum {
                        name: name.clone(),
                        variant: variant.clone(),
                        payload: p,
                    },
                );
            }
            IrInst::Destructure { value, slots } => {
                let v = deref_value(ctx, ctx.get(&frame, *value)).clone();
                let elems = match v {
                    IrValue::Arr(c) => match &ctx.cells[c] {
                        Cell::Elems(e) => e.clone(),
                        _ => {
                            return Err(IrError::msg("TupleArity", "expected tuple in destructure"))
                        }
                    },
                    _ => return Err(IrError::msg("TupleArity", "expected tuple in destructure")),
                };
                if elems.len() != slots.len() {
                    return Err(IrError::msg("TupleArity", "destructure arity mismatch"));
                }
                for (slot, elem) in slots.iter().zip(elems.iter()) {
                    if let Some(s) = slot {
                        let v = ctx.cell_value(*elem).clone();
                        ctx.set(&frame, *s, v);
                    }
                }
            }
            IrInst::Move { temp, a } => {
                let v = ctx.get(&frame, *a).clone();
                ctx.set(&frame, *temp, v);
            }
            IrInst::DeepCopy { temp, a } => {
                let v = ctx.get(&frame, *a).clone();
                // 运行时门：仅连续类深拷贝（标量/数组/非连续类恒等 = 引用别名）
                let v = if is_continuous_class(ctx, module, &v) {
                    deep_copy(ctx, v)
                } else {
                    v
                };
                ctx.set(&frame, *temp, v);
            }
            IrInst::Unwrap { temp, a } => {
                let v = deref_value(ctx, ctx.get(&frame, *a)).clone();
                let r = match v {
                    IrValue::Opt(Some(inner)) => *inner,
                    IrValue::Opt(None) => {
                        return Err(IrError::msg("NullUnwrap", "unwrap of null"));
                    }
                    other => other,
                };
                ctx.set(&frame, *temp, r);
            }
            // ---- Phase 3 switch / 区间 / for ----
            IrInst::MatchTest {
                temp,
                subject,
                pattern,
            } => {
                let sv = deref_value(ctx, ctx.get(&frame, *subject)).clone();
                ctx.set(&frame, *temp, IrValue::Bool(match_pattern(&sv, pattern)));
            }
            IrInst::MakeRange { temp, lo, hi } => {
                let lo_v = deref_value(ctx, ctx.get(&frame, *lo)).clone();
                let hi_v = deref_value(ctx, ctx.get(&frame, *hi)).clone();
                let (lo_i, hi_i) = match (lo_v, hi_v) {
                    (IrValue::Int(a), IrValue::Int(b)) => (a, b),
                    _ => return Err(IrError::msg("TypeError", "range bounds must be integers")),
                };
                let mut cells = Vec::new();
                let mut i = lo_i;
                while i < hi_i {
                    cells.push(ctx.alloc(Cell::Value(IrValue::Int(i))));
                    i += 1;
                }
                let c = ctx.alloc(Cell::Elems(cells));
                ctx.set(&frame, *temp, IrValue::Arr(c));
            }
            IrInst::EnumPayload { temp, a } => {
                let av = ctx.get(&frame, *a).clone();
                let v = enum_payload(ctx, &av)?;
                ctx.set(&frame, *temp, v);
            }
            IrInst::IterMake { temp, base } => {
                let bv = ctx.get(&frame, *base).clone();
                let items = make_iter(ctx, module, &bv, depth)?;
                let c = ctx.alloc(Cell::Iter { items, next: 0 });
                ctx.set(&frame, *temp, IrValue::Iter(c));
            }
            IrInst::IterNext {
                has,
                iter,
                slot,
                read_only,
            } => {
                let iter_c = match ctx.get(&frame, *iter) {
                    IrValue::Iter(c) => *c,
                    _ => return Err(IrError::msg("NotIterable", "expected iterator")),
                };
                let item = {
                    let c = &mut ctx.cells[iter_c];
                    match c {
                        Cell::Iter { items, next } => {
                            if *next < items.len() {
                                let it = items[*next].clone();
                                *next += 1;
                                Some(it)
                            } else {
                                None
                            }
                        }
                        _ => return Err(IrError::msg("NotIterable", "corrupt iterator cell")),
                    }
                };
                match item {
                    Some(it) => {
                        if *read_only {
                            // Read 捕获：槽 cell 置为该项值副本（与容器无别名；
                            // 非 Value cell——如 Map 的 KV Class 条目——用 read_cell 还原句柄）
                            let v = ctx.read_cell(it.cell);
                            ctx.set_cell(frame.cells[*slot], v);
                        } else {
                            // Mut/Move 捕获：槽 cell 绑定共享源 cell（写穿）；
                            // [IrInst::IterWriteBack] 在 run_ir 为无操作（槽 cell 即源 cell）。
                            frame.cells[*slot] = it.cell;
                        }
                        ctx.set(&frame, *has, IrValue::Bool(true));
                    }
                    None => {
                        ctx.set(&frame, *has, IrValue::Bool(false));
                    }
                }
            }
            IrInst::IterWriteBack { .. } => {}
            // ---- Phase 4 闭包 / 函数引用 / 方法 / 动态调用 ----
            IrInst::MakeClosure {
                temp,
                func,
                captures,
                is_move,
                is_mut,
            } => {
                let mut cap_cells = Vec::with_capacity(captures.len());
                for (_, slot) in captures {
                    let cell = frame.cells[*slot];
                    if *is_move {
                        // move 捕获：深拷贝到新 cell（闭包脱离原作用域生命周期）
                        let v = ctx.cell_value(cell).clone();
                        let dv = deep_copy(ctx, v);
                        let ncell = ctx.alloc(Cell::Value(dv));
                        cap_cells.push(ncell);
                    } else {
                        // 读/mut 捕获：共享源 cell（写穿）
                        cap_cells.push(cell);
                    }
                }
                ctx.set(
                    &frame,
                    *temp,
                    IrValue::Closure {
                        func: *func,
                        captures: cap_cells,
                        is_mut: *is_mut,
                    },
                );
            }
            IrInst::FnRef { temp, name } => {
                ctx.set(&frame, *temp, IrValue::Fn(name.clone()));
            }
            // ---- Phase 5：global / const ----
            IrInst::LoadGlobal { temp, name } => {
                let v = if name == "alloc" && ctx.current_alloc.is_some() {
                    // Q8：线程子任务期间 `alloc` 解析到每线程 arena（对齐 oracle `lookup`）
                    ctx.current_alloc.clone().unwrap()
                } else {
                    let cell = ctx.globals.get(name).copied().ok_or_else(|| {
                        IrError::msg("NoGlobal", format!("undefined global `{name}`"))
                    })?;
                    ctx.cell_value(cell).clone()
                };
                ctx.set(&frame, *temp, v);
            }
            IrInst::StoreGlobal { name, value } => {
                let cell = ctx.globals.get(name).copied().ok_or_else(|| {
                    IrError::msg("NoGlobal", format!("undefined global `{name}`"))
                })?;
                let v = ctx.get(&frame, *value).clone();
                ctx.set_cell(cell, v);
            }
            // `&global`：预分配 cell 的 Ptr 别名（与局部 `AddrSlot` 同构，写穿共享 cell）
            IrInst::GlobalAddr { temp, name } => {
                let cell = ctx.globals.get(name).copied().ok_or_else(|| {
                    IrError::msg("NoGlobal", format!("undefined global `{name}`"))
                })?;
                ctx.set(&frame, *temp, IrValue::Ptr(cell));
            }
            IrInst::CallIndirect { temp, callee, args } => {
                let callee_v = ctx.get(&frame, *callee).clone();
                let arg_vals: Vec<IrValue> =
                    args.iter().map(|a| ctx.get(&frame, *a).clone()).collect();
                let v = match callee_v {
                    IrValue::Fn(fname) => {
                        let idx = pick_func(ctx, module, &fname, &arg_vals).ok_or_else(|| {
                            IrError::msg("NoFunction", format!("no function `{fname}`"))
                        })?;
                        exec_func(ctx, module, idx, &arg_vals, depth + 1)?
                    }
                    IrValue::Closure {
                        func,
                        captures,
                        is_mut,
                        ..
                    } => {
                        call_closure_ir(ctx, module, func, &captures, &arg_vals, is_mut, depth + 1)?
                    }
                    other => {
                        return Err(IrError::msg(
                            "NotCallable",
                            format!("`{}` is not callable", type_descr(&other)),
                        ))
                    }
                };
                ctx.set(&frame, *temp, v);
            }
            IrInst::CallMethod {
                temp,
                base,
                method,
                args,
            } => {
                let raw = ctx.get(&frame, *base).clone();
                // G3：装箱胖指针 .alloc() → 携带的分配器引用（三字宽胖指针的 alloc 字）
                if let IrValue::Boxed(bc) = &raw {
                    if method == "alloc" {
                        if let Cell::Boxed { alloc, .. } = &ctx.cells[*bc] {
                            ctx.set(&frame, *temp, alloc.clone());
                            pc += 1;
                            continue;
                        }
                    }
                }
                // G4：集合 .alloc() → 构造 `init(alloc)` 时携带的分配器引用
                if let IrValue::Vec(vc) = &raw {
                    if method == "alloc" {
                        if let Cell::Vec { alloc, .. } = &ctx.cells[*vc] {
                            ctx.set(&frame, *temp, alloc.clone());
                            pc += 1;
                            continue;
                        }
                    }
                }
                if let IrValue::Map(mc) = &raw {
                    if method == "alloc" {
                        if let Cell::Map { alloc, .. } = &ctx.cells[*mc] {
                            ctx.set(&frame, *temp, alloc.clone());
                            pc += 1;
                            continue;
                        }
                    }
                }
                let self_v = deref_value(ctx, &raw).clone();
                let mut arg_vals = vec![self_v.clone()];
                for a in args {
                    arg_vals.push(ctx.get(&frame, *a).clone());
                }
                let v = call_method_ir(ctx, module, &self_v, method, &arg_vals)?;
                ctx.set(&frame, *temp, v);
            }
        }
        pc += 1;
    }
}

/// 方法调用（对齐 oracle `interp.rs:2405-2421`）：先试内建方法 shim（标量/Str/Arr），
/// 再 `"{type}.{method}"` 静态方法表分派（self 已注入为首参）。
pub(crate) fn call_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    self_v: &IrValue,
    method: &str,
    arg_vals: &[IrValue],
) -> R<IrValue> {
    if let Some(v) = call_builtin_method(ctx, module, self_v, method, &arg_vals[1..])? {
        return Ok(v);
    }
    let fname = format!("{}.{}", ir_type_name(ctx, self_v), method);
    let idx = pick_func(ctx, module, &fname, arg_vals)
        .ok_or_else(|| IrError::msg("NoMethod", format!("no method `{fname}`")))?;
    exec_func(ctx, module, idx, arg_vals, 0)
}

// ==================== Phase 7 内建运行时（对齐 oracle interp.rs call_builtin* 面） ====================

/// 隐式环境名（对齐 oracle interp.rs:1585-1595 的隐式环境注入表）
pub(crate) const IMPLICIT_ENV: &[&str] = &[
    "alloc", "io", "test_io", "stdout", "stderr", "pi", "Vec", "Deque", "Map", "Table",
];

/// 限定名根的隐式环境/虚拟根分派（io.*、alloc.*、json.parse、csv.parse、String.from、math.*、
/// Arena.init、serialize.*）
pub(crate) fn is_dotted_implicit_root(root: &str) -> bool {
    IMPLICIT_ENV.contains(&root)
        || matches!(
            root,
            "json" | "csv" | "String" | "math" | "Arena" | "serialize"
        )
}

/// 错误值（码 = 编译期错误码表；内建产生的错误与 `error.X` 字面量同码）
pub(crate) fn err_val(module: &IrModule, name: &str) -> IrValue {
    let code = module.error_codes.get(name).copied().unwrap_or(0);
    IrValue::Err {
        name: name.to_string(),
        code,
    }
}

/// 分配 n 字节零初始化内存；n ≤ 0 → 空；n 超出可表示容量 / 分配失败 → None
/// （调用方转 `error.OutOfMemory`——与 interp `alloc_zeroed_bytes` 对齐；
/// `vec![0u8; n]` 对超大 n 会直接中止进程，分配失败应为可 catch 的错误值）
pub(crate) fn alloc_zeroed_bytes_ir(n: i128) -> Option<Vec<u8>> {
    if n <= 0 {
        return Some(Vec::new());
    }
    if n as u128 > usize::MAX as u128 {
        return None;
    }
    let mut v = Vec::new();
    v.try_reserve_exact(n as usize).ok()?;
    v.resize(n as usize, 0u8);
    Some(v)
}

pub(crate) fn str_val(s: &str) -> IrValue {
    IrValue::Str(s.as_bytes().to_vec())
}
pub(crate) fn str_bytes_val(b: Vec<u8>) -> IrValue {
    IrValue::Str(b)
}
pub(crate) fn opt_val(v: Option<IrValue>) -> IrValue {
    IrValue::Opt(v.map(Box::new))
}

/// 元素数组 → Arr（元素为普通值 cell）
pub(crate) fn make_arr(ctx: &mut Ctx, items: Vec<IrValue>) -> IrValue {
    let elems: Vec<usize> = items
        .into_iter()
        .map(|v| ctx.alloc(Cell::Value(v)))
        .collect();
    IrValue::Arr(ctx.alloc(Cell::Elems(elems)))
}

/// 集合 Vec（G4）：items 存于共享 Elems cell；`alloc` = 构造时携带的分配器引用
pub(crate) fn make_vec_with(ctx: &mut Ctx, items: Vec<IrValue>, alloc: IrValue) -> IrValue {
    let arr = make_arr(ctx, items);
    let inner = match arr {
        IrValue::Arr(c) => c,
        _ => unreachable!("make_arr 恒返回 Arr"),
    };
    IrValue::Vec(ctx.alloc(Cell::Vec {
        arr: IrValue::Arr(inner),
        alloc,
    }))
}

/// 集合 Map（G4）：键 → 字段 cell；`alloc` = 构造时携带的分配器引用
pub(crate) fn make_map_with(
    ctx: &mut Ctx,
    fields: HashMap<String, usize>,
    alloc: IrValue,
) -> IrValue {
    IrValue::Map(ctx.alloc(Cell::Map { fields, alloc }))
}

/// M5.4 Io 实例（含 fs/time/net 子模块 + G1-G5 扩展——对齐 oracle `io_value_with_runtime`
/// interp.rs:2016-2043）。net 含 `udp` 子命名空间；stdout/stderr 独立字节流；
/// ipc/storage/archive/text/rng 各命名空间类名供 call_builtin_method 分派。
pub(crate) fn io_value_ir(ctx: &mut Ctx) -> IrValue {
    let fs = ctx.alloc(Cell::Class {
        name: "Fs".into(),
        fields: HashMap::new(),
    });
    let fs_cell = ctx.alloc(Cell::Value(IrValue::Class(fs)));
    let time = ctx.alloc(Cell::Class {
        name: "Time".into(),
        fields: HashMap::new(),
    });
    let time_cell = ctx.alloc(Cell::Value(IrValue::Class(time)));
    // G1（E3.1）：`io.net.udp` 子命名空间（bind/send_to/recv_from）——UdpSocket 实例
    // 方法由类名分派，命名空间形式委托同实现（对齐 oracle net_fields 含 udp）。
    let udp = ctx.alloc(Cell::Class {
        name: "Udp".into(),
        fields: HashMap::new(),
    });
    let udp_cell = ctx.alloc(Cell::Value(IrValue::Class(udp)));
    let mut net_fields = HashMap::new();
    net_fields.insert("udp".into(), udp_cell);
    let net = ctx.alloc(Cell::Class {
        name: "Net".into(),
        fields: net_fields,
    });
    let net_cell = ctx.alloc(Cell::Value(IrValue::Class(net)));
    let mut fields = HashMap::new();
    fields.insert("fs".into(), fs_cell);
    fields.insert("time".into(), time_cell);
    fields.insert("net".into(), net_cell);
    fields.insert(
        "runtime".into(),
        ctx.alloc(Cell::Value(str_val("threaded"))),
    );
    // G2（io 差异项）：io.stdout/io.stderr 独立字节流（write_all 写真实句柄；
    // 类名 Stdout/Stderr 供分派，无 fd 注册表）
    let stdout = ctx.alloc(Cell::Class {
        name: "Stdout".into(),
        fields: HashMap::new(),
    });
    fields.insert(
        "stdout".into(),
        ctx.alloc(Cell::Value(IrValue::Class(stdout))),
    );
    let stderr = ctx.alloc(Cell::Class {
        name: "Stderr".into(),
        fields: HashMap::new(),
    });
    fields.insert(
        "stderr".into(),
        ctx.alloc(Cell::Value(IrValue::Class(stderr))),
    );
    // G3（E3.2 ipc）：io.ipc.pipe() / io.ipc.shm(name, size)——进程内 IPC 原语
    let ipc = ctx.alloc(Cell::Class {
        name: "Ipc".into(),
        fields: HashMap::new(),
    });
    fields.insert("ipc".into(), ctx.alloc(Cell::Value(IrValue::Class(ipc))));
    // G4（E3.3 storage/archive）：io.storage.open(path) / io.archive.compress/decompress
    let storage = ctx.alloc(Cell::Class {
        name: "Storage".into(),
        fields: HashMap::new(),
    });
    fields.insert(
        "storage".into(),
        ctx.alloc(Cell::Value(IrValue::Class(storage))),
    );
    let archive = ctx.alloc(Cell::Class {
        name: "Archive".into(),
        fields: HashMap::new(),
    });
    fields.insert(
        "archive".into(),
        ctx.alloc(Cell::Value(IrValue::Class(archive))),
    );
    // G5（E3.3 text/rng）：io.text.* 正则；io.rng.* 伪随机数（类名 RngNs 避开示例
    // 84-rng 的用户类 Rng——内建方法先于用户方法分派）
    let text = ctx.alloc(Cell::Class {
        name: "Text".into(),
        fields: HashMap::new(),
    });
    fields.insert("text".into(), ctx.alloc(Cell::Value(IrValue::Class(text))));
    let rng = ctx.alloc(Cell::Class {
        name: "RngNs".into(),
        fields: HashMap::new(),
    });
    fields.insert("rng".into(), ctx.alloc(Cell::Value(IrValue::Class(rng))));
    // A6：标准库数据结构——Bitmap 位图命名空间
    let bitmap = ctx.alloc(Cell::Class {
        name: "BitmapNs".into(),
        fields: HashMap::new(),
    });
    fields.insert(
        "bitmap".into(),
        ctx.alloc(Cell::Value(IrValue::Class(bitmap))),
    );
    IrValue::Class(ctx.alloc(Cell::Class {
        name: "Io".into(),
        fields,
    }))
}

/// 隐式环境值（对齐 oracle 隐式环境注入：alloc→Alloc、io/test_io/stdout/stderr→Io、
/// pi→Float(PI)、Vec/Deque/Table→空 Arr、Map→空 Map）
pub(crate) fn implicit_env_value(ctx: &mut Ctx, name: &str) -> IrValue {
    match name {
        "alloc" => {
            // Q8：每线程 alloc 覆盖（线程 fn 运行期间）优先；否则全局 Class("Alloc") 哨兵
            if let Some(a) = &ctx.current_alloc {
                a.clone()
            } else {
                IrValue::Class(ctx.alloc(Cell::Class {
                    name: "Alloc".into(),
                    fields: HashMap::new(),
                }))
            }
        }
        // Arena 类型构造根（G1）：`Arena.init(alloc)` → 真实 arena 句柄
        "Arena" => IrValue::Arena(ctx.alloc(Cell::Arena(ArenaStateIr::new()))),
        "io" | "test_io" | "stdout" | "stderr" => io_value_ir(ctx),
        "pi" => IrValue::Float(std::f64::consts::PI),
        // G4：集合隐式根 = 空容器，持全局 alloc（`Vec<i32>` 类型表达式 / `Vec.init(alloc)` 基）
        "Vec" | "Deque" | "Table" => {
            let alloc = implicit_env_value(ctx, "alloc");
            make_vec_with(ctx, Vec::new(), alloc)
        }
        "Map" => {
            let alloc = implicit_env_value(ctx, "alloc");
            make_map_with(ctx, HashMap::new(), alloc)
        }
        _ => IrValue::Void,
    }
}

/// 可迭代值 → 元素值数组（iter/filter/map/sort/binary_search 共用；对齐 oracle
/// `iter_to_arr` interp.rs:1307-1357 的元素浅克隆语义）
pub(crate) fn arr_items(ctx: &mut Ctx, v: &IrValue) -> R<Vec<IrValue>> {
    match deref_value(ctx, v).clone() {
        IrValue::Arr(c) => match &ctx.cells[c] {
            Cell::Elems(e) => Ok(e.iter().map(|ec| ctx.cell_value(*ec).clone()).collect()),
            _ => Err(IrError::msg("TypeError", "bad array")),
        },
        // 集合（G4）：Vec 句柄（Ptr(Vec) 一层 deref 后为 Vec）——共享 Elems 元素
        IrValue::Vec(c) => match &ctx.cells[c] {
            Cell::Vec {
                arr: IrValue::Arr(ac),
                ..
            } => match &ctx.cells[*ac] {
                Cell::Elems(e) => Ok(e.iter().map(|ec| ctx.cell_value(*ec).clone()).collect()),
                _ => Err(IrError::msg("TypeError", "bad vec")),
            },
            _ => Err(IrError::msg("TypeError", "bad vec")),
        },
        IrValue::Slice { data, start, len } => match &ctx.cells[data] {
            Cell::Elems(e) => Ok(e[start..start + len]
                .iter()
                .map(|ec| ctx.cell_value(*ec).clone())
                .collect()),
            _ => Err(IrError::msg("TypeError", "bad slice")),
        },
        IrValue::Str(s) => Ok(s.iter().map(|b| IrValue::Int(*b as i128)).collect()),
        IrValue::Class(c) if class_name(ctx, c) == "Map" => {
            let fields = match &ctx.cells[c] {
                Cell::Class { fields, .. } => fields.clone(),
                _ => unreachable!(),
            };
            let mut out = Vec::new();
            for (k, vc) in fields {
                let mut f = HashMap::new();
                f.insert("key".into(), ctx.alloc(Cell::Value(str_val(&k))));
                f.insert("value".into(), vc);
                out.push(IrValue::Class(ctx.alloc(Cell::Class {
                    name: "KV".into(),
                    fields: f,
                })));
            }
            Ok(out)
        }
        // 集合（G4）：Map 句柄 → KV 条目（key/value 字段）
        IrValue::Map(c) => {
            let fields = match &ctx.cells[c] {
                Cell::Map { fields, .. } => fields.clone(),
                _ => unreachable!(),
            };
            let mut out = Vec::new();
            for (k, vc) in fields {
                let mut f = HashMap::new();
                f.insert("key".into(), ctx.alloc(Cell::Value(str_val(&k))));
                f.insert("value".into(), vc);
                out.push(IrValue::Class(ctx.alloc(Cell::Class {
                    name: "KV".into(),
                    fields: f,
                })));
            }
            Ok(out)
        }
        _ => Err(IrError::msg("NotIterable", "value is not iterable")),
    }
}

/// 任意可迭代值 → 元素数组（含用户 IIterable——复用 `make_iter` 的 next() 展开）
pub(crate) fn iter_to_arr_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    v: &IrValue,
    depth: usize,
) -> R<IrValue> {
    let items = make_iter(ctx, module, v, depth)?;
    let mut out = Vec::new();
    for it in items {
        out.push(ctx.cell_value(it.cell).clone());
    }
    Ok(make_arr(ctx, out))
}

/// Str/Arr/Slice → 字节（对齐 oracle `value_bytes` interp.rs:1436-1460）
pub(crate) fn value_bytes_ir(ctx: &Ctx, v: &IrValue) -> Option<Vec<u8>> {
    match deref_value(ctx, v) {
        IrValue::Str(s) => Some(s.clone()),
        IrValue::Arr(c) => match &ctx.cells[*c] {
            Cell::Elems(e) => Some(
                e.iter()
                    .map(|ec| match ctx.cell_value(*ec) {
                        IrValue::Int(i) => *i as u8,
                        _ => 0,
                    })
                    .collect(),
            ),
            _ => None,
        },
        IrValue::Slice { data, start, len } => match &ctx.cells[*data] {
            Cell::Elems(e) => {
                let mut out = Vec::with_capacity(*len);
                for i in 0..*len {
                    match ctx.cell_value(e[*start + i]) {
                        IrValue::Int(n) => out.push(*n as u8),
                        _ => return None,
                    }
                }
                Some(out)
            }
            _ => None,
        },
        _ => None,
    }
}

/// 任意值 → 字节（标量/嵌套；Int 在 i32 范围用 4 字节——对齐 oracle `value_to_bytes`
/// interp.rs:5345-5369；Class 无布局表 → 空（Phase 7 取舍：堆类型请用 to_json）。
/// 当前 run_ir 侧 Str/Arr 的 to_bytes 走内联实现；本 helper 预留给 P7e LLVM 原生后端。
#[allow(dead_code)]
pub(crate) fn value_to_bytes_ir(ctx: &Ctx, v: &IrValue) -> Vec<u8> {
    match v {
        IrValue::Int(i) => {
            if *i >= i32::MIN as i128 && *i <= i32::MAX as i128 {
                (*i as i32).to_le_bytes().to_vec()
            } else {
                (*i as i64).to_le_bytes().to_vec()
            }
        }
        IrValue::Float(f) => f.to_le_bytes().to_vec(),
        IrValue::Bool(b) => vec![if *b { 1 } else { 0 }],
        IrValue::Str(s) => {
            let mut out = (s.len() as u64).to_le_bytes().to_vec();
            out.extend_from_slice(s);
            out
        }
        IrValue::Ptr(c) => value_to_bytes_ir(ctx, ctx.cell_value(*c)),
        IrValue::Boxed(c) => match &ctx.cells[*c] {
            Cell::Boxed { data, .. } => value_to_bytes_ir(ctx, ctx.cell_value(*data)),
            _ => vec![],
        },
        // 集合（G4）：Vec 委托 Arr 字节化
        IrValue::Vec(c) => match &ctx.cells[*c] {
            Cell::Vec { arr, .. } => value_to_bytes_ir(ctx, arr),
            _ => vec![],
        },
        _ => vec![],
    }
}

/// 任意值 → JSON 字符串（对齐 oracle `value_to_json` interp.rs:5372-5412）
pub(crate) fn value_to_json_ir(ctx: &Ctx, v: &IrValue) -> String {
    match v {
        IrValue::Int(i) => i.to_string(),
        IrValue::Float(f) => f.to_string(),
        IrValue::Bool(b) => b.to_string(),
        IrValue::Str(s) => format!(
            "\"{}\"",
            String::from_utf8_lossy(s)
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
        ),
        IrValue::Arr(c) => match &ctx.cells[*c] {
            Cell::Elems(e) => {
                let items: Vec<String> = e
                    .iter()
                    .map(|ec| value_to_json_ir(ctx, ctx.cell_value(*ec)))
                    .collect();
                format!("[{}]", items.join(","))
            }
            _ => "null".into(),
        },
        IrValue::Slice { data, start, len } => match &ctx.cells[*data] {
            Cell::Elems(e) => {
                let items: Vec<String> = e[*start..*start + *len]
                    .iter()
                    .map(|ec| value_to_json_ir(ctx, ctx.cell_value(*ec)))
                    .collect();
                format!("[{}]", items.join(","))
            }
            _ => "null".into(),
        },
        // 集合（G4）：Vec 委托 Arr JSON 化；Map 序列化为对象
        IrValue::Vec(c) => match &ctx.cells[*c] {
            Cell::Vec { arr, .. } => value_to_json_ir(ctx, arr),
            _ => "null".into(),
        },
        IrValue::Map(c) => match &ctx.cells[*c] {
            Cell::Map { fields, .. } => {
                let items: Vec<String> = fields
                    .iter()
                    .map(|(k, vc)| {
                        format!("\"{k}\":{}", value_to_json_ir(ctx, ctx.cell_value(*vc)))
                    })
                    .collect();
                format!("{{{}}}", items.join(","))
            }
            _ => "null".into(),
        },
        IrValue::Class(c) => {
            let items: Vec<String> = match &ctx.cells[*c] {
                Cell::Class { fields, .. } => fields
                    .iter()
                    .map(|(k, vc)| {
                        format!("\"{k}\":{}", value_to_json_ir(ctx, ctx.cell_value(*vc)))
                    })
                    .collect(),
                _ => Vec::new(),
            };
            format!("{{{}}}", items.join(","))
        }
        IrValue::Opt(Some(b)) => value_to_json_ir(ctx, b),
        IrValue::Opt(None) => "null".into(),
        IrValue::Ptr(c) => value_to_json_ir(ctx, ctx.cell_value(*c)),
        IrValue::Boxed(c) => match &ctx.cells[*c] {
            Cell::Boxed { data, .. } => value_to_json_ir(ctx, ctx.cell_value(*data)),
            _ => "null".into(),
        },
        IrValue::Err { name, .. } => format!("\"error.{name}\""),
        _ => "null".into(),
    }
}

/// @intCast 目标宽度范围（Debug 溢出检查；对齐 oracle `int_width_bounds` interp.rs:5067-5083）
pub(crate) fn int_width_bounds_ir(ty: &str) -> Option<(i128, i128)> {
    match ty {
        "i8" => Some((i8::MIN as i128, i8::MAX as i128)),
        "i16" => Some((i16::MIN as i128, i16::MAX as i128)),
        "i32" => Some((i32::MIN as i128, i32::MAX as i128)),
        "i64" => Some((i64::MIN as i128, i64::MAX as i128)),
        "i128" => Some((i128::MIN, i128::MAX)),
        "isize" => Some((isize::MIN as i128, isize::MAX as i128)),
        "u8" => Some((0, u8::MAX as i128)),
        "u16" => Some((0, u16::MAX as i128)),
        "u32" => Some((0, u32::MAX as i128)),
        "u64" => Some((0, u64::MAX as i128)),
        "u128" => Some((0, u128::MAX as i128)),
        "usize" => Some((0, usize::MAX as i128)),
        _ => None,
    }
}

/// @sizeOf(T) 标量表（对齐 oracle `type_size_of` interp.rs:5086-5122 的标量/引用面；
/// 用户 class/enum 无布局表 → None）
pub(crate) fn scalar_size_ir(ty: &str) -> Option<usize> {
    match ty {
        "i8" | "u8" | "bool" => Some(1),
        "i16" | "u16" | "f16" => Some(2),
        "i32" | "u32" | "f32" => Some(4),
        "i64" | "u64" | "isize" | "usize" | "f64" => Some(8),
        "i128" | "u128" | "f128" => Some(16),
        "String" | "Vec" | "Map" | "Deque" | "Table" | "Allocator" => Some(8),
        _ => None,
    }
}

// ---------- K1 无标签 union（ADR-0014，2026-08-18）----------
// 运行时形态 = `Cell::Class` + `@union` 标记；写字段 → 字节重解释同步其余字段
// （C 风格内存双关）。helper 对齐 interp `union_write_scalar`/`union_read_scalar`/
// `union_sync_fields`。

/// 类型名（union 字段须为标量 → `Type::Named(n, _)`）
pub(crate) fn union_ty_name(t: &Type) -> Option<String> {
    match t.strip() {
        Type::Named(n, _) => Some(n.clone()),
        _ => None,
    }
}

/// 标量值 → 小端字节（i128/u128 全 16 字节，对齐 interp union_write_scalar）
pub(crate) fn write_scalar_ir(out: &mut [u8], n: &str, v: &IrValue) {
    match (n, v) {
        ("i8" | "u8", IrValue::Int(i)) => out[0] = *i as u8,
        ("i16" | "u16", IrValue::Int(i)) => out[..2].copy_from_slice(&(*i as i16).to_le_bytes()),
        ("i32" | "u32", IrValue::Int(i)) => out[..4].copy_from_slice(&(*i as i32).to_le_bytes()),
        ("i64" | "u64" | "isize" | "usize", IrValue::Int(i)) => {
            out[..8].copy_from_slice(&(*i as i64).to_le_bytes())
        }
        ("i128" | "u128", IrValue::Int(i)) => out[..16].copy_from_slice(&i.to_le_bytes()),
        ("f32", IrValue::Float(f)) => out[..4].copy_from_slice(&(*f as f32).to_le_bytes()),
        ("f64" | "f16" | "f128", IrValue::Float(f)) => out[..8].copy_from_slice(&f.to_le_bytes()),
        ("bool", IrValue::Bool(b)) => out[0] = if *b { 1 } else { 0 },
        _ => {}
    }
}

/// 小端字节 → 标量值（对齐 interp union_read_scalar）
pub(crate) fn read_scalar_ir(bytes: &[u8], n: &str) -> R<IrValue> {
    let trunc = |msg: &str| IrError::msg("InvalidBytes", msg);
    match n {
        "i8" | "u8" => Ok(IrValue::Int(bytes.first().copied().unwrap_or(0) as i128)),
        "i16" | "u16" => {
            let b = bytes
                .get(..2)
                .ok_or_else(|| trunc("truncated union bytes"))?;
            Ok(IrValue::Int(
                i16::from_le_bytes(b.try_into().unwrap()) as i128
            ))
        }
        "i32" | "u32" => {
            let b = bytes
                .get(..4)
                .ok_or_else(|| trunc("truncated union bytes"))?;
            Ok(IrValue::Int(
                i32::from_le_bytes(b.try_into().unwrap()) as i128
            ))
        }
        "i64" | "u64" | "isize" | "usize" => {
            let b = bytes
                .get(..8)
                .ok_or_else(|| trunc("truncated union bytes"))?;
            Ok(IrValue::Int(
                i64::from_le_bytes(b.try_into().unwrap()) as i128
            ))
        }
        "i128" | "u128" => {
            let b = bytes
                .get(..16)
                .ok_or_else(|| trunc("truncated union bytes"))?;
            Ok(IrValue::Int(i128::from_le_bytes(b.try_into().unwrap())))
        }
        "f32" => {
            let b = bytes
                .get(..4)
                .ok_or_else(|| trunc("truncated union bytes"))?;
            Ok(IrValue::Float(
                f32::from_le_bytes(b.try_into().unwrap()) as f64
            ))
        }
        "f64" | "f16" | "f128" => {
            let b = bytes
                .get(..8)
                .ok_or_else(|| trunc("truncated union bytes"))?;
            Ok(IrValue::Float(f64::from_le_bytes(b.try_into().unwrap())))
        }
        "bool" => Ok(IrValue::Bool(bytes.first().copied().unwrap_or(0) != 0)),
        _ => Ok(IrValue::Void),
    }
}

/// K1 union 写字段同步（IR 运行时）：写 `written` 字段后，把该字段字节重解释为
/// 其余每个字段的类型（C 风格 union 语义，字段全标量）。`c` = `Cell::Class` 索引。
pub(crate) fn union_sync_ir(ctx: &mut Ctx, module: &IrModule, c: usize, written: &str) -> R<()> {
    let (cname, fields) = match &ctx.cells[c] {
        Cell::Class { name, fields } => (name.clone(), fields.clone()),
        _ => return Err(IrError::msg("TypeError", "union sync on non-class")),
    };
    let decls = module
        .unions
        .get(&cname)
        .cloned()
        .ok_or_else(|| IrError::msg("TypeError", format!("`{cname}` 不是 union 类型")))?;
    let wcell = fields.get(written).copied().ok_or_else(|| {
        IrError::msg(
            "NoField",
            format!("union `{cname}` has no field `{written}`"),
        )
    })?;
    let wv = ctx.cell_value(wcell).clone();
    let wty = decls
        .iter()
        .find(|(n, _)| n == written)
        .map(|(_, t)| t.clone())
        .ok_or_else(|| {
            IrError::msg(
                "NoField",
                format!("union `{cname}` has no field `{written}`"),
            )
        })?;
    let wname =
        union_ty_name(&wty).ok_or_else(|| IrError::msg("TypeError", "union 字段必须为标量类型"))?;
    let width = scalar_size_ir(&wname)
        .ok_or_else(|| IrError::msg("TypeError", format!("字段 `{wname}` 无标量宽度")))?;
    let mut buf = vec![0u8; width];
    write_scalar_ir(&mut buf, &wname, &wv);
    for (fdname, fdty) in &decls {
        if fdname == written {
            continue;
        }
        let Some(fname) = union_ty_name(fdty) else {
            continue;
        };
        let dv = read_scalar_ir(&buf, &fname)?;
        let nc = ctx.alloc(Cell::Value(dv));
        if let Cell::Class { fields: fs, .. } = &mut ctx.cells[c] {
            fs.insert(fdname.clone(), nc);
        }
    }
    Ok(())
}

/// 调用函数值（Fn 引用 / Closure；对齐 oracle `call_closure_value` interp.rs:1504-1511）
pub(crate) fn call_closure_value_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    f: &IrValue,
    args: &[IrValue],
) -> R<IrValue> {
    match f {
        IrValue::Closure {
            func,
            captures,
            is_mut,
            ..
        } => call_closure_ir(ctx, module, *func, captures, args, *is_mut, 0),
        IrValue::Fn(name) => {
            let idx = pick_func(ctx, module, name, args)
                .ok_or_else(|| IrError::msg("NoFunction", format!("no function `{name}`")))?;
            exec_func(ctx, module, idx, args, 0)
        }
        _ => Err(IrError::msg("TypeError", "expected function")),
    }
}

pub(crate) fn call_closure_bool_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    f: &IrValue,
    args: &[IrValue],
) -> R<bool> {
    Ok(call_closure_value_ir(ctx, module, f, args)?.as_bool())
}

/// io.print（对齐 oracle `call_io_print` interp.rs:4029-4087）：`{}` 与 `{x}`/`{b}`/`{s}`
/// 占位符格式化；输出缓冲到 `ctx.out`（`execute_ir` 运行后冲刷）
pub(crate) fn call_io_print_ir(ctx: &mut Ctx, args: &[IrValue]) -> R<()> {
    if args.is_empty() {
        return Err(IrError::msg(
            "ArityMismatch",
            "io.print expects a format string",
        ));
    }
    let fmt = match deref_value(ctx, &args[0]) {
        IrValue::Str(s) => s.clone(),
        _ => return Err(IrError::msg("TypeError", "io.print expects &[u8]")),
    };
    let mut out = Vec::new();
    let mut argi = 1usize;
    let mut i = 0usize;
    while i < fmt.len() {
        if fmt[i] == b'{' {
            if let Some(close) = fmt[i + 1..].iter().position(|&c| c == b'}') {
                if argi < args.len() {
                    let v = deref_value(ctx, &args[argi]);
                    let s = format_spec_value_ir(ctx, v, &fmt[i + 1..i + 1 + close])?;
                    out.extend_from_slice(s.as_bytes());
                    argi += 1;
                }
                i += close + 2;
                continue;
            }
        }
        out.push(fmt[i]);
        i += 1;
    }
    ctx.out.extend_from_slice(&out);
    Ok(())
}

/// 格式说明符（B1/B3，镜像 interp `format_spec_value`）：`{}` 默认 / `{d}` / `{x}` /
/// `{X}` / `{b}` / `{e}` / `{s}` + 宽度/对齐/精度（`{:8}`、`{:<6}`、`{:.2}`）。
/// 未知类型字符 → `FormatError`（B2：不再按字面量静默输出）。
pub(crate) fn format_spec_value_ir(ctx: &Ctx, v: &IrValue, inner: &[u8]) -> R<String> {
    let mut p = if inner.first() == Some(&b':') { 1 } else { 0 };
    let align = match inner.get(p) {
        Some(b'<') | Some(b'>') | Some(b'^') => {
            let a = inner[p];
            p += 1;
            a
        }
        _ => b'>',
    };
    let mut width: Option<usize> = None;
    let mut ws = String::new();
    while p < inner.len() && inner[p].is_ascii_digit() {
        ws.push(inner[p] as char);
        p += 1;
    }
    if !ws.is_empty() {
        width = ws.parse().ok();
    }
    let mut precision: Option<usize> = None;
    if p < inner.len() && inner[p] == b'.' {
        p += 1;
        let mut ps = String::new();
        while p < inner.len() && inner[p].is_ascii_digit() {
            ps.push(inner[p] as char);
            p += 1;
        }
        precision = ps.parse().ok();
    }
    let ty = inner.get(p).copied();
    if p + usize::from(ty.is_some()) < inner.len() {
        return Err(IrError::msg("FormatError", "unknown format specifier"));
    }
    let display = v.display(ctx);
    let mut s = match ty {
        Some(b'd') => match v {
            IrValue::Int(n) => n.to_string(),
            IrValue::Float(f) => f.to_string(),
            _ => display,
        },
        Some(b'x') => match v {
            IrValue::Int(n) => format!("{n:x}"),
            _ => display,
        },
        Some(b'X') => match v {
            IrValue::Int(n) => format!("{n:X}"),
            _ => display,
        },
        Some(b'b') => match v {
            IrValue::Int(n) => format!("{n:b}"),
            _ => display,
        },
        Some(b'e') => match v {
            IrValue::Float(f) => format!("{f:e}"),
            _ => display,
        },
        Some(b's') => display,
        Some(_) => return Err(IrError::msg("FormatError", "unknown format specifier")),
        None => display,
    };
    if let Some(pr) = precision {
        if let IrValue::Float(f) = v {
            s = format!("{f:.pr$}");
        }
    }
    if let Some(w) = width {
        if s.len() < w {
            let pad = w - s.len();
            match align {
                b'<' => s = format!("{s}{}", " ".repeat(pad)),
                b'^' => {
                    let l = pad / 2;
                    s = format!("{}{s}{}", " ".repeat(l), " ".repeat(pad - l));
                }
                _ => s = format!("{}{s}", " ".repeat(pad)),
            }
        }
    }
    Ok(s)
}

/// 标量方法（ICompare/INumber 族内建：add/sub/mul/div/neg/mod/abs/eq/lt/pow；
/// 对齐 oracle `call_scalar_method` interp.rs:3408-3509）
pub(crate) fn parser_bytes(ctx: &Ctx, args: &[IrValue], ix: usize) -> R<Vec<u8>> {
    let v = args
        .get(ix)
        .ok_or_else(|| IrError::msg("ArityMismatch", "missing argument"))?;
    match deref_value(ctx, v) {
        IrValue::Str(s) => Ok(s.clone()),
        IrValue::Ptr(c) => match ctx.cell_value(*c) {
            IrValue::Str(s) => Ok(s.clone()),
            _ => Err(IrError::msg("TypeError", "expected bytes")),
        },
        _ => Err(IrError::msg("TypeError", "expected bytes")),
    }
}

pub(crate) fn parser_pos(_ctx: &Ctx, args: &[IrValue], ix: usize) -> R<usize> {
    let v = args
        .get(ix)
        .ok_or_else(|| IrError::msg("ArityMismatch", "missing argument"))?;
    // 不 deref：位置参数是 `&pos` 指针（AddrSlot → IrValue::Ptr(cell)），
    // deref_value 会追到 pointee（Int）导致 Ptr 匹配失败（对齐 oracle interp get_pos）
    match v {
        IrValue::Ptr(c) => Ok(*c),
        _ => Err(IrError::msg("TypeError", "expected pointer")),
    }
}

pub(crate) fn parser_pos_int(ctx: &Ctx, cell: usize) -> R<i128> {
    match ctx.cell_value(cell) {
        IrValue::Int(i) => Ok(*i),
        _ => Err(IrError::msg("TypeError", "expected int position")),
    }
}

/// 对齐 oracle `call_parser_builtin` interp.rs:4656-4769
pub(crate) fn call_parser_builtin_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    name: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let _ = module;
    match name {
        "skip_space" => {
            let data = parser_bytes(ctx, args, 0)?;
            let pc = parser_pos(ctx, args, 1)?;
            let mut i = parser_pos_int(ctx, pc)? as usize;
            while i < data.len() && data[i].is_ascii_whitespace() {
                i += 1;
            }
            ctx.set_cell(pc, IrValue::Int(i as i128));
            Ok(Some(IrValue::Void))
        }
        "peek" => {
            let data = parser_bytes(ctx, args, 0)?;
            let pc = parser_pos(ctx, args, 1)?;
            let i = parser_pos_int(ctx, pc)? as usize;
            Ok(Some(if i < data.len() {
                IrValue::Opt(Some(Box::new(IrValue::Int(data[i] as i128))))
            } else {
                IrValue::Opt(None)
            }))
        }
        "advance" => {
            let pc = parser_pos(ctx, args, 1)?;
            let i = parser_pos_int(ctx, pc)?;
            ctx.set_cell(pc, IrValue::Int(i + 1));
            Ok(Some(IrValue::Void))
        }
        "expect" => {
            let data = parser_bytes(ctx, args, 0)?;
            let pc = parser_pos(ctx, args, 1)?;
            let want = match deref_value(
                ctx,
                args.get(2)
                    .ok_or_else(|| IrError::msg("ArityMismatch", "expect"))?,
            ) {
                IrValue::Int(i) => *i as u8,
                _ => return Err(IrError::msg("TypeError", "expected byte")),
            };
            let i = parser_pos_int(ctx, pc)? as usize;
            if i < data.len() && data[i] == want {
                ctx.set_cell(pc, IrValue::Int(i as i128 + 1));
                Ok(Some(IrValue::Void))
            } else {
                Err(IrError::msg("UnexpectedToken", "expect: unexpected token"))
            }
        }
        "is_digit" => {
            let v = deref_value(
                ctx,
                args.get(0)
                    .ok_or_else(|| IrError::msg("ArityMismatch", "is_digit"))?,
            )
            .clone();
            match v {
                IrValue::Int(i) => Ok(Some(IrValue::Bool((i as u8 as char).is_ascii_digit()))),
                _ => Err(IrError::msg("TypeError", "expected int")),
            }
        }
        "parse_number" => {
            let data = parser_bytes(ctx, args, 0)?;
            let pc = parser_pos(ctx, args, 1)?;
            let mut i = parser_pos_int(ctx, pc)? as usize;
            let start = i;
            while i < data.len() && data[i].is_ascii_digit() {
                i += 1;
            }
            let n: i128 = String::from_utf8_lossy(&data[start..i])
                .parse()
                .unwrap_or(0);
            ctx.set_cell(pc, IrValue::Int(i as i128));
            Ok(Some(IrValue::Int(n)))
        }
        _ => Ok(None),
    }
}

// ---- 数据/路径参数辅助 ----

pub(crate) fn str_arg_ir(ctx: &Ctx, args: &[IrValue], i: usize) -> R<Vec<u8>> {
    let a = args
        .get(i)
        .ok_or_else(|| IrError::msg("ArityMismatch", "missing argument"))?;
    match deref_value(ctx, a) {
        IrValue::Str(s) => Ok(s.clone()),
        _ => Err(IrError::msg("TypeError", "expected &[u8]")),
    }
}

pub(crate) fn path_arg_ir(ctx: &Ctx, args: &[IrValue], i: usize) -> R<String> {
    Ok(String::from_utf8_lossy(&str_arg_ir(ctx, args, i)?).into_owned())
}

pub(crate) fn int_arg_ir(ctx: &Ctx, args: &[IrValue], i: usize) -> R<i128> {
    let a = args
        .get(i)
        .ok_or_else(|| IrError::msg("ArityMismatch", "missing argument"))?;
    match deref_value(ctx, a) {
        IrValue::Int(n) => Ok(*n),
        _ => Err(IrError::msg("TypeError", "expected int")),
    }
}

// ---- File/网络句柄 ----

pub(crate) fn file_fd_ir(ctx: &Ctx, v: &IrValue) -> R<i64> {
    match deref_value(ctx, v) {
        IrValue::Class(c) if class_name(ctx, *c) == "File" => match &ctx.cells[*c] {
            Cell::Class { fields, .. } => match fields.get("_fd") {
                Some(fc) => match ctx.cell_value(*fc) {
                    IrValue::Int(fd) => Ok(*fd as i64),
                    _ => Err(IrError::msg("BadFd", "bad file descriptor")),
                },
                _ => Err(IrError::msg("BadFd", "bad file descriptor")),
            },
            _ => Err(IrError::msg("BadFd", "bad file descriptor")),
        },
        _ => Err(IrError::msg("TypeError", "expected File")),
    }
}

pub(crate) fn net_fd_ir(ctx: &Ctx, v: &IrValue) -> R<i64> {
    match deref_value(ctx, v) {
        IrValue::Class(c) => match &ctx.cells[*c] {
            Cell::Class { fields, .. } => match fields.get("fd") {
                Some(fc) => match ctx.cell_value(*fc) {
                    IrValue::Int(fd) => Ok(*fd as i64),
                    _ => Err(IrError::msg("BadFd", "bad net descriptor")),
                },
                _ => Err(IrError::msg("BadFd", "bad net descriptor")),
            },
            _ => Err(IrError::msg("BadFd", "bad net descriptor")),
        },
        _ => Err(IrError::msg("TypeError", "expected connection")),
    }
}

pub(crate) fn io_error_name_ir(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "NotFound".into(),
        std::io::ErrorKind::PermissionDenied => "PermissionDenied".into(),
        _ => "Io".into(),
    }
}

pub(crate) fn register_file_ir(ctx: &mut Ctx, f: std::fs::File) -> IrValue {
    let fd = ctx.next_fd;
    ctx.next_fd += 1;
    ctx.files.insert(fd, f);
    let mut fields = HashMap::new();
    fields.insert(
        "_fd".into(),
        ctx.alloc(Cell::Value(IrValue::Int(fd as i128))),
    );
    IrValue::Class(ctx.alloc(Cell::Class {
        name: "File".into(),
        fields,
    }))
}

// ---- G1-G5 注册表句柄解析（Dir/Pipe/Shm/KvStore）----

/// Dir 值 → 注册表 fd（`_fd` 字段；先 deref_value 剥 Ptr）
pub(crate) fn dir_fd_ir(ctx: &Ctx, v: &IrValue) -> R<i64> {
    match deref_value(ctx, v) {
        IrValue::Class(c) => match &ctx.cells[*c] {
            Cell::Class { fields, .. } => match fields.get("_fd") {
                Some(fc) => match ctx.cell_value(*fc) {
                    IrValue::Int(fd) => Ok(*fd as i64),
                    _ => Err(IrError::msg("BadFd", "bad dir descriptor")),
                },
                _ => Err(IrError::msg("BadFd", "bad dir descriptor")),
            },
            _ => Err(IrError::msg("BadFd", "bad dir descriptor")),
        },
        _ => Err(IrError::msg("TypeError", "expected Dir")),
    }
}

/// Pipe 值 → 管道 id（`pipe` 字段；先 deref_value 剥 Ptr）
pub(crate) fn pipe_id_ir(ctx: &Ctx, v: &IrValue) -> R<i64> {
    match deref_value(ctx, v) {
        IrValue::Class(c) => match &ctx.cells[*c] {
            Cell::Class { fields, .. } => match fields.get("pipe") {
                Some(fc) => match ctx.cell_value(*fc) {
                    IrValue::Int(id) => Ok(*id as i64),
                    _ => Err(IrError::msg("BadFd", "bad pipe descriptor")),
                },
                _ => Err(IrError::msg("BadFd", "bad pipe descriptor")),
            },
            _ => Err(IrError::msg("BadFd", "bad pipe descriptor")),
        },
        _ => Err(IrError::msg("TypeError", "expected pipe")),
    }
}

/// Shm 值 → 共享内存 id（`shm` 字段）
pub(crate) fn shm_id_ir(ctx: &Ctx, v: &IrValue) -> R<i64> {
    match deref_value(ctx, v) {
        IrValue::Class(c) => match &ctx.cells[*c] {
            Cell::Class { fields, .. } => match fields.get("shm") {
                Some(fc) => match ctx.cell_value(*fc) {
                    IrValue::Int(id) => Ok(*id as i64),
                    _ => Err(IrError::msg("BadFd", "bad shm descriptor")),
                },
                _ => Err(IrError::msg("BadFd", "bad shm descriptor")),
            },
            _ => Err(IrError::msg("BadFd", "bad shm descriptor")),
        },
        _ => Err(IrError::msg("TypeError", "expected Shm")),
    }
}

/// KvStore 值 → 注册表 id（`store` 字段；先 deref_value 剥 Ptr）
pub(crate) fn store_id_ir(ctx: &Ctx, v: &IrValue) -> R<i64> {
    match deref_value(ctx, v) {
        IrValue::Class(c) => match &ctx.cells[*c] {
            Cell::Class { fields, .. } => match fields.get("store") {
                Some(fc) => match ctx.cell_value(*fc) {
                    IrValue::Int(id) => Ok(*id as i64),
                    _ => Err(IrError::msg("BadFd", "bad store descriptor")),
                },
                _ => Err(IrError::msg("BadFd", "bad store descriptor")),
            },
            _ => Err(IrError::msg("BadFd", "bad store descriptor")),
        },
        _ => Err(IrError::msg("TypeError", "expected KvStore")),
    }
}

// ---- G1-G5 网络/文件系统共享实现（对齐 oracle interp.rs 对应函数）----

/// 解析 UDP 对端地址串 "host:port" → (host, port)。
pub(crate) fn parse_udp_addr_ir(s: &str) -> std::result::Result<(String, u16), &'static str> {
    match s.rsplit_once(':') {
        Some((host, port)) => match port.parse::<u16>() {
            Ok(p) => Ok((host.to_string(), p)),
            Err(_) => Err("InvalidAddress"),
        },
        None => Err("InvalidAddress"),
    }
}

/// UDP 绑定共享实现：`udp_bind(host, port)` → UdpSocket 值（fd 注册表）；
/// 读超时 200ms（recv_from 空队列 → error.TimedOut，不阻塞挂起测试）。
pub(crate) fn udp_bind_ir(ctx: &mut Ctx, module: &IrModule, host: &str, port: u16) -> R<IrValue> {
    let addr = format!("{host}:{port}");
    match std::net::UdpSocket::bind(&addr) {
        Ok(sock) => {
            let _ = sock.set_read_timeout(Some(std::time::Duration::from_millis(200)));
            let fd = ctx.next_net_fd;
            ctx.next_net_fd += 1;
            ctx.udp_sockets.insert(fd, sock);
            let mut fields = HashMap::new();
            fields.insert(
                "fd".into(),
                ctx.alloc(Cell::Value(IrValue::Int(fd as i128))),
            );
            Ok(IrValue::Class(ctx.alloc(Cell::Class {
                name: "UdpSocket".into(),
                fields,
            })))
        }
        Err(e) => Ok(err_val(module, &io_error_name_ir(&e))),
    }
}

/// G1（E3.1）：HTTP GET 客户端——`http://host[:port][/path]` → TCP connect →
/// `GET {path} HTTP/1.1` + Host 头 → 读响应 → 按 Content-Length 提取体。
pub(crate) fn http_get_ir(url: &str) -> std::result::Result<Vec<u8>, String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| "InvalidUrl".to_string())?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (
            h.to_string(),
            p.parse::<u16>().map_err(|_| "InvalidUrl".to_string())?,
        ),
        None => (authority.to_string(), 80u16),
    };
    let mut stream =
        std::net::TcpStream::connect((host.as_str(), port)).map_err(|e| io_error_name_ir(&e))?;
    let req = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(req.as_bytes())
        .map_err(|e| io_error_name_ir(&e))?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| io_error_name_ir(&e))?;
    // 状态行 + 头段由第一个空行分隔；体按 Content-Length 取（无则取空行后全部）
    let head_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .ok_or_else(|| "BadResponse".to_string())?;
    let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
    let body = &raw[head_end..];
    if !head.starts_with("HTTP/1.1 200") && !head.starts_with("HTTP/1.0 200") {
        // 非 200：体返回给调用方诊断（错误名 = Http{code}）
        let code = head.split_whitespace().nth(1).unwrap_or("000").to_string();
        return Err(format!("Http{code}"));
    }
    let mut len: Option<usize> = None;
    for line in head.lines() {
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            if let Ok(n) = v.trim().parse::<usize>() {
                len = Some(n);
            }
        }
    }
    Ok(match len {
        Some(n) => body[..n.min(body.len())].to_vec(),
        None => body.to_vec(),
    })
}

/// G2（io 差异项）：枚举目录路径为 Vec(DirEntry)——每条 = {name: 文件名, is_dir: 是否目录}。
/// 供 io.fs.list_dir（路径/句柄双形态）与 dir.list_dir(alloc) 共用。
pub(crate) fn list_dir_entries_ir(ctx: &mut Ctx, module: &IrModule, path: &str) -> R<IrValue> {
    match std::fs::read_dir(path) {
        Ok(rd) => {
            let entries: Vec<IrValue> = rd
                .flatten()
                .map(|e| {
                    let mut fields = HashMap::new();
                    fields.insert(
                        "name".into(),
                        ctx.alloc(Cell::Value(str_val(&e.file_name().to_string_lossy()))),
                    );
                    fields.insert(
                        "is_dir".into(),
                        ctx.alloc(Cell::Value(IrValue::Bool(
                            e.file_type().map(|t| t.is_dir()).unwrap_or(false),
                        ))),
                    );
                    IrValue::Class(ctx.alloc(Cell::Class {
                        name: "DirEntry".into(),
                        fields,
                    }))
                })
                .collect();
            Ok(make_arr(ctx, entries))
        }
        Err(e) => Ok(err_val(module, &io_error_name_ir(&e))),
    }
}

// ---- io.fs / io.time / io.net 方法族（对齐 oracle call_fs_method 等）----
