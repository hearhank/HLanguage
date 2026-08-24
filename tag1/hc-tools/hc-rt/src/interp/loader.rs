use super::*;

impl Interp {
    pub fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
            funcs: HashMap::new(),
            types: HashMap::new(),
            globals: HashMap::new(),
            scopes: vec![Scope::new()],
            call_depth: 0,
            test_out: Vec::new(),
            fail_info: None,
            expected_ret: None,
            current_ret: None,
            in_main: false,
            exit_code: None,
            tmp_field_cells: Vec::new(),
            files: HashMap::new(),
            next_fd: 1,
            tcp_streams: HashMap::new(),
            tcp_listeners: HashMap::new(),
            udp_sockets: HashMap::new(),
            dirs: HashMap::new(),
            pipes: HashMap::new(),
            shms: HashMap::new(),
            next_pipe_fd: 1,
            next_shm_fd: 1,
            next_dir_fd: 1,
            next_net_fd: 1,
            stores: HashMap::new(),
            next_store_fd: 1,
            rng_state: 0x9e37_79b9_7f4a_7c15,
            args: std::env::args().skip(1).collect(),
            error_locs: HashMap::new(),
            tracked: Default::default(),
            addr_registry: HashMap::new(),
            debug_dangling: true,
            alloc_tracker: Rc::new(RefCell::new(Vec::new())),
            readonly_caps: Vec::new(),
            error_codes: HashMap::new(),
            error_names: Vec::new(),
            extern_programs: Vec::new(),
            dep_programs: Vec::new(),
            import_env: HashMap::new(),
            root_threads: Vec::new(),
            script_mode: false,
            instantiating: Vec::new(),
            comptime_value_depth: 0,
            program: None,
            thread_handles: HashMap::new(),
            next_tid: 1,
            channels: HashMap::new(),
            next_channel_id: 1,
        }
    }

    /// E1（ADR-0013）：进入受限脚本模式——`script { }` 块装载期求值。
    /// 置位后 io/alloc/stdout/stderr/argv/网络不可用，注入 `types` 元数据对象。
    pub fn set_script_mode(&mut self, on: bool) -> &mut Self {
        self.script_mode = on;
        self
    }

    /// M2.5/M4.7：Debug 悬垂标记开关（Debug 默认开；Release 裸读，用户负责）
    pub fn set_debug_dangling(&mut self, on: bool) -> &mut Self {
        self.debug_dangling = on;
        self
    }

    /// G5/§8.3：Debug 泄漏检测——返回当前仍存活（未释放）的全局 alloc 分配清单。
    /// 供程序/线程退出时报告（CLI 打印 + 非零退出）；测试可直接断言。
    pub fn leak_report(&self) -> String {
        let mut out = String::new();
        for r in self.alloc_tracker.borrow().iter() {
            if r.weak.upgrade().is_some() {
                out.push_str(&format!("leak: line {}: {} bytes\n", r.line, r.size));
            }
        }
        out
    }

    /// G5/§8.3：当前活跃（未释放）分配数
    pub fn leak_count(&self) -> usize {
        self.alloc_tracker
            .borrow()
            .iter()
            .filter(|r| r.weak.upgrade().is_some())
            .count()
    }

    /// M2.6/M4.2：从编译期错误码表记录——错误名 → 首次出现位置（同名保留首个）
    /// + 错误名 ↔ 码映射（运行时错误值携带码；未登记错误名动态分配）
    pub(crate) fn record_error_locs(&mut self, program: &Program) {
        let table = hc::error_code_table(program);
        for entry in table.entries() {
            self.error_locs
                .entry(entry.name.clone())
                .or_insert_with(|| entry.span.clone());
            self.error_codes
                .entry(entry.name.clone())
                .or_insert(entry.code);
        }
        // 反向表：码 → 名（按包内序对齐编译期表）
        for (name, code) in &self.error_codes {
            let idx = hc::ErrorCodeTable::index_of(*code) as usize;
            while self.error_names.len() <= idx {
                self.error_names.push(String::new());
            }
            self.error_names[idx] = name.clone();
        }
    }

    /// M4.2：错误名 → 错误值（码 = 编译期表；运行时未登记错误名动态分配，
    /// 沿用当前包 ID 高位——anyerror 任意码）
    pub(crate) fn err_val(&mut self, name: &str) -> Value {
        let code = match self.error_codes.get(name) {
            Some(c) => *c,
            None => {
                let pkg = hc::ErrorCodeTable::package_of(
                    self.error_codes.values().next().copied().unwrap_or(0),
                );
                let idx = self.error_names.len() as u16;
                let code = hc::ErrorCodeTable::encode(pkg, idx);
                self.error_codes.insert(name.to_string(), code);
                self.error_names.push(name.to_string());
                code
            }
        };
        Value::Err {
            name: name.to_string(),
            code,
        }
    }

    // ---------- 程序装载 ----------

    pub fn load(&mut self, program: &Program) -> Result<()> {
        // 第零遍：语义检查（M2 静态 pass——宽度/引用赋值/类型错误编译期报错；
        // M1.4：同包兄弟文件符号并入；M7.2：依赖包按包名前缀 + pub 过滤并入）
        let externs: Vec<&Program> = self.extern_programs.iter().collect();
        let deps: Vec<(&str, &Program)> = self
            .dep_programs
            .iter()
            .map(|(n, p)| (n.as_str(), p))
            .collect();
        let diags = hc::check_semantics_extern_deps(program, &externs, &deps);
        if let Some(d) = diags.iter().find(|d| d.is_error()) {
            return Err(RtError::msg(
                "CompileError",
                format!("{}:{}: {}", d.span.line, d.span.col, d.message),
            ));
        }
        self.record_error_locs(program);
        // 第一遍：登记类型
        for d in &program.decls {
            self.register_type_decl(d)?;
        }
        // 内建枚举（M4.2 L3）：ExitType{ Exit, Error }
        self.types
            .entry("ExitType".to_string())
            .or_insert(TypeDef::Enum {
                variants: vec![
                    EnumVariant {
                        name: "Exit".into(),
                        payload: None,
                        span: Span::new(0, 0, 0, 0),
                    },
                    EnumVariant {
                        name: "Error".into(),
                        payload: None,
                        span: Span::new(0, 0, 0, 0),
                    },
                ],
            });
        // 第二遍：登记函数（含类型方法）
        for d in &program.decls {
            self.register_fn_decl(d)?;
        }
        // 第三遍：global / const 初始化 + 执行 namespace 内声明（tag1：扁平化）
        for d in &program.decls {
            self.exec_decl_top(d)?;
        }
        // 第四遍：`using NS;` 别名解析（M1.4/Q21）——限定名 → 扁平名导入
        self.apply_usings(program);
        // ADR-0010：`import` 语句运行时绑定（A2a——镜像语义层 apply_imports）
        self.apply_imports(program);
        // D1-4：保存程序快照，供线程模式测试 fork 新 Interp
        self.program = Some(Arc::new(program.clone()));
        Ok(())
    }

    /// M1.4：`using NS;` 导入命名空间函数为扁平名（文件自身定义优先；
    /// 同包跨命名空间 using 即达；`using NS as M` 等价重命名前缀）
    pub(crate) fn apply_usings(&mut self, program: &Program) {
        for d in &program.decls {
            self.collect_using(d);
        }
    }

    pub(crate) fn collect_using(&mut self, d: &Decl) {
        match d {
            Decl::Using { path, alias, .. } => {
                let prefix = path.join(".");
                let qp = format!("{prefix}.");
                let flat_of = |member: &str| match alias {
                    Some(a) => format!("{a}.{member}"),
                    None => member.to_string(),
                };
                // 函数导入（跳过方法：成员名不含 `.`）
                let keys: Vec<String> = self
                    .funcs
                    .keys()
                    .filter(|k| k.starts_with(&qp) && !k[qp.len()..].contains('.'))
                    .cloned()
                    .collect();
                for k in keys {
                    let member = k[qp.len()..].to_string();
                    let flat = flat_of(&member);
                    // 文件自身定义优先：扁平名已存在则不覆盖
                    if !self.funcs.contains_key(&flat) {
                        let defs = self.funcs.get(&k).cloned().unwrap_or_default();
                        if !defs.is_empty() {
                            self.funcs.entry(flat).or_default().extend(defs);
                        }
                    }
                }
                // 类型导入（using NS 后 `Line` 可直接引用）
                let tkeys: Vec<String> = self
                    .types
                    .keys()
                    .filter(|k| k.starts_with(&qp))
                    .cloned()
                    .collect();
                for k in tkeys {
                    let member = k[qp.len()..].to_string();
                    let flat = flat_of(&member);
                    if !self.types.contains_key(&flat) {
                        if let Some(def) = self.types.get(&k) {
                            self.types.insert(flat, def.clone());
                        }
                    }
                }
                // 全局导入
                let gkeys: Vec<String> = self
                    .globals
                    .keys()
                    .filter(|k| k.starts_with(&qp))
                    .cloned()
                    .collect();
                for k in gkeys {
                    let member = k[qp.len()..].to_string();
                    let flat = flat_of(&member);
                    if !self.globals.contains_key(&flat) {
                        if let Some(def) = self.globals.get(&k) {
                            self.globals.insert(flat, def.clone());
                        }
                    }
                }
            }
            Decl::Include { .. } => {
                // B6-2：文件引用由 run_file_hs 在解析前处理
            }
            Decl::Namespace { decls, .. } => {
                for inner in decls {
                    self.collect_using(inner);
                }
            }
            _ => {}
        }
    }

    /// ADR-0010：文件级 `import` 语句运行时绑定——镜像语义层 `apply_imports`
    /// （semantic.rs）。三种形态（06-08 §import）：
    /// - `import pkg.mod;`：整模块——绑定名 = 末段/别名；io 族环境模块 → import_env
    ///   别名；用户命名空间/包成员复制为 `{绑定}.{member}`（限定名可用）
    /// - `import pkg.mod as m;`：整模块 + 别名
    /// - `import pkg.mod.{a, b as c};`：符号选择——函数/类型/全局复制到绑定名（直接可用）；
    ///   命名空间成员以前缀绑定（`my.print` 形态）
    pub(crate) fn apply_imports(&mut self, program: &Program) {
        for d in &program.decls {
            self.collect_import(d);
        }
    }

    /// io 族环境模块名（对象形态内建：eval Ident 注入 `io_value()`）
    pub(crate) fn env_module(name: &str) -> Option<&'static str> {
        match name {
            "io" | "stdout" | "stderr" => Some("io"),
            _ => None,
        }
    }

    pub(crate) fn collect_import(&mut self, d: &Decl) {
        match d {
            Decl::Import {
                path,
                alias,
                select,
                ..
            } => {
                let target_prefix = format!("{}.{}", path.join("."), "");
                let module = path.last().cloned().unwrap_or_default();
                match select {
                    Some(syms) => {
                        for (sym, sym_alias) in syms {
                            let bound = sym_alias.clone().unwrap_or_else(|| sym.clone());
                            // io 族环境模块符号 → 别名绑定（`my.print` 走对象分发）
                            if let Some(env) = Self::env_module(sym) {
                                self.import_env.insert(bound, env.to_string());
                                continue;
                            }
                            let q = format!("{target_prefix}{sym}");
                            self.bind_imported_symbol(&q, &bound);
                        }
                    }
                    None => {
                        let bound = alias.clone().unwrap_or_else(|| module.clone());
                        if let Some(env) = Self::env_module(&module) {
                            self.import_env.insert(bound, env.to_string());
                            return;
                        }
                        self.import_whole_module(&target_prefix, &bound);
                    }
                }
            }
            Decl::Include { .. } => {
                // B6-2：文件引用由 run_file_hs 在解析前处理
            }
            Decl::Namespace { decls, .. } => {
                for inner in decls {
                    self.collect_import(inner);
                }
            }
            _ => {}
        }
    }

    /// 符号选择绑定：q（`pkg.mod.sym`）→ 绑定名直调/直引；命名空间成员 → 子成员前缀复制
    /// （`n.f` 限定访问；文件自身定义优先，不覆盖）
    pub(crate) fn bind_imported_symbol(&mut self, q: &str, bound: &str) {
        // 命名空间成员（有子成员）→ 子成员前缀复制
        let qp = format!("{q}.");
        let sub: Vec<String> = self
            .funcs
            .keys()
            .filter(|k| k.starts_with(&qp))
            .cloned()
            .collect();
        for k in sub {
            let member = &k[qp.len()..];
            let flat = format!("{bound}.{member}");
            if !self.funcs.contains_key(&flat) {
                let defs = self.funcs.get(&k).cloned().unwrap_or_default();
                if !defs.is_empty() {
                    self.funcs.insert(flat, defs);
                }
            }
        }
        // 函数符号 → 绑定名直调（`sq(4)`）
        if let Some(defs) = self.funcs.get(q) {
            if !defs.is_empty() && !self.funcs.contains_key(bound) {
                self.funcs.insert(bound.to_string(), defs.clone());
            }
        }
        // 类型符号 → 绑定名直接引用（`Line{...}`）
        if let Some(info) = self.types.get(q) {
            if !self.types.contains_key(bound) {
                self.types.insert(bound.to_string(), info.clone());
            }
        }
        // 全局符号 → 绑定名
        if let Some(t) = self.globals.get(q) {
            if !self.globals.contains_key(bound) {
                self.globals.insert(bound.to_string(), t.clone());
            }
        }
        // 未解析符号：保守放行（库/兄弟文件未知，运行时诊断）——不报错
    }

    /// 整模块导入：绑定名前缀登记 + 全部成员复制为 `{bound}.{member}`（镜像语义层
    /// import_whole_module；文件自身定义优先，不覆盖）
    pub(crate) fn import_whole_module(&mut self, target_prefix: &str, bound: &str) {
        // 函数成员（跳过方法：成员名不含 `.`；含嵌套子命名空间 → 整体前缀替换）
        let fkeys: Vec<String> = self
            .funcs
            .keys()
            .filter(|k| k.starts_with(target_prefix))
            .cloned()
            .collect();
        for k in fkeys {
            let member = &k[target_prefix.len()..];
            let flat = format!("{bound}.{member}");
            if self.funcs.contains_key(&flat) {
                continue;
            }
            let defs = self.funcs.get(&k).cloned().unwrap_or_default();
            if !defs.is_empty() {
                self.funcs.insert(flat, defs);
            }
        }
        // 类型成员
        let tkeys: Vec<String> = self
            .types
            .keys()
            .filter(|k| k.starts_with(target_prefix))
            .cloned()
            .collect();
        for k in tkeys {
            let member = &k[target_prefix.len()..];
            let flat = format!("{bound}.{member}");
            if !self.types.contains_key(&flat) {
                if let Some(info) = self.types.get(&k) {
                    self.types.insert(flat, info.clone());
                }
            }
        }
        // 全局成员
        let gkeys: Vec<String> = self
            .globals
            .keys()
            .filter(|k| k.starts_with(target_prefix))
            .cloned()
            .collect();
        for k in gkeys {
            let member = &k[target_prefix.len()..];
            let flat = format!("{bound}.{member}");
            if !self.globals.contains_key(&flat) {
                if let Some(t) = self.globals.get(&k) {
                    self.globals.insert(flat, t.clone());
                }
            }
        }
    }

    /// M1.4：加载同包兄弟文件声明（符号登记），跳过其 test 与 main（入口/测试归属目标文件）
    pub fn load_siblings(&mut self, programs: &[&Program]) -> Result<()> {
        // M1.4：记录外部符号（跨文件语义检查用）
        for p in programs {
            self.extern_programs.push((*p).clone());
        }
        for p in programs {
            self.record_error_locs(p);
            for d in &p.decls {
                self.register_type_decl(d)?;
            }
        }
        for p in programs {
            for d in &p.decls {
                self.register_fn_decl_skip_entry(d)?;
            }
        }
        for p in programs {
            for d in &p.decls {
                self.exec_decl_top(d)?;
            }
        }
        Ok(())
    }

    /// M7.2：加载依赖包声明——包名前缀登记，仅 `pub` 项可见（跨包边界），
    /// 不登记扁平名（不污染主包命名空间）、不注入 ExitType、不展开依赖自身 using、
    /// 不并入错误集（错误码按包隔离，tag1 单包 ID 0）。
    pub fn load_dep(&mut self, name: &str, programs: &[&Program]) -> Result<()> {
        for p in programs {
            self.dep_programs.push((name.to_string(), (*p).clone()));
        }
        for p in programs {
            for d in &p.decls {
                self.register_type_decl_prefixed_filter(d, name, true, true)?;
            }
        }
        for p in programs {
            for d in &p.decls {
                self.register_fn_decl_prefixed_filter(d, name, true, true, false)?;
            }
        }
        for p in programs {
            for d in &p.decls {
                self.exec_decl_top_filter(d, name, true)?;
            }
        }
        Ok(())
    }

    pub(crate) fn register_type_decl(&mut self, d: &Decl) -> Result<()> {
        self.register_type_decl_prefixed(d, "")
    }

    /// 类型登记（Q21 命名空间）：扁平名 + 限定名双注册。
    /// 扁平名（`Line`）供包内直接引用；限定名（`Orders.Line`）供
    /// `Vec(Orders.Line)` / `Orders.Line{...}` 限定访问（M1.4）。
    pub(crate) fn register_type_decl_prefixed(&mut self, d: &Decl, prefix: &str) -> Result<()> {
        self.register_type_decl_prefixed_filter(d, prefix, false, false)
    }

    /// 类型登记核心：`skip_flat` 抑制扁平名（依赖包）；`pub_only` 只登记 pub（跨包边界）
    pub(crate) fn register_type_decl_prefixed_filter(
        &mut self,
        d: &Decl,
        prefix: &str,
        skip_flat: bool,
        pub_only: bool,
    ) -> Result<()> {
        if pub_only && !d.is_pub() {
            return Ok(());
        }
        match d {
            Decl::Class {
                name,
                ifaces,
                traits,
                fields,
                methods,
                ..
            } => {
                let qname = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                let type_def = TypeDef::Class {
                    ifaces: ifaces.clone(),
                    traits: traits.clone(),
                    fields: fields.clone(),
                    methods: methods.clone(),
                    is_struct: false,
                };
                if !skip_flat {
                    self.types.insert(name.clone(), type_def.clone());
                }
                if !prefix.is_empty() {
                    self.types.insert(qname.clone(), type_def);
                }
                // 类型方法登记：Type.method / Orders.Type.method —— 首参 self 由调用点注入
                for m in methods {
                    let fname = format!("{qname}.{}", m.name);
                    self.funcs.entry(fname.clone()).or_default().push(FnDef {
                        name: fname,
                        params: m.params.clone(),
                        ret: m.ret.clone(),
                        body: m.body.clone(),
                        is_test: false,
                        test_name: None,
                        test_mode: TestMode::Serial,
                        test_timeout: None,
                        method_of: Some(qname.clone()),
                        // 组 E：async 方法留 E3/E4（示例无 async 方法）
                        is_async: false,
                        is_extern: false,
                        span: m.span.clone(),
                    });
                    // M1-1：扁平类型名也注册方法（命名空间包裹后，类型字面量 `Lexer{}` 使用扁平名
                    // 创建 Value::Class，type_name() 返回扁平名；方法查找 `Lexer.run` 需命中）
                    if !skip_flat && !prefix.is_empty() {
                        let flat_fname = format!("{name}.{}", m.name);
                        self.funcs
                            .entry(flat_fname.clone())
                            .or_default()
                            .push(FnDef {
                                name: flat_fname,
                                params: m.params.clone(),
                                ret: m.ret.clone(),
                                body: m.body.clone(),
                                is_test: false,
                                test_name: None,
                                test_mode: TestMode::Serial,
                                test_timeout: None,
                                method_of: Some(name.clone()),
                                is_async: false,
                                is_extern: false,
                                span: m.span.clone(),
                            });
                    }
                }
            }
            Decl::Struct {
                name,
                traits,
                fields,
                ..
            } => {
                let qname = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                let type_def = TypeDef::Class {
                    ifaces: vec![],
                    traits: traits.clone(),
                    fields: fields.clone(),
                    methods: vec![],
                    is_struct: true,
                };
                if !skip_flat {
                    self.types.insert(name.clone(), type_def.clone());
                }
                if !prefix.is_empty() {
                    self.types.insert(qname.clone(), type_def);
                }
            }
            Decl::Enum { name, variants, .. } => {
                let type_def = TypeDef::Enum {
                    variants: variants.clone(),
                };
                if !skip_flat {
                    self.types.insert(name.clone(), type_def.clone());
                }
                if !prefix.is_empty() {
                    self.types.insert(format!("{prefix}.{name}"), type_def);
                }
            }
            Decl::Union { name, fields, .. } => {
                let type_def = TypeDef::Union {
                    fields: fields.clone(),
                };
                if !skip_flat {
                    self.types.insert(name.clone(), type_def.clone());
                }
                if !prefix.is_empty() {
                    self.types.insert(format!("{prefix}.{name}"), type_def);
                }
            }
            Decl::Interface { name, supers, .. } => {
                let type_def = TypeDef::Interface {
                    supers: supers.clone(),
                };
                if !skip_flat {
                    self.types.insert(name.clone(), type_def.clone());
                }
                if !prefix.is_empty() {
                    self.types.insert(format!("{prefix}.{name}"), type_def);
                }
            }
            Decl::Namespace {
                name,
                decls,
                is_module,
                ..
            } => {
                let new_prefix = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                // 模块隔离（A2b）：`[module]` 成员不登记扁平名
                let inner_flat = skip_flat || *is_module;
                for inner in decls {
                    self.register_type_decl_prefixed_filter(
                        inner,
                        &new_prefix,
                        inner_flat,
                        pub_only,
                    )?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn register_fn_decl(&mut self, d: &Decl) -> Result<()> {
        self.register_fn_decl_prefixed(d, "")
    }

    /// 兄弟文件函数注册：跳过 [test] fn 与 main（M1.4 包加载）
    pub(crate) fn register_fn_decl_skip_entry(&mut self, d: &Decl) -> Result<()> {
        self.register_fn_decl_prefixed_filter(d, "", true, false, false)
    }

    /// 函数注册（Q21 命名空间）：扁平名 + 限定名双注册。
    /// 扁平名（`square`）供 `using Math;` 后直接调用；限定名（`Math.square`）
    /// 供 `Math.square(5)` 静态调用（eval_call Dot 分支经 funcs 命中）。
    pub(crate) fn register_fn_decl_prefixed(&mut self, d: &Decl, prefix: &str) -> Result<()> {
        self.register_fn_decl_prefixed_filter(d, prefix, false, false, false)
    }

    pub(crate) fn register_fn_decl_prefixed_filter(
        &mut self,
        d: &Decl,
        prefix: &str,
        skip_entry: bool,
        pub_only: bool,
        skip_flat: bool,
    ) -> Result<()> {
        if pub_only && !d.is_pub() {
            return Ok(());
        }
        match d {
            Decl::Fn {
                name,
                params,
                ret,
                body,
                is_test,
                test_name,
                test_mode,
                test_timeout,
                is_async,
                is_extern,
                span,
                ..
            } => {
                // 兄弟文件：不登记 test fn（测试归属目标文件）与 main（入口归属目标文件）
                if skip_entry && (*is_test || name == "main") {
                    return Ok(());
                }
                // 兄弟文件（skip_entry）：顶层函数不注册（文件私有，避免跨文件污染
                // 同名重载池，如 64/74 各自 describe）；命名空间函数只注册限定名
                // （扁平名由目标文件 `using NS;` 导入）。自身文件：扁平 + 限定双注册。
                if skip_entry && prefix.is_empty() {
                    return Ok(());
                }
                let fdef = FnDef {
                    name: name.clone(),
                    params: params.clone(),
                    ret: ret.clone(),
                    body: body.clone(),
                    is_test: *is_test,
                    test_name: test_name.clone(),
                    test_mode: *test_mode,
                    test_timeout: *test_timeout,
                    method_of: None,
                    is_async: *is_async,
                    is_extern: *is_extern,
                    span: span.clone(),
                };
                // 模块隔离（A2b）：`[module]` 成员不登记扁平名（仅限定名，供 import 复制）
                if !skip_entry && !skip_flat {
                    self.funcs
                        .entry(name.clone())
                        .or_default()
                        .push(fdef.clone());
                }
                if !prefix.is_empty() {
                    let qname = format!("{prefix}.{name}");
                    self.funcs.entry(qname).or_default().push(fdef);
                }
            }
            Decl::Namespace {
                name,
                decls,
                is_module,
                ..
            } => {
                let new_prefix = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                let inner_flat = skip_flat || *is_module;
                for inner in decls {
                    self.register_fn_decl_prefixed_filter(
                        inner,
                        &new_prefix,
                        skip_entry,
                        pub_only,
                        inner_flat,
                    )?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn exec_decl_top(&mut self, d: &Decl) -> Result<()> {
        match d {
            Decl::Global { name, init, .. } => {
                let v = match init {
                    Some(e) => self.eval(e)?,
                    None => Value::Void,
                };
                self.globals.insert(name.clone(), Rc::new(RefCell::new(v)));
            }
            Decl::Const { name, init, ty, .. } => {
                // 错误集类型别名：注册为“错误集”类型占位
                if let Some(Type::Named(tn, _)) = ty {
                    if tn.starts_with("error_set:") {
                        self.types
                            .insert(name.clone(), TypeDef::Interface { supers: vec![] });
                        return Ok(());
                    }
                }
                let v = self.eval(init)?;
                self.globals.insert(name.clone(), Rc::new(RefCell::new(v)));
            }
            Decl::Namespace { decls, .. } => {
                for inner in decls {
                    self.exec_decl_top(inner)?;
                }
            }
            Decl::Using { path, .. } => {
                // tag1：using 无操作（模块扁平化；跨包解析归 M1.4/M7.2）
                let _ = path;
            }
            Decl::Include { .. } => {
                // B6-2：.hs 脚本文件引用；loader 内不执行，由 run_file_hs 解析
            }
            Decl::Comptime { .. } => {
                // E1.2 组 D D2：comptime 块装载期求值（comptimegen）后跳过——仅编译期存在
            }
            _ => {}
        }
        Ok(())
    }

    /// M7.2：依赖包 global/const 初始化——限定名登记（`json.CONST`），仅 pub 项。
    /// 错误集别名不导出（错误码按包隔离）。
    pub(crate) fn exec_decl_top_filter(
        &mut self,
        d: &Decl,
        prefix: &str,
        pub_only: bool,
    ) -> Result<()> {
        if pub_only && !d.is_pub() {
            return Ok(());
        }
        match d {
            Decl::Global { name, init, .. } => {
                let v = match init {
                    Some(e) => self.eval(e)?,
                    None => Value::Void,
                };
                let key = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                self.globals.insert(key, Rc::new(RefCell::new(v)));
            }
            Decl::Const { name, init, ty, .. } => {
                if let Some(Type::Named(tn, _)) = ty {
                    if tn.starts_with("error_set:") {
                        return Ok(());
                    }
                }
                let v = self.eval(init)?;
                let key = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                self.globals.insert(key, Rc::new(RefCell::new(v)));
            }
            Decl::Namespace { name, decls, .. } => {
                let new_prefix = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                for inner in decls {
                    self.exec_decl_top_filter(inner, &new_prefix, pub_only)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ---------- 作用域 ----------

    pub(crate) fn push_scope(&mut self) {
        self.scopes.push(Scope::new());
    }

    pub(crate) fn pop_scope(&mut self, err_path: bool) -> Result<()> {
        // defer 同作用域捕获：先取走 defers（释放借用），在作用域**仍压栈**时执行
        // （defer 表达式可引用本作用域局部变量），再弹出作用域标记悬垂。
        let defers = std::mem::take(
            &mut self
                .scopes
                .last_mut()
                .expect("scope stack underflow")
                .defers,
        );
        let defer_err = self.run_defers(&defers, err_path);
        let scope = self.scopes.pop().expect("scope stack underflow");
        // E2.2 根提升：作用域退出时未 join/未 detach 的 Thread 提升到根回收队列
        // （程序结束才运行；无隐式阻塞）。须在 Debug 悬垂标记之前——标记会把 cell
        // 内容替换为 Dangling，提升需要读到原始 Class。
        self.promote_unfinished_threads(&scope);
        // M2.5/M4.7 Debug 悬垂标记：作用域退出 = 目标销毁（LIFO）→ 把被取过地址的
        // 目标 cell 内容标记为 Dangling（有指针持有的 cell 不释放、地址唯一——
        // 无地址碰撞误判；Release 关闭时不标记）
        if self.debug_dangling {
            for (name, cell) in &scope.vars {
                let _ = name;
                let addr = Rc::as_ptr(cell) as usize;
                if self.tracked.remove(&addr) {
                    *cell.borrow_mut() = Value::Dangling;
                }
            }
        }
        defer_err
    }

    pub(crate) fn run_defers(&mut self, defers: &[DeferEntry], err_path: bool) -> Result<()> {
        // LIFO（Q21：后声明先执行）
        let mut err = None;
        for entry in defers.iter().rev() {
            if entry.errdefer && !err_path {
                continue;
            }
            if let Err(e) = self.eval(&entry.expr) {
                err = Some(e);
            }
        }
        match err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// E2.2：作用域退出回收——未 join/未 detach 的 Thread 提升到根回收队列
    /// （程序结束时运行到完成，副作用发生；同 Rc 去重防重复入队）。
    /// 嵌套持有（Thread 存于数组/类字段等）不在此扫描范围——仅作用域直接绑定。
    pub(crate) fn promote_unfinished_threads(&mut self, scope: &Scope) {
        for (_name, cell) in &scope.vars {
            let v = cell.borrow();
            if let Value::Class(c) = &*v {
                let d = c.borrow();
                if d.name == "Thread" {
                    let done = matches!(d.fields.get("done"), Some(Value::Bool(true)));
                    let detached = matches!(d.fields.get("detached"), Some(Value::Bool(true)));
                    if !done
                        && !detached
                        && !self
                            .root_threads
                            .iter()
                            .any(|t| matches!(t, Value::Class(rc) if Rc::ptr_eq(rc, c)))
                    {
                        self.root_threads.push(Value::Class(c.clone()));
                    }
                }
            }
        }
    }

    /// E4 true-OMP：程序结束（main 返回 / 全部测试完成）时等待所有 OS 线程结束。
    /// 错误丢弃（副作用已发生；无隐式阻塞、不改变测试通过判定）。
    pub(crate) fn drain_root_threads(&mut self) {
        let pending = std::mem::take(&mut self.root_threads);
        for t in pending {
            let tid = self.get_thread_tid(&t);
            self.thread_join_impl(tid);
        }
        // 清理仍在 thread_handles 中的线程
        let tids: Vec<i64> = self.thread_handles.keys().copied().collect();
        for tid in tids {
            self.thread_join_impl(tid);
        }
    }

    /// 判定退出作用域是否处于"错误路径"（errdefer 触发条件）：
    /// 块执行返回真错误（非信号），或流携带/值是错误值（`return error.X`、
    /// `try` 传播的 `Flow::Return(err)` 信号、块/臂表达式值本身为错误）。
    pub(crate) fn is_err_path(r: &Result<Flow>) -> bool {
        match r {
            Err(e) => {
                if !e.is_signal() {
                    return true;
                }
                matches!(
                    &e.signal,
                    Some(Flow::Return(v)) | Some(Flow::Value(v)) if value_is_err(v)
                )
            }
            Ok(Flow::Return(v)) | Ok(Flow::Value(v)) => value_is_err(v),
            _ => false,
        }
    }

    pub(crate) fn bind(&mut self, name: &str, v: Value) -> Rc<RefCell<Value>> {
        let cell = Rc::new(RefCell::new(v));
        self.scopes
            .last_mut()
            .unwrap()
            .vars
            .insert(name.to_string(), cell.clone());
        cell
    }

    pub(crate) fn lookup(&self, name: &str) -> Option<Rc<RefCell<Value>>> {
        for s in self.scopes.iter().rev() {
            if let Some(v) = s.vars.get(name) {
                return Some(v.clone());
            }
        }
        self.globals.get(name).cloned()
    }
}
