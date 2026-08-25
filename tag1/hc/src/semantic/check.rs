//! 语义检查主流程：声明与语句的语义分析入口

use super::*;
use crate::ast::*;
use crate::diag::Diagnostic;

impl Checker {
    // ---------- 第二遍：检查 ----------

    pub(crate) fn check_program(&mut self, program: &Program) {
        for d in &program.decls {
            self.check_decl(d);
        }
    }

    pub(crate) fn check_decl(&mut self, d: &Decl) {
        match d {
            Decl::Fn {
                name,
                ret,
                body,
                is_test,
                span,
                params,
                is_async,
                extension_of,
                ..
            } => {
                let _ = (name, is_test, span);
                // 组 E E1：async fn 必须声明返回类型（调用点需 `Future(R)` 包装）
                if *is_async && ret.is_none() {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        "`async fn` must declare a return type (call sites wrap it in `Future(R)`)",
                    ));
                }
                // 组 D D4：缓存 anytype 函数体——调用点具体化解析 `anytype` 返回类型用
                // （ADR-0012 #5：anytype 参数 = 参数类型不预先绑定，调用点按实参类型实例化）
                if params.iter().any(|p| matches!(p.ty.strip(), Type::Infer)) {
                    self.anytype_bodies.insert(name.clone(), body.clone());
                }
                let ret_ty = self.ret_stype(ret);
                let constraint = self.fn_error_constraint(ret);
                let mut scopes: Vec<HashMap<String, VarInfo>> = Vec::new();
                // 函数参数登记（含 main(io: Io) 内建注入）
                scopes.push(HashMap::new());
                for p in params {
                    let st = self.ty_of(&p.ty);
                    scopes.last_mut().unwrap().insert(
                        p.name.clone(),
                        VarInfo {
                            ty: Some(st),
                            pending_fields: None,
                            // 参数来源由调用点决定（o T 拥有 / 借用）——保守放行
                            source: AllocSource::Unknown,
                            thread: None,
                        },
                    );
                }
                // Q15：扩展方法体内不能访问私有字段——保存当前扩展目标并设置
                let prev_ext = self.extension_of.clone();
                self.extension_of = extension_of.clone();
                // M2.3：未标注返回类型 → 从 return 表达式收集（多路径统一推断）
                self.collect_infer_ret = ret_ty.is_none();
                self.infer_ret = None;
                self.infer_ret_conflict = false;
                self.check_block(body, &mut scopes, constraint, ret_ty);
                self.collect_infer_ret = false;
                // 恢复之前的扩展目标
                self.extension_of = prev_ext;
            }
            Decl::Class {
                name,
                ifaces,
                fields,
                methods,
                ..
            } => {
                // C3：Send/Sync 标记接口验证——声明类字段必须满足对应标记
                for iface in ifaces {
                    let iface_name = match iface.strip() {
                        Type::Named(n, _) => n.as_str(),
                        _ => continue,
                    };
                    match iface_name {
                        "Send" => {
                            for f in fields {
                                let ft = self.ty_of(&f.ty);
                                if !self.type_is_send(&ft) {
                                    self.diags.push(Diagnostic::error(
                                        f.span.clone(),
                                        format!(
                                            "field `{}` of type `{}` does not satisfy `Send`: \
                                             `{}` is not Send",
                                            f.name,
                                            name,
                                            ft.name()
                                        ),
                                    ));
                                }
                            }
                        }
                        "Sync" => {
                            for f in fields {
                                let ft = self.ty_of(&f.ty);
                                if !self.type_is_sync(&ft) {
                                    self.diags.push(Diagnostic::error(
                                        f.span.clone(),
                                        format!(
                                            "field `{}` of type `{}` does not satisfy `Sync`",
                                            f.name, name
                                        ),
                                    ));
                                }
                            }
                        }
                        _ => {}
                    }
                }
                // C5-2：无限大小类型检测——检测值嵌入自引用/互递归（无间接层）
                for f in fields {
                    let ft = self.ty_of(&f.ty);
                    let mut path = vec![name.clone()];
                    if let Some(cycle) = self.detect_type_cycle(name, &mut path, &ft) {
                        self.diags.push(Diagnostic::error(
                            f.span.clone(),
                            format!(
                                "type `{}` has infinite size (field `{}` creates a cycle: {})",
                                name,
                                f.name,
                                cycle.join(" → ")
                            ),
                        ));
                        break; // 每个类只报一次错误
                    }
                }
                for m in methods {
                    let ret_ty = self.ret_stype(&m.ret);
                    let constraint = self.fn_error_constraint(&m.ret);
                    let mut scopes: Vec<HashMap<String, VarInfo>> = Vec::new();
                    // self 参数注入：按声明形态（*Self 只读 / *mut Self 可写 / Self 值）
                    let self_ty = match m.params.first() {
                        Some(p) if p.name == "self" => match p.ty.strip() {
                            Type::Ptr(_, mut_) => {
                                SType::Ptr(Box::new(SType::Named(name.clone(), vec![])), *mut_)
                            }
                            Type::Named(n, _) if n == "Self" => SType::Named(name.clone(), vec![]),
                            _ => SType::Ptr(Box::new(SType::Named(name.clone(), vec![])), false),
                        },
                        _ => SType::Ptr(Box::new(SType::Named(name.clone(), vec![])), false),
                    };
                    scopes.push(HashMap::new());
                    scopes.last_mut().unwrap().insert(
                        "self".into(),
                        VarInfo {
                            ty: Some(self_ty),
                            pending_fields: None,
                            source: AllocSource::Unknown,
                            thread: None,
                        },
                    );
                    // 方法参数（self 显式声明时已含；此处避免重复登记由 check_block 内 params
                    // 处理——方法参数在 body 检查时按 params 登记，见 check_method_params）
                    let _ = constraint;
                    self.check_method_params(m, &mut scopes);
                    // M2.3：未标注返回类型 → 从 return 表达式收集
                    self.collect_infer_ret = ret_ty.is_none();
                    self.infer_ret = None;
                    self.infer_ret_conflict = false;
                    self.check_block(&m.body, &mut scopes, constraint, ret_ty);
                    self.collect_infer_ret = false;
                }
            }
            Decl::Global {
                name,
                ty,
                init,
                pub_: _,
                span,
            } => {
                // 全局初始化宽度检查
                if let (Some(t), Some(Expr::IntLit { text, .. })) = (ty, init) {
                    if let Type::Named(tn, _) = t.strip() {
                        self.check_int_width_str(tn, text, span);
                    }
                }
                let _ = name;
            }
            Decl::Namespace { decls, .. } => {
                for inner in decls {
                    self.check_decl(inner);
                }
            }
            Decl::Const { name, init, .. } => {
                let _ = (name, init); // 常量表达式类型检查放行（简化）
            }
            // 组 D D4：comptime 块按函数体做类型检查（收窄溢出/类型不匹配在块内捕获）。
            // ret_ty/err_constraint = None——块非函数；`return error.X` 是其失败机制，
            // 经 in_comptime_block 守卫在 Stmt::Return 处放宽。
            Decl::Comptime { body, .. } => {
                let mut scopes: Vec<HashMap<String, VarInfo>> = Vec::new();
                let prev = self.in_comptime_block;
                self.in_comptime_block = true;
                self.check_block(body, &mut scopes, None, None);
                self.in_comptime_block = prev;
            }
            _ => {}
        }
    }

    /// 方法体检查前登记显式参数（不含 self——self 已在作用域）
    pub(crate) fn check_method_params(
        &mut self,
        m: &Method,
        scopes: &mut Vec<HashMap<String, VarInfo>>,
    ) {
        let mut params: Vec<Param> = m.params.clone();
        if params.first().map_or(false, |p| p.name == "self") {
            params.remove(0);
        }
        for p in params {
            let st = self.ty_of(&p.ty);
            scopes.last_mut().unwrap().insert(
                p.name,
                VarInfo {
                    ty: Some(st),
                    pending_fields: None,
                    source: AllocSource::Unknown,
                    thread: None,
                },
            );
        }
    }

    /// 函数返回类型 → 静态类型（供 return 期望类型传播）
    /// 未标注返回类型 → None（M2.3：从 return 表达式推断，多路径一致性检查）
    pub(crate) fn ret_stype(&self, ret: &Option<Type>) -> Option<SType> {
        match ret {
            Some(t) => Some(self.ty_of(t)),
            None => None,
        }
    }

    /// 当前函数返回的错误集约束（Some(集合名)）；None = anyerror/无约束
    pub(crate) fn fn_error_constraint(&self, ret: &Option<Type>) -> Option<String> {
        match ret {
            Some(Type::ErrorUnion(Some(err), _)) => match err.strip() {
                Type::Named(n, _) => Some(n.clone()),
                _ => None,
            },
            Some(Type::ErrorUnion(None, _)) => None, // anyerror：不检查
            _ => None,
        }
    }

    pub(crate) fn check_block(
        &mut self,
        b: &Block,
        scopes: &mut Vec<HashMap<String, VarInfo>>,
        err_constraint: Option<String>,
        ret_ty: Option<SType>,
    ) {
        scopes.push(HashMap::new());
        self.owned_stack.push(Vec::new());
        for stmt in &b.stmts {
            self.check_stmt(stmt, scopes, err_constraint.clone(), ret_ty.clone());
        }
        // G3 Q18：作用域退出——未 join/未 detach 线程逃逸检查（运行时提升到根回收）
        if let Some(scope) = scopes.last() {
            self.thread_escape_sweep(scope);
        }
        // 2026-08-25：检查未匹配 defer/move 的 owned 变量
        if let Some(remaining) = self.owned_stack.last() {
            for name in remaining {
                self.diags.push(Diagnostic::warning(
                    b.span.clone(),
                    format!(
                        "`{name}` is `owned` but has no matching `defer` or `move`;
                         use `defer {name}.deinit()` or `move {name}` to transfer ownership"
                    ),
                ));
            }
        }
        self.owned_stack.pop();
        scopes.pop();
    }

    pub(crate) fn check_stmt(
        &mut self,
        s: &Stmt,
        scopes: &mut Vec<HashMap<String, VarInfo>>,
        err_constraint: Option<String>,
        ret_ty: Option<SType>,
    ) {
        match s {
            Stmt::Block(inner) => self.check_block(inner, scopes, err_constraint, ret_ty),
            Stmt::VarDecl {
                name,
                ty,
                init,
                span,
                ..
            } => {
                let declared = ty.as_ref().map(|t| self.ty_of(t));
                // G3：spawn 初始化 → spawn_capture_info 提取 Thread(T) + 捕获分类
                // （Q18/Q19 静态检查）；普通初始化走 expr_ty。
                let mut spawn_thread: Option<ThreadState> = None;
                let init_ty = match init {
                    Some(Expr::Call { callee, args, .. }) if matches!(callee.as_ref(), Expr::Ident(f, _) if f == "spawn") =>
                    {
                        let (t, ts) = self.spawn_capture_info(args, span, scopes);
                        spawn_thread = Some(ts);
                        Some(t)
                    }
                    Some(e) => self.expr_ty(e, scopes, declared.as_ref()),
                    None => None,
                };
                // G3 Q18：初始化表达式中的线程方法调用（`var r = try th.join()`）
                if let Some(e) = init {
                    self.track_thread_method(e, scopes);
                }
                // 惰性宽度定型：var x: u8 = 256（期望类型已知时检查字面量范围）
                if let (Some(SType::Int { width: w }), Some(Expr::IntLit { text, .. })) =
                    (declared.as_ref(), init)
                {
                    if !matches!(w, IntWidth::Comptime) {
                        self.check_int_width_st(declared.as_ref().unwrap(), text, span);
                    }
                }
                // 期望类型兼容检查（可精确判定时）
                self.check_assignable(&declared, &init_ty, span, "variable initialization");
                // 引用赋值禁止（保守）：var x: 引用类型 = 直接变量复制（非 copy）
                if let Some(init) = init {
                    if let Expr::Ident(src, _) = init {
                        let src_ty = self.lookup_var_ty(src, scopes);
                        let is_ref = match (&src_ty, &declared) {
                            (Some(st), _) => self.type_is_ref_st(st),
                            (None, Some(dt)) => self.type_is_ref_st(dt),
                            _ => false,
                        };
                        if is_ref {
                            self.diags.push(Diagnostic::error(
                                span.clone(),
                                format!(
                                    "cannot assign reference type `{src}` by value; \
                                     use `copy(&{src})` for explicit copy or a pointer"
                                ),
                            ));
                        }
                    }
                }
                // definite assignment（C7）：alloc.init(T) 无参构造 → 跟踪待初始化字段
                let pending = self.alloc_init_pending(init.as_ref());
                // M2.4：分配来源（move 合法性用）
                let source = self.infer_source(init.as_ref(), init_ty.as_ref());
                let var_ty = declared.or(init_ty);
                scopes.last_mut().unwrap().insert(
                    name.clone(),
                    VarInfo {
                        ty: var_ty,
                        pending_fields: pending,
                        source,
                        thread: spawn_thread,
                    },
                );
                // 2026-08-25：owned 类型变量登记到 owned_stack
                if let Some(Type::Owned(_)) = ty {
                    self.owned_stack.last_mut().unwrap().push(name.clone());
                }
            }
            Stmt::ConstDecl { name, init, .. } => {
                let t = self.expr_ty(init, scopes, None);
                let source = self.infer_source(Some(init), t.as_ref());
                scopes.last_mut().unwrap().insert(
                    name.clone(),
                    VarInfo {
                        ty: t,
                        pending_fields: None,
                        source,
                        thread: None,
                    },
                );
            }
            Stmt::Expr(Expr::Assign {
                target,
                value,
                span,
                ..
            }) => {
                let target_ty = self.expr_ty(target, scopes, None);
                let value_ty = self.expr_ty(value, scopes, target_ty.as_ref());
                self.check_assignable(&target_ty, &value_ty, span, "assignment");
                // M2.3 指针形态：写只读指针 → 编译错误
                self.check_ptr_write(target, scopes, span);
                // G3 Q19：spawn→join 冻结窗口——写入被引用捕获目标 → 编译错误
                self.check_thread_freeze(target, scopes, span);
                // 字段赋值 x.field = v → 消除 definite assignment 待初始化字段
                let (x, field): (Option<&str>, Option<&str>) = match target.as_ref() {
                    Expr::Dot { base, field, .. } | Expr::Field { base, field, .. } => {
                        match base.as_ref() {
                            Expr::Ident(x, _) => (Some(x), Some(field)),
                            _ => (None, None),
                        }
                    }
                    _ => (None, None),
                };
                if let (Some(x), Some(field)) = (x, field) {
                    for s in scopes.iter_mut().rev() {
                        if let Some(info) = s.get_mut(x) {
                            if let Some(pending) = &mut info.pending_fields {
                                pending.remove(field);
                            }
                            break;
                        }
                    }
                }
            }
            Stmt::Expr(e) => {
                // G3 Q18：线程 join/detach 语句位置跟踪
                self.track_thread_method(e, scopes);
                // 2026-08-25：move 表达式 → 标记对应 owned 变量
                if let Expr::Move(inner, _) = &*e {
                    if let Expr::Ident(name, _) = inner.as_ref() {
                        self.mark_moved(name);
                    }
                }
                let _ = self.expr_ty(e, scopes, None);
            }
            Stmt::If(ifs) => {
                let ct = self.expr_ty(&ifs.cond, scopes, None);
                self.check_condition(ct.as_ref(), &ifs.cond.span());
                // G3 Q18：条件体内 join 不保证执行 → 非直线路径（不视为绑定）
                self.conditional_depth += 1;
                // 捕获：if (maybe) |v|——optional → 内层类型；错误联合 → 成功负载类型
                if let Some((_, n)) = &ifs.capture {
                    let cap_ty = match &ct {
                        Some(SType::Optional(inner)) => Some(inner.as_ref().clone()),
                        Some(SType::ErrorUnion(_, inner)) => Some(inner.as_ref().clone()),
                        _ => None,
                    };
                    scopes.push(HashMap::new());
                    scopes.last_mut().unwrap().insert(
                        n.clone(),
                        VarInfo {
                            ty: cap_ty,
                            pending_fields: None,
                            source: AllocSource::Unknown,
                            thread: None,
                        },
                    );
                    self.check_block(&ifs.then_b, scopes, err_constraint.clone(), ret_ty.clone());
                    scopes.pop();
                } else {
                    self.check_block(&ifs.then_b, scopes, err_constraint.clone(), ret_ty.clone());
                }
                if let Some(else_b) = &ifs.else_b {
                    // 错误捕获：else |err|——err 绑定为错误联合值（错误值，无负载）
                    if let Some((_, en)) = &ifs.err_capture {
                        let err_ty = match &ct {
                            Some(SType::ErrorUnion(e, _)) => {
                                Some(SType::ErrorUnion(e.clone(), Box::new(SType::Unknown)))
                            }
                            _ => Some(SType::ErrorUnion(None, Box::new(SType::Unknown))),
                        };
                        scopes.push(HashMap::new());
                        scopes.last_mut().unwrap().insert(
                            en.clone(),
                            VarInfo {
                                ty: err_ty,
                                pending_fields: None,
                                source: AllocSource::Unknown,
                                thread: None,
                            },
                        );
                        self.check_stmt(else_b, scopes, err_constraint, ret_ty);
                        scopes.pop();
                    } else {
                        self.check_stmt(else_b, scopes, err_constraint, ret_ty);
                    }
                }
                self.conditional_depth -= 1;
            }
            Stmt::While(w) => {
                let ct = self.expr_ty(&w.cond, scopes, None);
                self.check_condition(ct.as_ref(), &w.cond.span());
                self.conditional_depth += 1;
                // optional 捕获：while (maybe) |v|——Some 绑定 v 并循环
                if let Some((_, n)) = &w.capture {
                    let cap_ty = match &ct {
                        Some(SType::Optional(inner)) => Some(inner.as_ref().clone()),
                        Some(SType::ErrorUnion(_, inner)) => Some(inner.as_ref().clone()),
                        _ => None,
                    };
                    scopes.push(HashMap::new());
                    scopes.last_mut().unwrap().insert(
                        n.clone(),
                        VarInfo {
                            ty: cap_ty,
                            pending_fields: None,
                            source: AllocSource::Unknown,
                            thread: None,
                        },
                    );
                    self.check_block(&w.body, scopes, err_constraint, ret_ty);
                    scopes.pop();
                } else {
                    self.check_block(&w.body, scopes, err_constraint, ret_ty);
                }
                self.conditional_depth -= 1;
            }
            Stmt::For(f) => {
                let it = self.expr_ty(&f.iter, scopes, None);
                self.check_iterable(it.as_ref(), &f.iter.span());
                scopes.push(HashMap::new());
                scopes.last_mut().unwrap().insert(
                    f.capture_name.clone(),
                    VarInfo {
                        ty: None, // 元素类型：Map 为键值对，集合元素——保守放行
                        pending_fields: None,
                        source: AllocSource::Unknown,
                        thread: None,
                    },
                );
                // G3 Q18：循环体内 join 非直线路径
                self.conditional_depth += 1;
                self.check_block(&f.body, scopes, err_constraint, ret_ty);
                self.conditional_depth -= 1;
                scopes.pop();
            }
            Stmt::Switch(sw) => {
                let st = self.expr_ty(&sw.subject, scopes, None);
                // C3：switch 守卫验证——每个守卫必须是 bool 类型
                for arm in &sw.arms {
                    if let Some(guard) = &arm.guard {
                        let guard_ty = self.expr_ty(guard, scopes, Some(&SType::Bool));
                        if let Some(t) = guard_ty {
                            if !matches!(t, SType::Bool) {
                                self.diags.push(Diagnostic::error(
                                    guard.span(),
                                    format!(
                                        "switch guard must be a bool expression (got `{}`)",
                                        t.name()
                                    ),
                                ));
                            }
                        }
                    }
                }
                // C3：穷举性检查——至少一个非守卫臂或 else 臂
                let has_non_guard = sw.arms.iter().any(|arm| arm.guard.is_none());
                let has_else = sw.arms.iter().any(|arm| {
                    arm.patterns
                        .iter()
                        .any(|p| matches!(p, SwitchPattern::Else))
                });
                if !has_non_guard && !has_else {
                    self.diags.push(Diagnostic::error(
                        sw.span.clone(),
                        "switch with guards must have at least one non-guard branch or `else` arm \
                         for exhaustiveness",
                    ));
                }
                // G3 Q18：分支体内 join 非直线路径
                self.conditional_depth += 1;
                for arm in &sw.arms {
                    scopes.push(HashMap::new());
                    if let Some((_, n)) = &arm.capture {
                        scopes.last_mut().unwrap().insert(
                            n.clone(),
                            VarInfo {
                                ty: match &st {
                                    Some(SType::Named(_, _)) => st.clone(),
                                    _ => None,
                                },
                                pending_fields: None,
                                source: AllocSource::Unknown,
                                thread: None,
                            },
                        );
                    }
                    self.check_block(&arm.body, scopes, err_constraint.clone(), ret_ty.clone());
                    scopes.pop();
                }
                self.conditional_depth -= 1;
            }
            Stmt::Return(e, span) => {
                // M2.4 Q18：返回值引用被编译期禁止（引用逃逸到比目标更长寿的
                // 作用域 = 悬垂唯一产生路径）——局部变量与参数均不可；
                // 带所有权参数须 `return move param` 转移所有权
                if let Some(Expr::AddrOf(inner, _, _)) = e {
                    if let Expr::Ident(name, _) = inner.as_ref() {
                        if scopes.iter().rev().any(|s| s.contains_key(name)) {
                            self.diags.push(Diagnostic::error(
                                span.clone(),
                                format!(
                                    "cannot return reference to `{name}`: reference escapes \
                                     function scope（若 `{name}` 拥有所有权，用 `return move {name}` 转移）"
                                ),
                            ));
                        }
                    }
                }
                // M2.6：错误传播模型——函数声明了错误联合（E!T/!T）→ error.X 沿调用链
                // 传播直到 try/catch 处理；**未标记错误类型**（返回值非错误联合）→ 编译错误
                // （错误不进入传播链，运行时由根作用域记录输出后 panic 式中止）
                let ret_is_error_union = matches!(&ret_ty, Some(SType::ErrorUnion(..)));
                if let Some(Expr::ErrorLit(ename, _)) = e {
                    // comptime 块：`return error.X` 是其失败机制（装载期求值失败 = 编译错误），
                    // 不适用「未声明错误联合」报错
                    if !ret_is_error_union && !self.in_comptime_block {
                        self.diags.push(Diagnostic::error(
                            span.clone(),
                            format!(
                                "cannot return `error.{ename}`: function does not declare an \
                                 error union return type (`E!T` / `!T`)——未标记错误类型，\
                                 错误不参与传播链"
                            ),
                        ));
                    } else if let Some(constraint) = &err_constraint {
                        // 错误集成员检查：return error.X 必须属于函数返回的错误集
                        let members = self.error_sets.get(constraint);
                        match members {
                            Some(set) if set.contains(ename) => {}
                            Some(_) => {
                                self.diags.push(Diagnostic::error(
                                    span.clone(),
                                    format!(
                                        "error `error.{ename}` not in declared error set `{constraint}`"
                                    ),
                                ));
                            }
                            None => {
                                // 错误集未收集到（如内建/别名未解析）——不拦截
                            }
                        }
                    }
                }
                // 期望类型传播：return 表达式的类型 vs 函数返回类型
                // （return error.X = 返回错误值，非 payload——错误集成员检查已单独处理；
                //  return f() 且 f 返回 E!T → 错误联合直接传递，跳过 payload 检查）
                if let Some(Expr::ErrorLit(..)) = e {
                    // 跳过 payload 兼容检查
                } else if let Some(e) = e {
                    let et = self.expr_ty(e, scopes, ret_ty.as_ref());
                    if let Some(expect) = &ret_ty {
                        // 拆错误联合：E!T 的 payload 期望是 T
                        let payload = match expect {
                            SType::ErrorUnion(_, inner) => inner.as_ref(),
                            other => other,
                        };
                        // 错误联合值 → 错误联合函数：直接传递（错误或 payload 都合法）
                        let error_union_pass = matches!(expect, SType::ErrorUnion(..))
                            && matches!(&et, Some(SType::ErrorUnion(..)));
                        if !error_union_pass {
                            self.check_assignable(&Some(payload.clone()), &et, span, "return");
                        }
                    }
                    // M2.3：未标注返回类型 → 收集 return 类型，多路径不一致报错要求显式
                    if self.collect_infer_ret {
                        if let Some(t) = &et {
                            if t.definite() && !matches!(t, SType::Void) {
                                match self.infer_ret.take() {
                                    Some(prev) => {
                                        if !self.ret_infer_unifies(&prev, t)
                                            && !self.infer_ret_conflict
                                        {
                                            self.diags.push(Diagnostic::error(
                                                span.clone(),
                                                format!(
                                                    "inferred return type mismatch across paths: `{}` vs `{}`; annotate the return type explicitly",
                                                    prev.name(),
                                                    t.name()
                                                ),
                                            ));
                                            self.infer_ret_conflict = true;
                                        }
                                        self.infer_ret = Some(prev);
                                    }
                                    None => self.infer_ret = Some(t.clone()),
                                }
                            }
                        }
                    }
                }
                // definite assignment（C7 保守版）：返回未完全初始化的 alloc.init(T) 实例
                if let Some(Expr::Ident(name, _)) = e {
                    let missing = self.missing_fields(name, scopes);
                    if let Some(fields) = missing {
                        if !fields.is_empty() {
                            self.diags.push(Diagnostic::error(
                                span.clone(),
                                format!(
                                    "cannot return partially-initialized `{name}`; \
                                     missing field(s): {}",
                                    fields.iter().cloned().collect::<Vec<_>>().join(", ")
                                ),
                            ));
                        }
                    }
                }
            }
            Stmt::Defer(expr, _) => {
                let _ = self.expr_ty(expr, scopes, None);
                self.covered_by_defer(expr);
            }
            Stmt::Errdefer(expr, _) => {
                let _ = self.expr_ty(expr, scopes, None);
                self.covered_by_defer(expr);
            }
            _ => {}
        }
    }

    /// 2026-08-25：defer/errdefer 表达式中引用的 owned 变量标记为已覆盖
    fn covered_by_defer(&mut self, expr: &Expr) {
        let mut refs = Vec::new();
        Self::collect_idents(expr, &mut refs);
        for name in &refs {
            self.mark_covered(name);
        }
    }

    /// 收集表达式中的所有标识符引用
    fn collect_idents(expr: &Expr, out: &mut Vec<String>) {
        match expr {
            Expr::Ident(name, _) => out.push(name.clone()),
            Expr::Call { callee, args, .. } => {
                Self::collect_idents(callee, out);
                for arg in args {
                    Self::collect_idents(arg, out);
                }
            }
            Expr::Dot { base, .. } | Expr::Field { base, .. } => Self::collect_idents(base, out),
            Expr::Index { base, indices, .. } => {
                Self::collect_idents(base, out);
                for idx in indices {
                    Self::collect_idents(idx, out);
                }
            }
            Expr::Unary(_, inner, _)
            | Expr::Deref(inner, _)
            | Expr::AddrOf(inner, _, _)
            | Expr::Try(inner, _)
            | Expr::Await(inner, _)
            | Expr::Move(inner, _) => {
                Self::collect_idents(inner, out);
            }
            Expr::Binary(_, left, right, _) | Expr::Orelse(left, right, _) => {
                Self::collect_idents(left, out);
                Self::collect_idents(right, out);
            }
            Expr::Catch(expr, kind, _) => {
                Self::collect_idents(expr, out);
                match kind.as_ref() {
                    CatchKind::Default(e) => Self::collect_idents(e, out),
                    CatchKind::Bind { name: _, body } => {
                        for stmt in &body.stmts {
                            if let Stmt::Expr(e) = stmt {
                                Self::collect_idents(e, out);
                            }
                        }
                    }
                }
            }
            Expr::IfExpr {
                cond,
                then_e,
                else_e,
                ..
            } => {
                Self::collect_idents(cond, out);
                Self::collect_idents(then_e, out);
                Self::collect_idents(else_e, out);
            }
            Expr::Block(inner, _) => {
                for stmt in &inner.stmts {
                    if let Stmt::Expr(e) = stmt {
                        Self::collect_idents(e, out);
                    }
                }
            }
            Expr::Closure { body, .. } => {
                for stmt in &body.stmts {
                    if let Stmt::Expr(e) = stmt {
                        Self::collect_idents(e, out);
                    }
                }
            }
            _ => {}
        }
    }

    /// 2026-08-25：标记 owned 变量已被 defer 覆盖
    fn mark_covered(&mut self, name: &str) {
        for scope in self.owned_stack.iter_mut().rev() {
            scope.retain(|v| v != name);
        }
    }

    /// 2026-08-25：标记 owned 变量已被 move 转移
    fn mark_moved(&mut self, name: &str) {
        for scope in self.owned_stack.iter_mut().rev() {
            scope.retain(|v| v != name);
        }
    }

    /// C5-2：检测类型图是否包含无限大自引用（值嵌入无间接层）。
    /// 返回 Some(cycle_path) 如果检测到非法循环，None 表示安全。
    /// `root` = 正在检查的类名，`path` = 当前类型链（用于路径追踪和防重复）。
    fn detect_type_cycle(
        &self,
        root: &str,
        path: &mut Vec<String>,
        t: &SType,
    ) -> Option<Vec<String>> {
        match t {
            // 指针/可选/切片 → 间接层，安全（固定大小）
            SType::Ptr(_, _) | SType::Optional(_) | SType::Slice(_) => None,
            SType::Named(n, _) => {
                // 集合类型 Vec/Map/Deque/Table/String → 堆分配，安全
                if is_collection(n) {
                    return None;
                }
                // 发现根类型 → 循环
                if n == root {
                    let mut cycle = path.clone();
                    cycle.push(n.clone());
                    return Some(cycle);
                }
                // 已在路径中 → 菱形引用，非循环，跳过防无限递归
                if path.contains(n) {
                    return None;
                }
                // 递归检查该类的字段
                if let Some(TypeInfo {
                    kind: TypeKind::Class { fields, .. },
                    ..
                }) = self.types.get(n)
                {
                    path.push(n.clone());
                    for f in fields {
                        let ft = self.ty_of(&f.ty);
                        if let Some(cycle) = self.detect_type_cycle(root, path, &ft) {
                            return Some(cycle);
                        }
                    }
                    path.pop();
                }
                // Struct 同样需要循环检测
                if let Some(TypeInfo {
                    kind: TypeKind::Struct { fields, .. },
                    ..
                }) = self.types.get(n)
                {
                    path.push(n.clone());
                    for f in fields {
                        let ft = self.ty_of(&f.ty);
                        if let Some(cycle) = self.detect_type_cycle(root, path, &ft) {
                            return Some(cycle);
                        }
                    }
                    path.pop();
                }
                None
            }
            SType::Array(_, inner) => self.detect_type_cycle(root, path, inner),
            SType::Tuple(items) => {
                for item in items {
                    if let Some(cycle) = self.detect_type_cycle(root, path, item) {
                        return Some(cycle);
                    }
                }
                None
            }
            SType::ErrorUnion(_, inner) => self.detect_type_cycle(root, path, inner),
            // 基础类型/泛型/未知 → 安全
            _ => None,
        }
    }
}
