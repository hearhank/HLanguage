//! IR 降级实现：AST → IR 指令的降级转换
//!
//! 定义：枚举：ErrPath
//! 定义：结构体：LowerCtx, LoopCtx, DeferRecord

use super::*;

pub fn lower(program: &Program) -> Result<IrModule, IrError> {
    let types = build_type_table(program);
    lower_with_types(program, types)
}

/// 使用外部提供的类型表降级程序（用于多文件合并时共享类型定义）。
/// 类型表应包含所有文件的类型定义，使跨文件类型引用（如 `using` 导入的命名空间类）
/// 在降级时能够解析。
pub fn lower_with_types(program: &Program, types: TypeTable) -> Result<IrModule, IrError> {
    let errors = crate::runtime::errorcodes::collect(program, 0);
    let funcs = collect_func_names(program);
    let mut globals = collect_globals(program);
    // Phase 7：隐式环境名（alloc/io/pi/Vec…）按全局处理——`io.print` 等限定名根标识符
    // 须经 `LoadGlobal` 解析（对齐 oracle interp.rs:1585-1595 的隐式环境注入）。
    for g in IMPLICIT_ENV {
        globals.insert((*g).to_string());
    }
    // E1.2 组 D：类型函数定义表（comptime-only，体降级跳过；类型应用点惰性具体化）
    let type_fns = collect_type_fns(program);
    // E1.2 组 D D5：comptime 值函数定义表（运行时调用点常量折叠，IR 无调用残留）
    let value_fns = collect_value_fns(program);
    let mut module = IrModule::default();
    // C3：文件级 import 展开表（bound → 完整限定名 / 模块前缀）——原生链接与 IR 调用名对齐
    let (import_syms, import_mods) = collect_imports(program);
    // 错误码表（名 → 码）：内建运行时错误值（io.fs 等）须与 `error.X` 字面量同码
    for e in errors.entries() {
        module.error_codes.insert(e.name.clone(), e.code);
    }
    // 枚举变体序（Phase 7）：`@intFromEnum`/`@enumFromInt` 运行时分派
    for (n, ei) in &types.enums {
        module.enum_variants.insert(n.clone(), ei.variants.clone());
    }
    // K1 无标签 union（ADR-0014）：字段声明表（扁平 + 全限定）→ 写路径字节重解释同步
    for (n, ui) in &types.unions {
        module.unions.insert(n.clone(), ui.fields.clone());
    }
    // [continuous] 类名集（扁平 + 全限定）：DeepCopy 指令运行时门
    for (n, ci) in &types.classes {
        if ci.continuous {
            module.continuous.insert(n.clone());
        }
    }
    // ADR-0027：类型→接口映射表（编译期接口分派）
    module.type_implements = collect_type_implements(program);
    for d in &program.decls {
        lower_decl(
            d,
            &mut module,
            &errors,
            &types,
            &funcs,
            &globals,
            &type_fns,
            &value_fns,
            &import_syms,
            &import_mods,
        )?;
    }
    // Phase 5：合成 `@__init__` 函数（声明序初始化 global/const；多文件合并 = 各模块
    // 自带 init，运行时按 funcs 序依次执行）。不登记 func_index（不可被用户调用）。
    if let Some(init) = lower_init_func(
        program,
        &errors,
        &types,
        &funcs,
        &globals,
        &type_fns,
        &value_fns,
        &mut module.closures,
        &import_syms,
        &import_mods,
    )? {
        module.funcs.push(init);
    }
    let mut ordered = globals_set_to_ordered(program);
    for g in IMPLICIT_ENV {
        if !ordered.iter().any(|x| x == g) {
            ordered.push((*g).to_string());
        }
    }
    module.globals = ordered;
    Ok(module)
}

/// 收集全局/常量名集合（扁平；错误集别名除外——类型级构造，非值全局）。
pub(crate) fn collect_globals(program: &Program) -> HashSet<String> {
    let mut set = HashSet::new();
    collect_globals_in(&program.decls, &mut set);
    set
}

/// ADR-0027：扫描所有类声明，收集类型→接口映射表
/// 例如 `class Vec<T> : ICollection<T> { ... }` → {"Vec": ["ICollection"]}
pub(crate) fn collect_type_implements(program: &Program) -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();
    collect_type_implements_in(&program.decls, &mut map);
    map
}

fn collect_type_implements_in(decls: &[Decl], map: &mut HashMap<String, Vec<String>>) {
    for d in decls {
        match d {
            Decl::Class { name, ifaces, .. } => {
                for iface in ifaces {
                    if let Type::Named(n, _) = iface.strip() {
                        map.entry(name.clone()).or_default().push(n.to_string());
                    }
                }
            }
            Decl::Namespace { decls, .. } => collect_type_implements_in(decls, map),
            _ => {}
        }
    }
}

/// C3：文件级 import 展开表——(符号选择 bound → 完整限定名, 整模块 bound → 包路径)。
/// `H.std` 根跳过（内建虚拟根，`io.print` 等走 CallBuiltin 路由，不展开）。
pub(crate) fn collect_imports(
    program: &Program,
) -> (HashMap<String, String>, HashMap<String, String>) {
    let mut syms = HashMap::new();
    let mut mods = HashMap::new();
    for d in &program.decls {
        if let Decl::Import {
            path,
            alias,
            select,
            ..
        } = d
        {
            if path.first().map_or(false, |p| p == "H") {
                continue;
            }
            let base = path.join(".");
            match select {
                Some(syms_sel) => {
                    for (sym, sym_alias) in syms_sel {
                        let bound = sym_alias.clone().unwrap_or_else(|| sym.clone());
                        syms.insert(bound, format!("{base}.{sym}"));
                    }
                }
                None => {
                    let bound = alias
                        .clone()
                        .unwrap_or_else(|| path.last().cloned().unwrap_or_default());
                    mods.insert(bound, base);
                }
            }
        }
    }
    (syms, mods)
}

/// 收集全局/常量名（声明序，供 `IrModule::globals` + `@__init__` 复用）。
pub(crate) fn globals_set_to_ordered(program: &Program) -> Vec<String> {
    let mut ordered = Vec::new();
    collect_globals_ordered(&program.decls, &mut ordered);
    ordered
}

pub(crate) fn collect_globals_in(decls: &[Decl], set: &mut HashSet<String>) {
    for d in decls {
        match d {
            Decl::Global { name, .. } => {
                set.insert(name.clone());
            }
            Decl::Const { name, ty, .. } => {
                // 错误集别名：`const X = error{...}` / `const X = A || B`——类型级构造
                if let Some(Type::Named(tn, _)) = ty {
                    if tn.starts_with("error_set:") {
                        continue;
                    }
                }
                set.insert(name.clone());
            }
            Decl::Namespace { decls: nested, .. } => collect_globals_in(nested, set),
            _ => {}
        }
    }
}

pub(crate) fn collect_globals_ordered(decls: &[Decl], ordered: &mut Vec<String>) {
    for d in decls {
        match d {
            Decl::Global { name, .. } => ordered.push(name.clone()),
            Decl::Const { name, ty, .. } => {
                if let Some(Type::Named(tn, _)) = ty {
                    if tn.starts_with("error_set:") {
                        continue;
                    }
                }
                ordered.push(name.clone());
            }
            Decl::Namespace { decls: nested, .. } => collect_globals_ordered(nested, ordered),
            _ => {}
        }
    }
}

/// 预收集全部函数名（扁平 + 限定 + `{Type}.{method}`），供未解析 Ident → FnRef、
/// 静态方法/namespace 调用 vs 实例方法调用的降级期判定（对齐 oracle 的
/// `funcs: HashMap<String, Vec<FnDef>>` 预建表）。
pub(crate) fn collect_func_names(program: &Program) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_fn_names(&program.decls, &mut names, &[]);
    names
}

pub(crate) fn collect_fn_names(decls: &[Decl], names: &mut HashSet<String>, path: &[String]) {
    for d in decls {
        match d {
            Decl::Fn { name, .. } => {
                names.insert(name.clone());
                if !path.is_empty() {
                    let mut q = path.join(".");
                    q.push('.');
                    q.push_str(name);
                    names.insert(q);
                }
            }
            Decl::Namespace {
                name,
                decls: nested,
                ..
            } => {
                let mut p = path.to_vec();
                p.push(name.clone());
                collect_fn_names(nested, names, &p);
            }
            Decl::Class { name, methods, .. } => {
                for m in methods {
                    let bare = format!("{name}.{}", m.name);
                    names.insert(bare.clone());
                    if !path.is_empty() {
                        let mut q = path.join(".");
                        q.push('.');
                        q.push_str(&bare);
                        names.insert(q);
                    }
                }
            }
            Decl::Struct { .. } => {}
            _ => {}
        }
    }
}

/// E1.2 组 D：收集类型函数定义（name → params+body），供 NamedLit 惰性具体化。
/// 顶层 + namespace 内均收集；键 = 扁平名 + 限定名（对齐 `collect_fn_names`）。
/// 类型函数体本身由降级**跳过**（comptime-only，运行时不执行），仅在类型应用点
/// （`Pair<i32>`）经 `comptime::instantiate` 编译期求值。
pub(crate) fn collect_type_fns(program: &Program) -> HashMap<String, (Vec<Param>, Block)> {
    let mut map = HashMap::new();
    collect_type_fns_in(&program.decls, &mut map, &[]);
    map
}

pub(crate) fn collect_type_fns_in(
    decls: &[Decl],
    map: &mut HashMap<String, (Vec<Param>, Block)>,
    path: &[String],
) {
    for d in decls {
        match d {
            Decl::Fn {
                name,
                params,
                body,
                ret,
                ..
            } => {
                if comptime::is_type_fn(params, ret) {
                    let def = (params.clone(), body.clone());
                    map.insert(name.clone(), def.clone());
                    if !path.is_empty() {
                        let mut q = path.join(".");
                        q.push('.');
                        q.push_str(name);
                        map.insert(q, def);
                    }
                }
            }
            Decl::Namespace {
                name,
                decls: nested,
                ..
            } => {
                let mut p = path.to_vec();
                p.push(name.clone());
                collect_type_fns_in(nested, map, &p);
            }
            _ => {}
        }
    }
}

/// E1.2 组 D D5：收集 comptime 值函数定义（name → params+body），供运行时调用点折叠。
/// 顶层 + namespace 内均收集；键 = 扁平名 + 限定名（对齐 `collect_type_fns`）。
/// 与类型函数不同（体降级跳过），值函数体为普通常量表达式，**运行时不执行**——
/// 调用点（`var n = array_len(i32);`）经常量求值折叠为 `IrConst`，IR 中无调用残留。
pub(crate) fn collect_value_fns(program: &Program) -> HashMap<String, (Vec<Param>, Block)> {
    let mut map = HashMap::new();
    collect_value_fns_in(&program.decls, &mut map, &[]);
    map
}

pub(crate) fn collect_value_fns_in(
    decls: &[Decl],
    map: &mut HashMap<String, (Vec<Param>, Block)>,
    path: &[String],
) {
    for d in decls {
        match d {
            Decl::Fn {
                name,
                params,
                body,
                ret,
                ..
            } => {
                if comptime::is_comptime_value_fn(params, ret) {
                    let def = (params.clone(), body.clone());
                    map.insert(name.clone(), def.clone());
                    if !path.is_empty() {
                        let mut q = path.join(".");
                        q.push('.');
                        q.push_str(name);
                        map.insert(q, def);
                    }
                }
            }
            Decl::Namespace {
                name,
                decls: nested,
                ..
            } => {
                let mut p = path.to_vec();
                p.push(name.clone());
                collect_value_fns_in(nested, map, &p);
            }
            _ => {}
        }
    }
}

/// 构造「原生/IR 后端暂不支持」的降级错误（阶段外特性 → 硬错误而非静默丢弃）。
pub(crate) fn unsupported_ir_err(what: &str, span: &Span) -> IrError {
    IrError::msg(
        "Unsupported",
        format!(
            "原生/IR 后端暂不支持{what}（第 {} 行第 {} 列）——请用默认 tree-walking 模式 `hc run <file>`",
            span.line, span.col
        ),
    )
}

pub(crate) fn lower_decl(
    d: &Decl,
    module: &mut IrModule,
    errors: &ErrorCodeTable,
    types: &TypeTable,
    funcs: &HashSet<String>,
    globals: &HashSet<String>,
    type_fns: &HashMap<String, (Vec<Param>, Block)>,
    value_fns: &HashMap<String, (Vec<Param>, Block)>,
    import_syms: &HashMap<String, String>,
    import_mods: &HashMap<String, String>,
) -> Result<(), IrError> {
    match d {
        Decl::Fn {
            name,
            params,
            body,
            is_test,
            ret,
            exported,
            is_extern,
            extension_of,
            ..
        } => {
            // E1.2 组 D：类型函数（返回 `type`）= comptime-only，跳过体降级
            // （体含 `struct { ... }` 类型值，运行时后端不可求值；类型应用点
            // 经 `comptime::instantiate` 编译期求值）。函数名已在 funcs 集合，
            // 调用位判定不受影响。
            if comptime::is_type_fn(params, ret) {
                return Ok(());
            }
            // A1（ADR-0020）：`extern fn`——纯声明（无 body，链接期解析外部 C 符号）。
            // 跳过体降级，仅注册函数签名
            if *is_extern {
                let param_ty: Vec<Type> = params.iter().map(|p| p.ty.clone()).collect();
                let func = IrFunc {
                    name: name.clone(),
                    params: (0..params.len()).collect(),
                    param_ty,
                    ret_ty: ret
                        .clone()
                        .unwrap_or(Type::Named("void".to_string(), vec![])),
                    param_defaults: vec![],
                    defaults: vec![],
                    n_slots: params.len(),
                    body: vec![],
                    is_test: *is_test,
                    exported: false,
                    is_extern: true,
                };
                register_func(module, name, func);
                return Ok(());
            }
            let func = lower_func(
                name,
                params,
                body,
                *is_test,
                *exported,
                ret,
                errors,
                types,
                funcs,
                globals,
                type_fns,
                value_fns,
                &mut module.closures,
                import_syms,
                import_mods,
            )?;
            // 登记扁平名（直接调用用）
            let idx = module.funcs.len();
            module.funcs.push(func);
            module.func_index.entry(name.clone()).or_default().push(idx);
            // Q8：扩展方法 —— 同时登记为 {TypeName}.{method} 供 CallMethod 运行时分派
            if let Some(ext_ty) = extension_of {
                let qname = format!("{ext_ty}.{name}");
                module.func_index.entry(qname).or_default().push(idx);
            }
        }
        Decl::Namespace { name, decls, .. } => {
            // namespace 内函数：扁平名 + 限定名双注册（与运行时/语义一致）；
            // 多级 namespace（io.net.connect）注册全限定名
            let mut inner: Vec<(String, String, IrFunc)> = Vec::new();
            collect_ns_funcs(
                decls,
                &[name.clone()],
                &mut inner,
                errors,
                types,
                funcs,
                globals,
                type_fns,
                value_fns,
                &mut module.closures,
                import_syms,
                import_mods,
            )?;
            for (flat, qn, func) in inner {
                let idx = module.funcs.len();
                module.funcs.push(func);
                // 扁平名（using 导入后直接调用）：先到先得
                module.func_index.entry(flat).or_default().push(idx);
                // 限定名（Math.square / io.net.connect）
                module.func_index.entry(qn).or_default().push(idx);
            }
        }
        // 全局/常量声明：由合成 `@__init__` 函数处理（Phase 5）——此处跳过，
        // 启动初始化语义在 IrRuntime::init 中落地
        Decl::Global { .. } | Decl::Const { .. } => {}
        // 类型级声明（class/enum/interface/using/script）：无顶层运行时代码；
        // class 方法登记为 `{Type}.{method}`（对齐 oracle interp.rs:522-535）——IIterable
        // 用户类型的 `next()` 经此查找。方法体降级失败 → 跳过登记（调用点 NoFunction
        // 硬错误，不使整个程序降级失败——方法与调用分属 Phase 3/4 边界）。
        Decl::Class { name, methods, .. } => {
            for m in methods {
                let fname = format!("{name}.{}", m.name);
                if let Ok(func) = lower_func(
                    &fname,
                    &m.params,
                    &m.body,
                    false,
                    false,
                    &m.ret,
                    errors,
                    types,
                    funcs,
                    globals,
                    type_fns,
                    value_fns,
                    &mut module.closures,
                    import_syms,
                    import_mods,
                ) {
                    register_func(module, &fname, func);
                }
            }
        }
        Decl::Enum { .. }
        | Decl::Struct { .. }
        | Decl::Union { .. }
        | Decl::Interface { .. }
        | Decl::Using { .. }
        | Decl::Import { .. }
        | Decl::Comptime { .. }
        | Decl::Include { .. } => {}
    }
    Ok(())
}

/// 递归收集 namespace 内非测试函数：(扁平名, 全限定名, IR 函数)
pub(crate) fn collect_ns_funcs(
    decls: &[Decl],
    path: &[String],
    out: &mut Vec<(String, String, IrFunc)>,
    errors: &ErrorCodeTable,
    types: &TypeTable,
    funcs: &HashSet<String>,
    globals: &HashSet<String>,
    type_fns: &HashMap<String, (Vec<Param>, Block)>,
    value_fns: &HashMap<String, (Vec<Param>, Block)>,
    closures: &mut Vec<IrFunc>,
    import_syms: &HashMap<String, String>,
    import_mods: &HashMap<String, String>,
) -> Result<(), IrError> {
    for d in decls {
        match d {
            Decl::Fn {
                name,
                params,
                body,
                is_test,
                ret,
                ..
            } if !*is_test => {
                // E1.2 组 D：类型函数跳过体降级（comptime-only）
                if comptime::is_type_fn(params, ret) {
                    continue;
                }
                let mut qn = path.to_vec();
                qn.push(name.clone());
                let func = lower_func(
                    name,
                    params,
                    body,
                    false,
                    false,
                    ret,
                    errors,
                    types,
                    funcs,
                    globals,
                    type_fns,
                    value_fns,
                    closures,
                    import_syms,
                    import_mods,
                )?;
                out.push((name.clone(), qn.join("."), func));
            }
            Decl::Namespace {
                name,
                decls: nested,
                ..
            } => {
                let mut p = path.to_vec();
                p.push(name.clone());
                collect_ns_funcs(
                    nested,
                    &p,
                    out,
                    errors,
                    types,
                    funcs,
                    globals,
                    type_fns,
                    value_fns,
                    closures,
                    import_syms,
                    import_mods,
                )?;
            }
            // namespace 内 global/const：扁平登记（对齐 oracle `exec_decl_top`），由 `@__init__` 处理
            Decl::Global { .. } | Decl::Const { .. } => {}
            // 类型级声明在 namespace 内：安全忽略
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn register_func(module: &mut IrModule, name: &str, func: IrFunc) {
    let idx = module.funcs.len();
    module.funcs.push(func);
    // 重载/可选参数：同名多候选按声明序追加（对齐 oracle funcs: HashMap<String, Vec<FnDef>>）
    module
        .func_index
        .entry(name.to_string())
        .or_default()
        .push(idx);
}

pub(crate) fn lower_func(
    name: &str,
    params: &[Param],
    body: &Block,
    is_test: bool,
    exported: bool,
    ret: &Option<Type>,
    errors: &ErrorCodeTable,
    types: &TypeTable,
    funcs: &HashSet<String>,
    globals: &HashSet<String>,
    type_fns: &HashMap<String, (Vec<Param>, Block)>,
    value_fns: &HashMap<String, (Vec<Param>, Block)>,
    closures: &mut Vec<IrFunc>,
    import_syms: &HashMap<String, String>,
    import_mods: &HashMap<String, String>,
) -> Result<IrFunc, IrError> {
    let mut ctx = LowerCtx::new(
        errors.clone(),
        types.clone(),
        funcs,
        globals,
        type_fns,
        value_fns,
        closures,
        import_syms.clone(),
        import_mods.clone(),
    );
    ctx.push_scope();
    // 参数槽（声明序，从 0 开始）
    let param_slots: Vec<usize> = params.iter().map(|_| ctx.alloc_slot()).collect();
    // 局部变量槽（变量名 → 槽）
    for (p, slot) in params.iter().zip(param_slots.iter()) {
        ctx.bind(&p.name, *slot);
    }
    for stmt in &body.stmts {
        ctx.lower_stmt(stmt);
    }
    ctx.pop_scope();
    // 子集外特性 → 硬错误（不静默丢弃语句；降级已推进完毕以保持槽号连续）
    if let Some(e) = ctx.err {
        return Err(e);
    }
    // 隐式末尾 return void
    ctx.insts.push(IrInst::ReturnVoid);
    let n_slots = ctx.next_slot;
    // 重载/可选参数元数据（Phase 4）：类型 + 尾部默认常量（ADR-0009）
    let param_ty: Vec<Type> = params.iter().map(|p| p.ty.clone()).collect();
    let param_defaults: Vec<bool> = params.iter().map(|p| p.default.is_some()).collect();
    let defaults: Vec<Option<IrConst>> = params
        .iter()
        .map(|p| {
            p.default
                .as_ref()
                .and_then(|d| lower_default_const(d, errors))
        })
        .collect();
    Ok(IrFunc {
        name: name.to_string(),
        params: param_slots,
        param_ty,
        ret_ty: ret
            .clone()
            .unwrap_or(Type::Named("void".to_string(), vec![])),
        param_defaults,
        defaults,
        n_slots,
        body: ctx.insts,
        is_test,
        exported,
        is_extern: false,
    })
}

/// Phase 5：合成 `@__init__` 函数——声明序初始化全部 global/const（`StoreGlobal`）。
/// 错误集别名（`const X = error{...}` / `A || B`）为类型级构造，跳过。
/// 返回 None 表示无值全局（无需启动初始化）。
pub(crate) fn lower_init_func(
    program: &Program,
    errors: &ErrorCodeTable,
    types: &TypeTable,
    funcs: &HashSet<String>,
    globals: &HashSet<String>,
    type_fns: &HashMap<String, (Vec<Param>, Block)>,
    value_fns: &HashMap<String, (Vec<Param>, Block)>,
    closures: &mut Vec<IrFunc>,
    import_syms: &HashMap<String, String>,
    import_mods: &HashMap<String, String>,
) -> Result<Option<IrFunc>, IrError> {
    if globals.is_empty() {
        return Ok(None);
    }
    let mut ctx = LowerCtx::new(
        errors.clone(),
        types.clone(),
        funcs,
        globals,
        type_fns,
        value_fns,
        closures,
        import_syms.clone(),
        import_mods.clone(),
    );
    ctx.push_scope();
    for d in &program.decls {
        lower_global_decl(d, &mut ctx)?;
    }
    ctx.pop_scope();
    if let Some(e) = ctx.err {
        return Err(e);
    }
    ctx.insts.push(IrInst::ReturnVoid);
    let n_slots = ctx.next_slot;
    Ok(Some(IrFunc {
        name: "@__init__".to_string(),
        params: vec![],
        param_ty: vec![],
        ret_ty: Type::Named("void".to_string(), vec![]),
        param_defaults: vec![],
        defaults: vec![],
        n_slots,
        body: ctx.insts,
        is_test: false,
        exported: false,
        is_extern: false,
    }))
}

/// 递归降级 global/const 声明初始化（namespace 内扁平化，对齐 oracle `exec_decl_top`）。
pub(crate) fn lower_global_decl(d: &Decl, ctx: &mut LowerCtx) -> Result<(), IrError> {
    match d {
        Decl::Global { name, init, .. } => {
            let t = match init {
                Some(e) => ctx.lower_expr(e),
                None => {
                    let t = ctx.alloc_slot();
                    ctx.push(IrInst::Const {
                        temp: t,
                        val: IrConst::Void,
                    });
                    t
                }
            };
            ctx.push(IrInst::StoreGlobal {
                name: name.clone(),
                value: t,
            });
        }
        Decl::Const { name, init, ty, .. } => {
            // 错误集别名跳过（类型级）
            if let Some(Type::Named(tn, _)) = ty {
                if tn.starts_with("error_set:") {
                    return Ok(());
                }
            }
            let t = ctx.lower_expr(init);
            ctx.push(IrInst::StoreGlobal {
                name: name.clone(),
                value: t,
            });
        }
        Decl::Namespace { decls, .. } => {
            for inner in decls {
                lower_global_decl(inner, ctx)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// 可选参数默认值折叠为编译期常量（ADR-0009：可选参数 = 尾部 + 编译期常量默认值）。
/// 字面量/枚举常量/错误字面量 → `IrConst`；其余（依赖参数/非常量表达式）→ None
/// （运行时按「未提供」处理——pick_func 默认回退依赖 param_defaults，padding 依赖此常量）。
pub(crate) fn lower_default_const(e: &Expr, errors: &ErrorCodeTable) -> Option<IrConst> {
    match e {
        Expr::IntLit { text, .. } => Some(IrConst::Int(parse_int_lit(text))),
        Expr::FloatLit { text, .. } => Some(IrConst::Float(text.parse().unwrap_or(0.0))),
        Expr::BoolLit(b, _) => Some(IrConst::Bool(*b)),
        Expr::StrLit { value, .. } => Some(IrConst::Str(value.clone())),
        Expr::CharLit(c, _) => Some(IrConst::Int(*c as i128)),
        Expr::NullLit(_) => Some(IrConst::Null),
        Expr::VoidLit(_) => Some(IrConst::Void),
        Expr::ErrorLit(name, _) => Some(IrConst::Err {
            name: name.clone(),
            code: errors.code_of(name).unwrap_or(0),
        }),
        Expr::ContainerLit { .. } => None,
        _ => None,
    }
}

struct LowerCtx<'a> {
    /// 作用域栈：名字 → 槽（词法作用域，块退出恢复外层绑定——对齐解释器作用域）
    scopes: Vec<HashMap<String, usize>>,
    next_slot: usize,
    insts: Vec<IrInst>,
    next_label: usize,
    /// 编译期错误码表（error.Name → 码，M2.6；Err 常量携带码）
    errors: ErrorCodeTable,
    /// 类型元数据（Phase 2：NamedLit/Dot 判型 class vs enum vs namespace）
    types: TypeTable,
    /// 已知函数名集合（Phase 4）：未解析 Ident → 函数引用（FnRef）/ 静态方法调用判定
    funcs: &'a HashSet<String>,
    /// E1.2 组 D：类型函数定义表（name → params+body，comptime-only）。`Pair<i32>`
    /// 类型应用点惰性具体化：instantiate → 具体化 Class 登记进 `self.types`。
    type_fns: &'a HashMap<String, (Vec<Param>, Block)>,
    /// E1.2 组 D D5：comptime 值函数定义表（name → params+body）。运行时调用点
    /// （`var n = array_len(i32);`）常量折叠为 `IrConst`，IR 中无调用残留。
    value_fns: &'a HashMap<String, (Vec<Param>, Block)>,
    /// E1.2 组 D D3：具体化登记期进行中的具体化名集合（`Pair<@i32>` 键）。
    /// 自/互递归类型函数（`LinkedList(T) { next: ?LinkedList(T) }`）在登记期重入时
    /// 命中即返回键本身（叶），防止无限实例化。
    instantiating: Vec<String>,
    /// C3：文件级 import 符号选择展开表（bound 名 → 完整限定名 `jsonlib.parse`）
    import_syms: HashMap<String, String>,
    /// C3：整模块 import 前缀展开表（bound 模块名 → 包路径 `pkg.mod`）
    import_mods: HashMap<String, String>,
    /// 已知全局/常量名集合（Phase 5）：未解析 Ident → LoadGlobal；赋值目标 → StoreGlobal
    globals: &'a HashSet<String>,
    /// 循环栈（Phase 3）：无标签 break/continue 定位（对齐 oracle 单级跳出；标签 → Phase 6）
    loops: Vec<LoopCtx>,
    /// 已登记 defers（Phase 6，按登记序累积；**不弹**——作用域标记划分发射范围）。
    /// 退出点（作用域自然结束 / return / break / continue / try 错误返回）按 LIFO
    /// 发射内联体（守卫 JumpIfNotDefer + PopDefer）。作用域弹栈时截断到标记。
    defers: Vec<DeferRecord>,
    /// 与 `scopes` 平行的 defer 标记：进入作用域时的 `defers.len()`。`pop_scope` 发射
    /// 从当前长度下到标记的 defers（仅该作用域登记的部分），再截断——对齐 oracle
    /// 每作用域独立 defer 列表、弹栈即运行。
    defer_markers: Vec<usize>,
    /// defer 体缓冲：非 None 时 `push`/`label` 路由到缓冲（defer 体单独降级，
    /// 退出点整体发射）。defer 语句降级完即复位为外层缓冲。
    pending: Option<Vec<IrInst>>,
    /// 下一个 defer id（函数级单调递增；每个 defer 语句唯一）。
    next_defer_id: usize,
    /// 首个子集外特性错误（降级失败信号；降级继续推进以收集更多槽号，但最终报错）
    err: Option<IrError>,
    /// 闭包函数共享缓冲（Phase 4，模块级）：`MakeClosure.func` = 追加前长度
    /// （同一 LowerCtx 内嵌套闭包也追加至此 → 全局索引稳定，无需事后重定位）
    closures: &'a mut Vec<IrFunc>,
}

impl<'a> LowerCtx<'a> {
    pub(crate) fn new(
        errors: ErrorCodeTable,
        types: TypeTable,
        funcs: &'a HashSet<String>,
        globals: &'a HashSet<String>,
        type_fns: &'a HashMap<String, (Vec<Param>, Block)>,
        value_fns: &'a HashMap<String, (Vec<Param>, Block)>,
        closures: &'a mut Vec<IrFunc>,
        import_syms: HashMap<String, String>,
        import_mods: HashMap<String, String>,
    ) -> Self {
        LowerCtx {
            scopes: Vec::new(),
            next_slot: 0,
            insts: Vec::new(),
            next_label: 0,
            errors,
            types,
            funcs,
            type_fns,
            value_fns,
            instantiating: Vec::new(),
            import_syms,
            import_mods,
            globals,
            loops: Vec::new(),
            defers: Vec::new(),
            defer_markers: Vec::new(),
            pending: None,
            next_defer_id: 0,
            err: None,
            closures,
        }
    }
}

/// 循环上下文（Phase 3）：break 目标 + continue 目标；
/// Phase 6 增补：循环标签 + 进入时 defer 深度（退出该循环须排空其体内 defers）。
struct LoopCtx {
    break_label: usize,
    continue_label: usize,
    /// 循环标签（`:label while` / `:label for`），供 `break :label` / `continue :label` 定位
    label: Option<String>,
    /// 进入循环时已登记 defers 数：break/continue 排空 [depth..len)（含嵌套作用域，
    /// 但不含循环外层的 defers——外层退出点另行处理）。
    defer_depth_at_entry: usize,
}

/// 一个 defer/errdefer 语句的编译期记录：id（PushDefer/PopDefer/守卫共用）+ 内联体 + 是否 errdefer。
/// 体已确保无控制流指令（带 label 的跳转会因重复发射而冲突——降级期硬错误）。
#[derive(Clone)]
struct DeferRecord {
    id: usize,
    body: Vec<IrInst>,
    errdefer: bool,
}

/// 退出点的 errdefer 策略（对齐 oracle `run_defers(err_path)`）：
/// - `Never`：正常路径（作用域自然结束 / break / continue）——errdefer 不运行，裸 PopDefer 清理。
/// - `Always`：错误路径（`try` 错误返回）——全部 defers（含 errdefer）运行。
/// - `Value(t)`：`return e` 按运行期值判定——错误值走 `Always` 分支，否则 `Never`。
#[derive(Clone, Copy)]
enum ErrPath {
    Never,
    Always,
    Value(usize),
}

impl<'a> LowerCtx<'a> {
    pub(crate) fn alloc_slot(&mut self) -> usize {
        let s = self.next_slot;
        self.next_slot += 1;
        s
    }
    pub(crate) fn new_label(&mut self) -> usize {
        let l = self.next_label;
        self.next_label += 1;
        l
    }
    pub(crate) fn push(&mut self, inst: IrInst) {
        if let Some(buf) = &mut self.pending {
            buf.push(inst);
        } else {
            self.insts.push(inst);
        }
    }
    pub(crate) fn label(&mut self, id: usize) {
        self.push(IrInst::Label { id });
    }
    pub(crate) fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
        self.defer_markers.push(self.defers.len());
    }
    /// 弹作用域：先发射本作用域 defers（正常路径，仅非 errdefer；守卫 + 内联体），
    /// 截断到进入时标记，再弹作用域。对齐 oracle `pop_scope`——先跑 defers 再弹
    /// （同作用域局部变量仍可解析）。
    pub(crate) fn pop_scope(&mut self) {
        let marker = self
            .defer_markers
            .pop()
            .expect("defer marker underflow (push_scope/pop_scope 不配对)");
        self.emit_defers(marker, ErrPath::Never);
        self.defers.truncate(marker);
        self.scopes.pop();
    }
    /// 退出点发射 defers（LIFO：从最新登记下到 `depth`；`depth` 以下归外层退出点）。
    /// 守卫（JumpIfNotDefer）跳过未登记/已运行路径——分支 DAG 下同一退出点代码
    /// 可被多条路径到达，运行时活跃计数判定「本路径是否待运行」。
    pub(crate) fn emit_defers(&mut self, depth: usize, err_path: ErrPath) {
        let n = self.defers.len();
        match err_path {
            ErrPath::Never => {
                for i in (depth..n).rev() {
                    let rec = self.defers[i].clone();
                    if rec.errdefer {
                        // 正常路径：errdefer 不运行，仅清理活跃计数（防跨路径泄漏）
                        self.push(IrInst::PopDefer { id: rec.id });
                    } else {
                        self.emit_defer_guarded(&rec);
                    }
                }
            }
            ErrPath::Always => {
                for i in (depth..n).rev() {
                    let rec = self.defers[i].clone();
                    self.emit_defer_guarded(&rec);
                }
            }
            ErrPath::Value(v) => {
                // `return e`：按运行期值分派——错误 → 全 defers；否则仅非 errdefer
                let l_err = self.new_label();
                let l_done = self.new_label();
                self.push(IrInst::JumpIfErr {
                    temp: v,
                    label: l_err,
                });
                for i in (depth..n).rev() {
                    let rec = self.defers[i].clone();
                    if rec.errdefer {
                        self.push(IrInst::PopDefer { id: rec.id });
                    } else {
                        self.emit_defer_guarded(&rec);
                    }
                }
                self.push(IrInst::Jump { label: l_done });
                self.label(l_err);
                for i in (depth..n).rev() {
                    let rec = self.defers[i].clone();
                    self.emit_defer_guarded(&rec);
                }
                self.label(l_done);
            }
        }
    }
    /// 单条 defer 守卫 + 内联体 + 排空。体允许含控制流（如 `defer try f()`），
    /// 续块标签确保 PopDefer 在 LLVM 后端始终位于合法基本块中。
    /// 体中的标签（Label/JumpIfErr/Jump 等）在每次发射时重新分配唯一 ID，
    /// 避免同一 DeferRecord 在多个退出点发射时产生重复标签。
    pub(crate) fn emit_defer_guarded(&mut self, rec: &DeferRecord) {
        let l_skip = self.new_label();
        let l_cont = self.new_label();
        self.push(IrInst::JumpIfNotDefer {
            id: rec.id,
            label: l_skip,
        });
        // 重映射体中的标签（Label/JumpIfErr/Jump 等），使每次发射获得唯一标签 ID
        let remapped = self.remap_body_labels(&rec.body);
        for inst in &remapped {
            self.push(inst.clone());
        }
        // 续块标签：体可能含 Return 终止了当前块，PopDefer 需在合法基本块中
        self.label(l_cont);
        self.push(IrInst::PopDefer { id: rec.id });
        self.label(l_skip);
    }

    /// 重映射指令序列中的标签（Label 定义 + Jump/JumpIfErr/JumpIfNotDefer 等引用），
    /// 使每次发射获得唯一标签 ID。体中的 Label 指令使用 new_label() 分配新 ID，
    /// 所有引用旧标签的指令同步更新。
    fn remap_body_labels(&mut self, body: &[IrInst]) -> Vec<IrInst> {
        use std::collections::HashMap;
        // 收集体中的 Label 定义，分配新 ID
        let mut label_map: HashMap<usize, usize> = HashMap::new();
        for inst in body {
            if let IrInst::Label { id } = inst {
                label_map.entry(*id).or_insert_with(|| self.new_label());
            }
        }
        if label_map.is_empty() {
            return body.to_vec();
        }
        // 重映射每条指令中的标签引用
        body.iter()
            .map(|inst| self.map_inst_labels(inst, &label_map))
            .collect()
    }

    /// 重映射单条指令中的标签引用（Label 定义 + 跳转目标）。
    fn map_inst_labels(&self, inst: &IrInst, map: &HashMap<usize, usize>) -> IrInst {
        macro_rules! remap {
            ($id:expr) => {{
                map.get(&$id).copied().unwrap_or($id)
            }};
        }
        match inst {
            IrInst::Label { id } => IrInst::Label { id: remap!(*id) },
            IrInst::Jump { label } => IrInst::Jump {
                label: remap!(*label),
            },
            IrInst::JumpIf { temp, label } => IrInst::JumpIf {
                temp: *temp,
                label: remap!(*label),
            },
            IrInst::JumpIfNot { temp, label } => IrInst::JumpIfNot {
                temp: *temp,
                label: remap!(*label),
            },
            IrInst::JumpIfNull { temp, label } => IrInst::JumpIfNull {
                temp: *temp,
                label: remap!(*label),
            },
            IrInst::JumpIfErr { temp, label } => IrInst::JumpIfErr {
                temp: *temp,
                label: remap!(*label),
            },
            IrInst::JumpIfNotDefer { id, label } => IrInst::JumpIfNotDefer {
                id: *id,
                label: remap!(*label),
            },
            // 不含标签引用的指令，直接克隆
            _ => inst.clone(),
        }
    }
    /// 当前作用域绑定（遮蔽时分配新槽，旧绑定保留在外层）
    pub(crate) fn bind(&mut self, name: &str, slot: usize) {
        self.scopes
            .last_mut()
            .expect("bind outside any scope")
            .insert(name.to_string(), slot);
    }
    pub(crate) fn resolve(&self, name: &str) -> Option<usize> {
        self.scopes.iter().rev().find_map(|m| m.get(name).copied())
    }
    /// 记录「原生/IR 后端不支持」的硬错误（首个生效，避免报错刷屏）
    pub(crate) fn fail(&mut self, what: &str, span: &Span) {
        if self.err.is_none() {
            self.err = Some(unsupported_ir_err(what, span));
        }
    }
    /// 子集外表达式：记录硬错误 + 返回 void 占位（保持槽号连续）
    pub(crate) fn fail_void(&mut self, t: usize, what: &str, span: &Span) {
        self.fail(what, span);
        self.push(IrInst::Const {
            temp: t,
            val: IrConst::Void,
        });
    }
    /// 块语句序列（推/弹作用域）；空块安全
    pub(crate) fn lower_block(&mut self, b: &Block) {
        self.push_scope();
        for stmt in &b.stmts {
            self.lower_stmt(stmt);
        }
        self.pop_scope();
    }

    /// 表达式 → 临时槽号
    pub(crate) fn lower_expr(&mut self, e: &Expr) -> usize {
        let t = self.alloc_slot();
        match e {
            Expr::IntLit { text, .. } => {
                let v = parse_int_lit(text);
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Int(v),
                });
            }
            Expr::FloatLit { text, .. } => {
                let v: f64 = text.parse().unwrap_or(0.0);
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Float(v),
                });
            }
            Expr::BoolLit(b, _) => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Bool(*b),
                });
            }
            Expr::StrLit { value, .. } => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Str(value.clone()),
                });
            }
            Expr::CharLit(c, _) => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Int(*c as i128),
                });
            }
            Expr::NullLit(_) => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Null,
                });
            }
            Expr::VoidLit(_) => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Void,
                });
            }
            Expr::ErrorLit(name, _) => {
                let code = self.errors.code_of(name).unwrap_or(0);
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Err {
                        name: name.clone(),
                        code,
                    },
                });
            }
            Expr::Ident(name, span) => match self.resolve(name) {
                Some(slot) => self.push(IrInst::Load { temp: t, slot }),
                // 函数名作为值（FnRef：apply(square, 5) / var f = square）——对齐 oracle
                // interp.rs:1530-1535
                None if self.funcs.contains(name) => {
                    self.push(IrInst::FnRef {
                        temp: t,
                        name: name.clone(),
                    });
                }
                // 全局/常量引用（Phase 5）：`LoadGlobal`——cell 由 IrRuntime::init 预分配
                None if self.globals.contains(name) => {
                    self.push(IrInst::LoadGlobal {
                        temp: t,
                        name: name.clone(),
                    });
                }
                None => self.fail_void(t, "未知标识符", span),
            },
            Expr::Binary(op, l, r, _span) => {
                let a = self.lower_expr(l);
                match op {
                    // 短路 and/or（与运行时 eval_binary 一致）
                    BinOp::And => {
                        let l_false = self.new_label();
                        let done = self.new_label();
                        self.push(IrInst::JumpIfNot {
                            temp: a,
                            label: l_false,
                        });
                        let b = self.lower_expr(r);
                        self.push(IrInst::Load { temp: t, slot: b });
                        self.push(IrInst::Jump { label: done });
                        self.label(l_false);
                        self.push(IrInst::Const {
                            temp: t,
                            val: IrConst::Bool(false),
                        });
                        self.label(done);
                    }
                    BinOp::Or => {
                        let l_true = self.new_label();
                        let done = self.new_label();
                        self.push(IrInst::JumpIf {
                            temp: a,
                            label: l_true,
                        });
                        let b = self.lower_expr(r);
                        self.push(IrInst::Load { temp: t, slot: b });
                        self.push(IrInst::Jump { label: done });
                        self.label(l_true);
                        self.push(IrInst::Const {
                            temp: t,
                            val: IrConst::Bool(true),
                        });
                        self.label(done);
                    }
                    // 区间糖：`[lo, hi)` 整数区间数组（对齐 oracle `BinOp::Range`）
                    BinOp::Range => {
                        let b = self.lower_expr(r);
                        self.push(IrInst::MakeRange {
                            temp: t,
                            lo: a,
                            hi: b,
                        });
                    }
                    _ => {
                        let b = self.lower_expr(r);
                        self.push(IrInst::Bin {
                            op: to_ir_binop(*op),
                            temp: t,
                            a,
                            b,
                        });
                    }
                }
            }
            Expr::Unary(op, inner, _) => {
                let a = self.lower_expr(inner);
                let un = match op {
                    UnaryOp::Neg => IrUnOp::Neg,
                    UnaryOp::Not => IrUnOp::Not,
                    UnaryOp::BitNot => IrUnOp::BitNot,
                };
                self.push(IrInst::Un { op: un, temp: t, a });
            }
            Expr::Try(inner, _) => {
                // try：错误值 → 从当前函数返回（值通道）。错误路径为运行期「返回错误」，
                // errdefer 须触发（对齐 oracle `is_err_path(Err(signal(Flow::Return(err))))`）——
                // 故用 ErrPath::Always 排空函数级 defers（含 errdefer）。
                let a = self.lower_expr(inner);
                let l_ret = self.new_label();
                let done = self.new_label();
                self.push(IrInst::JumpIfErr {
                    temp: a,
                    label: l_ret,
                });
                self.push(IrInst::Load { temp: t, slot: a });
                self.push(IrInst::Jump { label: done });
                self.label(l_ret);
                self.emit_defers(0, ErrPath::Always);
                self.push(IrInst::Return { temp: a });
                self.label(done);
            }
            Expr::Await(inner, _) => {
                // 组 E E2 子集边界：IR 无 Future 延迟任务抽象，async fn 调用降级为同步
                // 执行（lower 普通 Call），await 透传内层值——纯函数下与 interp lazy 语义
                // 结果一致（consistency e2_async_await_consistent）；副作用时序/取消为
                // interp 特有（E4 原生异步落地后对齐）。interp 侧见 future_run。
                let a = self.lower_expr(inner);
                self.push(IrInst::Load { temp: t, slot: a });
            }
            Expr::Catch(inner, kind, _) => {
                // catch：错误值 → 处理分支；结果统一到目标槽
                let a = self.lower_expr(inner);
                let l_catch = self.new_label();
                let done = self.new_label();
                let res_slot = self.alloc_slot();
                self.push(IrInst::JumpIfErr {
                    temp: a,
                    label: l_catch,
                });
                self.push(IrInst::Store {
                    slot: res_slot,
                    temp: a,
                });
                self.push(IrInst::Jump { label: done });
                self.label(l_catch);
                match kind.as_ref() {
                    CatchKind::Default(d) => {
                        let h = self.lower_expr(d);
                        self.push(IrInst::Store {
                            slot: res_slot,
                            temp: h,
                        });
                    }
                    CatchKind::Bind { name: bname, body } => {
                        let err_slot = self.alloc_slot();
                        self.push(IrInst::Store {
                            slot: err_slot,
                            temp: a,
                        });
                        self.push_scope();
                        self.bind(bname, err_slot);
                        // 块值：最后语句为表达式时取其值（只求值一次——对齐解释器 exec_block_inner）；
                        // 其余（赋值/return/块等作值）→ void 占位
                        let last_is_value = matches!(body.stmts.last(), Some(Stmt::Expr(_)));
                        let n = body.stmts.len() - usize::from(last_is_value);
                        for stmt in &body.stmts[..n] {
                            self.lower_stmt(stmt);
                        }
                        if last_is_value {
                            if let Some(Stmt::Expr(last)) = body.stmts.last() {
                                let h = self.lower_expr(last);
                                self.push(IrInst::Store {
                                    slot: res_slot,
                                    temp: h,
                                });
                            }
                        } else {
                            let h = self.alloc_slot();
                            self.push(IrInst::Const {
                                temp: h,
                                val: IrConst::Void,
                            });
                            self.push(IrInst::Store {
                                slot: res_slot,
                                temp: h,
                            });
                        }
                        self.pop_scope();
                    }
                }
                self.label(done);
                self.push(IrInst::Load {
                    temp: t,
                    slot: res_slot,
                });
            }
            Expr::Call {
                callee,
                args,
                span: _,
            } => {
                // `@` 内建的类型位置参数（@sizeOf(i32) 等）在调用点编码为 `Const Str(type_name)`，
                // 运行时按名解析——对齐 oracle 从 `Expr::Ident` 读类型名。
                // 限定名调用（alloc.init(ABC) 等）展平为 `"alloc.init"` 后同样适用。
                let callee_name = match callee.as_ref() {
                    Expr::Ident(n, _) => Some(n.clone()),
                    Expr::Dot { base, field, .. } | Expr::Field { base, field, .. } => {
                        let mut parts = vec![field.clone()];
                        let mut b = base.as_ref();
                        while let Expr::Dot {
                            base: b2,
                            field: f2,
                            ..
                        }
                        | Expr::Field {
                            base: b2,
                            field: f2,
                            ..
                        } = b
                        {
                            parts.push(f2.clone());
                            b = b2.as_ref();
                        }
                        if let Expr::Ident(ns, _) = b {
                            parts.push(ns.clone());
                        }
                        parts.reverse();
                        Some(parts.join("."))
                    }
                    _ => None,
                };
                // `Type.new(args, alloc)` 构造器（对齐 oracle `call_new_builtin` interp.rs:
                // 4661-4695）：已知 class 类型名 → MakeClass 按字段声明序填位置参数，alloc
                // first/last 跳过，缺省字段落默认值。用户静态函数 `Type.new` 优先。
                if let Some(qn) = &callee_name {
                    if let Some((ns, method)) = qn.rsplit_once('.') {
                        if method == "new"
                            && !self.funcs.contains(qn)
                            && self.types.classes.contains_key(ns)
                        {
                            return self.lower_new_constructor(ns, args);
                        }
                    }
                }
                // E1.2 组 D D5：comptime 值函数运行时调用点折叠（`var n = array_len(i32);`）
                // ——类型实参收已知类型表达式、值实参常量求值、体常量求值 → `Const`，
                // 类型值仅编译期存在，IR 中无调用/类型值残留。折叠失败回落既有调用路径。
                if let Some(qn) = &callee_name {
                    if self.try_fold_comptime_value_call(qn, args, t) {
                        return t;
                    }
                }
                let arg_ts: Vec<usize> = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        if let Some(cn) = &callee_name {
                            // alloc.init(SomeClass)：已知 class 类型名 → 默认字段 MakeClass
                            // （对齐 oracle 无参构造 = 类型空实例，字段逐默认值；未知/枚举
                            // 类型名回退 Const Str——运行时建空实例）。
                            if matches!(cn.as_str(), "alloc.init" | "arena.init") && i == 0 {
                                if let Expr::Ident(n, _) = a {
                                    if self.types.classes.contains_key(n) {
                                        return self.lower_alloc_init_defaults(n);
                                    }
                                }
                            }
                            if is_type_arg_pos(cn, i) {
                                let name = match a {
                                    Expr::Ident(n, _) => Some(n.clone()),
                                    Expr::StrLit { value, .. } => Some(value.clone()),
                                    _ => None,
                                };
                                if let Some(n) = name {
                                    let at = self.alloc_slot();
                                    self.push(IrInst::Const {
                                        temp: at,
                                        val: IrConst::Str(n),
                                    });
                                    return at;
                                }
                            }
                        }
                        self.lower_expr(a)
                    })
                    .collect();
                match callee.as_ref() {
                    Expr::Ident(name, _) => {
                        // `@`/断言恒为内建；自由内建名被用户函数遮蔽时走用户函数
                        // （对齐 oracle eval_call：先查用户函数，后回退内建）。
                        let builtin = name.starts_with('@')
                            || is_assert_builtin(name)
                            || (is_free_builtin(name) && !self.funcs.contains(name));
                        if builtin {
                            self.push(IrInst::CallBuiltin {
                                name: name.clone(),
                                args: arg_ts,
                                temp: t,
                            });
                        } else if let Some(_slot) = self.resolve(name) {
                            // 局部变量作为调用目标（存函数/闭包值）→ 间接调用
                            let cal = self.lower_expr(callee);
                            self.push(IrInst::CallIndirect {
                                temp: t,
                                callee: cal,
                                args: arg_ts,
                            });
                        } else {
                            // 全局/namespace 函数静态调用（含重载，按名分派）；
                            // C3：import 符号选择 bound → 展开完整限定名（`parse` → `jsonlib.parse`，
                            // 原生经 extern links 链接 / IR 运行时 NoFunction）
                            let qn = self
                                .import_syms
                                .get(name)
                                .cloned()
                                .unwrap_or_else(|| name.clone());
                            self.push(IrInst::Call {
                                name: qn,
                                args: arg_ts,
                                temp: t,
                            });
                        }
                    }
                    Expr::Dot { base, field, .. } | Expr::Field { base, field, .. } => {
                        // 展平限定名链：io.net.double → "io.net.double"
                        // （多级限定名经后缀二次处理后外层为 Field 形态）
                        let mut parts = vec![field.clone()];
                        let mut b = base.as_ref();
                        while let Expr::Dot {
                            base: b2,
                            field: f2,
                            ..
                        }
                        | Expr::Field {
                            base: b2,
                            field: f2,
                            ..
                        } = b
                        {
                            parts.push(f2.clone());
                            b = b2.as_ref();
                        }
                        if let Expr::Ident(ns, _) = b {
                            parts.push(ns.clone());
                            parts.reverse();
                            let qn = parts.join(".");
                            // C3：整模块 import 前缀替换——`import jsonlib;` 后 `jsonlib.f` →
                            // `{包路径}.f`（原生经 extern links 链接）
                            let qn = if let Some(base) = self.import_mods.get(ns) {
                                format!("{base}.{field}")
                            } else {
                                qn
                            };
                            // 已知静态函数（namespace 函数 / `Type.method` 静态调用）→ 直接调用；
                            // `Rect.area(&rect)` 静态调用显式传 self，无注入（对齐 oracle eval_call）
                            if self.funcs.contains(&qn) {
                                self.push(IrInst::Call {
                                    name: qn,
                                    args: arg_ts,
                                    temp: t,
                                });
                                return t;
                            }
                            // 根标识符不解析为局部变量 → 未注册限定名（io.print 等内建/未声明
                            // 函数）：静态名调用 → 运行时 NoFunction（含切片外提示）。保持
                            // Phase 4 前行为；解析为局部时才是实例方法接收者。
                            if self.resolve(ns).is_none() {
                                self.push(IrInst::Call {
                                    name: qn,
                                    args: arg_ts,
                                    temp: t,
                                });
                                return t;
                            }
                        }
                        // 实例方法调用：base 求值 + 运行时按类型名分派 `{Type}.{method}`，
                        // self 注入首参（对齐 oracle interp.rs:2405-2421）
                        let base_t = self.lower_expr(base);
                        self.push(IrInst::CallMethod {
                            temp: t,
                            base: base_t,
                            method: field.clone(),
                            args: arg_ts,
                        });
                    }
                    _ => {
                        // 其它调用形态（闭包字面量立即调用 `(|v| v+a)(5)` / 复合目标）：
                        // 求值 callee → 运行时 Fn/Closure 分派（对齐 oracle eval_call `_` 臂）
                        let cal = self.lower_expr(callee);
                        self.push(IrInst::CallIndirect {
                            temp: t,
                            callee: cal,
                            args: arg_ts,
                        });
                    }
                }
            }
            Expr::IfExpr {
                cond,
                capture,
                then_e,
                else_e,
                ..
            } => {
                // if 表达式：两分支结果统一到 res_slot（对齐解释器 IfExpr）
                let c = self.lower_expr(cond);
                let l_else = self.new_label();
                let l_done = self.new_label();
                let res_slot = self.alloc_slot();
                match capture.as_ref() {
                    Some((_, name)) => {
                        // optional 捕获：null → else；否则绑定 cond 值
                        self.push(IrInst::JumpIfNull {
                            temp: c,
                            label: l_else,
                        });
                        self.push_scope();
                        self.bind(name, c);
                        let tv = self.lower_expr(then_e);
                        self.pop_scope();
                        self.push(IrInst::Store {
                            slot: res_slot,
                            temp: tv,
                        });
                    }
                    None => {
                        self.push(IrInst::JumpIfNot {
                            temp: c,
                            label: l_else,
                        });
                        let tv = self.lower_expr(then_e);
                        self.push(IrInst::Store {
                            slot: res_slot,
                            temp: tv,
                        });
                    }
                }
                self.push(IrInst::Jump { label: l_done });
                self.label(l_else);
                let ev = self.lower_expr(else_e);
                self.push(IrInst::Store {
                    slot: res_slot,
                    temp: ev,
                });
                self.label(l_done);
                self.push(IrInst::Load {
                    temp: t,
                    slot: res_slot,
                });
            }
            Expr::Orelse(l, r, _) => {
                // orelse：null → 默认值；非 null → 解包负载（Opt(Some(x)) → x，
                // 对齐 oracle interp.rs Orelse——此前直接存 a 导致 Opt 泄漏）
                let a = self.lower_expr(l);
                let l_null = self.new_label();
                let done = self.new_label();
                let res_slot = self.alloc_slot();
                let unwrapped = self.alloc_slot();
                self.push(IrInst::JumpIfNull {
                    temp: a,
                    label: l_null,
                });
                self.push(IrInst::Unwrap { temp: unwrapped, a });
                self.push(IrInst::Store {
                    slot: res_slot,
                    temp: unwrapped,
                });
                self.push(IrInst::Jump { label: done });
                self.label(l_null);
                let d = self.lower_expr(r);
                self.push(IrInst::Store {
                    slot: res_slot,
                    temp: d,
                });
                self.label(done);
                self.push(IrInst::Load {
                    temp: t,
                    slot: res_slot,
                });
            }
            Expr::Assign {
                target,
                op,
                value,
                span,
            } => match self.lower_assign(*op, target, value) {
                // 赋值表达式（while 续步 i += 1 等）：值 = 新值（对齐 eval_assign）
                Some(stored) => self.push(IrInst::Load {
                    temp: t,
                    slot: stored,
                }),
                // 目标不是局部变量（字段/索引/解构等）→ 子集外硬错误
                None => self.fail_void(t, "字段/索引/解构赋值", span),
            },
            // ---- Phase 2 聚合 ----
            // 数组/元组字面量：运行时等价（Arr），逐元素求值 + 独立共享 cell
            Expr::ArrayLit(items, _) | Expr::TupleLit(items, _) => {
                let item_ts: Vec<usize> = items.iter().map(|e| self.lower_expr(e)).collect();
                self.push(IrInst::MakeArr {
                    temp: t,
                    items: item_ts,
                });
            }
            Expr::ContainerLit {
                ty,
                ty_args,
                items,
                span,
                ..
            } => {
                // ContainerLiteral: Vec<T>[1, 2, 3] → MakeArr(items) + Vec.init(alloc, arr)
                if ty == "Vec" || ty == "Deque" {
                    // Lower each item
                    let mut item_temps = Vec::new();
                    for it in items {
                        item_temps.push(self.lower_expr(it));
                    }
                    // Create array of items
                    let arr_t = self.alloc_slot();
                    self.push(IrInst::MakeArr {
                        temp: arr_t,
                        items: item_temps,
                    });
                    // Create Vec from array: CallBuiltin("Vec.init", [alloc, arr])
                    let alloc_t = self.alloc_slot();
                    // Load alloc from implicit env
                    self.push(IrInst::LoadGlobal {
                        temp: alloc_t,
                        name: "alloc".to_string(),
                    });
                    self.push(IrInst::CallBuiltin {
                        name: "Vec.init".to_string(),
                        args: vec![alloc_t, arr_t],
                        temp: t,
                    });
                    return t;
                }
                self.fail_void(t, &format!("unknown container literal type `{ty}`"), span);
                return t;
            }
            Expr::NamedLit {
                ty,
                ty_args,
                fields,
                span,
                ..
            } => {
                // Map<K,V>{k = v, ...} 容器字面量（ADR-0027）
                if ty == "Map" {
                    let alloc_t = self.alloc_slot();
                    self.push(IrInst::LoadGlobal {
                        temp: alloc_t,
                        name: "alloc".to_string(),
                    });
                    self.push(IrInst::CallBuiltin {
                        name: "Map.init".to_string(),
                        args: vec![alloc_t],
                        temp: t,
                    });
                    for (key, val_expr) in fields {
                        let key_t = self.alloc_slot();
                        self.push(IrInst::Const {
                            temp: key_t,
                            val: IrConst::Str(key.clone()),
                        });
                        let val_t = self.lower_expr(val_expr);
                        let discard_t = self.alloc_slot();
                        self.push(IrInst::CallMethod {
                            temp: discard_t,
                            base: t,
                            method: "put".to_string(),
                            args: vec![key_t, val_t],
                        });
                    }
                    return t;
                }
                // Vec<T>{} / Deque<T>{} 空容器字面量（ADR-0027）
                if ty == "Vec" || ty == "Deque" {
                    let alloc_t = self.alloc_slot();
                    self.push(IrInst::LoadGlobal {
                        temp: alloc_t,
                        name: "alloc".to_string(),
                    });
                    self.push(IrInst::CallBuiltin {
                        name: "Vec.init".to_string(),
                        args: if fields.is_empty() {
                            vec![alloc_t]
                        } else {
                            // 非空字段暂不支持（Vec<T>{a = 1} 无意义）
                            self.fail_void(t, &format!("容器 `{ty}` 的字面量不允许命名字段"), span);
                            return t;
                        },
                        temp: t,
                    });
                    return t;
                }
                // E1.2 组 D：泛型应用 `Pair<i32>{...}` → 惰性具体化后按具体化名构造。
                // 具体化失败（实参个数/形态不符）→ 硬错误。
                let ty = if ty_args.is_empty() {
                    ty.clone()
                } else {
                    match self.concrete_type_name(ty, ty_args) {
                        Ok(cn) => cn,
                        Err(msg) => {
                            self.fail_void(t, &msg, span);
                            return t;
                        }
                    }
                };
                // struct 字面量 → MakeClass；枚举字面量（恰一个变体）→ MakeEnum（对齐 oracle）
                if self.types.classes.contains_key(&ty) {
                    let f: Vec<(String, usize)> = fields
                        .iter()
                        .map(|(k, v)| (k.clone(), self.lower_expr(v)))
                        .collect();
                    self.push(IrInst::MakeClass {
                        temp: t,
                        ty,
                        fields: f,
                    });
                } else if self.types.unions.contains_key(&ty) {
                    // K1 union 字面量（ADR-0014）：`Foo { field = v }`——单字段。
                    // 运行时形态 = `Cell::Class` + `@union` 标记；缺省字段落标量零值，
                    // 构造后 `UnionSync` 把 `written` 字段字节重解释同步其余字段。
                    if fields.len() != 1 {
                        self.fail_void(t, "union 字面量应为单字段（K1）", span);
                        return t;
                    }
                    let (fname, fval) = &fields[0];
                    let fvt = self.lower_expr(fval);
                    // 先克隆字段表释放 `self.types` 借用，再可变借用 `self` 降级默认值
                    let ufields: Vec<(String, Type)> = self
                        .types
                        .unions
                        .get(&ty)
                        .map(|u| u.fields.clone())
                        .unwrap_or_default();
                    let mut fs: Vec<(String, usize)> = Vec::with_capacity(ufields.len() + 2);
                    for (fdname, fdty) in &ufields {
                        let dt = self.lower_default_value(fdty);
                        fs.push((fdname.clone(), dt));
                    }
                    let mk = self.alloc_slot();
                    self.push(IrInst::Const {
                        temp: mk,
                        val: IrConst::Bool(true),
                    });
                    fs.push(("@union".to_string(), mk));
                    fs.push((fname.clone(), fvt));
                    self.push(IrInst::MakeClass {
                        temp: t,
                        ty: ty.clone(),
                        fields: fs,
                    });
                    self.push(IrInst::UnionSync {
                        class: t,
                        written: fname.clone(),
                    });
                } else if self.types.enums.contains_key(&ty) {
                    if fields.len() != 1 {
                        self.fail_void(t, "多字段枚举字面量（应为单变体）", span);
                        return t;
                    }
                    let (variant, payload) = &fields[0];
                    let pv = self.lower_expr(payload);
                    self.push(IrInst::MakeEnum {
                        temp: t,
                        name: ty,
                        variant: variant.clone(),
                        payload: Some(pv),
                    });
                } else {
                    self.fail_void(t, &format!("未知类型 `{ty}` 的字面量构造"), span);
                }
            }
            // struct 类型字面量（E1.2 组 D）：类型值——仅 comptime 类型函数体内求值；
            // 运行时表达式位置 = 用法错误（类型函数体由 IR 降级跳过，不会到达这里）
            Expr::StructType { span, .. } => {
                self.fail_void(
                    t,
                    "类型值 `struct { ... }`（仅 comptime 类型函数内可求值）",
                    span,
                );
            }
            // 数组类型值 `[n]T`（组 D）：同 struct 类型字面量——仅 comptime 类型函数
            // 体内编译期求值；运行时表达式位置 = 用法错误（类型函数体降级跳过）
            Expr::ArrayType { span, .. } => {
                self.fail_void(t, "类型值 `[n]T`（仅 comptime 类型函数内可求值）", span);
            }
            Expr::Dot { base, field, span } => {
                // 类型名（enum/class）限定 → 枚举常量（对齐 oracle：不做变体验证，全类型名同权）
                if let Expr::Ident(bname, _) = base.as_ref() {
                    // ExitType 内建枚举特判（对齐 oracle eval_dot）
                    if bname == "ExitType" {
                        self.push(IrInst::MakeEnum {
                            temp: t,
                            name: "ExitType".into(),
                            variant: field.clone(),
                            payload: None,
                        });
                        return t;
                    }
                    if self.types.enums.contains_key(bname)
                        || self.types.classes.contains_key(bname)
                    {
                        self.push(IrInst::MakeEnum {
                            temp: t,
                            name: bname.clone(),
                            variant: field.clone(),
                            payload: None,
                        });
                        return t;
                    }
                    // namespace 限定的值位置（非调用位）：oracle 运行时 UndefinedName
                    if self.types.namespaces.contains(bname) {
                        self.fail_void(t, "namespace 限定的值（非调用位）", span);
                        return t;
                    }
                }
                // 推断枚举字面量 `.variant`（base=VoidLit）：L1 兜底名 __inferred__（对齐 oracle）
                if matches!(base.as_ref(), Expr::VoidLit(_)) {
                    self.push(IrInst::MakeEnum {
                        temp: t,
                        name: "__inferred__".into(),
                        variant: field.clone(),
                        payload: None,
                    });
                    return t;
                }
                let b = self.lower_expr(base);
                self.push(IrInst::Field {
                    temp: t,
                    base: b,
                    field: field.clone(),
                });
            }
            Expr::Field { base, field, .. } => {
                let b = self.lower_expr(base);
                self.push(IrInst::Field {
                    temp: t,
                    base: b,
                    field: field.clone(),
                });
            }
            Expr::Index {
                base,
                indices,
                span,
            } => {
                let b = self.lower_expr(base);
                if indices.len() == 1 {
                    if let Expr::Binary(BinOp::Range, lo, hi, _) = &indices[0] {
                        // 切片 `base[lo..hi]`（hi 可为 `__end__` 开区间哨兵）
                        let lo_t = self.lower_expr(lo);
                        let hi_t = self.lower_slice_end(hi);
                        self.push(IrInst::SliceOf {
                            temp: t,
                            base: b,
                            lo: lo_t,
                            hi: hi_t,
                        });
                        return t;
                    }
                    let idx = self.lower_expr(&indices[0]);
                    self.push(IrInst::Index {
                        temp: t,
                        base: b,
                        index: idx,
                    });
                } else {
                    // 多索引 t[i,j] → Index(base, indices[0]) → Index(row, indices[1])
                    let row = self.lower_expr(&indices[0]);
                    let row_t = self.alloc_slot();
                    self.push(IrInst::Index {
                        temp: row_t,
                        base: b,
                        index: row,
                    });
                    let col = self.lower_expr(&indices[1]);
                    self.push(IrInst::Index {
                        temp: t,
                        base: row_t,
                        index: col,
                    });
                }
            }
            // 指针（Phase 1）：`p.*` 解引用
            Expr::Deref(inner, _) => {
                let a = self.lower_expr(inner);
                self.push(IrInst::Deref { temp: t, a });
            }
            // `&x`/`&mut x` 取址：变量 → AddrSlot 别名（写穿共享 cell）；
            // 非 lvalue → AddrValue 快照（对齐 tree-walking `&expr` 兜底分支）
            Expr::AddrOf(target, _, span) => match target.as_ref() {
                Expr::Ident(name, _) => match self.resolve(name) {
                    Some(slot) => self.push(IrInst::AddrSlot { temp: t, slot }),
                    // 全局/常量（Phase 5）：`&global` 别名 cell——`IrRuntime::init` 已
                    // 预分配 cell，`Deref`/`StorePtr` 写穿回全局（对齐 oracle lookup→globals）
                    None if self.globals.contains(name) => {
                        self.push(IrInst::GlobalAddr {
                            temp: t,
                            name: name.clone(),
                        });
                    }
                    None => self.fail_void(t, "未知标识符取址", span),
                },
                _ => {
                    let v = self.lower_expr(target);
                    self.push(IrInst::AddrValue { temp: t, value: v });
                }
            },
            Expr::Unwrap(inner, _) => {
                let a = self.lower_expr(inner);
                self.push(IrInst::Unwrap { temp: t, a });
            }
            Expr::SwitchExpr {
                subject,
                arms,
                span,
            } => {
                let has_else = arms
                    .iter()
                    .any(|a| a.patterns.iter().any(|p| matches!(p, SwitchPattern::Else)));
                self.lower_switch_inner(subject, arms, has_else, span, Some(t));
            }
            // 块表达式：值 = 最后语句（若为 Expr）的值；否则 void（对齐 exec_block_inner）
            Expr::Block(b, _) => {
                self.push_scope();
                let n = b.stmts.len();
                let last_is_value = matches!(b.stmts.last(), Some(Stmt::Expr(_)));
                let m = n - usize::from(last_is_value);
                for stmt in &b.stmts[..m] {
                    self.lower_stmt(stmt);
                }
                if last_is_value {
                    if let Some(Stmt::Expr(e)) = b.stmts.last() {
                        let v = self.lower_expr(e);
                        self.push(IrInst::Load { temp: t, slot: v });
                    }
                } else {
                    self.push(IrInst::Const {
                        temp: t,
                        val: IrConst::Void,
                    });
                }
                self.pop_scope();
            }
            Expr::FnRef(name, _span) => {
                self.push(IrInst::FnRef {
                    temp: t,
                    name: name.clone(),
                });
            }
            // 元组解构：源求值 + Destructure（运行时 arity 检查 + 逐元素克隆绑定）
            Expr::TupleDestructure(names, e, _) => {
                let v = self.lower_expr(e);
                let mut slots = Vec::with_capacity(names.len());
                for n in names {
                    if n == "_" {
                        slots.push(None);
                    } else {
                        let slot = self.alloc_slot();
                        self.bind(n, slot);
                        slots.push(Some(slot));
                    }
                }
                self.push(IrInst::Destructure { value: v, slots });
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Void,
                });
            }
            Expr::Move(inner, _) => {
                let a = self.lower_expr(inner);
                self.push(IrInst::Move { temp: t, a });
            }
            Expr::Closure {
                params,
                body,
                is_move,
                is_mut,
                span,
            } => {
                let ct = self.lower_closure(params, body, *is_mut, *is_move, span);
                self.push(IrInst::Load { temp: t, slot: ct });
            }
        }
        t
    }

    /// 闭包降级（对齐 oracle `Expr::Closure` interp.rs:1931-1963 + `capture_env`）：
    /// **自由变量精确分析**（Phase 8，`closure_free_vars`）——只捕获 body 实际引用、
    /// 未被体内绑定遮蔽的外部变量（`(名字, 槽号)`，最近作用域优先——遮蔽解析正确），
    /// 生成独立闭包函数（前 n_caps 个参数 = 捕获参数，之后 = 显式参数），
    /// 返回闭包值临时槽（MakeClosure 结果）。move → 运行时深拷贝捕获 cell。
    /// 块值语义（对齐 `exec_block_inner`）：末语句为表达式 → 作为返回值（单表达式
    /// 闭包 `|v| v+a` 即此形态）；否则末尾 ReturnVoid。
    pub(crate) fn lower_closure(
        &mut self,
        params: &[String],
        body: &Block,
        is_mut: bool,
        is_move: bool,
        _span: &Span,
    ) -> usize {
        // 捕获集合：自由变量精确集 ∩ 当前作用域链绑定（名字, 槽号），
        // 最近作用域优先（遮蔽正确）。自由集外的名字不捕获（闭包不可见）。
        let free = closure_free_vars(params, body);
        let mut captures: Vec<(String, usize)> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for scope in self.scopes.iter().rev() {
            for (name, slot) in scope {
                if free.contains(name) && seen.insert(name.clone()) {
                    captures.push((name.clone(), *slot));
                }
            }
        }
        let n_caps = captures.len();
        let temp = self.alloc_slot();
        // 闭包体用独立 LowerCtx（共享闭包缓冲、错误码表、类型表、函数名/全局名集合）
        let errors = self.errors.clone();
        let types = self.types.clone();
        let funcs = self.funcs;
        let globals = self.globals;
        let type_fns = self.type_fns;
        let value_fns = self.value_fns;
        let closures = &mut *self.closures;
        let import_syms = self.import_syms.clone();
        let import_mods = self.import_mods.clone();
        let mut ctx = LowerCtx::new(
            errors,
            types,
            funcs,
            globals,
            type_fns,
            value_fns,
            closures,
            import_syms,
            import_mods,
        );
        ctx.push_scope();
        // 捕获参数槽（0..n_caps）与显式参数槽（n_caps..）
        for _ in 0..n_caps {
            ctx.alloc_slot();
        }
        for (i, (name, _)) in captures.iter().enumerate() {
            ctx.bind(name, i);
        }
        let param_slots: Vec<usize> = params.iter().map(|_| ctx.alloc_slot()).collect();
        for (p, slot) in params.iter().zip(param_slots.iter()) {
            ctx.bind(p, *slot);
        }
        // 块值语义：末语句为表达式 → 返回值
        let n = body.stmts.len();
        let last_is_value = matches!(body.stmts.last(), Some(Stmt::Expr(_)));
        let m = n - usize::from(last_is_value);
        for stmt in &body.stmts[..m] {
            ctx.lower_stmt(stmt);
        }
        if last_is_value {
            if let Some(Stmt::Expr(e)) = body.stmts.last() {
                let v = ctx.lower_expr(e);
                ctx.insts.push(IrInst::Return { temp: v });
            }
        } else {
            // 末语句非值表达式：上面循环已按 m = n 降级了全部语句，这里只补
            // 尾部 ReturnVoid——**不得**再 lower_stmt(last)（会重复降级末语句，
            // 非 Return 时副作用双重执行）。
            ctx.insts.push(IrInst::ReturnVoid);
        }
        ctx.pop_scope();
        // 提取闭包体结果后释放 ctx（结束对 self.closures 的重借），再操作 self
        let cerr = ctx.err.take();
        let body_insts = ctx.insts;
        let n_slots = ctx.next_slot;
        // 子集外特性传播到外层（首个生效）
        if let Some(e) = cerr {
            if self.err.is_none() {
                self.err = Some(e);
            }
        }
        // 闭包索引须在**体降级完成后**取：嵌套闭包在体降级期间已推入
        // `self.closures`（先内后外），此前快照的索引会指向错误函数。
        let func_idx = self.closures.len();
        let mut fparams: Vec<usize> = (0..n_caps).collect();
        fparams.extend(param_slots.iter().copied());
        self.closures.push(IrFunc {
            name: format!("<closure#{func_idx}>"),
            params: fparams,
            param_ty: Vec::new(),
            ret_ty: Type::Named("void".to_string(), vec![]),
            param_defaults: Vec::new(),
            defaults: Vec::new(),
            n_slots,
            body: body_insts,
            is_test: false,
            exported: false,
            is_extern: false,
        });
        self.push(IrInst::MakeClosure {
            temp,
            func: func_idx,
            captures,
            is_move,
            is_mut,
        });
        temp
    }

    /// 切片上界降级：`__end__` 哨兵 → End 常量；否则普通表达式（对齐 parser open-end 标记）。
    pub(crate) fn lower_slice_end(&mut self, hi: &Expr) -> usize {
        if let Expr::IntLit { text, .. } = hi {
            if text == "__end__" {
                let t = self.alloc_slot();
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::End,
                });
                return t;
            }
        }
        self.lower_expr(hi)
    }

    /// 赋值：返回写入目标槽的新值临时槽（目标不在 IR 范围 → None）
    /// 复合赋值 x op= v → x = x op v（对齐解释器 eval_assign）
    pub(crate) fn lower_assign(
        &mut self,
        op: AssignOp,
        target: &Expr,
        value: &Expr,
    ) -> Option<usize> {
        match target {
            Expr::Ident(name, _) => {
                if let Some(slot) = self.resolve(name) {
                    let v = self.lower_expr(value);
                    return Some(match op {
                        AssignOp::Set => {
                            self.push(IrInst::Store { slot, temp: v });
                            v
                        }
                        _ => {
                            let cur = self.alloc_slot();
                            self.push(IrInst::Load { temp: cur, slot });
                            let r = self.alloc_slot();
                            self.push(IrInst::Bin {
                                op: to_assign_binop(op),
                                temp: r,
                                a: cur,
                                b: v,
                            });
                            self.push(IrInst::Store { slot, temp: r });
                            r
                        }
                    });
                }
                // 全局变量赋值（Phase 5）：`StoreGlobal`（复合赋值 = LoadGlobal + Bin + StoreGlobal）
                if self.globals.contains(name) {
                    let v = self.lower_expr(value);
                    return Some(match op {
                        AssignOp::Set => {
                            self.push(IrInst::StoreGlobal {
                                name: name.clone(),
                                value: v,
                            });
                            v
                        }
                        _ => {
                            let cur = self.alloc_slot();
                            self.push(IrInst::LoadGlobal {
                                temp: cur,
                                name: name.clone(),
                            });
                            let r = self.alloc_slot();
                            self.push(IrInst::Bin {
                                op: to_assign_binop(op),
                                temp: r,
                                a: cur,
                                b: v,
                            });
                            self.push(IrInst::StoreGlobal {
                                name: name.clone(),
                                value: r,
                            });
                            r
                        }
                    });
                }
            }
            // 指针写穿（Phase 1）：`p.* = v` / `p.* op= v`（对齐 eval_assign Deref 臂）
            Expr::Deref(inner, _) => {
                let p = self.lower_expr(inner);
                let v = self.lower_expr(value);
                return Some(match op {
                    AssignOp::Set => {
                        self.push(IrInst::StorePtr {
                            target: p,
                            value: v,
                        });
                        v
                    }
                    _ => {
                        let cur = self.alloc_slot();
                        self.push(IrInst::Deref { temp: cur, a: p });
                        let r = self.alloc_slot();
                        self.push(IrInst::Bin {
                            op: to_assign_binop(op),
                            temp: r,
                            a: cur,
                            b: v,
                        });
                        self.push(IrInst::StorePtr {
                            target: p,
                            value: r,
                        });
                        r
                    }
                });
            }
            // 字段赋值：`p.x = v`（仅 Class 目标；非 Class → TypeError——对齐 eval_assign Field 臂）。
            // 复合（`p.x += v`）：cur = 字段读 + binop + 写回（对齐 oracle eval_assign 先
            // eval(target) 求当前值再 binop；base 双求值语义与 oracle 一致）。
            Expr::Field { base, field, .. } => {
                let b = self.lower_expr(base);
                let v = match op {
                    AssignOp::Set => self.lower_expr(value),
                    _ => {
                        let cur = self.lower_expr(target);
                        let rhs = self.lower_expr(value);
                        let r = self.alloc_slot();
                        self.push(IrInst::Bin {
                            op: to_assign_binop(op),
                            temp: r,
                            a: cur,
                            b: rhs,
                        });
                        r
                    }
                };
                self.push(IrInst::StoreField {
                    base: b,
                    field: field.clone(),
                    value: v,
                });
                return Some(v);
            }
            Expr::Dot { base, field, .. } => {
                // `Type.x = v`：类型名限定的赋值 → 运行时 BadAssign（对齐 eval_assign Dot 臂；
                // base 保证非 Class → StoreField 抛 TypeError，错误名差异不影响 PASS/FAIL）
                if let Expr::Ident(bname, _) = base.as_ref() {
                    if self.types.enums.contains_key(bname)
                        || self.types.classes.contains_key(bname)
                        || self.types.namespaces.contains(bname)
                    {
                        let base_t = self.alloc_slot();
                        self.push(IrInst::Const {
                            temp: base_t,
                            val: IrConst::Void,
                        });
                        let v = self.lower_expr(value);
                        self.push(IrInst::StoreField {
                            base: base_t,
                            field: field.clone(),
                            value: v,
                        });
                        return Some(v);
                    }
                }
                let b = self.lower_expr(base);
                let v = match op {
                    AssignOp::Set => self.lower_expr(value),
                    // 复合（`p.x += v`）：cur = 字段读 + binop + 写回（对齐 eval_assign）
                    _ => {
                        let cur = self.lower_expr(target);
                        let rhs = self.lower_expr(value);
                        let r = self.alloc_slot();
                        self.push(IrInst::Bin {
                            op: to_assign_binop(op),
                            temp: r,
                            a: cur,
                            b: rhs,
                        });
                        r
                    }
                };
                self.push(IrInst::StoreField {
                    base: b,
                    field: field.clone(),
                    value: v,
                });
                return Some(v);
            }
            // 索引赋值：单索引 → StoreIndex（复合 = 读 cur + binop + 写回）；
            // 区间 → StoreSlice（仅 Set；复合/开区间 → 运行时错误）
            Expr::Index {
                base,
                indices,
                span,
            } => {
                if indices.len() >= 2 {
                    // 多索引赋值 t[i,j] = v → Index(base, indices[0]) → StoreIndex(row, indices[1], value)
                    let b = self.lower_expr(base);
                    if op == AssignOp::Set {
                        let row = self.lower_expr(&indices[0]);
                        let row_t = self.alloc_slot();
                        self.push(IrInst::Index {
                            temp: row_t,
                            base: b,
                            index: row,
                        });
                        let col = self.lower_expr(&indices[1]);
                        let v = self.lower_expr(value);
                        self.push(IrInst::StoreIndex {
                            base: row_t,
                            index: col,
                            value: v,
                        });
                        return Some(v);
                    }
                    // 复合赋值：先读 target（t[i,j]）得到当前值，计算，再写回
                    let cur = self.lower_expr(target);
                    let rhs = self.lower_expr(value);
                    let r = self.alloc_slot();
                    self.push(IrInst::Bin {
                        op: to_assign_binop(op),
                        temp: r,
                        a: cur,
                        b: rhs,
                    });
                    let row = self.lower_expr(&indices[0]);
                    let row_t = self.alloc_slot();
                    self.push(IrInst::Index {
                        temp: row_t,
                        base: b,
                        index: row,
                    });
                    let col = self.lower_expr(&indices[1]);
                    self.push(IrInst::StoreIndex {
                        base: row_t,
                        index: col,
                        value: r,
                    });
                    return Some(r);
                }
                if let Expr::Binary(BinOp::Range, lo, hi, _) = &indices[0] {
                    // 复合区间赋值：对齐 oracle 仅允许 Set → 运行时 BadAssign
                    if op != AssignOp::Set {
                        let base_t = self.alloc_slot();
                        self.push(IrInst::Const {
                            temp: base_t,
                            val: IrConst::Void,
                        });
                        let v = self.lower_expr(value);
                        self.push(IrInst::StoreField {
                            base: base_t,
                            field: "".to_string(),
                            value: v,
                        });
                        return Some(v);
                    }
                    let b = self.lower_expr(base);
                    let lo_t = self.lower_expr(lo);
                    let hi_t = self.lower_slice_end(hi);
                    let v = self.lower_expr(value);
                    self.push(IrInst::StoreSlice {
                        base: b,
                        lo: lo_t,
                        hi: hi_t,
                        value: v,
                    });
                    return Some(v);
                }
                let b = self.lower_expr(base);
                if op == AssignOp::Set {
                    let idx = self.lower_expr(&indices[0]);
                    let v = self.lower_expr(value);
                    self.push(IrInst::StoreIndex {
                        base: b,
                        index: idx,
                        value: v,
                    });
                    return Some(v);
                }
                // 复合：cur = base[idx]；r = cur op v；base[idx] = r（对齐 oracle 双求值 base）
                let cur = self.lower_expr(target);
                let v = self.lower_expr(value);
                let r = self.alloc_slot();
                self.push(IrInst::Bin {
                    op: to_assign_binop(op),
                    temp: r,
                    a: cur,
                    b: v,
                });
                let idx = self.lower_expr(&indices[0]);
                self.push(IrInst::StoreIndex {
                    base: b,
                    index: idx,
                    value: r,
                });
                return Some(r);
            }
            _ => {}
        }
        None
    }

    /// [continuous] 值语义判定（P11d，对齐 oracle VarDecl `interp.rs:926-949`）：
    /// 声明类型 `ty` 为连续类（`Type::Named` 且 `TypeTable.continuous`），或
    /// 未标注类型且初始值为标识符（`var p2 = p`——运行时门按值实际类名判定，
    /// 标量/数组/非连续类恒等 = 引用别名）。
    pub(crate) fn needs_deep_copy(&self, ty: Option<&Type>, init: Option<&Expr>) -> bool {
        if let Some(t) = ty {
            return match t.strip() {
                Type::Named(tn, _) => self
                    .types
                    .classes
                    .get(tn)
                    .map(|c| c.continuous)
                    .unwrap_or(false),
                _ => false,
            };
        }
        matches!(init, Some(Expr::Ident(..)))
    }

    /// `alloc.init(SomeClass)` 的默认字段构造：MakeClass + 逐字段默认值（对齐 oracle
    /// 无参构造 `alloc.init(T)` interp.rs:3912-3919——字段逐 `default_value`）。
    /// 运行时 `call_alloc_method_ir("init")` 对 `IrValue::Class` 实参原样返回，语义等价。
    pub(crate) fn lower_alloc_init_defaults(&mut self, ty_name: &str) -> usize {
        // 先克隆字段表释放 `self.types` 借用，再可变借用 `self` 递归降级默认值。
        let fields: Vec<(String, Type)> = self
            .types
            .classes
            .get(ty_name)
            .map(|c| c.fields.clone())
            .unwrap_or_default();
        let mut field_temps = Vec::with_capacity(fields.len());
        for (fname, fty) in &fields {
            let v = self.lower_default_value(fty);
            field_temps.push((fname.clone(), v));
        }
        let t = self.alloc_slot();
        self.push(IrInst::MakeClass {
            temp: t,
            ty: ty_name.to_string(),
            fields: field_temps,
        });
        t
    }

    /// `Type.new(args, alloc)` 构造器降级（对齐 oracle `call_new_builtin` interp.rs:
    /// 4661-4695）：位置参数按字段声明序填充，alloc-first/alloc-last 跳过分配器实参，
    /// 缺省字段落默认值（Vec 字段 → 空 Arr，余同 `lower_default_value`）。发射 `MakeClass`
    /// —— run_ir/字节码/LLVM 三后端经既有指令语义对齐 tree-walking oracle。
    pub(crate) fn lower_new_constructor(&mut self, ty_name: &str, args: &[Expr]) -> usize {
        // 先克隆字段表释放 `self.types` 借用，再可变借用 `self` 递归降级默认值。
        let fields: Vec<(String, Type)> = self
            .types
            .classes
            .get(ty_name)
            .map(|c| c.fields.clone())
            .unwrap_or_default();
        let (vals_start, vals_end) = if args.len() > 1 {
            let is_alloc_first = matches!(&args[0], Expr::Ident(n, _) if n == "alloc");
            let is_alloc_last = matches!(args.last(), Some(Expr::Ident(n, _)) if n == "alloc");
            if is_alloc_first {
                (1usize, args.len())
            } else if is_alloc_last {
                (0usize, args.len() - 1)
            } else {
                (0usize, args.len())
            }
        } else {
            (0usize, args.len())
        };
        let mut ai = vals_start;
        let mut field_temps = Vec::with_capacity(fields.len());
        for (fname, fty) in &fields {
            if ai < vals_end {
                let t = self.lower_expr(&args[ai]);
                field_temps.push((fname.clone(), t));
                ai += 1;
            } else {
                let t = self.lower_default_value(fty);
                field_temps.push((fname.clone(), t));
            }
        }
        let t = self.alloc_slot();
        self.push(IrInst::MakeClass {
            temp: t,
            ty: ty_name.to_string(),
            fields: field_temps,
        });
        t
    }

    /// 类型默认值（对齐 oracle `default_value` interp.rs:1036-1080）：标量零值 /
    /// 空字符串 / 空集合 / `?T`→Opt(None) / 命名 class 递归默认字段 / 枚举空变体。
    pub(crate) fn lower_default_value(&mut self, ty: &Type) -> usize {
        let t = self.alloc_slot();
        match ty.strip() {
            Type::Named(n, args) => {
                // E1.2 组 D D3：类型函数应用（`Pair<i32>`）声明式无初值 → 惰性具体化后
                // 递归（对齐 oracle default_value interp.rs:1438-1440，消除 `__none__`
                // 静默损坏）。具体化失败（类型函数体形状非法）→ 降级硬错误 + void 占位。
                if !args.is_empty() {
                    match self.concrete_type_name(n, args) {
                        Ok(cn) => {
                            let inner = self.lower_default_value(&Type::Named(cn, vec![]));
                            self.push(IrInst::Move { temp: t, a: inner });
                            return t;
                        }
                        Err(msg) => {
                            self.fail_void(t, &msg, &Span::new(0, 0, 0, 0));
                            return t;
                        }
                    }
                }
                match n.as_str() {
                    "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32"
                    | "u64" | "u128" | "usize" => {
                        self.push(IrInst::Const {
                            temp: t,
                            val: IrConst::Int(0),
                        });
                    }
                    "f32" | "f64" | "f16" | "f128" => {
                        self.push(IrInst::Const {
                            temp: t,
                            val: IrConst::Float(0.0),
                        });
                    }
                    "bool" => {
                        self.push(IrInst::Const {
                            temp: t,
                            val: IrConst::Bool(false),
                        });
                    }
                    "void" => {
                        self.push(IrInst::Const {
                            temp: t,
                            val: IrConst::Void,
                        });
                    }
                    "String" | "&[u8]" => {
                        self.push(IrInst::Const {
                            temp: t,
                            val: IrConst::Str(String::new()),
                        });
                    }
                    // G4：集合默认值 = 隐式环境空容器（持全局 alloc）
                    "Vec" | "Deque" | "Table" => {
                        self.push(IrInst::LoadGlobal {
                            temp: t,
                            name: "Vec".into(),
                        });
                    }
                    "Map" => {
                        self.push(IrInst::LoadGlobal {
                            temp: t,
                            name: "Map".into(),
                        });
                    }
                    _ => {
                        // Vec(T) / Map(K,V) 泛型集合形态
                        if n == "Vec" || n == "Deque" {
                            self.push(IrInst::LoadGlobal {
                                temp: t,
                                name: "Vec".into(),
                            });
                        } else if n == "Map" {
                            self.push(IrInst::LoadGlobal {
                                temp: t,
                                name: "Map".into(),
                            });
                        } else if let Some(ci) = self.types.classes.get(n) {
                            // 命名 class：递归默认字段（先克隆字段表释放 `self.types` 借用）
                            let cls_fields = ci.fields.clone();
                            let mut fields = Vec::with_capacity(cls_fields.len());
                            for (fname, fty) in &cls_fields {
                                let v = self.lower_default_value(fty);
                                fields.push((fname.clone(), v));
                            }
                            self.push(IrInst::MakeClass {
                                temp: t,
                                ty: n.clone(),
                                fields,
                            });
                        } else {
                            // 未知命名类型（enum 等）：空变体（对齐 oracle default_value Enum 臂）
                            self.push(IrInst::MakeEnum {
                                temp: t,
                                name: n.clone(),
                                variant: "__none__".into(),
                                payload: None,
                            });
                        }
                    }
                }
            }
            Type::Optional(_) => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Null,
                });
            }
            Type::Ptr(_, _) | Type::Infer | Type::Owned(_) => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Void,
                });
            }
            Type::Slice(_, _) => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Str(String::new()),
                });
            }
            _ => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Void,
                });
            }
        }
        t
    }

    /// E1.2 组 D：惰性具体化——`Pair<i32>` → 具体化名 `Pair<@i32>`。
    ///
    /// `self.types` 缓存命中即回；未命中则查类型函数定义表（`type_fns`，comptime-only）
    /// → `comptime::instantiate` → 以具体化名登记 `ClassInfo` → 返回具体化名。
    /// `args` 为空 / 非类型函数（内建泛型 `Vec(T)` 等）→ 回退基础名，由调用方既有路径处理。
    ///
    /// 透传形态（`return T;`）产物是**实参类型自身**：返回其规范名（`type_key`），
    /// 使 `Pair<i32>` 与 `i32` 同义。
    pub(crate) fn concrete_type_name(
        &mut self,
        name: &str,
        args: &[Type],
    ) -> Result<String, String> {
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
        if self.types.classes.contains_key(&cname) || self.types.enums.contains_key(&cname) {
            return Ok(cname);
        }
        // 自/互递归守卫：`LinkedList<i32>` 字段内自引用在登记期重入 → 返回键本身（叶）。
        if self.instantiating.contains(&cname) {
            return Ok(cname);
        }
        if let Some((params, body)) = self.type_fns.get(name) {
            self.instantiating.push(cname.clone());
            let inst = comptime::instantiate(name, params, body, &resolved);
            let result = match inst {
                Ok(Instantiated::Class(mut decl)) => match self.normalize_decl_fields(&mut decl) {
                    Ok(()) => {
                        if let Decl::Class {
                            name: cn,
                            fields,
                            methods,
                            ..
                        } = &decl
                        {
                            let ci = ClassInfo {
                                fields: fields
                                    .iter()
                                    .map(|f| (f.name.clone(), f.ty.clone()))
                                    .collect(),
                                methods: methods.iter().map(|m| m.name.clone()).collect(),
                                continuous: false,
                            };
                            self.types.classes.insert(cn.clone(), ci);
                        }
                        Ok(cname)
                    }
                    Err(msg) => Err(msg),
                },
                Ok(Instantiated::Type(t)) => Ok(comptime::type_key(&t)),
                Err(msg) => Err(msg),
            };
            self.instantiating.pop();
            return result;
        }
        // 非类型函数（内建泛型 `Vec(T)`/`Map(K,V)` 等）：
        // 若实参含具体化名（含 `@`），则生成具体化名保留嵌套类型信息
        // （如 `Vec<@List<@i32>>`）；否则回退基础名（`Vec<i32>` → `Vec`），
        // 由既有路径处理（空集合 / 类型未登记 → 未知类型，保持原语义）。
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
    pub(crate) fn resolve_nested_types(&mut self, ty: &Type) -> Result<Type, String> {
        comptime::map_type_apps(ty, &mut |n, a| self.concrete_type_name(n, a))
    }

    /// E1.2 组 D D3：把具体化 Class 声明的字段类型深度规范化——嵌套类型函数应用
    /// （`Pair<i32>`）替换为具体化键（`Pair<@i32>`）；自/互递归经守卫终止。
    pub(crate) fn normalize_decl_fields(&mut self, decl: &mut Decl) -> Result<(), String> {
        match decl {
            Decl::Class { fields, .. } | Decl::Struct { fields, .. } => {
                for fd in fields.iter_mut() {
                    fd.ty = self.resolve_nested_types(&fd.ty)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ---------- E1.2 组 D D5：comptime 值函数运行时调用点折叠 ----------

    /// 已知类型名判定（对齐 oracle interp.rs `is_known_type_name`）：基础类型 + 内建
    /// 容器 + 已登记 class/enum + 类型函数。值函数 `T: type` 实参须为已知类型表达式。
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
            "Vec" | "Map" | "Deque" | "Table" | "Allocator" | "Arena" | "ExitType"
        ) {
            return true;
        }
        if self.types.classes.contains_key(name) || self.types.enums.contains_key(name) {
            return true;
        }
        // 类型函数名（`fn X(...) type`）
        if self.type_fns.contains_key(name) {
            return true;
        }
        false
    }

    /// 折叠 comptime 值函数调用（`array_len(i32)`）→ `IrConst` 并发射 `Const`。
    /// 类型实参经 `comptime::expr_to_type` 收已知类型表达式（编译期类型值，无运行时
    /// 残留）；值实参常量求值入 bindings；体常量求值取最后 return。任一失败回退
    /// `false` → 调用方走既有路径（未知类型实参等错误在实参降级处报告）。
    pub(crate) fn try_fold_comptime_value_call(
        &mut self,
        name: &str,
        args: &[Expr],
        t: usize,
    ) -> bool {
        let Some((params, body)) = self.value_fns.get(name).cloned() else {
            return false;
        };
        if params.len() != args.len() {
            return false;
        }
        let mut bindings: HashMap<String, IrConst> = HashMap::new();
        for (p, a) in params.iter().zip(args.iter()) {
            if comptime::is_type_param(p) {
                match comptime::expr_to_type(a) {
                    Some(Type::Named(n, _)) if self.is_known_type_name(&n) => {}
                    _ => return false,
                }
            } else {
                match self.eval_const_expr(a, &bindings) {
                    Some(v) => {
                        bindings.insert(p.name.clone(), v);
                    }
                    None => return false,
                }
            }
        }
        if let Ok(Some(v)) = self.eval_const_block(&body, &mut bindings) {
            self.push(IrInst::Const { temp: t, val: v });
            return true;
        }
        false
    }

    /// 常量表达式求值（编译期纯函数）：字面量、值参数引用、一元/二元、if 分支折叠、
    /// 块（委托 `eval_const_block`）。不支持 → None（回退既有路径）。
    pub(crate) fn eval_const_expr(
        &self,
        e: &Expr,
        bindings: &HashMap<String, IrConst>,
    ) -> Option<IrConst> {
        match e {
            Expr::IntLit { text, .. } => Some(IrConst::Int(parse_int_lit(text))),
            Expr::FloatLit { text, .. } => {
                let t = text.trim_end_matches(|c: char| c.is_alphabetic());
                let f: f64 = t.replace('_', "").parse().ok()?;
                Some(IrConst::Float(f))
            }
            Expr::BoolLit(b, _) => Some(IrConst::Bool(*b)),
            Expr::StrLit { value, .. } => Some(IrConst::Str(value.clone())),
            Expr::CharLit(c, _) => Some(IrConst::Int(*c as i128)),
            Expr::Ident(n, _) => bindings.get(n).cloned(),
            Expr::Unary(op, inner, _) => {
                let v = self.eval_const_expr(inner, bindings)?;
                const_unary(*op, &v)
            }
            Expr::Binary(BinOp::And, a, b, _) => match self.eval_const_expr(a, bindings)? {
                IrConst::Bool(false) => Some(IrConst::Bool(false)),
                IrConst::Bool(true) => self.eval_const_expr(b, bindings),
                _ => None,
            },
            Expr::Binary(BinOp::Or, a, b, _) => match self.eval_const_expr(a, bindings)? {
                IrConst::Bool(true) => Some(IrConst::Bool(true)),
                IrConst::Bool(false) => self.eval_const_expr(b, bindings),
                _ => None,
            },
            Expr::Binary(op, a, b, _) => {
                let av = self.eval_const_expr(a, bindings)?;
                let bv = self.eval_const_expr(b, bindings)?;
                const_binop(*op, &av, &bv)
            }
            Expr::IfExpr {
                cond,
                then_e,
                else_e,
                ..
            } => match self.eval_const_expr(cond, bindings)? {
                IrConst::Bool(true) => self.eval_const_expr(then_e, bindings),
                IrConst::Bool(false) => self.eval_const_expr(else_e, bindings),
                _ => None,
            },
            Expr::Block(b, _) => {
                let mut b2 = bindings.clone();
                self.eval_const_block(b, &mut b2).ok().flatten()
            }
            Expr::ContainerLit { .. } => None,
            _ => None,
        }
    }

    /// 块常量执行（comptime 值函数体求值，对齐 oracle 顺序语义）：
    /// - 语句按序执行；`var`/`const` 初始化并入 bindings；`return` 即返回其值。
    /// - `Stmt::If` 常量条件折叠分支（then/else/else-if）；分支块**未返回**则继续后续语句。
    /// - `Err(())` = 块含无法常量求值的语句（while/for/switch/丢弃调用等）→ 折叠回退；
    ///   `Ok(None)` = 块正常执行完（未返回）；`Ok(Some(v))` = 块返回 v。
    pub(crate) fn eval_const_block(
        &self,
        body: &Block,
        bindings: &mut HashMap<String, IrConst>,
    ) -> Result<Option<IrConst>, ()> {
        for stmt in &body.stmts {
            match stmt {
                Stmt::VarDecl {
                    name,
                    init: Some(e),
                    ..
                } => {
                    let v = self.eval_const_expr(e, bindings).ok_or(())?;
                    bindings.insert(name.clone(), v);
                }
                Stmt::ConstDecl { name, init, .. } => {
                    let v = self.eval_const_expr(init, bindings).ok_or(())?;
                    bindings.insert(name.clone(), v);
                }
                Stmt::Return(Some(e), _) => {
                    return Ok(Some(self.eval_const_expr(e, bindings).ok_or(())?))
                }
                Stmt::Return(None, _) => return Ok(Some(IrConst::Void)),
                Stmt::If(ifst) => {
                    let c = self.eval_const_expr(&ifst.cond, bindings).ok_or(())?;
                    match c {
                        IrConst::Bool(true) => {
                            let r = self.eval_const_block(&ifst.then_b, bindings)?;
                            if r.is_some() {
                                return Ok(r);
                            }
                        }
                        IrConst::Bool(false) => {
                            if let Some(else_b) = &ifst.else_b {
                                let r = match else_b.as_ref() {
                                    Stmt::Block(b2) => self.eval_const_block(b2, bindings)?,
                                    // else-if 链：伪块包一层继续求值
                                    Stmt::If(inner) => {
                                        let pseudo = Block {
                                            stmts: vec![Stmt::If(inner.clone())],
                                            span: inner.span.clone(),
                                        };
                                        self.eval_const_block(&pseudo, bindings)?
                                    }
                                    _ => None,
                                };
                                if r.is_some() {
                                    return Ok(r);
                                }
                            }
                        }
                        _ => return Err(()),
                    }
                }
                Stmt::Block(b2) => {
                    let r = self.eval_const_block(b2, bindings)?;
                    if r.is_some() {
                        return Ok(r);
                    }
                }
                // while/for/switch/丢弃调用等不可常量求值 → 折叠回退
                _ => return Err(()),
            }
        }
        Ok(None)
    }

    pub(crate) fn lower_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::VarDecl { name, init, ty, .. } => {
                // 遮蔽时分配新槽（词法作用域，块退出恢复外层绑定）
                let slot = self.alloc_slot();
                self.bind(name, slot);
                let t = match init {
                    Some(e) => self.lower_expr(e),
                    // 声明式无初值：有类型标注 → 类型默认值（对齐 oracle `default_value`，
                    // 含 D3 类型函数应用的惰性具体化）；无标注 → Void 占位（原行为）。
                    None => match ty {
                        Some(ty) => self.lower_default_value(ty),
                        None => {
                            let t = self.alloc_slot();
                            self.push(IrInst::Const {
                                temp: t,
                                val: IrConst::Void,
                            });
                            t
                        }
                    },
                };
                // M3 语法糖：`var s: String = "hello"` → `String.from("hello")`
                let t = if matches!(ty, Some(Type::Named(n, _)) if n == "String") && init.is_some()
                {
                    let t2 = self.alloc_slot();
                    self.push(IrInst::Call {
                        name: "String.from".into(),
                        args: vec![t],
                        temp: t2,
                    });
                    t2
                } else {
                    t
                };
                // [continuous] 值语义（P11d）：声明类型为连续类，或未标注类型且初始
                // 值为标识符 → 赋值前 DeepCopy（后者由运行时门判定，仅连续类深拷贝）。
                if self.needs_deep_copy(ty.as_ref(), init.as_ref()) {
                    let t2 = self.alloc_slot();
                    self.push(IrInst::DeepCopy { temp: t2, a: t });
                    self.push(IrInst::Store { slot, temp: t2 });
                } else {
                    self.push(IrInst::Store { slot, temp: t });
                }
            }
            Stmt::ConstDecl { name, init, .. } => {
                let slot = self.alloc_slot();
                self.bind(name, slot);
                let t = self.lower_expr(init);
                self.push(IrInst::Store { slot, temp: t });
            }
            Stmt::Expr(Expr::Assign {
                target,
                op,
                value,
                span,
            }) => {
                // 语句级赋值：副作用即可；目标不在 IR 范围（字段/索引/解构）→ 硬错误
                if self.lower_assign(*op, target, value).is_none() {
                    self.fail("字段/索引/解构赋值", span);
                }
            }
            Stmt::Expr(e) => {
                let _ = self.lower_expr(e);
            }
            Stmt::If(ifs) => {
                let c = self.lower_expr(&ifs.cond);
                let l_else = self.new_label();
                let l_end = self.new_label();
                // 错误捕获：if (e!T) |v| else |err|——错误值走 l_err（绑定 err）。
                // 仅当存在 then 捕获才分流（`if (x) else |err|` 无捕获 → 维持普通 if 语义，
                // 对齐解释器 exec_if）。
                let l_err = if ifs.capture.is_some() && ifs.err_capture.is_some() {
                    Some(self.new_label())
                } else {
                    None
                };
                match &ifs.capture {
                    // 捕获：if (maybe) |v| / if (e!T) |v|——错误 → l_err；
                    // null → else；否则解包负载绑定捕获名（对齐解释器 exec_if）
                    Some((_, name)) => {
                        if let Some(le) = l_err {
                            self.push(IrInst::JumpIfErr { temp: c, label: le });
                        }
                        self.push(IrInst::JumpIfNull {
                            temp: c,
                            label: l_else,
                        });
                        self.push_scope();
                        let u = self.alloc_slot();
                        self.push(IrInst::Unwrap { temp: u, a: c });
                        self.bind(name, u);
                        for stmt in &ifs.then_b.stmts {
                            self.lower_stmt(stmt);
                        }
                        self.pop_scope();
                    }
                    None => {
                        self.push(IrInst::JumpIfNot {
                            temp: c,
                            label: l_else,
                        });
                        // then 块是独立作用域（对齐 oracle `exec_block`）：块内变量/
                        // defer 随块结束（弹栈）——defer 时序依赖此作用域边界。
                        self.push_scope();
                        for stmt in &ifs.then_b.stmts {
                            self.lower_stmt(stmt);
                        }
                        self.pop_scope();
                    }
                }
                match &ifs.else_b {
                    Some(else_b) => {
                        self.push(IrInst::Jump { label: l_end });
                        self.label(l_else);
                        if let Some(le) = l_err {
                            // err_capture：null 非错误路径不进入 else（else 体仅在
                            // 错误路径执行，err 绑定在作用域内）
                            self.push(IrInst::Jump { label: l_end });
                            self.label(le);
                            if let Some((_, en)) = &ifs.err_capture {
                                self.push_scope();
                                self.bind(en, c);
                            }
                            self.lower_stmt(else_b);
                            if ifs.err_capture.is_some() {
                                self.pop_scope();
                            }
                        } else {
                            self.lower_stmt(else_b);
                        }
                    }
                    None => {
                        self.label(l_else);
                        if let Some(le) = l_err {
                            // err_capture 但无 else（解析器不会产生，防御兜底）
                            self.label(le);
                        }
                    }
                }
                self.label(l_end);
            }
            Stmt::While(w) => {
                let l_top = self.new_label();
                // continue 目标：步进（如有）→ 重测条件（对齐 oracle exec_while）
                let l_cont = self.new_label();
                let l_end = self.new_label();
                // optional 捕获：错误值沿调用链传播（对齐 oracle exec_while `Flow::Return`）
                let l_err = if w.capture.is_some() {
                    Some(self.new_label())
                } else {
                    None
                };
                self.label(l_top);
                let c = self.lower_expr(&w.cond);
                if let Some((_, name)) = &w.capture {
                    if let Some(le) = l_err {
                        self.push(IrInst::JumpIfErr { temp: c, label: le });
                    }
                    // null → 退出循环；否则解包负载绑定捕获名
                    self.push(IrInst::JumpIfNull {
                        temp: c,
                        label: l_end,
                    });
                    self.push_scope();
                    let u = self.alloc_slot();
                    self.push(IrInst::Unwrap { temp: u, a: c });
                    self.bind(name, u);
                    let defer_depth = self.defers.len();
                    self.loops.push(LoopCtx {
                        break_label: l_end,
                        continue_label: l_cont,
                        label: w.label.clone(),
                        defer_depth_at_entry: defer_depth,
                    });
                    self.lower_block(&w.body);
                    self.loops.pop();
                    self.pop_scope();
                } else {
                    self.push(IrInst::JumpIfNot {
                        temp: c,
                        label: l_end,
                    });
                    let defer_depth = self.defers.len();
                    self.loops.push(LoopCtx {
                        break_label: l_end,
                        continue_label: l_cont,
                        label: w.label.clone(),
                        defer_depth_at_entry: defer_depth,
                    });
                    self.lower_block(&w.body);
                    self.loops.pop();
                }
                self.label(l_cont);
                if let Some(step) = &w.step {
                    let _ = self.lower_expr(step);
                }
                self.push(IrInst::Jump { label: l_top });
                if let Some(le) = l_err {
                    // 错误传播：return 错误值（errdefer 按值判定）
                    self.label(le);
                    self.emit_defers(0, ErrPath::Value(c));
                    self.push(IrInst::Return { temp: c });
                }
                self.label(l_end);
            }
            Stmt::Return(e, _) => match e {
                Some(e) => {
                    let t = self.lower_expr(e);
                    // 返回排空函数级 defers：errdefer 仅当返回值为错误值（运行期判定）触发
                    self.emit_defers(0, ErrPath::Value(t));
                    self.push(IrInst::Return { temp: t });
                }
                None => {
                    // void 返回：正常路径（无错误值），仅非 errdefer
                    self.emit_defers(0, ErrPath::Never);
                    self.push(IrInst::ReturnVoid);
                }
            },
            Stmt::Block(b) => self.lower_block(b),
            Stmt::For(f) => self.lower_for(f),
            Stmt::Switch(s) => self.lower_switch(s),
            Stmt::Break(l, span) => {
                if let Some(label) = l {
                    self.lower_labeled_exit(label, true, span);
                } else {
                    self.lower_break(span);
                }
            }
            Stmt::Continue(l, span) => {
                if let Some(label) = l {
                    self.lower_labeled_exit(label, false, span);
                } else {
                    self.lower_continue(span);
                }
            }
            // defer/errdefer（Phase 6）：体降级入缓冲 → 登记 + PushDefer；退出点排空
            Stmt::Defer(e, span) => self.lower_defer(e, false, span),
            Stmt::Errdefer(e, span) => self.lower_defer(e, true, span),
            Stmt::Empty => {}
        }
    }

    /// `for` 循环（对齐 oracle `exec_for`/`iter_items`）：
    /// IterMake 展开迭代项 → 每项 IterNext 重绑定捕获槽 → 循环体 →（Mut/Move）写回。
    /// continue → l_next（重新取下一项）；break → l_end。
    pub(crate) fn lower_for(&mut self, f: &ForStmt) {
        let base = self.lower_expr(&f.iter);
        let iter = self.alloc_slot();
        self.push(IrInst::IterMake { temp: iter, base });
        // 捕获槽：每个迭代由 IterNext 重绑定（Read → 值副本；Mut/Move → 共享源 cell）
        let slot = self.alloc_slot();
        let read_only = matches!(f.capture, CaptureMode::Read);

        // 捕获名作用域（循环结束后弹出，防泄漏）
        self.push_scope();
        self.bind(&f.capture_name, slot);

        let l_next = self.new_label();
        let l_body = self.new_label();
        let l_end = self.new_label();

        self.label(l_next);
        let has = self.alloc_slot();
        self.push(IrInst::IterNext {
            has,
            iter,
            slot,
            read_only,
        });
        self.push(IrInst::JumpIfNot {
            temp: has,
            label: l_end,
        });
        self.label(l_body);

        let defer_depth = self.defers.len();
        self.loops.push(LoopCtx {
            break_label: l_end,
            continue_label: l_next,
            label: f.label.clone(),
            defer_depth_at_entry: defer_depth,
        });
        self.lower_block(&f.body);
        self.loops.pop();

        // Mut/Move 捕获写回（LLVM 拷贝进出；run_ir 槽 cell 即源 cell → 无操作）
        if !read_only {
            self.push(IrInst::IterWriteBack { iter, slot });
        }
        self.push(IrInst::Jump { label: l_next });
        self.label(l_end);

        self.pop_scope();
    }

    /// 无标签 break：跳到最近循环的结束标签（对齐 oracle 单级跳出）。
    /// 排空 [循环进入时 defer 深度 .. 当前] 的 defers（含嵌套作用域内登记的），
    /// 正常路径（errdefer 不运行）——对齐 oracle `exec_while` 的 `pop_scope(is_err_path=false)`。
    pub(crate) fn lower_break(&mut self, span: &Span) {
        let (depth, label) = match self.loops.last() {
            Some(l) => (l.defer_depth_at_entry, l.break_label),
            None => {
                self.fail("`break` 在循环外", span);
                return;
            }
        };
        self.emit_defers(depth, ErrPath::Never);
        self.push(IrInst::Jump { label });
    }

    /// 无标签 continue：跳到最近循环的 continue 标签（排空该循环体内 defers，同 break）。
    pub(crate) fn lower_continue(&mut self, span: &Span) {
        let (depth, label) = match self.loops.last() {
            Some(l) => (l.defer_depth_at_entry, l.continue_label),
            None => {
                self.fail("`continue` 在循环外", span);
                return;
            }
        };
        self.emit_defers(depth, ErrPath::Never);
        self.push(IrInst::Jump { label });
    }

    /// 带标签 break/continue：从循环栈向外找最近匹配标签的循环，跳到其 break/continue
    /// 标签。排空从目标循环进入深度到当前的 defers——中间层循环体内登记的 defers
    /// （未在各自退出点运行）一并排空；外层（目标循环之外）defers 由后续退出点处理。
    pub(crate) fn lower_labeled_exit(&mut self, label: &str, is_break: bool, span: &Span) {
        let Some(pos) = self
            .loops
            .iter()
            .rposition(|lc| lc.label.as_deref() == Some(label))
        else {
            self.fail(&format!("未找到标签 `:{label}` 对应的循环"), span);
            return;
        };
        let depth = self.loops[pos].defer_depth_at_entry;
        self.emit_defers(depth, ErrPath::Never);
        let jump = if is_break {
            self.loops[pos].break_label
        } else {
            self.loops[pos].continue_label
        };
        self.push(IrInst::Jump { label: jump });
    }

    /// defer/errdefer 降级：体表达式降级入独立缓冲（无控制流指令——硬错误保证重复
    /// 发射安全），登记 + 主流 PushDefer。体在退出点（作用域结束/return/break/
    /// continue/try 错误）由守卫 + 内联体排空。
    pub(crate) fn lower_defer(&mut self, e: &Expr, errdefer: bool, span: &Span) {
        let id = self.next_defer_id;
        self.next_defer_id += 1;
        // 1) 体降级入缓冲（push/label 路由到 pending）
        let prev = self.pending.take();
        self.pending = Some(Vec::new());
        let _ = self.lower_expr(e);
        let mut body = self.pending.take().expect("pending 缓冲已初始化");
        self.pending = prev;
        // 2) 体含 defer 管理指令 → 硬错误（PushDefer/PopDefer/JumpIfNotDefer
        //    重复发射会导致运行期守卫错乱）。跳转/标签/Return 等由 new_label()
        //    保证唯一性，安全可重复发射（如 `defer try f()`）。
        if body.iter().any(|i| is_defer_admin_inst(i)) {
            self.fail("`defer`/`errdefer` 体不允许嵌套 defer 管理指令", span);
            body.clear(); // 避免污染退出点发射
        }
        // 3) 主流登记
        self.push(IrInst::PushDefer { id });
        self.defers.push(DeferRecord { id, body, errdefer });
    }

    /// `switch` 语句：降级为 first-match 线性链（不穷举检查，对齐 oracle `exec_switch`）。
    pub(crate) fn lower_switch(&mut self, s: &SwitchStmt) {
        self.lower_switch_inner(&s.subject, &s.arms, s.has_else, &s.span, None);
    }

    /// switch 通用降级（语句 `value_slot=None`；表达式 `value_slot=Some(t)`）。
    /// 模式链：每个非 Else 模式 MatchTest → JumpIfNot 下一模式；命中 → 臂体 → 跳 l_done。
    /// 全部失败 → 兜底（has_else → else 臂；否则表达式为 Void / 语句无事发生）。
    pub(crate) fn lower_switch_inner(
        &mut self,
        subject: &Expr,
        arms: &[SwitchArm],
        has_else: bool,
        span: &Span,
        value_slot: Option<usize>,
    ) {
        let _ = span;
        let s = self.lower_expr(subject);
        let l_done = self.new_label();
        let l_fb = self.new_label();

        // 平坦化非 Else 模式链（顺序 = 臂序 × 臂内模式序）
        let mut flat: Vec<(&SwitchArm, IrPattern)> = Vec::new();
        for arm in arms {
            for p in &arm.patterns {
                if let Some(p) = to_ir_pattern(p) {
                    flat.push((arm, p));
                }
            }
        }
        let n = flat.len();
        for (i, (arm, p)) in flat.iter().enumerate() {
            let t_pat = self.alloc_slot();
            self.push(IrInst::MatchTest {
                temp: t_pat,
                subject: s,
                pattern: p.clone(),
            });
            let l_next = if i + 1 < n { self.new_label() } else { l_fb };
            self.push(IrInst::JumpIfNot {
                temp: t_pat,
                label: l_next,
            });
            // C3：switch 守卫——模式匹配后检查守卫条件，守卫失败跳下一模式
            if let Some(guard) = &arm.guard {
                let t_guard = self.lower_expr(guard);
                self.push(IrInst::JumpIfNot {
                    temp: t_guard,
                    label: l_next,
                });
            }
            self.emit_switch_arm_body(arm, s, value_slot);
            self.push(IrInst::Jump { label: l_done });
            if i + 1 < n {
                self.label(l_next);
            }
        }

        // 兜底：else 臂（无论其是否还带非 Else 模式——与 oracle 一致，臂体可能被发射两次）
        self.label(l_fb);
        if has_else {
            if let Some(arm) = arms
                .iter()
                .find(|a| a.patterns.iter().any(|p| matches!(p, SwitchPattern::Else)))
            {
                self.emit_switch_arm_body(arm, s, value_slot);
                self.push(IrInst::Jump { label: l_done });
            }
        }
        // 无匹配（oracle `Flow::None`）→ 表达式 Void；语句无事发生
        if let Some(t) = value_slot {
            self.push(IrInst::Const {
                temp: t,
                val: IrConst::Void,
            });
        }
        self.label(l_done);
    }

    /// 发射单臂体：捕获绑定（EnumPayload 负载或 subject 本身）+ 臂体。
    pub(crate) fn emit_switch_arm_body(
        &mut self,
        arm: &SwitchArm,
        subject: usize,
        value_slot: Option<usize>,
    ) {
        // 对齐 oracle `exec_switch_arm`：push_scope → bind capture → exec body → pop_scope
        self.push_scope();
        if let Some((_, name)) = &arm.capture {
            let cap = self.alloc_slot();
            self.push(IrInst::EnumPayload {
                temp: cap,
                a: subject,
            });
            self.bind(name, cap);
        }
        match value_slot {
            Some(t) => self.lower_block_value(&arm.body, t),
            None => self.lower_block(&arm.body),
        }
        self.pop_scope();
    }

    /// 块求值（值 = 最后语句若为表达式，否则 Void；对齐 oracle `exec_block_inner`）。
    /// switch 表达式臂体专用。
    pub(crate) fn lower_block_value(&mut self, b: &Block, t: usize) {
        self.push_scope();
        let n = b.stmts.len();
        let last_is_value = matches!(b.stmts.last(), Some(Stmt::Expr(_)));
        let m = n - usize::from(last_is_value);
        for stmt in &b.stmts[..m] {
            self.lower_stmt(stmt);
        }
        if last_is_value {
            if let Some(Stmt::Expr(e)) = b.stmts.last() {
                let v = self.lower_expr(e);
                self.push(IrInst::Load { temp: t, slot: v });
            }
        } else {
            self.push(IrInst::Const {
                temp: t,
                val: IrConst::Void,
            });
        }
        self.pop_scope();
    }
}
