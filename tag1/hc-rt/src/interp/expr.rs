//! 解释器表达式求值：字面量、标识符、运算符、调用等表达式求值

use super::*;

impl Interp {
    pub fn eval(&mut self, e: &Expr) -> Result<Value> {
        match e {
            Expr::IntLit { text, .. } => {
                let (n, _) = parse_int_text(text)?;
                Ok(Value::Int(n))
            }
            Expr::FloatLit { text, .. } => {
                let t = text.trim_end_matches(|c: char| c.is_alphabetic());
                let f: f64 = t.replace('_', "").parse().map_err(|_| {
                    RtError::msg("BadFloat", format!("invalid float literal `{text}`"))
                })?;
                Ok(Value::Float(f))
            }
            Expr::StrLit { value, .. } => Ok(Value::str(value)),
            Expr::CharLit(c, _) => Ok(Value::Int(*c as i128)),
            Expr::BoolLit(b, _) => Ok(Value::Bool(*b)),
            Expr::NullLit(_) => Ok(Value::Opt(None)),
            Expr::VoidLit(_) => Ok(Value::Void),
            Expr::ErrorLit(name, _) => Ok(self.err_val(name)),
            Expr::Ident(name, span) => {
                // 隐式环境注入
                match name.as_str() {
                    "alloc" | "page_allocator" => {
                        // E1（ADR-0013）：受限脚本模式——分配不可用（无运行时环境）
                        if self.script_mode {
                            return Err(RtError::msg(
                                "ScriptForbidden",
                                "script 块中 alloc 不可用（受限 H 核心子集）",
                            ));
                        }
                        // Q8：alloc 先查作用域（线程子任务可绑定每线程 alloc 实例），
                        // 否则回退全局 Page 分配器（Phase 1：AllocatorImpl::Page）
                        if let Some(cell) = self.lookup(name) {
                            return Ok(cell.borrow().clone());
                        }
                        return Ok(Value::Allocator(Rc::new(RefCell::new(AllocatorImpl::Page))));
                    }
                    "io" => {
                        if self.script_mode {
                            return Err(RtError::msg(
                                "ScriptForbidden",
                                "script 块中 io 不可用（受限 H 核心子集）",
                            ));
                        }
                        return Ok(self.io_value());
                    }
                    "stdout" | "stderr" => {
                        if self.script_mode {
                            return Err(RtError::msg(
                                "ScriptForbidden",
                                "script 块中 io 不可用（受限 H 核心子集）",
                            ));
                        }
                        return Ok(self.io_value());
                    }
                    "pi" => return Ok(Value::Float(std::f64::consts::PI)),
                    "Vec" | "Deque" => return Ok(Value::vec(vec![], Value::Alloc)),
                    "Map" => return Ok(Value::map(HashMap::new(), Value::Alloc)),
                    "Table" => return Ok(Value::vec(vec![], Value::Alloc)),
                    "Pipe" | "Tee" | "Funnel" | "Hub" => {
                        return Ok(Value::class(name, HashMap::new()))
                    }
                    _ => {}
                }
                // ADR-0010：import 环境别名（`import H.std.{io as my}` → `my` = io 环境）
                if let Some(env) = self.import_env.get(name.as_str()) {
                    if env == "io" {
                        if self.script_mode {
                            return Err(RtError::msg(
                                "ScriptForbidden",
                                "script 块中 io 不可用（受限 H 核心子集）",
                            ));
                        }
                        return Ok(self.io_value());
                    }
                }
                match self.lookup(name) {
                    Some(cell) => Ok(cell.borrow().clone()),
                    None => {
                        // 函数名作为值（FnRef：apply(square, 5) / var f = square）
                        if self.funcs.contains_key(name) {
                            Ok(Value::Fn(name.clone()))
                        } else {
                            Err(RtError::new("UndefinedName", Some(span.clone())))
                        }
                    }
                }
            }
            Expr::ArrayLit(items, _) => {
                let mut vals = Vec::new();
                for it in items {
                    vals.push(self.eval(it)?);
                }
                Ok(Value::arr(vals))
            }
            Expr::TupleLit(items, _) => Ok(Value::arr(
                items.iter().map(|e| self.eval(e)).collect::<Result<_>>()?,
            )),
            // struct 类型字面量（E1.2 组 D）：类型值——comptime 类型函数体内求值
            // （经 `hc::comptime` 具体化引擎），运行时表达式位置 = 用法错误
            Expr::StructType { span, .. } => Err(RtError::msg(
                "TypeValue",
                format!(
                    "类型值 `struct {{ ... }}` 仅 comptime 类型函数内可求值（第 {} 行第 {} 列）",
                    span.line, span.col
                ),
            )),
            Expr::ArrayType { span, .. } => Err(RtError::msg(
                "TypeValue",
                format!(
                    "类型值 `[n]T` 仅 comptime 类型函数内可求值（第 {} 行第 {} 列）",
                    span.line, span.col
                ),
            )),
            Expr::NamedLit {
                ty,
                ty_args,
                fields,
                ..
            } => {
                // class 字面量构造 / enum 带负载字面量。
                // E1.2 组 D：泛型应用 `Pair<i32>{...}` → 惰性具体化后按具体化名构造。
                let ty = if ty_args.is_empty() {
                    ty.clone()
                } else {
                    self.concrete_type_name(ty, ty_args)?
                };
                match self.types.get(&ty) {
                    Some(TypeDef::Class { .. }) => {
                        let mut f = HashMap::new();
                        for (k, v) in fields {
                            f.insert(k.clone(), self.eval(v)?);
                        }
                        Ok(Value::class(&ty, f))
                    }
                    Some(TypeDef::Enum { .. }) => {
                        // enum 变体字面量：Type{variant = payload}——单字段
                        if fields.len() == 1 {
                            let (variant, payload) = &fields[0];
                            let pv = self.eval(payload)?;
                            Ok(Value::Enum {
                                name: ty.clone(),
                                variant: variant.clone(),
                                payload: Some(Rc::new(pv)),
                            })
                        } else {
                            Err(RtError::msg(
                                "BadEnumLiteral",
                                "enum literal takes exactly one variant",
                            ))
                        }
                    }
                    Some(TypeDef::Union { fields: uf }) => {
                        // K1 union 字面量：`Foo { field = v }`——单字段，其余字段 = 该字段字节重解释。
                        // 运行时形态 = `Value::Class` + `@union` 标记（写路径同步重解释）。
                        if fields.len() != 1 {
                            return Err(RtError::msg(
                                "BadUnionLiteral",
                                "union literal takes exactly one field",
                            ));
                        }
                        let (fname, fval) = &fields[0];
                        // 先克隆字段声明，结束对 self.types 的借用，才能 self.eval
                        let uf = uf.clone();
                        let v = self.eval(fval)?;
                        let mut f = HashMap::new();
                        f.insert("@union".into(), Value::Bool(true));
                        for fd in &uf {
                            f.insert(fd.name.clone(), Self::union_default_value(&fd.ty));
                        }
                        f.insert(fname.clone(), v.clone());
                        let c = Value::class(&ty, f);
                        if let Value::Class(cr) = &c {
                            self.union_sync_fields(&mut cr.borrow_mut(), fname, &v)?;
                        }
                        Ok(c)
                    }
                    _ => Err(RtError::msg("UnknownType", format!("unknown type `{ty}`"))),
                }
            }
            Expr::Dot { base, field, .. } => {
                // E1（ADR-0013）：`types` 元数据对象（仅受限脚本模式）——
                // `types.all` / `types.type` 非调用形态取值
                if self.script_mode {
                    if let Expr::Ident(bname, _) = base.as_ref() {
                        if bname == "types" {
                            return self.types_meta(field, &[]);
                        }
                    }
                }
                // ExitType 内建枚举（L3/M4.2）：ExitType.Exit / ExitType.Error
                if let Expr::Ident(bname, _) = base.as_ref() {
                    if bname == "ExitType" {
                        return Ok(Value::Enum {
                            name: "ExitType".into(),
                            variant: field.clone(),
                            payload: None,
                        });
                    }
                }
                // 枚举常量 Type.name（base 为类型名）
                if let Expr::Ident(bname, _) = base.as_ref() {
                    if self.types.contains_key(bname) {
                        return Ok(Value::Enum {
                            name: bname.clone(),
                            variant: field.clone(),
                            payload: None,
                        });
                    }
                }
                // 推断枚举值字面量 .name（L1）：类型未知——用兜底枚举名
                if matches!(base.as_ref(), Expr::VoidLit(_)) {
                    return Ok(Value::Enum {
                        name: "__inferred__".into(),
                        variant: field.clone(),
                        payload: None,
                    });
                }
                let b = self.eval(base)?;
                self.eval_dot(b, field)
            }
            Expr::Field { base, field, span } => {
                let b = self.eval(base)?;
                self.check_dangling(&b, span)?;
                self.eval_field(b, field, span)
            }
            Expr::Index {
                base,
                indices,
                span,
            } => {
                let b = self.eval(base)?;
                self.check_dangling(&b, span)?;
                let b = self.deref_value(b);
                // 切片取段 &arr[1..3] / "abc"[0..2]：索引为 Range 表达式
                if indices.len() == 1 {
                    if let Expr::Binary(BinOp::Range, lo, hi, _) = &indices[0] {
                        let lo_v = self.eval(lo)?;
                        let lo_i = self.as_index(&lo_v, span)?;
                        let (hi_i, open_end) = match hi.as_ref() {
                            Expr::IntLit { text, .. } if text == "__end__" => (0usize, true),
                            other => {
                                let hv = self.eval(other)?;
                                (self.as_index(&hv, span)?, false)
                            }
                        };
                        if let Value::Arr(a) = &b {
                            let total = a.borrow().len();
                            let hi_i = if open_end { total } else { hi_i };
                            let len = hi_i.saturating_sub(lo_i);
                            if hi_i > total || lo_i > total {
                                return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                            }
                            return Ok(Value::Slice {
                                data: a.clone(),
                                start: lo_i,
                                len,
                            });
                        }
                        if let Value::Str(s) = &b {
                            let bytes = s.borrow().clone();
                            let hi_i = if open_end { bytes.len() } else { hi_i };
                            if hi_i > bytes.len() || lo_i > bytes.len() {
                                return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                            }
                            return Ok(Value::str_bytes(bytes[lo_i..hi_i].to_vec()));
                        }
                        // 切片再切片（57-protocol-parse：data[0..8]——data 是 &[u8] 参数）
                        if let Value::Slice { data, start, len } = &b {
                            let total = *len;
                            let hi_i = if open_end { total } else { hi_i };
                            if hi_i > total || lo_i > total {
                                return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                            }
                            return Ok(Value::Slice {
                                data: data.clone(),
                                start: *start + lo_i,
                                len: hi_i.saturating_sub(lo_i),
                            });
                        }
                    }
                }
                // 普通索引；多参索引 t[i, j] 仅 Table（嵌套 Arr）合法（M8 定案）
                match &b {
                    Value::Arr(a) => {
                        // 多参索引：行 → 列（Table 语义）
                        if indices.len() >= 2 {
                            let r = self.eval(&indices[0])?;
                            let c = self.eval(&indices[1])?;
                            let ri = self.as_index(&r, span)?;
                            let ci = self.as_index(&c, span)?;
                            let arr = a.borrow();
                            if ri >= arr.len() {
                                return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                            }
                            let row_v = arr[ri].borrow().clone();
                            drop(arr);
                            let row_v = self.deref_value(row_v);
                            if let Value::Arr(row) = row_v {
                                let row = row.borrow();
                                if ci >= row.len() {
                                    return Err(RtError::new(
                                        "IndexOutOfBounds",
                                        Some(span.clone()),
                                    ));
                                }
                                return Ok(row[ci].borrow().clone());
                            }
                            return Err(RtError::new("BadIndex", Some(span.clone())));
                        }
                        if indices.len() != 1 {
                            return Err(RtError::new("BadIndex", Some(span.clone())));
                        }
                        let idx = self.eval(&indices[0])?;
                        let i = self.as_index(&idx, span)?;
                        let arr = a.borrow();
                        if i >= arr.len() {
                            return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                        }
                        let v = arr[i].borrow().clone();
                        drop(arr);
                        Ok(v)
                    }
                    Value::Str(s) => {
                        let idx = self.eval(&indices[0])?;
                        let i = self.as_index(&idx, span)?;
                        let bytes = s.borrow();
                        if i >= bytes.len() {
                            return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                        }
                        Ok(Value::Int(bytes[i] as i128))
                    }
                    Value::Slice { data, start, len } => {
                        let idx = self.eval(&indices[0])?;
                        let i = self.as_index(&idx, span)?;
                        if i >= *len {
                            return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                        }
                        let d = data.borrow();
                        let v = d[*start + i].borrow().clone();
                        drop(d);
                        Ok(v)
                    }
                    _ => Err(RtError::new("NotIndexable", Some(span.clone()))),
                }
            }
            Expr::Deref(e, span) => {
                let v = self.eval(e)?;
                self.deref_checked(v, span)
            }
            Expr::AddrOf(e, _, span) => {
                // &x / &mut x：产生共享槽指针
                match e.as_ref() {
                    Expr::Ident(name, _) => match self.lookup(name) {
                        Some(cell) => {
                            // M2.5 Debug 悬垂标记：登记目标——目标销毁时标记指针
                            if self.debug_dangling {
                                self.tracked.insert(Rc::as_ptr(&cell) as usize);
                            }
                            Ok(Value::Ptr(cell))
                        }
                        None => Err(RtError::msg("UndefinedName", format!("undefined `{name}`"))),
                    },
                    Expr::Field { base, field, .. } => {
                        let b = self.eval(base)?;
                        self.check_dangling(&b, span)?;
                        let b = self.deref_value(b);
                        match b {
                            Value::Class(c) => {
                                let cell = Rc::new(RefCell::new(
                                    c.borrow().fields.get(field).cloned().unwrap_or(Value::Void),
                                ));
                                // 写回需要字段级共享——tag1：修改经 Assign 的 field 路径处理
                                self.tmp_field_cells.push(cell.clone());
                                Ok(Value::Ptr(cell))
                            }
                            _ => Err(RtError::msg("BadAddrOf", "cannot take address")),
                        }
                    }
                    _ => {
                        let v = self.eval(e)?;
                        Ok(Value::Ptr(Rc::new(RefCell::new(v))))
                    }
                }
            }
            Expr::Unary(op, inner, span) => {
                let v = self.eval(inner)?;
                let v = self.deref_value(v);
                match op {
                    UnaryOp::Neg => match v {
                        Value::Int(i) => Ok(Value::Int(-i)),
                        Value::Float(f) => Ok(Value::Float(-f)),
                        _ => Err(RtError::new("TypeError", Some(span.clone()))),
                    },
                    UnaryOp::Not => Ok(Value::Bool(!v.as_bool())),
                    UnaryOp::BitNot => match v {
                        Value::Int(i) => Ok(Value::Int(!i)),
                        _ => Err(RtError::new("TypeError", Some(span.clone()))),
                    },
                }
            }
            Expr::Binary(op, l, r, span) => self.eval_binary(*op, l, r, span),
            Expr::Orelse(l, r, _) => {
                let v = self.eval(l)?;
                let v = self.deref_value(v);
                match v {
                    Value::Opt(None) => {
                        // orelse return/continue/break（控制流兜底）：向函数/循环边界传播
                        if let Expr::Block(b, _) = r.as_ref() {
                            let flow = self.exec_block_inner(b)?;
                            match flow {
                                Flow::None => Ok(Value::Void),
                                Flow::Value(v) => Ok(v),
                                Flow::Return(v) => Err(RtError::signal(Flow::Return(v))),
                                Flow::Break(l) => Err(RtError::signal(Flow::Break(l))),
                                Flow::Continue(l) => Err(RtError::signal(Flow::Continue(l))),
                            }
                        } else {
                            self.eval(r)
                        }
                    }
                    Value::Opt(Some(inner)) => Ok((*inner).clone()),
                    other => Ok(other),
                }
            }
            Expr::Unwrap(e, span) => {
                let v = self.eval(e)?;
                let v = self.deref_value(v);
                match v {
                    Value::Opt(Some(inner)) => Ok((*inner).clone()),
                    Value::Opt(None) => Err(RtError::new("NullUnwrap", Some(span.clone()))),
                    other => Ok(other),
                }
            }
            Expr::Try(e, _) => {
                let v = self.eval(e)?;
                match v {
                    // M2.6：错误沿**值通道**从当前函数返回（signal → 函数边界转
                    // Ok(Value::Err)），调用方 catch/try 可拦截；不转 RtError（抛错
                    // 通道会绕过 catch——错误传播必须经 try/catch 处理）
                    Value::Err { .. } => Err(RtError::signal(Flow::Return(v))),
                    other => Ok(other),
                }
            }
            Expr::Await(e, span) => {
                // 组 E E2：await ≡ join()——求值 Future 值并运行到完成（协作式）。
                // 非 Future 值（语义层已拦截，防御性）→ TypeError。
                let fut = self.eval(e)?;
                let fut = self.deref_value(fut);
                if let Value::Class(c) = &fut {
                    if c.borrow().name == "Future" {
                        return self.future_run(&fut, span);
                    }
                }
                Err(RtError::new("TypeError", Some(span.clone())))
            }
            Expr::Catch(e, kind, _) => {
                let v = self.eval(e)?;
                match &v {
                    Value::Err { .. } => match kind.as_ref() {
                        CatchKind::Default(d) => self.eval(d),
                        CatchKind::Bind { name: bname, body } => {
                            self.push_scope();
                            // 捕获绑定携带完整错误值（名 + 码）
                            let err_clone = v.clone();
                            self.bind(bname, err_clone);
                            let r = self.exec_block_inner(body);
                            let _ = self.pop_scope(Self::is_err_path(&r));
                            match r? {
                                Flow::None => Ok(Value::Void),
                                Flow::Value(v) => Ok(v),
                                // 语句 return/break/continue：向函数/循环边界传播（与块表达式一致）
                                Flow::Return(v) => Err(RtError::signal(Flow::Return(v))),
                                Flow::Break(l) => Err(RtError::signal(Flow::Break(l))),
                                Flow::Continue(l) => Err(RtError::signal(Flow::Continue(l))),
                            }
                        }
                    },
                    _ => Ok(v),
                }
            }
            Expr::Call { callee, args, span } => self.eval_call(callee, args, span),
            Expr::IfExpr {
                cond,
                capture,
                then_e,
                else_e,
                ..
            } => {
                let c = self.eval(cond)?;
                // optional 捕获表达式：if (maybe) |v| v else 0
                if let Some((_, name)) = capture {
                    match self.deref_value(c) {
                        Value::Opt(Some(v)) => {
                            self.push_scope();
                            self.bind(name, (*v).clone());
                            let r = self.eval(then_e);
                            let _ = self.pop_scope(Self::is_err_path(&r.clone().map(Flow::Value)));
                            return r;
                        }
                        Value::Opt(None) => return self.eval(else_e),
                        other => {
                            self.push_scope();
                            self.bind(name, other);
                            let r = self.eval(then_e);
                            let _ = self.pop_scope(Self::is_err_path(&r.clone().map(Flow::Value)));
                            return r;
                        }
                    }
                }
                if c.as_bool() {
                    self.eval(then_e)
                } else {
                    self.eval(else_e)
                }
            }
            Expr::SwitchExpr { subject, arms, .. } => {
                let sw = SwitchStmt {
                    subject: (**subject).clone(),
                    arms: arms.clone(),
                    has_else: arms
                        .iter()
                        .any(|a| a.patterns.iter().any(|p| matches!(p, SwitchPattern::Else))),
                    span: Span::new(0, 0, 0, 0),
                };
                match self.exec_switch(&sw)? {
                    Flow::None | Flow::Break(_) | Flow::Continue(_) => Ok(Value::Void),
                    // 表达式臂值：switch 表达式结果
                    Flow::Value(v) => Ok(v),
                    // 语句 return（`=> return x`）：向函数边界传播
                    Flow::Return(v) => Err(RtError::signal(Flow::Return(v))),
                }
            }
            Expr::Block(b, _) => {
                self.push_scope();
                let r = self.exec_block_inner(b);
                let _ = self.pop_scope(Self::is_err_path(&r));
                match r? {
                    Flow::None => Ok(Value::Void),
                    Flow::Value(v) => Ok(v),
                    // 语句 return/break/continue：向函数/循环边界传播
                    Flow::Return(v) => Err(RtError::signal(Flow::Return(v))),
                    Flow::Break(l) => Err(RtError::signal(Flow::Break(l))),
                    Flow::Continue(l) => Err(RtError::signal(Flow::Continue(l))),
                }
            }
            Expr::Assign {
                target,
                op,
                value,
                span,
            } => self.eval_assign(target, *op, value, span),
            Expr::FnRef(name, _) => Ok(Value::Fn(name.clone())),
            Expr::Closure {
                params,
                body,
                is_mut,
                is_move,
                ..
            } => {
                // 自由变量精确分析（Phase 8）：只捕获 body 实际引用、未被体内绑定
                // 遮蔽的外部变量（含嵌套闭包传递）；未捕获变量闭包不可见。
                let free = hc::ast::closure_free_vars(params, body);
                let env = self.capture_env(&free);
                // move 捕获（M2.7）：深拷贝自由变量环境——闭包持有独立副本，
                // 脱离原作用域生命周期（原绑定销毁/悬垂不影响闭包）
                let env = if *is_move {
                    env.into_iter()
                        .map(|m| {
                            m.into_iter()
                                .map(|(k, cell)| {
                                    let v = self.deep_copy(cell.borrow().clone());
                                    (k, Rc::new(RefCell::new(v)))
                                })
                                .collect()
                        })
                        .collect()
                } else {
                    env
                };
                Ok(Value::Closure(ClosureData {
                    params: params.clone(),
                    body: body.clone(),
                    is_mut: *is_mut,
                    is_move: *is_move,
                    env,
                }))
            }
            Expr::TupleDestructure(names, e, _) => {
                let v = self.eval(e)?;
                let v = self.deref_value(v);
                if let Value::Arr(items) = v {
                    let items = items.borrow().clone();
                    if items.len() != names.len() {
                        return Err(RtError::msg("TupleArity", "destructure arity mismatch"));
                    }
                    for (n, it) in names.iter().zip(items.iter()) {
                        if n != "_" {
                            self.bind(n, it.borrow().clone());
                        }
                    }
                    Ok(Value::Void)
                } else {
                    Err(RtError::msg("TupleArity", "expected tuple in destructure"))
                }
            }
            // M2.4：move 运行时等同内层（所有权转移语义由作用域销毁体现；
            // 合法性检查在语义层）
            Expr::Move(inner, _) => self.eval(inner),
        }
    }

    // ---------- 表达式求值结束 ----------

    pub(crate) fn eval_dot(&mut self, b: Value, field: &str) -> Result<Value> {
        // math 命名空间特判（Fn 引用形式）
        if let Value::Fn(fname) = &b {
            return Ok(Value::Fn(format!("{fname}.{field}")));
        }
        match &b {
            Value::Enum { name, .. } => {
                // Type.variant 枚举常量
                return Ok(Value::Enum {
                    name: name.clone(),
                    variant: field.to_string(),
                    payload: None,
                });
            }
            _ => {
                // 字段访问（Str.len / Class.field）
                let span = Span::new(0, 0, 0, 0);
                self.eval_field(b, field, &span)
            }
        }
    }

    pub(crate) fn eval_field(&mut self, b: Value, field: &str, span: &Span) -> Result<Value> {
        let b = self.deref_value(b);
        match &b {
            Value::Class(c) => {
                let d = c.borrow();
                // Io 内建字段：io.alloc（M5.4 程序环境——默认分配器）
                if d.name == "Io" && field == "alloc" {
                    return Ok(Value::Alloc);
                }
                // Map 内建字段：len
                if d.name == "Map" && field == "len" {
                    return Ok(Value::Int(d.fields.len() as i128));
                }
                match d.fields.get(field) {
                    Some(v) => Ok(v.clone()),
                    None => Err(RtError::new("NoField", Some(span.clone()))),
                }
            }
            Value::Str(s) => match field {
                "len" => Ok(Value::Int(s.borrow().len() as i128)),
                _ => Err(RtError::new("NoField", Some(span.clone()))),
            },
            Value::Arr(a) => match field {
                "len" => Ok(Value::Int(a.borrow().len() as i128)),
                _ => Err(RtError::new("NoField", Some(span.clone()))),
            },
            // 集合（G4）：Vec 字段读（.len）——委托 Arr
            Value::Vec(d) => match field {
                "len" => Ok(Value::Int(d.borrow().items.borrow().len() as i128)),
                _ => Err(RtError::new("NoField", Some(span.clone()))),
            },
            // 集合（G4）：Map 字段读（.len）
            Value::Map(m) => match field {
                "len" => Ok(Value::Int(m.borrow().fields.len() as i128)),
                _ => Err(RtError::new("NoField", Some(span.clone()))),
            },
            Value::Slice { len, .. } => match field {
                "len" => Ok(Value::Int(*len as i128)),
                _ => Err(RtError::new("NoField", Some(span.clone()))),
            },
            _ => Err(RtError::new("NoField", Some(span.clone()))),
        }
    }

    pub(crate) fn as_index(&self, v: &Value, span: &Span) -> Result<usize> {
        match self.deref_value(v.clone()) {
            Value::Int(i) if i >= 0 => Ok(i as usize),
            _ => Err(RtError::new("BadIndex", Some(span.clone()))),
        }
    }

    pub(crate) fn eval_binary(
        &mut self,
        op: BinOp,
        l: &Expr,
        r: &Expr,
        span: &Span,
    ) -> Result<Value> {
        // 短路
        match op {
            BinOp::And => {
                let lv = self.eval(l)?;
                if !lv.as_bool() {
                    return Ok(Value::Bool(false));
                }
                let rv = self.eval(r)?;
                return Ok(Value::Bool(rv.as_bool()));
            }
            BinOp::Or => {
                let lv = self.eval(l)?;
                if lv.as_bool() {
                    return Ok(Value::Bool(true));
                }
                let rv = self.eval(r)?;
                return Ok(Value::Bool(rv.as_bool()));
            }
            _ => {}
        }
        let lv = self.eval(l)?;
        let rv = self.eval(r)?;
        self.binop_values(op, &lv, &rv, span)
    }

    pub(crate) fn binop_values(
        &self,
        op: BinOp,
        l: &Value,
        r: &Value,
        span: &Span,
    ) -> Result<Value> {
        let l = self.deref_value(l.clone());
        let r = self.deref_value(r.clone());
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::EucMod => {
                self.arith(op, &l, &r, span)
            }
            BinOp::Eq => Ok(Value::Bool(l.value_eq(&r))),
            BinOp::Ne => Ok(Value::Bool(!l.value_eq(&r))),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let lt = l.value_lt(&r);
                match lt {
                    Some(lt) => {
                        let v = match op {
                            BinOp::Lt => lt,
                            BinOp::Le => lt || l.value_eq(&r),
                            BinOp::Gt => !lt && !l.value_eq(&r),
                            BinOp::Ge => !lt || l.value_eq(&r),
                            _ => unreachable!(),
                        };
                        Ok(Value::Bool(v))
                    }
                    None => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                match (&l, &r) {
                    (Value::Int(a), Value::Int(b)) => {
                        let v = match op {
                            BinOp::BitAnd => Value::Int(a & b),
                            BinOp::BitOr => Value::Int(a | b),
                            BinOp::BitXor => Value::Int(a ^ b),
                            BinOp::Shl => {
                                // u64 语义：源值 ≤ u64::MAX 时按 64 位截断（xorshift 等）
                                if *a >= 0 && *a <= u64::MAX as i128 && *b < 64 {
                                    let v = (*a as u64).wrapping_shl(*b as u32);
                                    Value::Int(v as i128)
                                } else {
                                    Value::Int(a << b)
                                }
                            }
                            BinOp::Shr => {
                                if *a >= 0 && *a <= u64::MAX as i128 && *b < 64 {
                                    let v = (*a as u64).wrapping_shr(*b as u32);
                                    Value::Int(v as i128)
                                } else {
                                    Value::Int(a >> b)
                                }
                            }
                            _ => unreachable!(),
                        };
                        Ok(v)
                    }
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            BinOp::Range => {
                // 区间糖（Q29）：[lo, hi) 展开为数组
                match (&l, &r) {
                    (Value::Int(a), Value::Int(b)) => {
                        let mut items = Vec::new();
                        let mut i = *a;
                        while i < *b {
                            items.push(Value::Int(i));
                            i += 1;
                        }
                        Ok(Value::arr(items))
                    }
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            _ => Err(RtError::new("TypeError", Some(span.clone()))),
        }
    }

    pub(crate) fn arith(&self, op: BinOp, l: &Value, r: &Value, span: &Span) -> Result<Value> {
        match (l, r) {
            (Value::Int(a), Value::Int(b)) => {
                let v = match op {
                    BinOp::Add => a.checked_add(*b),
                    BinOp::Sub => a.checked_sub(*b),
                    BinOp::Mul => a.checked_mul(*b),
                    BinOp::Div => {
                        if *b == 0 {
                            return Err(RtError::new("DivisionByZero", Some(span.clone())));
                        }
                        Some(a / b)
                    }
                    BinOp::Mod => {
                        if *b == 0 {
                            return Err(RtError::new("DivisionByZero", Some(span.clone())));
                        }
                        Some(a % b)
                    }
                    BinOp::EucMod => {
                        if *b == 0 {
                            return Err(RtError::new("DivisionByZero", Some(span.clone())));
                        }
                        Some(a.rem_euclid(*b))
                    }
                    _ => None,
                };
                match v {
                    Some(v) => Ok(Value::Int(v)),
                    None => Err(RtError::new("Overflow", Some(span.clone()))),
                }
            }
            (Value::Float(a), Value::Float(b)) => {
                let v = match op {
                    BinOp::Add => a + b,
                    BinOp::Sub => a - b,
                    BinOp::Mul => a * b,
                    BinOp::Div => a / b,
                    BinOp::Mod | BinOp::EucMod => a % b,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                Ok(Value::Float(v))
            }
            (Value::Int(a), Value::Float(_b)) => self.arith(op, &Value::Float(*a as f64), r, span),
            (Value::Float(_a), Value::Int(b)) => self.arith(op, l, &Value::Float(*b as f64), span),
            _ => Err(RtError::new("TypeError", Some(span.clone()))),
        }
    }

    pub(crate) fn eval_assign(
        &mut self,
        target: &Expr,
        op: AssignOp,
        value: &Expr,
        span: &Span,
    ) -> Result<Value> {
        let new_v = match op {
            AssignOp::Set => self.eval(value)?,
            _ => {
                let cur = self.eval(target)?;
                let rhs = self.eval(value)?;
                let bop = match op {
                    AssignOp::Add => BinOp::Add,
                    AssignOp::Sub => BinOp::Sub,
                    AssignOp::Mul => BinOp::Mul,
                    AssignOp::Div => BinOp::Div,
                    AssignOp::BitOr => BinOp::BitOr,
                    AssignOp::BitAnd => BinOp::BitAnd,
                    AssignOp::BitXor => BinOp::BitXor,
                    AssignOp::Set => unreachable!(),
                };
                self.binop_values(bop, &cur, &rhs, span)?
            }
        };
        // 写入目标
        match target {
            Expr::Ident(name, _) => {
                let cell = self
                    .lookup(name)
                    .ok_or_else(|| RtError::new("UndefinedName", Some(span.clone())))?;
                // M2.7 只读捕获强制（Phase 8）：非 `mut` 闭包体内直接重绑定被捕获
                // 变量 → 错误（经指针/字段/索引写穿被捕获值本身不受限）。
                if self.readonly_caps.contains(&(Rc::as_ptr(&cell) as usize)) {
                    return Err(RtError::msg(
                        "ReadonlyCapture",
                        format!(
                            "cannot assign to captured variable `{name}` in non-mut closure \
                             (declare the closure `mut` to capture mutably)"
                        ),
                    ));
                }
                *cell.borrow_mut() = new_v;
            }
            Expr::Deref(inner, _) => {
                // p.* = v：写入指针指向的槽
                let p = self.eval(inner)?;
                self.check_dangling(&p, span)?;
                match p {
                    Value::Ptr(cell) => {
                        *cell.borrow_mut() = new_v;
                    }
                    Value::Boxed(b) => {
                        *b.borrow_mut().data.borrow_mut() = new_v;
                    }
                    _ => return Err(RtError::new("BadAssign", Some(span.clone()))),
                }
            }
            Expr::Field { base, field, .. } => {
                let b = self.eval(base)?;
                self.check_dangling(&b, span)?;
                let b = self.deref_value(b);
                if let Value::Class(c) = b {
                    self.assign_class_field(c, field, new_v)?;
                } else {
                    return Err(RtError::new("TypeError", Some(span.clone())));
                }
            }
            Expr::Dot { base, field, .. } => {
                // 实例字段赋值（hp.x = v）；base 为类型名时非赋值目标
                if let Expr::Ident(bname, _) = base.as_ref() {
                    if self.types.contains_key(bname) {
                        return Err(RtError::new("BadAssign", Some(span.clone())));
                    }
                }
                let b = self.eval(base)?;
                self.check_dangling(&b, span)?;
                let b = self.deref_value(b);
                if let Value::Class(c) = b {
                    self.assign_class_field(c, field, new_v)?;
                } else {
                    return Err(RtError::new("TypeError", Some(span.clone())));
                }
            }
            Expr::Index { base, indices, .. } => {
                let b = self.eval(base)?;
                self.check_dangling(&b, span)?;
                let b = self.deref_value(b);
                // 可写切片 &mut arr[0..2]：索引为 Range
                if indices.len() == 1 {
                    if let Expr::Binary(BinOp::Range, lo, hi, _) = &indices[0] {
                        let lo_v = self.eval(lo)?;
                        let hi_v = self.eval(hi)?;
                        let lo_i = self.as_index(&lo_v, span)?;
                        let hi_i = self.as_index(&hi_v, span)?;
                        if let Value::Arr(a) = &b {
                            let total = a.borrow().len();
                            let _len = hi_i.saturating_sub(lo_i);
                            if hi_i > total || lo_i > total {
                                return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                            }
                            let new_v = match op {
                                AssignOp::Set => self.eval(value)?,
                                _ => {
                                    return Err(RtError::new("BadAssign", Some(span.clone())));
                                }
                            };
                            // 写回切片元素
                            if let Value::Arr(src) = new_v {
                                let src_items = src.borrow().clone();
                                let arr = a.borrow_mut();
                                for (k, item) in src_items.iter().enumerate() {
                                    if lo_i + k < arr.len() {
                                        *arr[lo_i + k].borrow_mut() = item.borrow().clone();
                                    }
                                }
                            }
                            return Ok(Value::Void);
                        }
                    }
                }
                if indices.len() >= 2 {
                    // 多索引表格赋值：t[i,j] = v
                    if let Value::Arr(a) = b {
                        let r = self.eval(&indices[0])?;
                        let c = self.eval(&indices[1])?;
                        let ri = self.as_index(&r, span)?;
                        let ci = self.as_index(&c, span)?;
                        let arr = a.borrow();
                        if ri >= arr.len() {
                            return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                        }
                        let row_v = arr[ri].borrow().clone();
                        drop(arr);
                        let row_v = self.deref_value(row_v);
                        if let Value::Arr(row) = row_v {
                            let new_v = match op {
                                AssignOp::Set => self.eval(value)?,
                                _ => {
                                    let row_ref = row.borrow();
                                    if ci >= row_ref.len() {
                                        return Err(RtError::new(
                                            "IndexOutOfBounds",
                                            Some(span.clone()),
                                        ));
                                    }
                                    let cur = row_ref[ci].borrow().clone();
                                    drop(row_ref);
                                    let rhs = self.eval(value)?;
                                    let bop = match op {
                                        AssignOp::Add => BinOp::Add,
                                        AssignOp::Sub => BinOp::Sub,
                                        AssignOp::Mul => BinOp::Mul,
                                        AssignOp::Div => BinOp::Div,
                                        AssignOp::BitOr => BinOp::BitOr,
                                        AssignOp::BitAnd => BinOp::BitAnd,
                                        AssignOp::BitXor => BinOp::BitXor,
                                        AssignOp::Set => unreachable!(),
                                    };
                                    self.binop_values(bop, &cur, &rhs, span)?
                                }
                            };
                            let row_ref = row.borrow_mut();
                            if ci >= row_ref.len() {
                                return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                            }
                            *row_ref[ci].borrow_mut() = new_v;
                            return Ok(Value::Void);
                        }
                        return Err(RtError::new("BadIndex", Some(span.clone())));
                    }
                    return Err(RtError::new("TypeError", Some(span.clone())));
                }
                if let Value::Arr(a) = b {
                    let idx = self.eval(&indices[0])?;
                    let i = self.as_index(&idx, span)?;
                    let new_v = match op {
                        AssignOp::Set => self.eval(value)?,
                        _ => {
                            let arr = a.borrow();
                            if i >= arr.len() {
                                return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                            }
                            let cur = arr[i].borrow().clone();
                            drop(arr);
                            let rhs = self.eval(value)?;
                            let bop = match op {
                                AssignOp::Add => BinOp::Add,
                                AssignOp::Sub => BinOp::Sub,
                                AssignOp::Mul => BinOp::Mul,
                                AssignOp::Div => BinOp::Div,
                                AssignOp::BitOr => BinOp::BitOr,
                                AssignOp::BitAnd => BinOp::BitAnd,
                                AssignOp::BitXor => BinOp::BitXor,
                                AssignOp::Set => unreachable!(),
                            };
                            self.binop_values(bop, &cur, &rhs, span)?
                        }
                    };
                    let arr = a.borrow_mut();
                    if i >= arr.len() {
                        return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                    }
                    *arr[i].borrow_mut() = new_v;
                } else {
                    return Err(RtError::new("TypeError", Some(span.clone())));
                }
            }
            _ => return Err(RtError::new("BadAssign", Some(span.clone()))),
        }
        Ok(Value::Void)
    }

    pub(crate) fn eval_call(&mut self, callee: &Expr, args: &[Expr], span: &Span) -> Result<Value> {
        // 方法调用 p.dist(q)：注入 self
        if let Expr::Field { base, field, .. } = callee {
            // M3.4：多级命名空间限定调用（io.net.double）→ 静态函数表查找
            // （与单级 Dot 形式一致；对象方法链如 io.net.connect 不在此表 → 落方法派发）
            if let Some(qn) = qualified_flat_name(base, field) {
                // serialize 命名空间（M5.3）：serialize.json.parse / serialize.csv.parse
                if qn.starts_with("serialize.") {
                    return self.call_serialize_builtin(&qn, args, span);
                }
                // E1.2 组 D D4c：命名空间限定 comptime 值函数调用（ns.array_len(i32)）
                if let Some(v) = self.try_comptime_value_call(&qn, args, span)? {
                    return Ok(v);
                }
                if self.funcs.contains_key(&qn) {
                    let mut vals = Vec::new();
                    for a in args {
                        vals.push(self.eval(a)?);
                    }
                    let fdef = self.pick_fn(&qn, &vals)?;
                    return self.call_fn(&fdef, &vals, span);
                }
            }
            // Type.new(...) 构造（base 为类型名）
            if let Expr::Ident(bname, _) = base.as_ref() {
                if field == "new" && self.types.contains_key(bname) {
                    return self.call_new_builtin(bname, args, span);
                }
                // 集合类型 Vec(T).init(alloc) / Map(K,V).init(alloc)（此处 base 为类型名时）
                if matches!(bname.as_str(), "Vec" | "Map" | "Deque") && field == "init" {
                    // G4：捕获分配器引用（arg0 = alloc；缺省回退全局 alloc）
                    let alloc_v = if !args.is_empty() {
                        let a = self.eval(&args[0])?;
                        self.deref_value(a)
                    } else {
                        Value::Alloc
                    };
                    if bname == "Map" {
                        return Ok(Value::map(HashMap::new(), alloc_v));
                    }
                    return Ok(Value::vec(vec![], alloc_v));
                }
                // Table(T).init(alloc, rows, cols, init)（M8；G4：外层 Vec 持分配器引用）
                if bname == "Table" && field == "init" {
                    if args.len() < 4 {
                        return Err(RtError::new("ArityMismatch", Some(span.clone())));
                    }
                    let alloc_v = self.eval(&args[0])?;
                    let alloc_v = self.deref_value(alloc_v);
                    let rows = self.eval(&args[1])?;
                    let cols = self.eval(&args[2])?;
                    let init_v = self.eval(&args[3])?;
                    let rows = match self.deref_value(rows) {
                        Value::Int(i) => i.max(0) as usize,
                        _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                    };
                    let cols = match self.deref_value(cols) {
                        Value::Int(i) => i.max(0) as usize,
                        _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                    };
                    let mut grid = Vec::new();
                    for _ in 0..rows {
                        let mut row = Vec::new();
                        for _ in 0..cols {
                            row.push(init_v.clone());
                        }
                        grid.push(Value::arr(row));
                    }
                    return Ok(Value::vec(grid, alloc_v));
                }
                // E4：chan.init(alloc[, cap]) 内建：通道构造
                if bname == "chan" && field == "init" {
                    if args.is_empty() || args.len() > 2 {
                        return Err(RtError::new("ArityMismatch", Some(span.clone())));
                    }
                    let _alloc = self.eval(&args[0])?; // consume alloc arg
                    let capacity = if args.len() == 2 {
                        let cap_v = self.eval(&args[1])?;
                        let cap_v = self.deref_value(cap_v);
                        match cap_v {
                            Value::Int(i) => i.max(0) as usize,
                            _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                        }
                    } else {
                        0 // unbuffered
                    };
                    return Ok(Value::Chan(Arc::new(ChanState {
                        inner: std::sync::Mutex::new(ChanInner {
                            queue: VecDeque::new(),
                            closed: false,
                        }),
                        send_cond: Condvar::new(),
                        recv_cond: Condvar::new(),
                        capacity,
                    })));
                }
            }
            let self_v = self.eval(base)?;
            let self_v = self.deref_value(self_v);
            // 内建方法（Str / Arr / Class 上的 len、concat 等）
            if let Some(v) = self.call_builtin_method(&self_v, field, args, span)? {
                return Ok(v);
            }
            let type_name = self_v.type_name();
            // 注入 self 为首参
            let mut all_args = vec![Expr::VoidLit(span.clone())]; // 占位，用运行时值
            let _ = &mut all_args;
            let mut vals = vec![self_v.clone()];
            for a in args {
                vals.push(self.eval(a)?);
            }
            let fname = format!("{type_name}.{field}");
            let fdef = self.pick_fn(&fname, &vals)?;
            return self.call_fn(&fdef, &vals, span);
        }
        // Dot 形式：Type.method 静态调用 / io.print 等实例方法 / math 命名空间
        match callee {
            Expr::Dot { base, field, .. } => {
                if let Expr::Ident(bname, _) = base.as_ref() {
                    // E1（ADR-0013）：`types.fields(name)` 元数据查询（受限脚本模式）
                    if bname == "types" && self.script_mode {
                        return self.types_meta(field, args);
                    }
                    // math.sqrt / math.nan
                    if let Some(v) = self.call_math(bname, field, args, span)? {
                        return Ok(v);
                    }
                    // serialize 命名空间（M5.3）：解析辅助组
                    // （serialize.parse_int / parse_number / skip_space / … 对齐自由内建；
                    // serialize.json.parse 等三级名经 Field 分支路由）
                    if bname == "serialize" {
                        return self.call_serialize_builtin(
                            &format!("serialize.{field}"),
                            args,
                            span,
                        );
                    }
                    // Arena.init(alloc) 内建：真实 arena 句柄（G1：bump + 块链表）
                    if bname == "Arena" && field == "init" {
                        let mut arena = ArenaState::new();
                        arena.alloc_tracker = Some(self.alloc_tracker.clone());
                        return Ok(Value::Arena(Rc::new(RefCell::new(arena))));
                    }
                    // Pool.init(backing, item_size) 内建：固定大小对象池（Phase 3）
                    if bname == "Pool" && field == "init" {
                        if args.len() < 2 {
                            return Err(RtError::new("ArityMismatch", Some(span.clone())));
                        }
                        let backing = self.eval(&args[0])?;
                        let backing = self.deref_value(backing);
                        let backing_impl = self.value_to_allocator_impl(&backing, span)?;
                        let size_v = self.eval(&args[1])?;
                        let size_v = self.deref_value(size_v);
                        let item_size = match size_v {
                            Value::Int(i) if i > 0 => i as usize,
                            _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                        };
                        return Ok(Value::Allocator(Rc::new(RefCell::new(
                            AllocatorImpl::Pool(Rc::new(RefCell::new(PoolState::new(
                                backing_impl,
                                item_size,
                            )))),
                        ))));
                    }
                    // E4：Mutex.init(v) 内建：互斥锁（Arc<std::sync::Mutex>）
                    if bname == "Mutex" && field == "init" {
                        if args.is_empty() {
                            return Err(RtError::new("ArityMismatch", Some(span.clone())));
                        }
                        let v = self.eval(&args[0])?;
                        let v = self.deref_value(v);
                        return Ok(Value::Mutex(Arc::new(std::sync::Mutex::new(v))));
                    }
                    // E4：chan.init(alloc[, cap]) 内建：通道构造
                    if bname == "chan" && field == "init" {
                        if args.is_empty() || args.len() > 2 {
                            return Err(RtError::new("ArityMismatch", Some(span.clone())));
                        }
                        let _alloc = self.eval(&args[0])?; // consume alloc arg
                        let capacity = if args.len() == 2 {
                            let cap_v = self.eval(&args[1])?;
                            let cap_v = self.deref_value(cap_v);
                            match cap_v {
                                Value::Int(i) => i.max(0) as usize,
                                _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                            }
                        } else {
                            0 // unbuffered
                        };
                        return Ok(Value::Chan(Arc::new(ChanState {
                            inner: std::sync::Mutex::new(ChanInner {
                                queue: VecDeque::new(),
                                closed: false,
                            }),
                            send_cond: Condvar::new(),
                            recv_cond: Condvar::new(),
                            capacity,
                        })));
                    }
                    // 组 E E3：Io.threaded(alloc) / Io.evented(alloc) 运行时构造
                    // （协作式单线程；evented = 单线程事件循环风味，携带 runtime 字段）
                    if bname == "Io" && (field == "threaded" || field == "evented") {
                        // 实参（alloc）求值后丢弃——Io 无独立分配器（对齐 io_value 形态）
                        for a in args {
                            let _ = self.eval(a)?;
                        }
                        return Ok(self.io_value_with_runtime(if field == "evented" {
                            "evented"
                        } else {
                            "threaded"
                        }));
                    }
                    // String.from / String.concat 内建（String = 内建新类型，M3 定案）
                    if bname == "String" {
                        return self.call_string_builtin(field, args, span);
                    }
                    // X.new(...) 旧样板构造（审计 C1 取消后示例未迁移；tag1 兼容）
                    if field == "new" && self.types.contains_key(bname) {
                        return self.call_new_builtin(bname, args, span);
                    }
                    // Vec.init(alloc) / Map.init(alloc) 集合构造（G4：捕获分配器引用）
                    if matches!(bname.as_str(), "Vec" | "Map" | "Deque") && field == "init" {
                        let alloc_v = if !args.is_empty() {
                            let a = self.eval(&args[0])?;
                            self.deref_value(a)
                        } else {
                            Value::Alloc
                        };
                        if bname == "Map" {
                            return Ok(Value::map(HashMap::new(), alloc_v));
                        }
                        return Ok(Value::vec(vec![], alloc_v));
                    }
                    // Table(T).init(alloc, rows, cols, init)：二维表（M8 定案；G4 持有 alloc）
                    if bname == "Table" && field == "init" {
                        if args.len() < 4 {
                            return Err(RtError::new("ArityMismatch", Some(span.clone())));
                        }
                        let alloc_v = {
                            let a = self.eval(&args[0])?;
                            self.deref_value(a)
                        };
                        let rows = self.eval(&args[1])?;
                        let cols = self.eval(&args[2])?;
                        let init_v = self.eval(&args[3])?;
                        let rows = match self.deref_value(rows) {
                            Value::Int(i) => i.max(0) as usize,
                            _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                        };
                        let cols = match self.deref_value(cols) {
                            Value::Int(i) => i.max(0) as usize,
                            _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                        };
                        let mut grid = Vec::new();
                        for _ in 0..rows {
                            let mut row = Vec::new();
                            for _ in 0..cols {
                                row.push(init_v.clone());
                            }
                            grid.push(Value::arr(row));
                        }
                        return Ok(Value::vec(grid, alloc_v));
                    }
                    // Vec<i32>.from_bytes：集合反序列化（u64 长度前缀 + 元素）
                    if matches!(bname.as_str(), "Vec" | "Deque") && field == "from_bytes" {
                        let bytes = self.eval(&args[0])?;
                        let bytes = self.deref_value(bytes);
                        let b = match self.value_bytes(&bytes) {
                            Some(b) => b,
                            None => return Err(RtError::new("TypeError", Some(span.clone()))),
                        };
                        if b.len() < 8 {
                            return Err(RtError::new("InvalidBytes", Some(span.clone())));
                        }
                        let n = u64::from_le_bytes(b[0..8].try_into().unwrap()) as usize;
                        let mut items = Vec::new();
                        let mut pos = 8usize;
                        for _ in 0..n {
                            // tag1：按 i32 元素 4 字节解析
                            let v = if b.len() >= pos + 4 {
                                let i = i32::from_le_bytes(b[pos..pos + 4].try_into().unwrap());
                                pos += 4;
                                Value::Int(i as i128)
                            } else {
                                break;
                            };
                            items.push(v);
                        }
                        return Ok(Value::arr(items));
                    }
                    // String.from(s, alloc) 内建
                    if bname == "String" && field == "from" {
                        let v = self.eval(&args[0])?;
                        let v = self.deref_value(v);
                        if let Value::Str(s) = v {
                            return Ok(Value::Str(s));
                        }
                        return Ok(Value::str(&v.display()));
                    }
                    // json.parse(data)（M5.3 序列化辅助）：JSON 对象 → Map
                    if bname == "json" && field == "parse" {
                        let v = self.eval(&args[0])?;
                        let v = self.deref_value(v);
                        if let Value::Str(s) = v {
                            let text = String::from_utf8_lossy(&s.borrow()).to_string();
                            let obj = self.parse_json_obj(&text)?;
                            return Ok(Value::class("Map", obj));
                        }
                        return Err(RtError::new("TypeError", Some(span.clone())));
                    }
                    // csv.parse(data)（序列化辅助）：CSV 文本 → 二维数组（行 × 列字符串）
                    if bname == "csv" && field == "parse" {
                        let v = self.eval(&args[0])?;
                        let v = self.deref_value(v);
                        if let Value::Str(s) = v {
                            let text = String::from_utf8_lossy(&s.borrow()).to_string();
                            let rows = text
                                .split('\n')
                                .map(|line| line.strip_suffix('\r').unwrap_or(line))
                                .filter(|line| !line.is_empty())
                                .map(|line| line.split(',').map(Value::str).collect::<Vec<_>>())
                                .map(Value::arr)
                                .collect::<Vec<_>>();
                            return Ok(Value::arr(rows));
                        }
                        return Err(RtError::new("TypeError", Some(span.clone())));
                    }
                    // Type.method 静态调用：注入 self 为第一个实参
                    if self.types.contains_key(bname)
                        || self.funcs.contains_key(&format!("{bname}.{field}"))
                    {
                        // 序列化静态入口：Type.from_bytes / Type.from_json
                        if field == "from_bytes" && self.types.contains_key(bname) {
                            let bytes = self.eval(&args[0])?;
                            let bytes = self.deref_value(bytes);
                            let v = match self.value_bytes(&bytes) {
                                Some(b) => b,
                                None => return Err(RtError::new("TypeError", Some(span.clone()))),
                            };
                            return self.class_from_bytes(bname, &v);
                        }
                        if field == "from_json" && self.types.contains_key(bname) {
                            let json = self.eval(&args[0])?;
                            let json = self.deref_value(json);
                            let s = match json {
                                Value::Str(s) => s.borrow().clone(),
                                _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                            };
                            let obj = self.parse_json_obj(&String::from_utf8_lossy(&s))?;
                            return self.class_from_json(bname, &obj);
                        }
                        let mut vals = Vec::new();
                        for a in args {
                            vals.push(self.eval(a)?);
                        }
                        let fname = format!("{bname}.{field}");
                        let fdef = self.pick_fn(&fname, &vals)?;
                        return self.call_or_defer(&fdef, &vals, span);
                    }
                    // 实例方法：io.print(...) / arena.alloc(...)
                    let self_v = self.eval(base)?;
                    // G3：装箱胖指针 .alloc() → 携带的分配器引用（三字宽胖指针的 alloc 字）
                    if let Value::Boxed(b) = &self_v {
                        if field == "alloc" {
                            return Ok(b.borrow().alloc.clone());
                        }
                    }
                    // G4：集合 .alloc() → 构造 `init(alloc)` 时携带的分配器引用
                    if let Value::Vec(d) = &self_v {
                        if field == "alloc" {
                            return Ok(d.borrow().alloc.clone());
                        }
                    }
                    if let Value::Map(m) = &self_v {
                        if field == "alloc" {
                            return Ok(m.borrow().alloc.clone());
                        }
                    }
                    let self_v = self.deref_value(self_v);
                    if let Some(v) = self.call_builtin_method(&self_v, field, args, span)? {
                        return Ok(v);
                    }
                    let type_name = self_v.type_name();
                    let mut vals = vec![self_v];
                    for a in args {
                        vals.push(self.eval(a)?);
                    }
                    let fname = format!("{type_name}.{field}");
                    let fdef = self.pick_fn(&fname, &vals)?;
                    return self.call_or_defer(&fdef, &vals, span);
                }
                Err(RtError::new("NoMethod", Some(span.clone())))
            }
            Expr::Ident(name, _) => {
                // 集合类型实例化 Vec<i32>/Map(...)（类型表达式上下文 → 空容器，G4 持全局 alloc）
                if matches!(name.as_str(), "Vec" | "Deque") {
                    return Ok(Value::vec(vec![], Value::Alloc));
                }
                if name == "Map" {
                    return Ok(Value::map(HashMap::new(), Value::Alloc));
                }
                if name == "Table" {
                    // Table<i32> 类型实例化：空二维容器（init 填充）
                    return Ok(Value::vec(vec![], Value::Alloc));
                }
                // 组 F：四模式类型实例化 Pipe<i32> → 空容器标记（init 构造真实容器）
                if is_four_mode_type(name) {
                    return Ok(Value::class(name, HashMap::new()));
                }
                // E1.2 组 D D4c：comptime 值函数调用（参数含 `T: type`）→ 编译期求值折叠
                if let Some(v) = self.try_comptime_value_call(name, args, span)? {
                    return Ok(v);
                }
                // 用户函数优先于内建（同名冲突时，如 parse_int）
                if self.funcs.contains_key(name) {
                    let mut vals = Vec::new();
                    for a in args {
                        vals.push(self.eval(a)?);
                    }
                    let fdef = self.pick_fn(name, &vals)?;
                    // 组 E E2：async fn 调用点返回 Future(R)（延迟执行），await 运行体
                    return self.call_or_defer(&fdef, &vals, span);
                }
                // 内建函数
                if let Some(v) = self.call_builtin(name, args, span)? {
                    return Ok(v);
                }
                // 函数指针调用（apply(square, ...) → square 已是 Fn 值）
                if let Some(cell) = self.lookup(name) {
                    let v = cell.borrow().clone();
                    if let Value::Fn(fname) = v {
                        let mut vals = Vec::new();
                        for a in args {
                            vals.push(self.eval(a)?);
                        }
                        let fdef = self.pick_fn(&fname, &vals)?;
                        return self.call_or_defer(&fdef, &vals, span);
                    }
                    if let Value::Closure(closure) = v {
                        let mut vals = Vec::new();
                        for a in args {
                            vals.push(self.eval(a)?);
                        }
                        return self.call_closure(&closure, &vals, span);
                    }
                }
                let mut vals = Vec::new();
                for a in args {
                    vals.push(self.eval(a)?);
                }
                let fdef = self.pick_fn(name, &vals)?;
                self.call_or_defer(&fdef, &vals, span)
            }
            _ => {
                // 任意表达式求值后调用（Fn 值 / 闭包）
                let c = self.eval(callee)?;
                let c = self.deref_value(c);
                if let Value::Fn(fname) = c {
                    let mut vals = Vec::new();
                    for a in args {
                        vals.push(self.eval(a)?);
                    }
                    let fdef = self.pick_fn(&fname, &vals)?;
                    return self.call_or_defer(&fdef, &vals, span);
                }
                if let Value::Closure(closure) = c {
                    let mut vals = Vec::new();
                    for a in args {
                        vals.push(self.eval(a)?);
                    }
                    return self.call_closure(&closure, &vals, span);
                }
                Err(RtError::new("NotCallable", Some(span.clone())))
            }
        }
    }

    pub(crate) fn pick_fn(&self, name: &str, arg_vals: &[Value]) -> Result<FnDef> {
        let candidates = self
            .funcs
            .get(name)
            .ok_or_else(|| RtError::msg("NoFunction", format!("no function `{name}`")))?;
        // 1) 精确参数数量匹配
        let exact: Vec<&FnDef> = candidates
            .iter()
            .filter(|f| f.params.len() == arg_vals.len())
            .collect();
        let pool: Vec<&FnDef> = if exact.is_empty() {
            candidates.iter().collect()
        } else {
            exact
        };
        if pool.len() == 1 {
            return Ok(pool[0].clone());
        }
        // 2) 按实参值类型匹配（具体优先于泛型）
        let mut best: Option<&FnDef> = None;
        for f in &pool {
            let mut ok = true;
            let mut is_generic = false;
            for (p, a) in f.params.iter().zip(arg_vals.iter()) {
                let pt = p.ty.strip();
                // 指针/装箱实参解引用后匹配（克隆为持有值——链式 Ref 借用无法作 let 引用）
                let a = match a {
                    Value::Ptr(cell) => cell.borrow().clone(),
                    Value::Boxed(b) => b.borrow().data.borrow().clone(),
                    other => other.clone(),
                };
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
                        match &a {
                            Value::Int(_) if want_float => ok = false,
                            Value::Float(_) if want_int => ok = false,
                            Value::Str(_) if want_int || want_float || want_bool => ok = false,
                            Value::Bool(_) if !want_bool => ok = false,
                            Value::Class(c) if n != "String" && c.borrow().name != *n => ok = false,
                            // 泛型 T（where T: INumber 等）：不排除（编译时验证归 M2）
                            _ if n.chars().next().map_or(false, |c| c.is_uppercase())
                                && !n.starts_with("String")
                                && !n.starts_with("Vec")
                                && !n.starts_with("Map") =>
                            {
                                is_generic = true;
                            }
                            _ => {}
                        }
                    }
                    Type::Slice(inner, _) => {
                        // &[u8] / &[T]：Str 或数组；泛型元素 T 标记为泛型
                        match &a {
                            Value::Str(_) => {}
                            Value::Arr(_) | Value::Slice { .. } => {}
                            _ => ok = false,
                        }
                        if let Type::Named(n, _) = inner.strip() {
                            if n.chars().next().map_or(false, |c| c.is_uppercase())
                                && !n.starts_with("String")
                                && !n.starts_with("Vec")
                                && !n.starts_with("Map")
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
                // 具体优先于泛型；同级时优先返回类型匹配期望类型（M2.3/M2.7
                // 期望类型传播：var f: f64 = parse(...) / return parse(...)）；再同级保留首个
                match &best {
                    None => best = Some(f),
                    Some(b) => {
                        let b_generic = b.params.iter().any(|p| type_has_generic(&p.ty));
                        let f_ret = self.ret_matches_expected(f.ret.as_ref());
                        let b_ret = self.ret_matches_expected(b.ret.as_ref());
                        if !is_generic && b_generic {
                            // 具体优先于泛型
                            best = Some(f);
                        } else if is_generic && !b_generic {
                            // 保留 best（泛型不替换具体）
                        } else if f_ret && !b_ret {
                            // 同具体度：返回类型匹配期望 → 替换
                            best = Some(f);
                        }
                        // 同具体度同期望匹配：保留 best（首个注册，稳定）
                    }
                }
            }
        }
        if let Some(b) = best {
            return Ok(b.clone());
        }
        // 3) 带默认参数的回退（参数数 <= 声明数且尾部默认）
        for f in candidates {
            if f.params.len() > arg_vals.len() {
                let missing = f.params.len() - arg_vals.len();
                let tail_has_default = f.params[f.params.len() - missing..]
                    .iter()
                    .all(|p| p.default.is_some());
                if tail_has_default {
                    return Ok(f.clone());
                }
            }
        }
        Err(RtError::msg(
            "AmbiguousCall",
            format!(
                "no matching overload of `{name}` ({} arg(s))",
                arg_vals.len()
            ),
        ))
    }

    /// 期望类型传播（M2.3/M2.7）：函数返回类型是否匹配当前期望类型
    /// （`!T` 错误联合拆内层；`void` 为 Named("void")；无返回类型或泛型返回不匹配）
    pub(crate) fn ret_matches_expected(&self, ret: Option<&Type>) -> bool {
        let Some(exp) = &self.expected_ret else {
            return false;
        };
        let Some(ret) = ret else {
            return false;
        };
        let inner = match ret.strip() {
            Type::ErrorUnion(_, inner) => inner.strip(),
            other => other,
        };
        match inner {
            Type::Named(n, _) => n == exp,
            _ => false,
        }
    }

    pub(crate) fn call_fn(
        &mut self,
        fdef: &FnDef,
        arg_vals: &[Value],
        span: &Span,
    ) -> Result<Value> {
        // A1（ADR-0020）：`extern fn`——纯声明，解释器拒绝调用
        if fdef.is_extern {
            return Err(RtError::msg(
                "NotCallable",
                format!(
                    "`extern fn {}` is a C declaration and cannot be called in interpreter mode",
                    fdef.name
                ),
            ));
        }
        if fdef.params.len() < arg_vals.len() {
            return Err(RtError::new("ArityMismatch", Some(span.clone())));
        }
        let mut bound: Vec<(String, Value)> = Vec::new();
        for (i, p) in fdef.params.iter().enumerate() {
            if i < arg_vals.len() {
                bound.push((p.name.clone(), arg_vals[i].clone()));
            } else if let Some(d) = &p.default {
                let v = self.eval(d)?;
                bound.push((p.name.clone(), v));
            } else {
                return Err(RtError::new("ArityMismatch", Some(span.clone())));
            }
        }
        let prev_ret = self.current_ret.clone();
        self.current_ret = fdef.ret.clone();
        let r = self.exec_fn_body(&fdef.body, &bound);
        self.current_ret = prev_ret;
        r
    }

    /// E1.2 组 D D4c：comptime 值函数调用——参数含 `T: type`、非返回 `type` 的普通函数
    /// （`fn array_len(T: type) comptime_int`）在调用点编译期求值（ADR-0012「参数含
    /// type/anytype 触发编译期执行」）。类型实参（`array_len(i32)` 的 `i32`）经
    /// `comptime::expr_to_type` 作类型绑定（最小切片：体不引用类型参数值，绑定不入
    /// 运行时作用域）；值实参（comptime_int/anytype/普通）按常量求值；然后求值体 →
    /// 折叠结果。comptime 块装载期求值（script_mode）与运行时 interp 共用此路径；
    /// IR/原生后端由 D5 对齐。无匹配候选/实参不合法 → None（回落既有调用路径）。
    pub(crate) fn try_comptime_value_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        let Some(defs) = self.funcs.get(name).cloned() else {
            return Ok(None);
        };
        for f in defs {
            if !comptime::is_comptime_value_fn(&f.params, &f.ret) {
                continue;
            }
            if f.params.len() != args.len() {
                continue;
            }
            // 把实参绑定到参数：`T: type` 收已知类型表达式；其余求值。
            let mut value_bindings: Vec<(String, Value)> = Vec::new();
            let mut ok = true;
            for (p, a) in f.params.iter().zip(args.iter()) {
                if comptime::is_type_param(p) {
                    match comptime::expr_to_type(a) {
                        Some(Type::Named(n, _)) if self.is_known_type_name(&n) => {}
                        _ => {
                            ok = false;
                            break;
                        }
                    }
                } else {
                    match self.eval(a) {
                        Ok(v) => value_bindings.push((p.name.clone(), v)),
                        Err(_) => {
                            ok = false;
                            break;
                        }
                    }
                }
            }
            if !ok {
                continue;
            }
            // 自递归守卫：`fn f(T: type) { return f<i32>; }` 无限编译期求值 → 报错
            if self.comptime_value_depth >= 100 {
                return Err(RtError::new("ComptimeRecursion", Some(span.clone())));
            }
            self.comptime_value_depth += 1;
            let r = self.exec_fn_body(&f.body, &value_bindings);
            self.comptime_value_depth -= 1;
            return r.map(Some);
        }
        Ok(None)
    }

    /// 已知类型名判定（comptime 值函数 `T: type` 实参合法性）：基础类型 / 内建容器 /
    /// 已登记类型 / 类型函数名。非类型名的实参（变量、字面量）→ false。
    pub(crate) fn is_known_type_name(&self, name: &str) -> bool {
        if matches!(
            name,
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
                | "f16"
                | "f32"
                | "f64"
                | "f128"
                | "bool"
                | "void"
                | "String"
                | "comptime_int"
                | "comptime_float"
        ) {
            return true;
        }
        if matches!(
            name,
            "Vec"
                | "Map"
                | "Deque"
                | "Table"
                | "Allocator"
                | "Arena"
                | "Pool"
                | "ExitType"
                | "Mutex"
                | "chan"
        ) {
            return true;
        }
        if self.types.contains_key(name) {
            return true;
        }
        // 类型函数名（`fn X(...) type`）
        if let Some(defs) = self.funcs.get(name) {
            if defs.iter().any(|f| comptime::is_type_fn(&f.params, &f.ret)) {
                return true;
            }
        }
        false
    }

    /// 从 Value 提取 AllocatorImpl（用于 Pool.init 等需要分配器引用的场景）
    pub(crate) fn value_to_allocator_impl(&self, v: &Value, span: &Span) -> Result<AllocatorImpl> {
        match v {
            Value::Allocator(a) => Ok(a.borrow().clone()),
            Value::Alloc => Ok(AllocatorImpl::Page),
            Value::Arena(a) => Ok(AllocatorImpl::Arena(a.clone())),
            _ => Err(RtError::new("TypeError", Some(span.clone()))),
        }
    }
}
