use super::*;

pub(crate) fn call_scalar_method_ir(
    ctx: &Ctx,
    self_v: &IrValue,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    // 一元整数操作
    if args.is_empty() {
        if let IrValue::Int(a) = self_v {
            let v = match field {
                "neg" => Some(IrValue::Int(-*a)),
                "abs" => Some(IrValue::Int(a.abs())),
                _ => None,
            };
            if let Some(v) = v {
                return Ok(Some(v));
            }
        }
    }
    // 二元整数操作（保持整数语义：div 截断、mod 取余、溢出检查）
    if args.len() == 1 {
        let b = deref_value(ctx, &args[0]);
        if let (IrValue::Int(a), IrValue::Int(b)) = (self_v, b) {
            let v = match field {
                "add" => {
                    Some(IrValue::Int(a.checked_add(*b).ok_or_else(|| {
                        IrError::msg("Overflow", "integer overflow")
                    })?))
                }
                "sub" => {
                    Some(IrValue::Int(a.checked_sub(*b).ok_or_else(|| {
                        IrError::msg("Overflow", "integer overflow")
                    })?))
                }
                "mul" => {
                    Some(IrValue::Int(a.checked_mul(*b).ok_or_else(|| {
                        IrError::msg("Overflow", "integer overflow")
                    })?))
                }
                "div" => {
                    if *b == 0 {
                        return Err(IrError::msg("DivisionByZero", "division by zero"));
                    }
                    Some(IrValue::Int(a / b))
                }
                "mod" => {
                    if *b == 0 {
                        return Err(IrError::msg("DivisionByZero", "modulo by zero"));
                    }
                    Some(IrValue::Int(a % b))
                }
                "eq" => Some(IrValue::Bool(a == b)),
                "lt" => Some(IrValue::Bool(a < b)),
                _ => None,
            };
            if let Some(v) = v {
                return Ok(Some(v));
            }
        }
    }
    // 浮点路径（混合 Int/Float 也走此路径）
    let v = match self_v {
        IrValue::Int(i) => *i as f64,
        IrValue::Float(f) => *f,
        _ => return Ok(None),
    };
    let arg_num = |ix: usize| -> R<f64> {
        let a = args
            .get(ix)
            .ok_or_else(|| IrError::msg("ArityMismatch", "missing argument"))?;
        match deref_value(ctx, a) {
            IrValue::Int(i) => Ok(*i as f64),
            IrValue::Float(f) => Ok(*f),
            _ => Err(IrError::msg("TypeError", "expected number")),
        }
    };
    let r = match field {
        "add" => v + arg_num(0)?,
        "sub" => v - arg_num(0)?,
        "mul" => v * arg_num(0)?,
        "div" => v / arg_num(0)?,
        "mod" => v % arg_num(0)?,
        "neg" => -v,
        "abs" => v.abs(),
        "pow" => v.powf(arg_num(0)?),
        "eq" | "lt" => {
            let other = arg_num(0)?;
            let b = match field {
                "eq" => v == other,
                _ => v < other,
            };
            return Ok(Some(IrValue::Bool(b)));
        }
        _ => return Ok(None),
    };
    // 整数保持整数（无小数部分时）
    if r.fract() == 0.0 && r.is_finite() && r.abs() < 9e18 {
        Ok(Some(IrValue::Int(r as i128)))
    } else {
        Ok(Some(IrValue::Float(r)))
    }
}

// ---- 解析器辅助内建（71：peek/advance/expect/skip_space/is_digit/parse_number）----

pub(crate) fn call_map_method_ir(
    ctx: &mut Ctx,
    self_v: &IrValue,
    method: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let c = match self_v {
        IrValue::Class(c) => *c,
        IrValue::Map(c) => *c,
        _ => return Ok(None),
    };
    match method {
        // G4：.alloc() → 构造 `init(alloc)` 时携带的分配器引用（Class("Map") 无 alloc → 全局）
        "alloc" => {
            let alloc = match self_v {
                IrValue::Map(mc) => match &ctx.cells[*mc] {
                    Cell::Map { alloc, .. } => Some(alloc.clone()),
                    _ => None,
                },
                _ => None,
            };
            Ok(Some(match alloc {
                Some(a) => a,
                None => implicit_env_value(ctx, "alloc"),
            }))
        }
        "put" => {
            let k = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "put"))?;
            let v = args
                .get(1)
                .ok_or_else(|| IrError::msg("ArityMismatch", "put"))?;
            let key = deref_value(ctx, k).display(ctx);
            let nc = ctx.alloc(Cell::Value(v.clone()));
            match &mut ctx.cells[c] {
                Cell::Class { fields, .. } | Cell::Map { fields, .. } => {
                    fields.insert(key, nc);
                    Ok(Some(IrValue::Void))
                }
                _ => Err(IrError::msg("TypeError", "put expects Map")),
            }
        }
        "get" => {
            let k = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "get"))?;
            let key = deref_value(ctx, k).display(ctx);
            let v = match &ctx.cells[c] {
                Cell::Class { fields, .. } | Cell::Map { fields, .. } => {
                    fields.get(&key).map(|fc| ctx.cell_value(*fc).clone())
                }
                _ => None,
            };
            Ok(Some(opt_val(v)))
        }
        "contains" => {
            let k = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "contains"))?;
            let key = deref_value(ctx, k).display(ctx);
            let b = match &ctx.cells[c] {
                Cell::Class { fields, .. } | Cell::Map { fields, .. } => fields.contains_key(&key),
                _ => false,
            };
            Ok(Some(IrValue::Bool(b)))
        }
        "remove" => {
            let k = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "remove"))?;
            let key = deref_value(ctx, k).display(ctx);
            match &mut ctx.cells[c] {
                Cell::Class { fields, .. } | Cell::Map { fields, .. } => {
                    fields.remove(&key);
                    Ok(Some(IrValue::Void))
                }
                _ => Err(IrError::msg("TypeError", "remove expects Map")),
            }
        }
        "len" => {
            let n = match &ctx.cells[c] {
                Cell::Class { fields, .. } | Cell::Map { fields, .. } => fields.len(),
                _ => 0,
            };
            Ok(Some(IrValue::Int(n as i128)))
        }
        // Map.iter() → KV 条目数组（key/value 字段，与 for |kv| 捕获一致；
        // 对齐 oracle `call_builtin_method` 的 `(_, "iter")` 分支）
        "iter" => {
            let items = arr_items(ctx, self_v)?;
            Ok(Some(make_arr(ctx, items)))
        }
        "to_json" => Ok(Some(str_val(&value_to_json_ir(ctx, self_v)))),
        "from_json" => {
            let json = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "from_json"))?;
            let s = str_arg_ir(ctx, &[json.clone()], 0)?;
            let obj = parse_json_obj_ir(ctx, &String::from_utf8_lossy(&s))?;
            let mut fields = HashMap::new();
            for (k, v) in obj {
                fields.insert(k, ctx.alloc(Cell::Value(v)));
            }
            match self_v {
                // G4：Map 句柄 → 新 Map（携带自身 alloc）；Class("Map") → 旧形态 Class
                IrValue::Map(mc) => {
                    let alloc = match &ctx.cells[*mc] {
                        Cell::Map { alloc, .. } => alloc.clone(),
                        _ => implicit_env_value(ctx, "alloc"),
                    };
                    Ok(Some(make_map_with(ctx, fields, alloc)))
                }
                _ => Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                    name: "Map".into(),
                    fields,
                })))),
            }
        }
        _ => Ok(None),
    }
}

pub(crate) fn call_alloc_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    method: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match method {
        // alloc.init(T)：类型名参数建空实例（tag1 IR 无布局表——字段型构造请用
        // 字面量 `alloc.init(T{...})`；对齐 oracle interp.rs:3865-3891 的 Ident 分支）。
        // 实参已是类实例（字面量构造）→ 原样返回（对齐 oracle 字面量分支）。
        "init" => {
            if args.len() != 1 {
                return Err(IrError::msg("ArityMismatch", "alloc.init expects 1 arg"));
            }
            match deref_value(ctx, &args[0]).clone() {
                IrValue::Str(s) => Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                    name: String::from_utf8_lossy(&s).to_string(),
                    fields: HashMap::new(),
                })))),
                IrValue::Class(c) => Ok(Some(IrValue::Class(c))),
                _ => Err(IrError::msg(
                    "TypeError",
                    "alloc.init expects type name or literal",
                )),
            }
        }
        "alloc" => {
            let n = int_arg_ir(ctx, args, 0)?;
            match alloc_zeroed_bytes_ir(n) {
                Some(b) => {
                    // G5/§8.3 Debug 泄漏检测：登记分配（IR 无行号 → line 0；无引用计数不注销）
                    ctx.alloc_tracker.push((b.len(), 0));
                    Ok(Some(str_bytes_val(b)))
                }
                None => Ok(Some(err_val(module, "OutOfMemory"))),
            }
        }
        // G5/§8.3 Debug 泄漏检测：本 run 内已分配数
        "leaks" => Ok(Some(IrValue::Int(ctx.alloc_tracker.len() as i128))),
        // G5/§8.3 Debug 泄漏检测：分配清单文本（`leak: line L: N bytes` 每行）
        "leak_report" => {
            let mut out = Vec::new();
            for (size, line) in &ctx.alloc_tracker {
                out.extend_from_slice(&format!("leak: line {line}: {size} bytes\n").into_bytes());
            }
            Ok(Some(str_bytes_val(out)))
        }
        "deinit" => Ok(Some(IrValue::Void)),
        _ => Ok(None),
    }
}

pub(crate) fn call_arena_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    arena_cell: usize,
    method: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match method {
        "alloc" => {
            if args.len() != 1 {
                return Err(IrError::msg("ArityMismatch", "arena.alloc expects 1 arg"));
            }
            if let IrValue::Int(_) = deref_value(ctx, &args[0]) {
                let n = int_arg_ir(ctx, args, 0)?;
                if n < 0 {
                    return Err(IrError::msg("TypeError", "arena.alloc size must be >= 0"));
                }
                if n as u128 > usize::MAX as u128 {
                    return Ok(Some(err_val(module, "OutOfMemory")));
                }
                let n = n as usize;
                let bump_res = match &mut ctx.cells[arena_cell] {
                    Cell::Arena(st) => st.bump(n),
                    _ => unreachable!("cell {arena_cell} is not an arena"),
                };
                match bump_res {
                    Ok((bidx, off)) => {
                        let region = match &ctx.cells[arena_cell] {
                            Cell::Arena(st) => st.blocks[bidx][off..off + n].to_vec(),
                            _ => unreachable!(),
                        };
                        Ok(Some(str_bytes_val(region)))
                    }
                    Err(ArenaAllocErrIr::Deinit) => Err(IrError::msg(
                        "ArenaDeinitialized",
                        "arena.alloc after deinit",
                    )),
                    Err(ArenaAllocErrIr::Oom) => Ok(Some(err_val(module, "OutOfMemory"))),
                }
            } else {
                // 非整数实参：类型字面量构造（arena.alloc(Node{...}) 兼容形态）
                Ok(args.first().cloned())
            }
        }
        // arena.init(T) / arena.init(T{...})（E2：typed 构造，对齐 oracle call_arena_method
        // interp.rs "init" 双形态；bump 记账——堆上 class = 指针宽 8，连续 class IR 无布局
        // 表也按 8，与 alloc.init IR 同源简化）。
        "init" => {
            if args.len() != 1 {
                return Err(IrError::msg("ArityMismatch", "arena.init expects 1 arg"));
            }
            let v = deref_value(ctx, &args[0]).clone();
            let inst = match v {
                // 类型名参数（未知/枚举类型回退 Const Str）→ 空 class 实例
                IrValue::Str(s) => IrValue::Class(ctx.alloc(Cell::Class {
                    name: String::from_utf8_lossy(&s).to_string(),
                    fields: HashMap::new(),
                })),
                // 字面量 / 已知 class 默认字段构造（lower_alloc_init_defaults）→ 原样返回
                IrValue::Class(c) => IrValue::Class(c),
                _ => {
                    return Err(IrError::msg(
                        "TypeError",
                        "arena.init expects type name or literal",
                    ))
                }
            };
            let bump_res = match &mut ctx.cells[arena_cell] {
                Cell::Arena(st) => st.bump(8),
                _ => unreachable!("cell {arena_cell} is not an arena"),
            };
            match bump_res {
                Ok(_) => Ok(Some(inst)),
                Err(ArenaAllocErrIr::Deinit) => Err(IrError::msg(
                    "ArenaDeinitialized",
                    "arena.init after deinit",
                )),
                Err(ArenaAllocErrIr::Oom) => Ok(Some(err_val(module, "OutOfMemory"))),
            }
        }
        "deinit" => {
            if !args.is_empty() {
                return Err(IrError::msg("ArityMismatch", "arena.deinit expects 0 args"));
            }
            match &mut ctx.cells[arena_cell] {
                Cell::Arena(st) => st.deinit(),
                _ => unreachable!("cell {arena_cell} is not an arena"),
            }
            Ok(Some(IrValue::Void))
        }
        "bytes" => {
            if !args.is_empty() {
                return Err(IrError::msg("ArityMismatch", "arena.bytes expects 0 args"));
            }
            let total = match &ctx.cells[arena_cell] {
                Cell::Arena(st) => st.total,
                _ => unreachable!(),
            };
            Ok(Some(IrValue::Int(total as i128)))
        }
        "blocks" => {
            if !args.is_empty() {
                return Err(IrError::msg("ArityMismatch", "arena.blocks expects 0 args"));
            }
            let blocks = match &ctx.cells[arena_cell] {
                Cell::Arena(st) => st.blocks.len(),
                _ => unreachable!(),
            };
            Ok(Some(IrValue::Int(blocks as i128)))
        }
        _ => Ok(None),
    }
}

pub(crate) fn call_thread_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    self_v: &IrValue,
    method: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    if !args.is_empty() {
        return Err(IrError::msg(
            "ArityMismatch",
            format!("Thread.{method} expects 0 args"),
        ));
    }
    let thread = match self_v {
        IrValue::Class(c) => *c,
        _ => return Err(IrError::msg("TypeError", "Thread method on non-Thread")),
    };
    // 获取 _tid 字段定位 ThreadStateIr
    let tid = match &ctx.cells[thread] {
        Cell::Class { fields, .. } => match fields.get("_tid") {
            Some(c) => match ctx.cell_value(*c) {
                IrValue::Int(tid) => *tid as i64,
                _ => return Err(IrError::msg("TypeError", "Thread has no valid _tid")),
            },
            None => return Err(IrError::msg("TypeError", "Thread has no _tid field")),
        },
        _ => return Err(IrError::msg("TypeError", "bad Thread cell")),
    };
    match method {
        // join：等待 OS 线程结束，返回结果
        "join" => {
            let ts = ctx.thread_handles.get_mut(&tid).ok_or_else(|| {
                IrError::msg("ThreadError", "Thread handle not found (already joined?)")
            })?;
            if let Some(h) = ts.join_handle.take() {
                let _ = h
                    .join()
                    .map_err(|_| IrError::msg("Panic", "thread panicked"))?;
            } else {
                // 句柄已被 detach 或前次 join 取走，等待 done
                while !ts.done.load(Ordering::SeqCst) {
                    std::thread::yield_now();
                }
            }
            let result_guard = ts.result.lock().unwrap();
            let result = result_guard
                .as_ref()
                .ok_or_else(|| IrError::msg("ThreadError", "Thread has no result"))?;
            let ir_result = match result {
                ThreadResultIr::Ok(v) => v.clone(),
                ThreadResultIr::Err(e) => err_val(module, &e.name),
            };
            drop(result_guard);
            thread_set_field_ir(ctx, thread, "done", IrValue::Bool(true));
            Ok(Some(ir_result))
        }
        // detach：丢弃 join 句柄（线程继续运行），标记分离
        "detach" => {
            let ts = ctx
                .thread_handles
                .get_mut(&tid)
                .ok_or_else(|| IrError::msg("ThreadError", "Thread handle not found"))?;
            let _ = ts.join_handle.take();
            thread_set_field_ir(ctx, thread, "detached", IrValue::Bool(true));
            Ok(Some(IrValue::Void))
        }
        // is_done：检查线程是否已完成
        "is_done" => {
            let ts = ctx
                .thread_handles
                .get(&tid)
                .ok_or_else(|| IrError::msg("ThreadError", "Thread handle not found"))?;
            Ok(Some(IrValue::Bool(ts.done.load(Ordering::SeqCst))))
        }
        // cancel：设置取消标志（线程启动时检查，若已取消则返回 error.Cancelled）
        "cancel" => {
            let ts = ctx
                .thread_handles
                .get(&tid)
                .ok_or_else(|| IrError::msg("ThreadError", "Thread handle not found"))?;
            ts.cancel.store(true, Ordering::SeqCst);
            thread_set_field_ir(ctx, thread, "cancel", IrValue::Bool(true));
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

/// 写线程字段（Thread 类字段 cell 索引定位）
pub(crate) fn thread_set_field_ir(ctx: &mut Ctx, thread: usize, key: &str, v: IrValue) {
    let fc = match &ctx.cells[thread] {
        Cell::Class { fields, .. } => fields.get(key).copied(),
        _ => None,
    };
    if let Some(c) = fc {
        ctx.cells[c] = Cell::Value(v);
    }
}

// ---- 组 F：四模式共享容器（对齐 oracle interp.rs `make_four_mode_container`/
//      `call_four_mode_method`）----

/// 四模式容器构造（对齐 oracle interp.rs `make_four_mode_container`）：字段布局
/// queue=空 Arr / closed=false / alloc=构造时分配器引用 / cap（仅通道形态二参 init）。
pub(crate) fn make_four_mode_ir(ctx: &mut Ctx, name: &str, args: &[IrValue]) -> R<IrValue> {
    if args.is_empty() || args.len() > 2 {
        return Err(IrError::msg("ArityMismatch", format!("{name}.init")));
    }
    let alloc_v = deref_value(ctx, &args[0]).clone();
    let queue_v = make_arr(ctx, vec![]);
    let mut f = HashMap::new();
    f.insert("queue".to_string(), ctx.alloc(Cell::Value(queue_v)));
    f.insert(
        "closed".to_string(),
        ctx.alloc(Cell::Value(IrValue::Bool(false))),
    );
    f.insert("alloc".to_string(), ctx.alloc(Cell::Value(alloc_v)));
    if args.len() == 2 {
        let cap = deref_value(ctx, &args[1]).clone();
        let cap_i = match cap {
            IrValue::Int(i) => i.max(0),
            _ => return Err(IrError::msg("TypeError", format!("{name}.init cap"))),
        };
        f.insert(
            "cap".to_string(),
            ctx.alloc(Cell::Value(IrValue::Int(cap_i))),
        );
    }
    Ok(IrValue::Class(ctx.alloc(Cell::Class {
        name: name.into(),
        fields: f,
    })))
}

/// 四模式容器方法分派：write/read/try_read/close（共享内存形态）+ send/recv（通道
/// 形态，有界队列）。协作式单线程下四变体运行时行为相同；空读/满 send 不阻塞而是
/// 报 `error.Empty`/`error.ChannelFull`（文档化偏差，对齐 oracle interp.rs）。
pub(crate) fn call_four_mode_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    self_v: &IrValue,
    m: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let c = match self_v {
        IrValue::Class(c) => *c,
        _ => return Err(IrError::msg("TypeError", "four-mode method on non-class")),
    };
    // 只读一次类字段（克隆 fields，随后可变借用安全）
    let (queue, closed, cap) = {
        let fields = match &ctx.cells[c] {
            Cell::Class { fields, .. } => fields.clone(),
            _ => return Err(IrError::msg("TypeError", "bad four-mode container")),
        };
        let queue = fields
            .get("queue")
            .map(|fc| ctx.cell_value(*fc).clone())
            .unwrap_or(IrValue::Arr(usize::MAX));
        let closed = matches!(
            fields.get("closed"),
            Some(fc) if matches!(ctx.cell_value(*fc), IrValue::Bool(true))
        );
        let cap = match fields.get("cap") {
            Some(fc) => match ctx.cell_value(*fc) {
                IrValue::Int(i) => Some(*i),
                _ => None,
            },
            _ => None,
        };
        (queue, closed, cap)
    };
    match m {
        // write(v)：队尾追加；close 后报错
        "write" => {
            if args.len() != 1 {
                return Err(IrError::msg("ArityMismatch", "write"));
            }
            if closed {
                return Ok(Some(err_val(module, "Closed")));
            }
            let qc = match queue {
                IrValue::Arr(qc) => qc,
                _ => return Err(IrError::msg("TypeError", "bad queue")),
            };
            let nc = ctx.alloc(Cell::Value(args[0].clone()));
            match &mut ctx.cells[qc] {
                Cell::Elems(e) => e.push(nc),
                _ => return Err(IrError::msg("TypeError", "bad queue")),
            }
            Ok(Some(IrValue::Void))
        }
        // read()/recv() T：队首弹出。空读在协作式下不能阻塞 → 运行时错误
        "read" | "recv" => {
            if !args.is_empty() {
                return Err(IrError::msg("ArityMismatch", "read"));
            }
            let qc = match queue {
                IrValue::Arr(qc) => qc,
                _ => return Err(IrError::msg("TypeError", "bad queue")),
            };
            let popped = match &mut ctx.cells[qc] {
                Cell::Elems(e) => {
                    if e.is_empty() {
                        return Ok(Some(err_val(module, "Empty")));
                    }
                    e.remove(0)
                }
                _ => return Err(IrError::msg("TypeError", "bad queue")),
            };
            Ok(Some(ctx.cell_value(popped).clone()))
        }
        // try_read() ?T：队首弹出或 null（空/close 后空亦 null）
        "try_read" => {
            if !args.is_empty() {
                return Err(IrError::msg("ArityMismatch", "try_read"));
            }
            let qc = match queue {
                IrValue::Arr(qc) => qc,
                _ => return Err(IrError::msg("TypeError", "bad queue")),
            };
            let popped = match &mut ctx.cells[qc] {
                Cell::Elems(e) => {
                    if e.is_empty() {
                        return Ok(Some(opt_val(None)));
                    }
                    e.remove(0)
                }
                _ => return Err(IrError::msg("TypeError", "bad queue")),
            };
            Ok(Some(opt_val(Some(ctx.cell_value(popped).clone()))))
        }
        // send(v)：有界通道写；满（len >= cap）→ 报错（协作式不能阻塞）；close 后报错
        "send" => {
            if args.len() != 1 {
                return Err(IrError::msg("ArityMismatch", "send"));
            }
            if closed {
                return Ok(Some(err_val(module, "Closed")));
            }
            let qc = match queue {
                IrValue::Arr(qc) => qc,
                _ => return Err(IrError::msg("TypeError", "bad queue")),
            };
            let nc = ctx.alloc(Cell::Value(args[0].clone()));
            match &mut ctx.cells[qc] {
                Cell::Elems(e) => {
                    if let Some(cap) = cap {
                        if e.len() as i128 >= cap {
                            return Ok(Some(err_val(module, "ChannelFull")));
                        }
                    }
                    e.push(nc);
                }
                _ => return Err(IrError::msg("TypeError", "bad queue")),
            }
            Ok(Some(IrValue::Void))
        }
        // close()：置结束标志；此后 write/send 报错、try_read 返回 null
        "close" => {
            if !args.is_empty() {
                return Err(IrError::msg("ArityMismatch", "close"));
            }
            match &mut ctx.cells[c] {
                Cell::Class { fields, .. } => {
                    if let Some(fc) = fields.get("closed").copied() {
                        ctx.cells[fc] = Cell::Value(IrValue::Bool(true));
                    }
                }
                _ => return Err(IrError::msg("TypeError", "bad four-mode container")),
            }
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

// ---- JSON 解析（Map.from_json / json.parse；对齐 oracle parse_json_*）----
