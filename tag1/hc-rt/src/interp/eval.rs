//! 解释器语句执行：函数体、块、声明、控制流等语句求值

use super::*;

impl Interp {
    // ---------- 语句 ----------

    pub fn exec_fn_body(&mut self, body: &Block, params: &[(String, Value)]) -> Result<Value> {
        if self.call_depth >= MAX_CALL_DEPTH {
            return Err(RtError::msg("StackOverflow", "maximum call depth exceeded"));
        }
        self.call_depth += 1;
        self.push_scope();
        for (name, v) in params {
            self.bind(name, v.clone());
        }
        let result = self.exec_block_inner(body);
        let _ = self.pop_scope(Self::is_err_path(&result));
        self.call_depth -= 1;
        match result {
            Ok(Flow::Return(v)) => Ok(v),
            Ok(Flow::Value(v)) => Ok(v),
            Ok(Flow::None) => Ok(Value::Void),
            Ok(Flow::Break(_)) | Ok(Flow::Continue(_)) => Err(RtError::msg(
                "InvalidControlFlow",
                "break/continue outside loop",
            )),
            Err(e) if e.is_signal() => match e.signal {
                Some(Flow::Return(v)) => Ok(v),
                Some(Flow::Value(v)) => Ok(v),
                Some(Flow::Break(_)) | Some(Flow::Continue(_)) => Err(RtError::msg(
                    "InvalidControlFlow",
                    "break/continue outside loop",
                )),
                _ => Err(e),
            },
            Err(e) => Err(e),
        }
    }

    pub(crate) fn exec_block(&mut self, b: &Block) -> Result<Flow> {
        self.push_scope();
        let r = self.exec_block_inner(b);
        let _ = self.pop_scope(Self::is_err_path(&r));
        r
    }

    pub(crate) fn exec_block_inner(&mut self, b: &Block) -> Result<Flow> {
        let n = b.stmts.len();
        for (i, stmt) in b.stmts.iter().enumerate() {
            // 块值：最后一条语句为表达式时取其值（M3.4 对齐 IR 语义源——
            // 之前被丢弃导致 catch 块值/块表达式恒为 void）
            if i + 1 == n {
                if let Stmt::Expr(e) = stmt {
                    return Ok(Flow::Value(self.eval(e)?));
                }
            }
            let f = self.exec_stmt(stmt)?;
            if !matches!(f, Flow::None) {
                return Ok(f);
            }
        }
        Ok(Flow::None)
    }

    pub(crate) fn exec_stmt(&mut self, s: &Stmt) -> Result<Flow> {
        match s {
            Stmt::Empty => Ok(Flow::None),
            // 语句位块：值丢弃（块值只经块表达式/函数末位表达式产生；
            // 否则中间块会以 Flow::Value 早退跳过后续语句）
            Stmt::Block(b) => match self.exec_block(b)? {
                Flow::Value(_) => Ok(Flow::None),
                other => Ok(other),
            },
            Stmt::VarDecl {
                name,
                mut_,
                ty,
                init,
                span: _,
            } => {
                // 期望类型传播（M2 定案）：目标类型已知时优先返回类型匹配的重载
                let prev_expected = self.expected_ret.clone();
                if let Some(t) = ty {
                    if let Type::Named(tn, _) = t.strip() {
                        self.expected_ret = Some(tn.clone());
                    }
                }
                let mut v = match init {
                    Some(e) => self.eval(e)?,
                    None => self.default_value(ty.as_ref())?,
                };
                // M3 语法糖：`var s: String = "hello"` → String.from("hello")
                if init.is_some() {
                    if let Some(t) = ty {
                        if let Type::Named(tn, _) = t.strip() {
                            if tn == "String" {
                                v = match v {
                                    Value::Str(s) => {
                                        Value::String(StringData::from_slice(&s.borrow()))
                                    }
                                    Value::String(_) => v,
                                    other => {
                                        let bytes = other.display().as_bytes().to_vec();
                                        Value::String(StringData::from_slice(&bytes))
                                    }
                                };
                            }
                        }
                    }
                }
                self.expected_ret = prev_expected;
                let _ = mut_;
                // [continuous] 值语义：目标类型连续时赋值即复制（显式标注或源类型可查）
                let continuous = match ty {
                    Some(t) => match t.strip() {
                        Type::Named(tn, _) => self.type_is_continuous(tn),
                        _ => false,
                    },
                    None => match init {
                        // var p2 = p1（p1 为连续类型值）
                        Some(Expr::Ident(src, _)) => match self.lookup(src) {
                            Some(cell) => match &*cell.borrow() {
                                Value::Class(c) => {
                                    let cname = c.borrow().name.clone();
                                    self.type_is_continuous(&cname)
                                }
                                _ => false,
                            },
                            None => false,
                        },
                        _ => false,
                    },
                };
                if continuous {
                    v = self.deep_copy(v);
                }
                self.bind(name, v);
                Ok(Flow::None)
            }
            Stmt::ConstDecl { name, init, .. } => {
                let v = self.eval(init)?;
                self.bind(name, v);
                Ok(Flow::None)
            }
            Stmt::Expr(e) => {
                self.eval(e)?;
                Ok(Flow::None)
            }
            Stmt::If(ifs) => match self.exec_if(ifs)? {
                // 语句位 if：值丢弃（if 表达式用 IfExpr 变体；否则中间 if 的
                // 值会早退跳过后续语句）
                Flow::Value(_) => Ok(Flow::None),
                other => Ok(other),
            },
            Stmt::While(w) => self.exec_while(w),
            Stmt::For(f) => self.exec_for(f),
            Stmt::Switch(sw) => match self.exec_switch(sw)? {
                // 语句级 switch：表达式臂值丢弃；语句 return/break/continue 原样传播
                Flow::Value(_) => Ok(Flow::None),
                other => Ok(other),
            },
            Stmt::Return(e, _) => {
                // 期望类型传播：return 上下文用当前函数返回类型参与重载选择
                let prev_expected = self.expected_ret.clone();
                if self.expected_ret.is_none() {
                    if let Some(rt) = &self.current_ret {
                        match rt.strip() {
                            Type::ErrorUnion(_, inner) => match inner.strip() {
                                Type::Named(n, _) => self.expected_ret = Some(n.clone()),
                                _ => {}
                            },
                            Type::Named(n, _) => self.expected_ret = Some(n.clone()),
                            _ => {}
                        }
                    }
                }
                let v = match e {
                    Some(e) => self.eval(e)?,
                    None => Value::Void,
                };
                self.expected_ret = prev_expected;
                Ok(Flow::Return(v))
            }
            Stmt::Break(l, _) => Ok(Flow::Break(l.clone())),
            Stmt::Continue(l, _) => Ok(Flow::Continue(l.clone())),
            Stmt::Defer(e, _) => {
                self.scopes.last_mut().unwrap().defers.push(DeferEntry {
                    expr: e.clone(),
                    errdefer: false,
                });
                Ok(Flow::None)
            }
            Stmt::Errdefer(e, _) => {
                self.scopes.last_mut().unwrap().defers.push(DeferEntry {
                    expr: e.clone(),
                    errdefer: true,
                });
                Ok(Flow::None)
            }
        }
    }

    /// 类型是否为连续内存（struct 始终连续，class 视情况）
    pub(crate) fn type_is_continuous(&self, tn: &str) -> bool {
        match self.types.get(tn) {
            Some(TypeDef::Class { is_struct, .. }) => *is_struct,
            _ => false,
        }
    }

    /// M5.4：Io 实例（含 fs/time/net 子模块；fs = 路径式文件 API，time = 毫秒时钟，
    /// net = TCP 基础）。默认运行时 = threaded（阻塞 IO，spec 06-10 §Io 执行模型）。
    pub(crate) fn io_value(&self) -> Value {
        self.io_value_with_runtime("threaded")
    }

    /// 组 E E3：Io 运行时构造——`Io.threaded(alloc)`（阻塞 IO + 每操作线程，默认风味）
    /// / `Io.evented(alloc)`（单线程事件循环风味）。协作式模型下两者同为单线程确定性
    /// 执行（ADR-0011：真线程/非阻塞 IO 归 1.x）；`runtime` 字段供程序查询，evented
    /// 的 `io.poll()` 事件循环每轮运行待处理延迟任务（见 io_poll）。
    pub(crate) fn io_value_with_runtime(&self, runtime: &str) -> Value {
        let mut f = HashMap::new();
        f.insert("fs".into(), Value::class("Fs", HashMap::new()));
        f.insert("time".into(), Value::class("Time", HashMap::new()));
        // G1（E3.1）：`io.net.udp` 子命名空间（bind/send_to/recv_from）——UdpSocket 方法
        // 由实例方法分派（sock.send_to/recv_from/close），命名空间形式委托同实现。
        let mut net_fields = HashMap::new();
        net_fields.insert("udp".into(), Value::class("Udp", HashMap::new()));
        f.insert("net".into(), Value::class("Net", net_fields));
        f.insert("runtime".into(), Value::str(runtime));
        // G2（io 差异项）：io.stdout/io.stderr 独立字节流（write_all 写真实句柄；
        // 类名 Stdout/Stderr 供 call_builtin_method 分派，无 fd 注册表）
        f.insert("stdout".into(), Value::class("Stdout", HashMap::new()));
        f.insert("stderr".into(), Value::class("Stderr", HashMap::new()));
        // G3（E3.2 ipc）：`io.ipc.pipe()` / `io.ipc.shm(name, size)`——进程内 IPC 原语
        //（管道/共享内存；Pipe/Shm 方法由类名分派，见 call_pipe_method/call_shm_method）
        f.insert("ipc".into(), Value::class("Ipc", HashMap::new()));
        // G4（E3.3 storage）：`io.storage.open(path) !KvStore`——文件持久化键值存储；
        // `io.archive.compress/decompress`——RLE 压缩（KvStore 方法见 call_store_method）
        f.insert("storage".into(), Value::class("Storage", HashMap::new()));
        f.insert("archive".into(), Value::class("Archive", HashMap::new()));
        // G5（E3.3 text/rng）：`io.text.*` 正则匹配/查找/替换/分割；`io.rng.*`
        // 伪随机数（seed/next/int/float）。rng 命名空间类名 RngNs——避开示例 84-rng
        // 的用户类 Rng（内建方法先于用户方法分派，同名会被拦截）。
        f.insert("text".into(), Value::class("Text", HashMap::new()));
        f.insert("rng".into(), Value::class("RngNs", HashMap::new()));
        // A6：标准库数据结构——Bitmap（位图）命名空间
        f.insert("bitmap".into(), Value::class("BitmapNs", HashMap::new()));
        // A6：标准库数据结构——RingBuf（环形缓冲）命名空间
        f.insert("ringbuf".into(), Value::class("RingBufNs", HashMap::new()));
        // A6：标准库数据结构——PageMem（页内存池）命名空间
        f.insert("pagemem".into(), Value::class("PageMemNs", HashMap::new()));
        // A6：标准库数据结构——IntrList（侵入式链表）命名空间
        f.insert(
            "intrlist".into(),
            Value::class("IntrListNs", HashMap::new()),
        );
        // A6：标准库数据结构——TreeMap（有序映射）命名空间
        f.insert("treemap".into(), Value::class("TreeMapNs", HashMap::new()));
        Value::class("Io", f)
    }

    /// E1（ADR-0013/Q23）：`types` 元数据对象——受限脚本模式的类型信息输入。
    /// 字段：`fields(name)` → `[["字段名", "类型串"], ...]`（class = 字段表；
    /// enum = 变体表）；`all` → 可见类型清单；`type` → 当前所在类型名（顶层 = ""）。
    pub(crate) fn types_meta(&mut self, field: &str, args: &[Expr]) -> Result<Value> {
        match field {
            "fields" => {
                let span = args
                    .first()
                    .map(|a| a.span())
                    .unwrap_or_else(|| Span::new(0, 0, 0, 0));
                let name = self.eval_str_arg(args, 0, &span)?;
                let name = String::from_utf8_lossy(&name).into_owned();
                match self.types.get(&name) {
                    Some(TypeDef::Class { fields, .. }) => {
                        let mut out = Vec::new();
                        for fd in fields {
                            out.push(Value::arr(vec![
                                Value::str(&fd.name),
                                Value::str(&fmt_type_str(&fd.ty)),
                            ]));
                        }
                        Ok(Value::arr(out))
                    }
                    Some(TypeDef::Enum { variants }) => {
                        let mut out = Vec::new();
                        for v in variants {
                            let payload = v.payload.as_ref().map(fmt_type_str).unwrap_or_default();
                            out.push(Value::arr(vec![Value::str(&v.name), Value::str(&payload)]));
                        }
                        Ok(Value::arr(out))
                    }
                    _ => Err(RtError::msg(
                        "UnknownType",
                        format!("types.fields: 未知类型 `{name}`"),
                    )),
                }
            }
            "all" => {
                let mut names: Vec<String> = self.types.keys().cloned().collect();
                names.sort();
                Ok(Value::arr(
                    names.into_iter().map(|s| Value::str(&s)).collect(),
                ))
            }
            "type" => {
                // script 块所在作用域的类型名；顶层/命名空间 = ""（随块位置收窄待补定）
                Ok(Value::str(""))
            }
            other => Err(RtError::msg(
                "ScriptForbidden",
                format!("types.{other}: 未知元数据字段（fields / all / type）"),
            )),
        }
    }

    pub(crate) fn default_value(&mut self, ty: Option<&Type>) -> Result<Value> {
        match ty {
            None => Ok(Value::Void),
            Some(t) => match t.strip() {
                Type::Named(n, args) => {
                    // E1.2 组 D：泛型类型应用（`Pair<i32>`）→ 惰性具体化后递归。
                    // 具体化产物：struct → `Pair<@i32>`（登记后按 class 空实例）；
                    // `return T;` 透传 → 实参类型自身（`Pair<i32>` ≡ `i32`）。
                    if !args.is_empty() {
                        let cn = self.concrete_type_name(n, args)?;
                        return self.default_value(Some(&Type::Named(cn, vec![])));
                    }
                    match n.as_str() {
                        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" => Ok(Value::Int(0)),
                        "u8" | "u16" | "u32" | "u64" | "u128" | "usize" => Ok(Value::Int(0)),
                        "f32" | "f64" | "f16" | "f128" => Ok(Value::Float(0.0)),
                        "bool" => Ok(Value::Bool(false)),
                        "void" => Ok(Value::Void),
                        "String" | "&[u8]" => Ok(Value::str("")),
                        "Vec" | "Deque" => Ok(Value::vec(
                            vec![],
                            Value::Allocator(Rc::new(RefCell::new(AllocatorImpl::Page))),
                        )),
                        "Map" => Ok(Value::map(
                            HashMap::new(),
                            Value::Allocator(Rc::new(RefCell::new(AllocatorImpl::Page))),
                        )),
                        _ => {
                            // Vec(T) / Map 集合类型
                            if n == "Vec" {
                                return Ok(Value::vec(
                                    vec![],
                                    Value::Allocator(Rc::new(RefCell::new(AllocatorImpl::Page))),
                                ));
                            }
                            if n == "Map" {
                                return Ok(Value::map(
                                    HashMap::new(),
                                    Value::Allocator(Rc::new(RefCell::new(AllocatorImpl::Page))),
                                ));
                            }
                            // 命名类型：class / enum 空实例
                            match self.types.get(n) {
                                Some(TypeDef::Class { fields, .. }) => {
                                    let mut f = HashMap::new();
                                    // 先克隆字段类型：default_value(&mut self) 具体化会重新借用 self
                                    let ftypes: Vec<(String, Type)> = fields
                                        .iter()
                                        .map(|fd| (fd.name.clone(), fd.ty.clone()))
                                        .collect();
                                    for (fname, fty) in &ftypes {
                                        f.insert(fname.clone(), self.default_value(Some(fty))?);
                                    }
                                    Ok(Value::class(n, f))
                                }
                                Some(TypeDef::Enum { .. }) => Ok(Value::Enum {
                                    name: n.clone(),
                                    variant: "__none__".into(),
                                    payload: None,
                                }),
                                _ => {
                                    Err(RtError::msg("UnknownType", format!("unknown type `{n}`")))
                                }
                            }
                        }
                    }
                }
                Type::Optional(_) => Ok(Value::Opt(None)),
                Type::Ptr(_, _) => Ok(Value::Void),
                Type::Slice(_, _) => Ok(Value::str("")),
                Type::Infer | Type::Owned(_) => Ok(Value::Void),
                _ => Ok(Value::Void),
            },
        }
    }

    /// E1.2 组 D：惰性具体化——`Pair<i32>` → 具体化名 `Pair<@i32>`。
    ///
    /// `self.types` 缓存命中即回（纯查找，immutable）；未命中则查类型函数定义
    /// （`funcs` 中返回 `type` 的函数）→ `comptime::instantiate` → 以具体化名登记
    /// 伪 Class 声明 → 返回具体化名。`args` 为空 → 原样返回（普通类型名）。
    ///
    /// 透传形态（`return T;`）产物是**实参类型自身**：返回其规范名（`type_key`），
    /// 使 `Pair<i32>` 与 `i32` 同义（递归 `default_value` 自然落到原始类型分支）。
    pub(crate) fn concrete_type_name(&mut self, name: &str, args: &[Type]) -> Result<String> {
        if args.is_empty() {
            return Ok(name.to_string());
        }
        // E1.2 组 D D3：预解析实参——内层类型函数应用先具体化登记（返回具体化键）。
        // 自/互递归类型函数经 `instantiating` 守卫终止（见下）。
        let mut resolved: Vec<Type> = Vec::with_capacity(args.len());
        for a in args {
            resolved.push(self.resolve_nested_types(a)?);
        }
        let cname = comptime::concrete_name(name, &resolved);
        if self.types.contains_key(&cname) {
            return Ok(cname);
        }
        // 自/互递归守卫：`LinkedList<i32>` 字段内自引用在登记期重入 → 返回键本身（叶）。
        if self.instantiating.contains(&cname) {
            return Ok(cname);
        }
        // 查类型函数定义（`fn name(T: type) type`）→ 编译期求值具体化
        if let Some(defs) = self.funcs.get(name) {
            for f in defs {
                if !comptime::is_type_fn(&f.params, &f.ret) {
                    continue;
                }
                self.instantiating.push(cname.clone());
                let inst = comptime::instantiate(name, &f.params, &f.body, &resolved);
                let result = match inst {
                    Ok(Instantiated::Class(mut decl)) => {
                        // D3：字段类型规范化——`Pair(T)` → 实参具体化键（自引用命中守卫）
                        match self.normalize_decl_fields(&mut decl) {
                            Ok(()) => match self.register_type_decl(&decl) {
                                Ok(()) => Ok(cname),
                                Err(e) => Err(e),
                            },
                            Err(e) => Err(e),
                        }
                    }
                    Ok(Instantiated::Type(t)) => {
                        // 透传：`Pair<i32>` ≡ 实参类型自身
                        Ok(comptime::type_key(&t))
                    }
                    Err(msg) => Err(RtError::msg("TypeInstantiation", msg)),
                };
                self.instantiating.pop();
                return result;
            }
        }
        // 非类型函数（内建泛型 `Vec(T)`/`Map(K,V)` 等）：
        // 若实参含具体化名（含 `@`），则生成具体化名保留嵌套类型信息
        // （如 `Vec<@List<@i32>>`）；否则回退基础名（`Vec<i32>` → `Vec`），
        // 由既有非泛型路径处理（空集合 / 类型未登记 → UnknownType，保持原语义）。
        let has_concrete_arg = resolved.iter().any(|a| match a.strip() {
            Type::Named(n, _) => n.contains('@'),
            _ => false,
        });
        if has_concrete_arg {
            Ok(comptime::concrete_name(name, &resolved))
        } else {
            Ok(name.to_string())
        }
    }

    /// E1.2 组 D D3：深度解析类型中的嵌套类型函数应用（`Pair<i32>` → `Pair<@i32>`）。
    /// 内层先具体化登记；自/互递归经 `instantiating` 守卫返回键（叶）。
    pub(crate) fn resolve_nested_types(&mut self, ty: &Type) -> Result<Type> {
        let src = self.source.clone();
        comptime::map_type_apps(ty, &mut |n, a| {
            self.concrete_type_name(n, a).map_err(|e| e.render(&src))
        })
        .map_err(|msg| RtError::msg("TypeInstantiation", msg))
    }

    /// E1.2 组 D D3：把具体化 Class 声明的字段类型深度规范化——嵌套类型函数应用
    /// （`Pair<i32>`）替换为具体化键（`Pair<@i32>`）；自/互递归经守卫终止。
    pub(crate) fn normalize_decl_fields(&mut self, decl: &mut Decl) -> Result<()> {
        if let Decl::Class { fields, .. } = decl {
            for fd in fields.iter_mut() {
                fd.ty = self.resolve_nested_types(&fd.ty)?;
            }
        }
        Ok(())
    }

    pub(crate) fn exec_if(&mut self, ifs: &IfStmt) -> Result<Flow> {
        let cond = self.eval(&ifs.cond)?;
        // 捕获：if (maybe) |v| { ... }——Some 绑定 v，None 走 else；
        // 错误联合：else |err| 绑定 err 走 else；无 err_capture 但有 else → 错误丢弃
        if let Some((_, name)) = &ifs.capture {
            match self.deref_value(cond) {
                Value::Opt(Some(v)) => {
                    self.push_scope();
                    self.bind(name, (*v).clone());
                    let r = self.exec_block(&ifs.then_b);
                    let _ = self.pop_scope(Self::is_err_path(&r));
                    return r;
                }
                Value::Opt(None) => {
                    // 错误捕获存在：null 非错误路径不进入 else（else 体仅在错误路径执行）
                    if ifs.err_capture.is_some() {
                        return Ok(Flow::None);
                    }
                    if let Some(else_b) = &ifs.else_b {
                        return self.exec_stmt(else_b);
                    }
                    return Ok(Flow::None);
                }
                err @ Value::Err { .. } => {
                    if let Some((_, en)) = &ifs.err_capture {
                        self.push_scope();
                        self.bind(en, err);
                        let r = if let Some(else_b) = &ifs.else_b {
                            self.exec_stmt(else_b)
                        } else {
                            Ok(Flow::None)
                        };
                        let _ = self.pop_scope(Self::is_err_path(&r));
                        return r;
                    }
                    if let Some(else_b) = &ifs.else_b {
                        return self.exec_stmt(else_b);
                    }
                    // 无 else 捕获：错误值绑到 then（保持旧 other 行为）
                    self.push_scope();
                    self.bind(name, err);
                    let r = self.exec_block(&ifs.then_b);
                    let _ = self.pop_scope(Self::is_err_path(&r));
                    return r;
                }
                other => {
                    self.push_scope();
                    self.bind(name, other);
                    let r = self.exec_block(&ifs.then_b);
                    let _ = self.pop_scope(Self::is_err_path(&r));
                    return r;
                }
            }
        }
        if cond.as_bool() {
            self.exec_block(&ifs.then_b)
        } else if let Some(else_b) = &ifs.else_b {
            self.exec_stmt(else_b)
        } else {
            Ok(Flow::None)
        }
    }

    pub(crate) fn exec_while(&mut self, w: &WhileStmt) -> Result<Flow> {
        loop {
            let cond = self.eval(&w.cond)?;
            // optional 捕获：while (maybe) |v|——Some 绑定 v 并循环，None 退出；
            // 错误联合错误值（无 else 捕获）→ 沿调用链传播
            let bind: Option<(String, Value)> = match &w.capture {
                Some((_, name)) => match self.deref_value(cond) {
                    Value::Opt(Some(v)) => Some((name.clone(), (*v).clone())),
                    Value::Opt(None) => return Ok(Flow::None),
                    err @ Value::Err { .. } => {
                        return Err(RtError::signal(Flow::Return(err)));
                    }
                    other => Some((name.clone(), other)),
                },
                None => {
                    if !cond.as_bool() {
                        return Ok(Flow::None);
                    }
                    None
                }
            };
            self.push_scope();
            if let Some((name, val)) = &bind {
                self.bind(name, val.clone());
            }
            let r = self.exec_block_inner(&w.body);
            let _ = self.pop_scope(Self::is_err_path(&r));
            match r {
                // 带标签 break/continue：仅匹配本循环标签才消费，否则向上一级传播
                Ok(Flow::Break(l)) => {
                    if l.is_some() && l != w.label {
                        return Ok(Flow::Break(l));
                    }
                    return Ok(Flow::None);
                }
                Ok(Flow::Continue(l)) => {
                    if l.is_some() && l != w.label {
                        return Ok(Flow::Continue(l));
                    }
                }
                Ok(Flow::Return(v)) => return Ok(Flow::Return(v)),
                Ok(Flow::Value(_)) => {}
                Ok(Flow::None) => {}
                // `orelse continue` / `catch break` 等表达式内信号 → 恢复为流
                Err(e) if e.is_signal() => match e.signal {
                    Some(Flow::Break(l)) => {
                        if l.is_some() && l != w.label {
                            return Ok(Flow::Break(l));
                        }
                        return Ok(Flow::None);
                    }
                    Some(Flow::Continue(l)) => {
                        if l.is_some() && l != w.label {
                            return Ok(Flow::Continue(l));
                        }
                    }
                    Some(Flow::Return(v)) => return Ok(Flow::Return(v)),
                    _ => return Err(e),
                },
                Err(e) => return Err(e),
            }
            if let Some(step) = &w.step {
                self.eval(step)?;
            }
        }
    }

    pub(crate) fn exec_for(&mut self, f: &ForStmt) -> Result<Flow> {
        let iter = self.eval(&f.iter)?;
        // 展开可迭代对象
        let items: Vec<(Rc<RefCell<Value>>, bool)> = self.iter_items(&iter)?;
        'outer: for (cell, is_ref) in items {
            self.push_scope();
            match f.capture {
                CaptureMode::Read => {
                    if is_ref {
                        // 只读捕获：绑定值副本（不写回）
                        let v = cell.borrow().clone();
                        self.bind(&f.capture_name, v);
                    } else {
                        self.bind(&f.capture_name, cell.borrow().clone());
                    }
                }
                CaptureMode::Mut | CaptureMode::Move => {
                    // 可写捕获：绑定共享槽（写回原数组）
                    self.scopes
                        .last_mut()
                        .unwrap()
                        .vars
                        .insert(f.capture_name.clone(), cell);
                }
            }
            let r = self.exec_block_inner(&f.body);
            let _ = self.pop_scope(Self::is_err_path(&r));
            match r {
                // 带标签 break/continue：仅匹配本循环标签才消费，否则向上一级传播
                Ok(Flow::Break(l)) => {
                    if l.is_some() && l != f.label {
                        return Ok(Flow::Break(l));
                    }
                    return Ok(Flow::None);
                }
                Ok(Flow::Continue(l)) => {
                    if l.is_some() && l != f.label {
                        return Ok(Flow::Continue(l));
                    }
                    continue 'outer;
                }
                Ok(Flow::Return(v)) => return Ok(Flow::Return(v)),
                Ok(Flow::Value(_)) => {}
                Ok(Flow::None) => {}
                // `orelse continue` 等表达式内信号 → 恢复为流
                Err(e) if e.is_signal() => match e.signal {
                    Some(Flow::Break(l)) => {
                        if l.is_some() && l != f.label {
                            return Ok(Flow::Break(l));
                        }
                        return Ok(Flow::None);
                    }
                    Some(Flow::Continue(l)) => {
                        if l.is_some() && l != f.label {
                            return Ok(Flow::Continue(l));
                        }
                        continue 'outer;
                    }
                    Some(Flow::Return(v)) => return Ok(Flow::Return(v)),
                    _ => return Err(e),
                },
                Err(e) => return Err(e),
            }
        }
        Ok(Flow::None)
    }

    /// 返回迭代项列表：(共享槽, 是否源容器引用)
    pub(crate) fn iter_items(&mut self, v: &Value) -> Result<Vec<(Rc<RefCell<Value>>, bool)>> {
        let deref = self.deref_value(v.clone());
        match &deref {
            Value::Arr(a) => Ok(a.borrow().iter().map(|c| (c.clone(), true)).collect()),
            // 集合（G4）：Vec 句柄遍历（Ptr(Vec) 一层 deref 后为 Vec——共享 items）
            Value::Vec(d) => Ok(d
                .borrow()
                .items
                .borrow()
                .iter()
                .map(|c| (c.clone(), true))
                .collect()),
            Value::Slice { data, start, len } => {
                let d = data.borrow();
                Ok((0..*len).map(|i| (d[*start + i].clone(), true)).collect())
            }
            Value::Class(c) if c.borrow().name == "Map" => {
                // Map 遍历：键值对捕获（|kv| → kv.key / kv.value）
                let d = c.borrow();
                let items: Vec<Value> = d
                    .fields
                    .iter()
                    .map(|(k, v)| {
                        let mut f = HashMap::new();
                        f.insert("key".to_string(), Value::str(k));
                        f.insert("value".to_string(), v.clone());
                        Value::class("KV", f)
                    })
                    .collect();
                Ok(items
                    .into_iter()
                    .map(|v| (Rc::new(RefCell::new(v)), false))
                    .collect())
            }
            // 集合（G4）：Map 句柄遍历（同 Class("Map")）
            Value::Map(m) => {
                let d = m.borrow();
                let items: Vec<Value> = d
                    .fields
                    .iter()
                    .map(|(k, v)| {
                        let mut f = HashMap::new();
                        f.insert("key".to_string(), Value::str(k));
                        f.insert("value".to_string(), v.clone());
                        Value::class("KV", f)
                    })
                    .collect();
                Ok(items
                    .into_iter()
                    .map(|v| (Rc::new(RefCell::new(v)), false))
                    .collect())
            }
            Value::Class(_c) => {
                // 用户类型迭代（IIterable 契约）：循环调用 next(self) 直到 null
                let mut items = Vec::new();
                loop {
                    let next_v = self.eval_next_method(&deref)?;
                    match next_v {
                        Value::Opt(Some(v)) => items.push((*v).clone()),
                        Value::Opt(None) => break,
                        Value::Void => break,
                        other => items.push(other),
                    }
                }
                Ok(items
                    .into_iter()
                    .map(|v| (Rc::new(RefCell::new(v)), false))
                    .collect())
            }
            Value::Str(s) => {
                let bytes: Vec<u8> = s.borrow().clone();
                Ok(bytes
                    .into_iter()
                    .map(|b| (Rc::new(RefCell::new(Value::Int(b as i128))), false))
                    .collect())
            }
            Value::LazyIter(li) => {
                // 惰性迭代器：逐项按需求值，收集为 (共享槽, false) 列表
                let mut items = Vec::new();
                let dummy_span = Span {
                    start: 0,
                    end: 0,
                    line: 0,
                    col: 0,
                };
                loop {
                    let v = self.lazy_iter_next(&mut li.borrow_mut(), &dummy_span)?;
                    match v {
                        Value::Opt(Some(val)) => {
                            items.push((Rc::new(RefCell::new((*val).clone())), false));
                        }
                        Value::Opt(None) => break,
                        _ => break,
                    }
                }
                Ok(items)
            }
            _ => Err(RtError::msg(
                "NotIterable",
                format!("value of type `{}` is not iterable", deref.type_name()),
            )),
        }
    }

    /// 调用用户类型迭代器的 next(self) 方法（IIterable 契约，tag1：next → ?T）
    pub(crate) fn eval_next_method(&mut self, v: &Value) -> Result<Value> {
        let type_name = v.type_name();
        let fname = format!("{type_name}.next");
        if !self.funcs.contains_key(&fname) {
            return Err(RtError::msg(
                "NotIterable",
                format!("type `{type_name}` has no `next` method (IIterable)"),
            ));
        }
        let self_v = v.clone();
        let vals = vec![self_v];
        let fdef = self.pick_fn(&fname, &vals)?;
        self.call_fn(&fdef, &vals, &Span::new(0, 0, 0, 0))
    }

    /// 将任意可迭代值转换为元素数组（立即求值；iter/filter/map 方法链共用）。
    /// Arr/Slice → 元素浅克隆；Str → 字节 Int；Map → KV 类；用户类型 → next() 直到 null。
    pub(crate) fn iter_to_arr(&mut self, v: &Value) -> Result<Value> {
        let deref = self.deref_value(v.clone());
        match &deref {
            Value::Arr(a) => {
                let items = a.borrow().iter().map(|c| c.borrow().clone()).collect();
                Ok(Value::arr(items))
            }
            Value::Slice { data, start, len } => {
                let d = data.borrow();
                let items = d[*start..*start + *len]
                    .iter()
                    .map(|c| c.borrow().clone())
                    .collect();
                Ok(Value::arr(items))
            }
            Value::Str(s) => {
                let items = s.borrow().iter().map(|b| Value::Int(*b as i128)).collect();
                Ok(Value::arr(items))
            }
            Value::Class(c) if c.borrow().name == "Map" => {
                let d = c.borrow();
                let items = d
                    .fields
                    .iter()
                    .map(|(k, val)| {
                        let mut f = HashMap::new();
                        f.insert("key".to_string(), Value::str(k));
                        f.insert("value".to_string(), val.clone());
                        Value::class("KV", f)
                    })
                    .collect();
                Ok(Value::arr(items))
            }
            // 集合（G4）：Map 句柄 → KV 条目数组（同 Class("Map")）
            Value::Map(m) => {
                let d = m.borrow();
                let items = d
                    .fields
                    .iter()
                    .map(|(k, val)| {
                        let mut f = HashMap::new();
                        f.insert("key".to_string(), Value::str(k));
                        f.insert("value".to_string(), val.clone());
                        Value::class("KV", f)
                    })
                    .collect();
                Ok(Value::arr(items))
            }
            Value::Class(_) => {
                let mut items = Vec::new();
                loop {
                    let next_v = self.eval_next_method(&deref)?;
                    match next_v {
                        Value::Opt(Some(v)) => items.push((*v).clone()),
                        Value::Opt(None) | Value::Void => break,
                        other => items.push(other),
                    }
                }
                Ok(Value::arr(items))
            }
            _ => Err(RtError::msg(
                "NotIterable",
                format!("value of type `{}` is not iterable", deref.type_name()),
            )),
        }
    }

    pub(crate) fn exec_switch(&mut self, sw: &SwitchStmt) -> Result<Flow> {
        let subject = self.eval(&sw.subject)?;
        let subject = self.deref_value(subject);
        for arm in &sw.arms {
            for pat in &arm.patterns {
                if self.match_pattern(&subject, pat)? {
                    // C3：switch 守卫——模式匹配后检查守卫条件，守卫失败继续下一分支
                    if let Some(guard) = &arm.guard {
                        let guard_val = self.eval(guard)?;
                        if !matches!(guard_val, Value::Bool(true)) {
                            continue;
                        }
                    }
                    return self.exec_switch_arm(arm, subject.clone());
                }
            }
        }
        if sw.has_else {
            for arm in &sw.arms {
                if arm
                    .patterns
                    .iter()
                    .any(|p| matches!(p, SwitchPattern::Else))
                {
                    // C3：else 臂守卫也检查
                    if let Some(guard) = &arm.guard {
                        let guard_val = self.eval(guard)?;
                        if !matches!(guard_val, Value::Bool(true)) {
                            continue;
                        }
                    }
                    return self.exec_switch_arm(arm, subject.clone());
                }
            }
        }
        Ok(Flow::None)
    }

    /// 执行 switch 臂；单表达式臂体（`int => |i| i`）作为 switch 表达式值返回
    pub(crate) fn exec_switch_arm(&mut self, arm: &SwitchArm, subject: Value) -> Result<Flow> {
        self.push_scope();
        if let Some((_, name)) = &arm.capture {
            // 枚举负载捕获：`int => |i| i` 中 i = 负载值
            let cap = match &subject {
                Value::Enum {
                    payload: Some(p), ..
                } => (**p).clone(),
                _ => subject.clone(),
            };
            self.bind(name, cap);
        }
        // 单表达式臂体：返回值（switch 表达式语义）——Flow::Value 区别于语句 return
        if arm.body.stmts.len() == 1 {
            if let Stmt::Expr(e) = &arm.body.stmts[0] {
                let v = self.eval(e);
                let _ = self.pop_scope(Self::is_err_path(&v.clone().map(Flow::Value)));
                return match v {
                    Ok(val) => Ok(Flow::Value(val)),
                    Err(err) => Err(err),
                };
            }
        }
        let r = self.exec_block_inner(&arm.body);
        let _ = self.pop_scope(Self::is_err_path(&r));
        r
    }

    pub(crate) fn match_pattern(&self, subject: &Value, pat: &SwitchPattern) -> Result<bool> {
        match (subject, pat) {
            (Value::Enum { variant, .. }, SwitchPattern::Ident(s)) => Ok(variant == s),
            (Value::Int(i), SwitchPattern::Int(s)) => {
                let (n, _) = parse_int_text(s)?;
                Ok(*i == n)
            }
            (Value::Float(f), SwitchPattern::Float(s)) => {
                Ok(*f == s.replace('_', "").parse::<f64>().unwrap_or(f64::NAN))
            }
            (Value::Str(st), SwitchPattern::Str(s)) => Ok(*st.borrow() == s.as_bytes()),
            (Value::Int(c), SwitchPattern::Char(pc)) => Ok(*c == *pc as i128),
            (Value::Err { name, .. }, SwitchPattern::Error(pe)) => Ok(name == pe),
            (Value::Bool(b), SwitchPattern::Ident(s)) => {
                Ok((*b && s == "true") || (!*b && s == "false"))
            }
            (Value::Opt(None), SwitchPattern::Ident(s)) => Ok(s == "null"),
            _ => Ok(false),
        }
    }

    // ---------- 表达式 ----------

    /// 任意字节容器 → 字节（Str / Arr(Int) / Slice 视图；57-protocol-parse 长度前缀帧）
    pub(crate) fn value_bytes(&self, v: &Value) -> Option<Vec<u8>> {
        match v {
            Value::Str(s) => Some(s.borrow().clone()),
            Value::Arr(a) => Some(
                a.borrow()
                    .iter()
                    .map(|c| match c.borrow().clone() {
                        Value::Int(i) => i as u8,
                        _ => 0,
                    })
                    .collect(),
            ),
            Value::Slice { data, start, len } => {
                let d = data.borrow();
                let mut out = Vec::with_capacity(*len);
                for i in 0..*len {
                    match d[*start + i].borrow().clone() {
                        Value::Int(n) => out.push(n as u8),
                        _ => return None,
                    }
                }
                Some(out)
            }
            // 集合（G4）：Vec 与 Arr 同为元素共享槽容器 → 委托
            Value::Vec(d) => self.value_bytes(&Value::Arr(d.borrow().items.clone())),
            _ => None,
        }
    }

    pub(crate) fn deref_value(&self, v: Value) -> Value {
        match v {
            // 递归解引用：Ptr/Boxed → pointee（可能又是指针/集合），Vec → 共享 Arr
            // （对齐 IR `deref_value`：Ptr(Vec)/Boxed(Vec) 一层即剥到 Arr）
            Value::Ptr(c) => self.deref_value(c.borrow().clone()),
            Value::Boxed(b) => self.deref_value(b.borrow().data.borrow().clone()),
            // 集合（G4）：剥为共享 Arr（items 同一存储）——方法分派复用全部 Arr 方法
            Value::Vec(d) => Value::Arr(d.borrow().items.clone()),
            other => other,
        }
    }

    /// M2.5/M4.7：仅检查悬垂（不解引用）——指针指向已销毁目标 → 抛错带位置
    pub(crate) fn check_dangling(&self, v: &Value, span: &Span) -> Result<()> {
        if self.debug_dangling {
            if let Value::Ptr(c) = v {
                if matches!(&*c.borrow(), Value::Dangling) {
                    return Err(RtError::new("DanglingPointer", Some(span.clone())));
                }
            }
        }
        Ok(())
    }

    /// M2.5/M4.7：解引用访问（带悬垂检查）——Debug 下访问已销毁目标
    /// 的指针 → 抛错带位置；Release（debug_dangling=false）裸读（用户负责）
    pub(crate) fn deref_checked(&self, v: Value, span: &Span) -> Result<Value> {
        self.check_dangling(&v, span)?;
        Ok(self.deref_value(v))
    }

    /// 捕获当前作用域链（闭包环境快照）——**自由变量精确化**（Phase 8）：
    /// 只捕获 `free` 集合内的名字（闭包体实际引用、未被体内绑定遮蔽）。
    /// 作用域链结构保留（`call_closure` 按链重建作用域 → 查找最近绑定优先，
    /// 遮蔽解析正确）；`free` 外的名字不进入环境（未捕获变量闭包不可见）。
    pub(crate) fn capture_env(
        &self,
        free: &std::collections::HashSet<String>,
    ) -> Vec<std::collections::HashMap<String, Rc<RefCell<Value>>>> {
        self.scopes
            .iter()
            .map(|s| {
                s.vars
                    .iter()
                    .filter(|(k, _)| free.contains(*k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .collect()
    }

    /// 调用闭包返回 bool（filter 谓词）
    pub(crate) fn call_closure_bool(
        &mut self,
        c: &ClosureData,
        args: &[Value],
        span: &Span,
    ) -> Result<bool> {
        let v = self.call_closure(c, args, span)?;
        Ok(v.as_bool())
    }

    /// 调用闭包返回值（map 变换）
    pub(crate) fn call_closure_value(
        &mut self,
        c: &ClosureData,
        args: &[Value],
        span: &Span,
    ) -> Result<Value> {
        self.call_closure(c, args, span)
    }

    /// 调用闭包：绑定参数到捕获环境之上
    pub(crate) fn call_closure(
        &mut self,
        c: &ClosureData,
        arg_vals: &[Value],
        span: &Span,
    ) -> Result<Value> {
        if c.params.len() != arg_vals.len() {
            return Err(RtError::new("ArityMismatch", Some(span.clone())));
        }
        let saved = std::mem::take(&mut self.scopes);
        // M2.7 只读强制（Phase 8）：非 `mut` 闭包的环境 cell 只读——
        // 直接重绑定捕获变量 → ReadonlyCapture；经指针/字段/索引写穿放行
        // （被捕获变量自身的槽未被改写）。嵌套闭包叠压（外层非 mut 仍生效）。
        let saved_readonly = std::mem::take(&mut self.readonly_caps);
        if !c.is_mut {
            for m in &c.env {
                for (_, cell) in m {
                    self.readonly_caps.push(Rc::as_ptr(cell) as usize);
                }
            }
        }
        // 闭包无声明返回类型：隔离期望类型（避免借用外层函数返回类型）
        let saved_ret = self.current_ret.take();
        let mut scopes: Vec<Scope> = c
            .env
            .iter()
            .map(|m| Scope {
                vars: m.clone(),
                defers: Vec::new(),
            })
            .collect();
        scopes.push(Scope::new());
        self.scopes = scopes;
        for (p, v) in c.params.iter().zip(arg_vals.iter()) {
            self.bind(p, v.clone());
        }
        // 单表达式闭包体（|v| v + a）：求值作为返回值
        let r: Result<Flow> = if c.body.stmts.len() == 1 {
            if let Stmt::Expr(e) = &c.body.stmts[0] {
                self.eval(e).map(|v| Flow::Value(v))
            } else {
                self.exec_block_inner(&c.body)
            }
        } else {
            self.exec_block_inner(&c.body)
        };
        let _ = self.pop_scope(Self::is_err_path(&r));
        self.scopes = saved;
        self.readonly_caps = saved_readonly;
        self.current_ret = saved_ret;
        match r {
            Ok(Flow::Return(v)) => Ok(v),
            Ok(Flow::Value(v)) => Ok(v),
            Ok(Flow::None) => Ok(Value::Void),
            Ok(Flow::Break(_)) | Ok(Flow::Continue(_)) => {
                Err(RtError::new("InvalidControlFlow", Some(span.clone())))
            }
            Err(e) if e.is_signal() => match e.signal {
                Some(Flow::Return(v)) => Ok(v),
                Some(Flow::Value(v)) => Ok(v),
                Some(Flow::Break(_)) | Some(Flow::Continue(_)) => {
                    Err(RtError::new("InvalidControlFlow", Some(span.clone())))
                }
                _ => Err(e),
            },
            Err(e) => Err(e),
        }
    }
}
