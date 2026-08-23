//! 表达式类型推断（M2.2 核心）+ 校验辅助 + 调用检查 + spawn 捕获 + 变量查询。

use super::*;
use crate::ast::*;
use crate::diag::Diagnostic;
use crate::token::Span;

impl Checker {
    // ---------- 表达式类型推断（M2.2 核心） ----------

    pub(crate) fn expr_ty(
        &mut self,
        e: &Expr,
        scopes: &[HashMap<String, VarInfo>],
        expected: Option<&SType>,
    ) -> Option<SType> {
        let t = match e {
            Expr::IntLit { .. } => {
                // 惰性宽度：期望类型已知且为整数 → 定型；否则保持惰性（使用处定型）
                match expected {
                    Some(SType::Int { width }) if !matches!(width, IntWidth::Comptime) => {
                        SType::Int { width: *width }
                    }
                    Some(SType::Float) => SType::Float,
                    _ => SType::Int {
                        width: IntWidth::Comptime,
                    },
                }
            }
            Expr::FloatLit { .. } => SType::Float,
            Expr::StrLit { .. } => SType::Str,
            Expr::CharLit(..) => SType::Int {
                width: IntWidth::U8,
            },
            Expr::BoolLit(..) => SType::Bool,
            Expr::NullLit(..) => SType::Optional(Box::new(SType::Unknown)),
            Expr::VoidLit(..) => SType::Void,
            Expr::Ident(name, _) => match self.lookup_var_ty(name, scopes) {
                Some(t) => t,
                None => SType::Unknown,
            },
            Expr::ArrayLit(items, _) => {
                // 期望为切片/数组/集合时元素期望传播；无期望 → 定长数组 [N]T
                let elem_exp = match expected {
                    Some(SType::Slice(inner)) => Some(inner.as_ref()),
                    Some(SType::Array(_, inner)) => Some(inner.as_ref()),
                    Some(SType::Named(n, args)) if n == "Vec" => args.first(),
                    _ => None,
                };
                let mut et: Option<SType> = None;
                for it in items {
                    let it_ty = self.expr_ty(it, scopes, elem_exp);
                    if let (Some(a), Some(b)) = (&et, &it_ty) {
                        if !self.compatible(a, b) {
                            self.diags.push(Diagnostic::error(
                                it.span(),
                                format!(
                                    "array literal element type mismatch: `{}` vs `{}`",
                                    b.name(),
                                    a.name()
                                ),
                            ));
                        }
                    }
                    if et.is_none() {
                        et = it_ty;
                    }
                }
                let elem = et.unwrap_or(SType::Unknown);
                match expected {
                    Some(SType::Array(n, _)) => SType::Array(*n, Box::new(elem)),
                    Some(SType::Named(n, args)) if n == "Vec" => {
                        SType::Named(n.clone(), vec![elem])
                    }
                    _ => SType::Array(items.len(), Box::new(elem)),
                }
            }
            Expr::TupleLit(items, _) => {
                let ts: Vec<SType> = items
                    .iter()
                    .map(|it| self.expr_ty(it, scopes, None).unwrap_or(SType::Unknown))
                    .collect();
                SType::Tuple(ts)
            }
            Expr::NamedLit {
                ty, fields, span, ..
            } => {
                self.check_named_lit(ty, fields, span, scopes);
                match self.types.get(ty) {
                    Some(_) => SType::Named(ty.clone(), vec![]),
                    None => SType::Unknown,
                }
            }
            // struct 类型字面量（E1.2 组 D）：类型值——comptime 类型函数体内求值；
            // 静态类型 = 元类型（无运行时表示），tag1 按 Unknown 放行（具体化由运行时登记）
            Expr::StructType { .. } => SType::Unknown,
            // 数组类型值 `[n]T`（组 D）：同 struct 类型字面量——类型值，按 Unknown 放行
            Expr::ArrayType { .. } => SType::Unknown,
            Expr::Dot { base, field, span } => {
                // 实例访问（v1.append / p2.x / self.count）：parse_primary 把变量成员的第一个 `.` 解析为 Dot
                if let Expr::Ident(n, _) = base.as_ref() {
                    if self.var_exists(n, scopes) {
                        let bt = self.lookup_var_ty(n, scopes);
                        if let Some(t) = &bt {
                            let dt = self.deref_member(t); // 自动解引用（A3）
                            self.check_field_access(Some(dt), field, span);
                            // `.len` 内建字段（切片/集合/String）
                            if field == "len" {
                                if let Some(lt) = self.len_field_ty(dt) {
                                    return Some(lt);
                                }
                            }
                            // class 字段类型
                            if let Some(ft) = self.class_field_ty(dt, field) {
                                return Some(ft);
                            }
                            return Some(t.clone());
                        }
                        return None;
                    }
                    // 类型名.变体（枚举）
                    if let Some(info) = self.types.get(n) {
                        if let TypeKind::Enum { variants } = &info.kind {
                            let ok = variants.iter().any(|v| v.name == *field);
                            if !ok {
                                self.diags.push(Diagnostic::error(
                                    span.clone(),
                                    format!("enum `{n}` has no variant `{field}`"),
                                ));
                            }
                            return Some(SType::Named(n.clone(), vec![]));
                        }
                        // class/接口的静态方法（含内建序列化 to_bytes/from_json 等）：放行
                        return None;
                    }
                    // 解释器注入的内建枚举/类型（ExitType 等）：放行
                    if is_builtin_type(n) {
                        return Some(if n == "String" {
                            SType::Str
                        } else {
                            SType::Named(n.clone(), vec![])
                        });
                    }
                    // 命名空间声明（M1.4）或内建命名空间：放行
                    if self.namespaces.contains(n) || is_builtin_ns(n) {
                        return None;
                    }
                    // 序列化内建方法（Type.from_json 等）：不要求类型登记
                    if is_serialize_builtin(field) {
                        return None;
                    }
                    // 大写未登记名 = 兄弟文件的类型/命名空间（M1.4 多文件）：放行
                    // 小写未登记名（json.parse 等）：库/内建注入未知——保守放行，运行时诊断
                    return None;
                }
                // 其他 . 访问：字段/方法链——放行（Field 分支已覆盖主要校验）
                let bt = self.expr_ty(base, scopes, None);
                if let Some(t) = &bt {
                    let dt = self.deref_member(t);
                    self.check_field_access(Some(dt), field, span);
                } else {
                    self.check_field_access(None, field, span);
                }
                bt.unwrap_or(SType::Unknown)
            }
            Expr::Field { base, field, span } => {
                let bt = self.expr_ty(base, scopes, None);
                // 自动解引用（A3：p.x、p.dist(q)）
                let dt = bt.as_ref().map(|t| self.deref_member(t).clone());
                self.check_field_access(dt.as_ref(), field, span);
                match &dt {
                    Some(SType::Named(n, args)) if !is_builtin_type(n) => {
                        if let Some(TypeKind::Class { fields, .. }) =
                            self.types.get(n).map(|i| &i.kind)
                        {
                            // 字段类型
                            if let Some(fd) = fields.iter().find(|f| f.name == *field) {
                                return Some(self.ty_of(&fd.ty));
                            }
                        }
                        // K1 union：字段类型（读取经字节重解释）
                        if let Some(TypeKind::Union { fields, .. }) =
                            self.types.get(n).map(|i| &i.kind)
                        {
                            if let Some(fd) = fields.iter().find(|f| f.name == *field) {
                                return Some(self.ty_of(&fd.ty));
                            }
                        }
                        SType::Unknown
                    }
                    Some(SType::Tuple(ts)) => {
                        // 元组索引 t.0
                        if let Ok(i) = field.parse::<usize>() {
                            if i < ts.len() {
                                return Some(ts[i].clone());
                            }
                        }
                        SType::Unknown
                    }
                    Some(t) => {
                        // `.len` 内建字段
                        if field == "len" {
                            if let Some(lt) = self.len_field_ty(t) {
                                return Some(lt);
                            }
                        }
                        SType::Unknown
                    }
                    _ => SType::Unknown,
                }
            }
            Expr::Index {
                base,
                indices,
                span,
            } => {
                let bt = self.expr_ty(base, scopes, None);
                let dt = bt.as_ref().map(|t| self.deref_member(t).clone());
                self.check_index(dt.as_ref(), indices, span);
                // 范围索引（arr[1..3] / arr[1..]）= 切片视图
                let is_range =
                    indices.len() == 1 && matches!(&indices[0], Expr::Binary(BinOp::Range, ..));
                if is_range {
                    if let Some(inner) = match &dt {
                        Some(SType::Slice(i)) => Some(i.as_ref().clone()),
                        Some(SType::Array(_, i)) => Some(i.as_ref().clone()),
                        Some(SType::Named(n, args)) if n == "Vec" => args.first().cloned(),
                        _ => None,
                    } {
                        return Some(SType::Slice(Box::new(inner)));
                    }
                }
                match &dt {
                    Some(SType::Slice(inner)) => inner.as_ref().clone(),
                    Some(SType::Array(_, inner)) => inner.as_ref().clone(),
                    Some(SType::Named(n, args)) if n == "Vec" => {
                        args.first().cloned().unwrap_or(SType::Unknown)
                    }
                    Some(SType::Named(n, args)) if n == "Table" => {
                        let inner = args.first().cloned().unwrap_or(SType::Unknown);
                        if indices.len() == 1 {
                            // Row view: t[i] -> Slice<T>
                            SType::Slice(Box::new(inner))
                        } else {
                            // Cell: t[i,j] -> T
                            inner
                        }
                    }
                    Some(SType::Named(n, _)) if n == "Map" => SType::Unknown,
                    _ => SType::Unknown,
                }
            }
            Expr::Deref(inner, _) => match self.expr_ty(inner, scopes, None) {
                Some(SType::Ptr(t, _)) => t.as_ref().clone(),
                _ => SType::Unknown,
            },
            Expr::AddrOf(inner, mut_, _) => {
                let it = self.expr_ty(inner, scopes, None).unwrap_or(SType::Unknown);
                // 切片的引用即切片视图（&arr[1..3] = &[T]）；数组取地址 = *[N]T
                match &it {
                    SType::Slice(_) => it,
                    _ => SType::Ptr(Box::new(it), *mut_),
                }
            }
            Expr::Unary(op, operand, span) => {
                let ot = self.expr_ty(operand, scopes, None);
                match op {
                    UnaryOp::Neg => {
                        if let Some(t) = &ot {
                            if t.definite() && !t.numeric() && !matches!(t, SType::Generic(_)) {
                                self.diags.push(Diagnostic::error(
                                    span.clone(),
                                    format!(
                                        "unary `-` requires a numeric operand (got `{}`)",
                                        t.name()
                                    ),
                                ));
                            }
                        }
                        ot.unwrap_or(SType::Unknown)
                    }
                    UnaryOp::Not => {
                        if let Some(t) = &ot {
                            if t.definite() && !matches!(t, SType::Bool | SType::Generic(_)) {
                                self.diags.push(Diagnostic::error(
                                    span.clone(),
                                    format!("`!` requires a bool operand (got `{}`)", t.name()),
                                ));
                            }
                        }
                        SType::Bool
                    }
                    UnaryOp::BitNot => {
                        if let Some(t) = &ot {
                            if t.definite() && !t.integer() && !matches!(t, SType::Generic(_)) {
                                self.diags.push(Diagnostic::error(
                                    span.clone(),
                                    format!("`~` requires an integer operand (got `{}`)", t.name()),
                                ));
                            }
                        }
                        ot.unwrap_or(SType::Unknown)
                    }
                }
            }
            Expr::Binary(op, l, r, span) => {
                let lt = self.expr_ty(l, scopes, None);
                let rt = self.expr_ty(r, scopes, lt.as_ref());
                self.check_binary(*op, lt.as_ref(), rt.as_ref(), span);
                // 结果类型
                match op {
                    BinOp::Eq
                    | BinOp::Ne
                    | BinOp::Lt
                    | BinOp::Le
                    | BinOp::Gt
                    | BinOp::Ge
                    | BinOp::And
                    | BinOp::Or => SType::Bool,
                    BinOp::Range => SType::Slice(Box::new(SType::Unknown)), // 范围可迭代
                    _ => lt.or(rt).unwrap_or(SType::Unknown),
                }
            }
            Expr::Orelse(l, _, span) => match self.expr_ty(l, scopes, None) {
                Some(SType::Optional(inner)) => inner.as_ref().clone(),
                Some(t) if t.definite() && !matches!(t, SType::Generic(_)) => {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        format!("`orelse` requires an optional value (got `{}`)", t.name()),
                    ));
                    t
                }
                _ => SType::Unknown,
            },
            Expr::Unwrap(inner, span) => match self.expr_ty(inner, scopes, None) {
                Some(SType::Optional(t)) => t.as_ref().clone(),
                Some(t) if t.definite() && !matches!(t, SType::Generic(_)) => {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        format!("`.?` requires an optional value (got `{}`)", t.name()),
                    ));
                    t
                }
                _ => SType::Unknown,
            },
            Expr::Try(inner, span) => match self.expr_ty(inner, scopes, None) {
                Some(SType::ErrorUnion(_, t)) => t.as_ref().clone(),
                // try void 断言惯用法（expect 等）：运行时透传，放行
                Some(t) if t.definite() && !matches!(t, SType::Generic(_) | SType::Void) => {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        format!("`try` requires an error union value (got `{}`)", t.name()),
                    ));
                    t
                }
                _ => SType::Unknown,
            },
            Expr::Await(inner, span) => match self.expr_ty(inner, scopes, None) {
                // 组 E E1：`await fut`——Future(R) 解包 → R（协作式 Future，ADR-0011）
                Some(SType::Named(n, args)) if n == "Future" => {
                    args.into_iter().next().unwrap_or(SType::Unknown)
                }
                Some(t) if t.definite() => {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        format!("`await` requires a Future value (got `{}`)", t.name()),
                    ));
                    t
                }
                _ => SType::Unknown,
            },
            Expr::Catch(inner, _, span) => match self.expr_ty(inner, scopes, None) {
                Some(SType::ErrorUnion(_, t)) => t.as_ref().clone(),
                // catch void 断言（expect 等内建）：运行时处理，放行
                Some(t) if t.definite() && !matches!(t, SType::Generic(_) | SType::Void) => {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        format!("`catch` requires an error union value (got `{}`)", t.name()),
                    ));
                    t
                }
                _ => SType::Unknown,
            },
            Expr::Call { callee, args, span } => {
                return self.check_call(callee, args, span, scopes, expected);
            }
            Expr::IfExpr {
                cond,
                then_e,
                else_e,
                ..
            } => {
                let ct = self.expr_ty(cond, scopes, None);
                self.check_condition(ct.as_ref(), &cond.span());
                let tt = self.expr_ty(then_e, scopes, expected);
                let et = self.expr_ty(else_e, scopes, expected);
                // 分支类型统一：明确且相等 → 该类型；否则放行
                match (tt, et) {
                    (Some(a), Some(b)) if self.compatible(&a, &b) => a,
                    (Some(a), None) => a,
                    (None, Some(b)) => b,
                    _ => SType::Unknown,
                }
            }
            Expr::SwitchExpr { subject, arms, .. } => {
                let _ = self.expr_ty(subject, scopes, None);
                let mut rt: Option<SType> = None;
                for arm in arms {
                    let at = self.expr_ty_arm(&arm.body, scopes);
                    if let Some(a) = at {
                        if let Some(r) = &rt {
                            if !self.compatible(r, &a) {
                                rt = None;
                                break;
                            }
                        } else {
                            rt = Some(a);
                        }
                    }
                }
                rt.unwrap_or(SType::Unknown)
            }
            Expr::Block(b, _) => {
                // 块表达式：最后语句为表达式时取其类型
                let mut sc2 = scopes.to_vec();
                sc2.push(HashMap::new());
                let mut last: Option<SType> = None;
                for st in &b.stmts {
                    match st {
                        Stmt::Expr(inner) => {
                            last = self.expr_ty(inner, &sc2, expected);
                        }
                        Stmt::VarDecl {
                            ty,
                            init,
                            name,
                            span,
                            ..
                        } => {
                            let declared = ty.as_ref().map(|t| self.ty_of(t));
                            let init_ty = match init {
                                Some(x) => self.expr_ty(x, &sc2, declared.as_ref()),
                                None => None,
                            };
                            if let (
                                Some(SType::Int { width: w }),
                                Some(Expr::IntLit { text, .. }),
                            ) = (declared.as_ref(), init)
                            {
                                if !matches!(w, IntWidth::Comptime) {
                                    self.check_int_width_st(&declared.clone().unwrap(), text, span);
                                }
                            }
                            self.check_assignable(
                                &declared,
                                &init_ty,
                                span,
                                "variable initialization",
                            );
                            if let Some(Expr::Ident(src, _)) = init {
                                let src_ty = self.lookup_var_ty(src, &sc2);
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
                            let pending = self.alloc_init_pending(init.as_ref());
                            let source = self.infer_source(init.as_ref(), init_ty.as_ref());
                            let var_ty = declared.or(init_ty);
                            sc2.last_mut().unwrap().insert(
                                name.clone(),
                                VarInfo {
                                    ty: var_ty,
                                    pending_fields: pending,
                                    source,
                                    thread: None,
                                },
                            );
                            last = None;
                        }
                        _ => last = None,
                    }
                }
                last.unwrap_or(SType::Unknown)
            }
            Expr::Assign {
                target,
                value,
                span,
                ..
            } => {
                let target_ty = self.expr_ty(target, scopes, None);
                let value_ty = self.expr_ty(value, scopes, target_ty.as_ref());
                self.check_assignable(&target_ty, &value_ty, span, "assignment");
                target_ty.unwrap_or(SType::Unknown)
            }
            Expr::ErrorLit(_, _) => SType::ErrorUnion(None, Box::new(SType::Unknown)),
            Expr::FnRef(_name, _) => SType::Unknown,
            Expr::TupleDestructure(names, value, span) => {
                let vt = self.expr_ty(value, scopes, None);
                if let Some(SType::Tuple(ts)) = &vt {
                    if names.len() != ts.len() {
                        self.diags.push(Diagnostic::error(
                            span.clone(),
                            format!(
                                "tuple destructure has {} names but value has {} elements",
                                names.len(),
                                ts.len()
                            ),
                        ));
                    }
                }
                vt.unwrap_or(SType::Unknown)
            }
            Expr::Move(inner, span) => {
                // M2.4：move 唯一约束 = 拥有所有权（非 Arena/global/值类型）
                if let Expr::Ident(name, _) = inner.as_ref() {
                    if let Some(src) = self.lookup_var_source(name, scopes) {
                        match src {
                            AllocSource::Arena => self.diags.push(Diagnostic::error(
                                span.clone(),
                                format!(
                                    "cannot move `{name}`: allocated by Arena (ownership \
                                     belongs to the arena; move the whole arena instead)"
                                ),
                            )),
                            AllocSource::Global => self.diags.push(Diagnostic::error(
                                span.clone(),
                                format!(
                                    "cannot move global `{name}` (ownership belongs to \
                                     root scope)"
                                ),
                            )),
                            AllocSource::None => self.diags.push(Diagnostic::error(
                                span.clone(),
                                format!(
                                    "cannot move `{name}`: value type has no ownership \
                                     (move transfers destroy responsibility)"
                                ),
                            )),
                            _ => {}
                        }
                    }
                }
                self.expr_ty(inner, scopes, expected)
                    .unwrap_or(SType::Unknown)
            }
            Expr::Closure { .. } => SType::Unknown,
        };
        Some(t)
    }

    /// switch 臂体（Block 值）类型
    pub(crate) fn expr_ty_arm(
        &mut self,
        b: &Block,
        scopes: &[HashMap<String, VarInfo>],
    ) -> Option<SType> {
        let mut last: Option<SType> = None;
        for st in &b.stmts {
            match st {
                Stmt::Expr(inner) => last = self.expr_ty(inner, scopes, None),
                _ => last = None,
            }
        }
        last
    }

    // ---------- 校验辅助 ----------

    /// 期望类型兼容（可精确判定时）
    pub(crate) fn check_assignable(
        &mut self,
        expected: &Option<SType>,
        actual: &Option<SType>,
        span: &Span,
        ctx: &str,
    ) {
        let (Some(exp), Some(act)) = (expected, actual) else {
            return;
        };
        if exp.definite() && act.definite() && !self.compatible(act, exp) {
            self.diags.push(Diagnostic::error(
                span.clone(),
                format!(
                    "type mismatch in {ctx}: cannot assign `{}` to `{}`",
                    act.name(),
                    exp.name()
                ),
            ));
        }
    }

    /// M2.3 指针形态：写目标若解引用只读指针（`*T`）→ 编译错误（须 `*mut T`）。
    /// `p.* = v` / `p.field = v` / `p[i] = v` 中基座为只读指针即拦截。
    pub(crate) fn check_ptr_write(
        &mut self,
        target: &Expr,
        scopes: &[HashMap<String, VarInfo>],
        span: &Span,
    ) {
        let base = match target {
            Expr::Deref(inner, _) => Some(inner.as_ref()),
            Expr::Field { base, .. } | Expr::Dot { base, .. } | Expr::Index { base, .. } => {
                Some(base.as_ref())
            }
            _ => None,
        };
        let Some(base) = base else {
            return;
        };
        let bt = self.expr_ty(base, scopes, None);
        if let Some(SType::Ptr(inner, mut_)) = &bt {
            if !*mut_ {
                self.diags.push(Diagnostic::error(
                    span.clone(),
                    format!(
                        "cannot write through read-only pointer `*{}`; use `*mut {}`",
                        inner.name(),
                        inner.name()
                    ),
                ));
            }
        }
    }

    pub(crate) fn check_condition(&mut self, t: Option<&SType>, span: &Span) {
        if let Some(t) = t {
            match t {
                SType::Bool
                | SType::Int { .. }
                | SType::Float
                | SType::Str
                | SType::Ptr(_, _)
                | SType::Optional(_)
                | SType::ErrorUnion(_, _)
                | SType::Generic(_)
                | SType::Infer
                | SType::Unknown => {}
                _ => {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        format!(
                            "condition must be a bool or optional value (got `{}`)",
                            t.name()
                        ),
                    ));
                }
            }
        }
    }

    pub(crate) fn check_iterable(&mut self, t: Option<&SType>, span: &Span) {
        if let Some(t) = t {
            let t = self.deref_member(t); // 自动解引用（self.children 等）
            let iterable = match t {
                SType::Slice(_)
                | SType::Array(_, _)
                | SType::Str
                | SType::Generic(_)
                | SType::Unknown
                | SType::Infer => true,
                SType::Named(n, _) if is_collection(n) => true,
                SType::Named(n, _) => {
                    // 用户类型：实现 next 方法即可迭代（IIterable 三态；88-iterators）
                    self.funcs.contains_key(&format!("{n}.next"))
                }
                _ => false,
            };
            if !iterable {
                self.diags.push(Diagnostic::error(
                    span.clone(),
                    format!("value of type `{}` is not iterable", t.name()),
                ));
            }
        }
    }

    /// 二元运算符操作数检查（接口族绑定：算术 → INumber、位 → 整数、序 → ICompare）
    pub(crate) fn check_binary(
        &mut self,
        op: BinOp,
        lt: Option<&SType>,
        rt: Option<&SType>,
        span: &Span,
    ) {
        let some_definite_non_generic = |t: Option<&SType>| -> bool {
            t.map_or(false, |t| t.definite() && !matches!(t, SType::Generic(_)))
        };
        let (l, r) = (lt.unwrap_or(&SType::Unknown), rt.unwrap_or(&SType::Unknown));
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::EucMod => {
                if some_definite_non_generic(lt)
                    && some_definite_non_generic(rt)
                    && !(l.numeric() && r.numeric())
                {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        format!(
                            "operator `{}` requires numeric operands (got `{}` and `{}`)",
                            op_name(op),
                            l.name(),
                            r.name()
                        ),
                    ));
                }
            }
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                if some_definite_non_generic(lt)
                    && some_definite_non_generic(rt)
                    && !(l.integer() && r.integer())
                {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        format!(
                            "operator `{}` requires integer operands (got `{}` and `{}`)",
                            op_name(op),
                            l.name(),
                            r.name()
                        ),
                    ));
                }
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                // 序比较绑定 ICompare
                if some_definite_non_generic(lt) && some_definite_non_generic(rt) {
                    let orderable = l.numeric()
                        || r.numeric()
                        || matches!(l, SType::Str | SType::Bool)
                        || matches!(r, SType::Str | SType::Bool)
                        || self.implements(&l, "ICompare")
                        || self.implements(&r, "ICompare");
                    if !orderable {
                        self.diags.push(Diagnostic::error(
                            span.clone(),
                            format!(
                                "operator `{}` requires ICompare-comparable operands (got `{}` and `{}`)",
                                op_name(op),
                                l.name(),
                                r.name()
                            ),
                        ));
                    }
                }
            }
            BinOp::Eq | BinOp::Ne => {
                // == 通用：class 需实现 ICompare 否则编译错误（H3）
                if let SType::Named(n, _) = l {
                    if !is_builtin_type(n)
                        && l.definite()
                        && r.definite()
                        && !self.compatible(l, r)
                        && !matches!(r, SType::Unknown)
                    {
                        self.diags.push(Diagnostic::error(
                            span.clone(),
                            format!(
                                "cannot compare `{}` with `{}` using `{}`",
                                l.name(),
                                r.name(),
                                op_name(op)
                            ),
                        ));
                    }
                }
            }
            BinOp::And | BinOp::Or => {
                if some_definite_non_generic(lt)
                    && some_definite_non_generic(rt)
                    && !(matches!(l, SType::Bool) && matches!(r, SType::Bool))
                {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        format!(
                            "operator `{}` requires bool operands (got `{}` and `{}`)",
                            op_name(op),
                            l.name(),
                            r.name()
                        ),
                    ));
                }
            }
            BinOp::Range => {}
        }
    }

    /// 类型是否实现接口（内建实现 / class 冒号标注）
    pub(crate) fn implements(&self, t: &SType, iface: &str) -> bool {
        match t {
            SType::Int { width } => match width {
                IntWidth::I8
                | IntWidth::I16
                | IntWidth::I32
                | IntWidth::I64
                | IntWidth::I128
                | IntWidth::ISize => matches!(iface, "IInt" | "INumber" | "ICompare"),
                IntWidth::U8
                | IntWidth::U16
                | IntWidth::U32
                | IntWidth::U64
                | IntWidth::U128
                | IntWidth::USize => matches!(iface, "IUint" | "INumber" | "ICompare"),
                IntWidth::Comptime => false,
            },
            SType::Float => matches!(iface, "IFloat" | "INumber" | "ICompare"),
            SType::Str => iface == "ICompare",
            SType::Named(n, _) => match self.types.get(n) {
                Some(TypeInfo {
                    kind: TypeKind::Class { ifaces, .. },
                    ..
                }) => ifaces.iter().any(|t| match t.strip() {
                    Type::Named(in_, _) => in_ == iface,
                    _ => false,
                }),
                _ => false,
            },
            _ => false,
        }
    }

    /// 字段访问校验（Field / Dot 链）
    pub(crate) fn check_field_access(&mut self, bt: Option<&SType>, field: &str, span: &Span) {
        let bt = bt.map(|t| self.deref_member(t)); // 自动解引用（A3）
        match bt {
            Some(SType::Named(n, _)) if !is_builtin_type(n) => {
                if let Some(TypeKind::Class { fields, .. }) = self.types.get(n).map(|i| &i.kind) {
                    let has_field = fields.iter().any(|f| f.name == *field);
                    let has_method = self.funcs.contains_key(&format!("{n}.{field}"));
                    if !has_field && !has_method {
                        self.diags.push(Diagnostic::error(
                            span.clone(),
                            format!("type `{n}` has no field or method `{field}`"),
                        ));
                    }
                }
                // K1 union：字段存在性校验（无方法）
                if let Some(TypeKind::Union { fields, .. }) = self.types.get(n).map(|i| &i.kind) {
                    let has_field = fields.iter().any(|f| f.name == *field);
                    if !has_field {
                        self.diags.push(Diagnostic::error(
                            span.clone(),
                            format!("union `{n}` has no field `{field}`"),
                        ));
                    }
                }
            }
            Some(SType::Tuple(ts)) => {
                if let Ok(i) = field.parse::<usize>() {
                    if i >= ts.len() {
                        self.diags.push(Diagnostic::error(
                            span.clone(),
                            format!(
                                "tuple index `{field}` out of bounds (tuple has {} elements)",
                                ts.len()
                            ),
                        ));
                    }
                }
            }
            _ => {}
        }
    }

    /// 索引校验：Table 支持 1 索引（行视图 `t[i]`）或 2 索引（单元格 `t[i,j]`）；其余单索引
    pub(crate) fn check_index(&mut self, bt: Option<&SType>, indices: &[Expr], span: &Span) {
        match bt {
            Some(SType::Named(n, _)) if n == "Table" => {
                if indices.len() < 1 || indices.len() > 2 {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        format!(
                            "`Table` requires 1 index `t[i]` (row view) or 2 indices `t[i, j]` (cell) (got {})",
                            indices.len()
                        ),
                    ));
                }
            }
            // 类型未知 / 待推断时保守放行
            // （如 var t = Table<i32>.init(...) 类型推断未完成，泛型实参被 parser 跳过）
            Some(SType::Infer) | Some(SType::Unknown) | None => {}
            _ => {
                if indices.len() > 1 {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        format!(
                            "only `Table` supports multi-argument indexing (got {} indices)",
                            indices.len()
                        ),
                    ));
                }
            }
        }
    }

    /// NamedLit 构造校验（字段存在 / 必填 / 类型 / 未知字段；枚举变体）
    pub(crate) fn check_named_lit(
        &mut self,
        ty: &str,
        fields: &[(String, Expr)],
        span: &Span,
        scopes: &[HashMap<String, VarInfo>],
    ) {
        // 先克隆元数据，避免与后续 &mut self 调用冲突
        let class_fields: Option<Vec<FieldDecl>> = match self.types.get(ty) {
            Some(TypeInfo {
                kind: TypeKind::Class { fields, .. },
                ..
            }) => Some(fields.clone()),
            _ => None,
        };
        let enum_variants: Option<Vec<EnumVariant>> = match self.types.get(ty) {
            Some(TypeInfo {
                kind: TypeKind::Enum { variants },
                ..
            }) => Some(variants.clone()),
            _ => None,
        };
        let union_fields: Option<Vec<FieldDecl>> = match self.types.get(ty) {
            Some(TypeInfo {
                kind: TypeKind::Union { fields },
                ..
            }) => Some(fields.clone()),
            _ => None,
        };
        if let Some(fdecls) = class_fields {
            for (name, expr) in fields {
                match fdecls.iter().find(|f| f.name == *name) {
                    Some(fd) => {
                        let exp = self.ty_of(&fd.ty);
                        let act = self.expr_ty(expr, scopes, Some(&exp));
                        self.check_assignable(&Some(exp), &act, &expr.span(), "field");
                    }
                    None => {
                        self.diags.push(Diagnostic::error(
                            span.clone(),
                            format!("unknown field `{name}` in literal of type `{ty}`"),
                        ));
                    }
                }
            }
            // 必填字段（连续类型字面量构造要求全字段）
            let provided: std::collections::HashSet<&str> =
                fields.iter().map(|(n, _)| n.as_str()).collect();
            let missing: Vec<&str> = fdecls
                .iter()
                .filter(|f| !provided.contains(f.name.as_str()))
                .map(|f| f.name.as_str())
                .collect();
            if !missing.is_empty() {
                self.diags.push(Diagnostic::error(
                    span.clone(),
                    format!(
                        "missing field(s) {} in literal of type `{ty}`",
                        missing
                            .iter()
                            .map(|m| format!("`{m}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ));
            }
        } else if let Some(variants) = enum_variants {
            if fields.len() > 1 {
                self.diags.push(Diagnostic::error(
                    span.clone(),
                    format!(
                        "enum literal `{ty}` expects a single variant (got {} fields)",
                        fields.len()
                    ),
                ));
            }
            if let Some((name, expr)) = fields.first() {
                match variants.iter().find(|v| v.name == *name) {
                    Some(v) => {
                        if let Some(payload_ty) = &v.payload {
                            let exp = self.ty_of(payload_ty);
                            let act = self.expr_ty(expr, scopes, Some(&exp));
                            self.check_assignable(&Some(exp), &act, &expr.span(), "variant");
                        }
                    }
                    None => {
                        self.diags.push(Diagnostic::error(
                            span.clone(),
                            format!("enum `{ty}` has no variant `{name}`"),
                        ));
                    }
                }
            }
        } else if let Some(ufields) = union_fields {
            // K1 union 字面量：恰一个字段（构造时其余字段 = 该字段字节重解释）
            if fields.len() != 1 {
                self.diags.push(Diagnostic::error(
                    span.clone(),
                    format!(
                        "union literal `{ty}` expects exactly one field (got {})",
                        fields.len()
                    ),
                ));
            }
            if let Some((name, expr)) = fields.first() {
                match ufields.iter().find(|f| f.name == *name) {
                    Some(fd) => {
                        let exp = self.ty_of(&fd.ty);
                        let act = self.expr_ty(expr, scopes, Some(&exp));
                        self.check_assignable(&Some(exp), &act, &expr.span(), "union field");
                    }
                    None => {
                        self.diags.push(Diagnostic::error(
                            span.clone(),
                            format!("union `{ty}` has no field `{name}`"),
                        ));
                    }
                }
            }
        } else {
            // 未登记类型（内建/未知）：放行
            let _ = span;
        }
    }

    // ---------- 调用检查（重载匹配 + where 约束验证） ----------

    pub(crate) fn check_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        span: &Span,
        scopes: &[HashMap<String, VarInfo>],
        expected: Option<&SType>,
    ) -> Option<SType> {
        match callee {
            Expr::Ident(name, _) => {
                // @ 内建
                if let Some(rest) = name.strip_prefix('@') {
                    return self.call_at_builtin(rest, args, span, scopes);
                }
                if is_builtin_fn(name) {
                    // G3：spawn 特殊处理——提取 callee 返回类型 T 得 `Thread(T)`，
                    // 并做捕获分类（Q18/Q19 由 VarDecl 持线程状态，此处仅返回类型）
                    if name == "spawn" {
                        let (t, _ts) = self.spawn_capture_info(args, span, scopes);
                        return Some(t);
                    }
                    return Some(self.builtin_fn_ret(name));
                }
                // 函数值/闭包调用（参数是 FnN 调用接口类型）：放行
                if self.var_exists(name, scopes) {
                    for a in args {
                        let _ = self.expr_ty(a, scopes, None);
                    }
                    return None;
                }
                // 全局函数重载匹配
                let arg_tys: Vec<Option<SType>> =
                    args.iter().map(|a| self.expr_ty(a, scopes, None)).collect();
                return self.match_overloads(name, None, &arg_tys, args, span, expected, false);
            }
            Expr::Field {
                base,
                field,
                span: _,
            } => {
                // 类型方法：Vec<i32>.init / Table<i32>.init / JsonParser.parse
                // 实例方法：p.dist(q)
                let bt = self.expr_ty(base, scopes, None);
                // 类型方法（base = 类型名）
                if let Expr::Ident(n, _) = base.as_ref() {
                    if self.types.contains_key(n) {
                        let sigs = self.funcs.get(&format!("{n}.{field}")).cloned();
                        let arg_tys: Vec<Option<SType>> =
                            args.iter().map(|a| self.expr_ty(a, scopes, None)).collect();
                        return self.match_overloads(
                            &format!("{n}.{field}"),
                            sigs,
                            &arg_tys,
                            args,
                            span,
                            expected,
                            false,
                        );
                    }
                    if is_builtin_type(n) {
                        // 内建类型方法：放行
                        return None;
                    }
                    // 未知类型名方法：放行（保守）
                    return None;
                }
                // 实例方法
                let dt = bt.as_ref().map(|t| self.deref_member(t).clone());
                let method_sigs = match &dt {
                    Some(SType::Named(n, _)) if !is_builtin_type(n) => {
                        self.funcs.get(&format!("{n}.{field}")).cloned()
                    }
                    _ => None,
                };
                if let Some(sigs) = method_sigs {
                    let arg_tys: Vec<Option<SType>> =
                        args.iter().map(|a| self.expr_ty(a, scopes, None)).collect();
                    return self.match_overloads(
                        &format!("{}.{}", field, field),
                        Some(sigs),
                        &arg_tys,
                        args,
                        span,
                        expected,
                        true,
                    );
                }
                // 内建方法（Vec.append 等）或未知：放行
                for a in args {
                    let _ = self.expr_ty(a, scopes, None);
                }
                None
            }
            Expr::Dot { base, field, .. } => {
                if let Expr::Ident(n, _) = base.as_ref() {
                    // 实例方法调用（v1.append(1) 被 parse_primary 解析为 Dot）：先查变量
                    if self.var_exists(n, scopes) {
                        let bt = self.lookup_var_ty(n, scopes);
                        let dt = bt.as_ref().map(|t| self.deref_member(t).clone());
                        let method_sigs = match &dt {
                            Some(SType::Named(cn, _)) if !is_builtin_type(cn) => {
                                self.funcs.get(&format!("{cn}.{field}")).cloned()
                            }
                            _ => None,
                        };
                        if let Some(sigs) = method_sigs {
                            let arg_tys: Vec<Option<SType>> =
                                args.iter().map(|a| self.expr_ty(a, scopes, None)).collect();
                            return self.match_overloads(
                                &format!("{}.{}", field, field),
                                Some(sigs),
                                &arg_tys,
                                args,
                                span,
                                expected,
                                true,
                            );
                        }
                        // 内建方法或未知：放行
                        for a in args {
                            let _ = self.expr_ty(a, scopes, None);
                        }
                        return None;
                    }
                    if is_builtin_ns(n) || self.namespaces.contains(n) {
                        // io.print / alloc.init / arena.alloc / math.sqrt / NS.fn：放行
                        let _ = args;
                        return None;
                    }
                    if self.types.contains_key(n) {
                        // 枚举变体调用：枚举返回；class 静态方法（含内建序列化）：放行
                        let is_enum = matches!(
                            self.types.get(n).map(|i| &i.kind),
                            Some(TypeKind::Enum { .. })
                        );
                        if is_enum {
                            return Some(SType::Named(n.clone(), vec![]));
                        }
                        return None;
                    }
                    // 解释器注入的内建类型（ExitType 等）：放行
                    if is_builtin_type(n) {
                        return Some(if n == "String" {
                            SType::Str
                        } else {
                            SType::Named(n.clone(), vec![])
                        });
                    }
                    // 序列化内建方法（Type.from_json 等）：不要求类型登记
                    if is_serialize_builtin(field) {
                        return None;
                    }
                    // 未登记命名空间（json/csv 等）：库或兄弟文件未知——保守放行，运行时诊断
                    return None;
                }
                // 链式调用：放行
                for a in args {
                    let _ = self.expr_ty(a, scopes, None);
                }
                None
            }
            _ => {
                // 表达式调用（函数指针等）：放行
                for a in args {
                    let _ = self.expr_ty(a, scopes, None);
                }
                None
            }
        }
    }

    /// 内建函数返回类型（子集：供 orelse/return 期望传播）
    pub(crate) fn builtin_fn_ret(&self, name: &str) -> SType {
        match name {
            // G3（设计文档 §6）：`box(v, alloc)` 返回拥有/可变指针  `owned *mut T`
            "box" => SType::Ptr(Box::new(SType::Unknown), true),
            "copy" => SType::Unknown,
            "parse_int" | "parse_char" => SType::Optional(Box::new(SType::Int {
                width: IntWidth::Comptime,
            })),
            "parse_float" => SType::Optional(Box::new(SType::Float)),
            "parse_bool" => SType::Optional(Box::new(SType::Bool)),
            "read_u64_le" | "read_i64_le" => SType::Int {
                width: IntWidth::U64,
            },
            "read_u32_le" => SType::Int {
                width: IntWidth::U32,
            },
            "read_u16_le" => SType::Int {
                width: IntWidth::U16,
            },
            "sqrt" => SType::Float,
            "binary_search" => SType::Optional(Box::new(SType::Int {
                width: IntWidth::USize,
            })),
            "min" | "max" | "sort" | "fmt_int" | "fmt_float" => SType::Unknown,
            // G1：`spawn(f, args...) owned Thread(T)` 返回线程句柄（协作式延迟执行）。
            // G3 精化：check_call 拦截 spawn 走 spawn_capture_info 提取 T；此处兜底。
            "spawn" => SType::Named("Thread".to_string(), vec![]),
            "with_arena" => SType::Void,
            _ => SType::Void,
        }
    }

    // ---------- 组 G Q18/Q19：spawn 捕获规则（协作式延迟执行） ----------

    /// G3：`spawn(f, args...)` 静态检查。提取 callee 返回类型 T（含错误联合）得
    /// `Thread(T)`；分类捕获：值复制 / `&global` / `move x` 安全，`&局部` 记入
    /// local_refs（线程逃逸 → Q18 错误；绑定场景 spawn→join 间写入 → Q19 冻结违例）。
    /// 返回 (类型, 线程状态)。
    pub(crate) fn spawn_capture_info(
        &mut self,
        args: &[Expr],
        span: &Span,
        scopes: &[HashMap<String, VarInfo>],
    ) -> (SType, ThreadState) {
        let mut local_refs: Vec<(String, Span)> = Vec::new();
        let mut t = SType::Unknown;
        if let Some(callee) = args.first() {
            // callee 返回类型 T（重载按参数个数匹配；闭包/函数引用 → Unknown）
            if let Expr::Ident(fname, _) = callee {
                if let Some(sigs) = self.funcs.get(fname) {
                    let want = args.len().saturating_sub(1);
                    let sig = sigs
                        .iter()
                        .find(|s| s.params.len() == want)
                        .or_else(|| sigs.first());
                    if let Some(sig) = sig {
                        if let Some(ret) = &sig.ret {
                            t = self.ty_of(ret);
                        }
                    }
                }
            }
            let _ = self.expr_ty(callee, scopes, None);
        }
        // 捕获分类 + Send 检查（跳过 callee）
        for a in args.iter().skip(1) {
            self.classify_spawn_capture(a, scopes, &mut local_refs);
            // C3：spawn 边界——捕获值必须满足 Send（跨线程安全传递）
            let capture_ty = self.expr_ty(a, scopes, None);
            if let Some(st) = &capture_ty {
                if !self.type_is_send(st) {
                    self.diags.push(Diagnostic::error(
                        a.span(),
                        format!(
                            "captured value of type `{}` does not satisfy `Send`: \
                             cannot send non-Send value across threads",
                            st.name()
                        ),
                    ));
                }
            }
        }
        let thread_ty = SType::Named("Thread".to_string(), vec![t]);
        let ts = ThreadState {
            bound: false,
            detached: false,
            local_refs,
        };
        let _ = span;
        (thread_ty, ts)
    }

    /// G3：单参数捕获分类——`&local`（含 `&local.f` / `&local[i]`）记根局部名；
    /// `move x` / 值复制 / `&global` / 堆值安全放行。裸 `*T` 变量别名局部为已知
    /// 缺口（形如 `&x` 的显式取址已覆盖，见计划「控制流扩展按测试需求推进」）。
    pub(crate) fn classify_spawn_capture(
        &mut self,
        a: &Expr,
        scopes: &[HashMap<String, VarInfo>],
        local_refs: &mut Vec<(String, Span)>,
    ) {
        if let Expr::AddrOf(inner, _, _) = a {
            if let Some(x) = self.addr_root_name(inner) {
                if self.var_is_local(&x, scopes) {
                    local_refs.push((x, a.span()));
                }
                // `&global` / `&未知` → 安全（全局程序期存活，不悬垂）
            }
        }
        // Move / 其它：值随线程持有（Rc/复制），安全
    }

    /// 表达式根变量名：`x` / `x.f` / `x[i]` / `x.*` → Some(x)；其它 → None
    pub(crate) fn addr_root_name(&self, e: &Expr) -> Option<String> {
        match e {
            Expr::Ident(n, _) => Some(n.clone()),
            Expr::Field { base, .. }
            | Expr::Dot { base, .. }
            | Expr::Index { base, .. }
            | Expr::Deref(base, _) => self.addr_root_name(base),
            _ => None,
        }
    }

    pub(crate) fn var_is_local(&self, name: &str, scopes: &[HashMap<String, VarInfo>]) -> bool {
        scopes.iter().rev().any(|s| s.contains_key(name))
    }

    /// G3 Q18：语句位置的线程方法调用跟踪——`th.join()` → bound（冻结窗口闭合）；
    /// `th.detach()` → detached。仅直线路径（conditional_depth == 0）计数：
    /// 条件体内 join 不保证执行 → 不视为绑定（保守：逃逸错误仍报）。
    pub(crate) fn track_thread_method(
        &mut self,
        e: &Expr,
        scopes: &mut Vec<HashMap<String, VarInfo>>,
    ) {
        let inner = match e {
            Expr::Try(inner, _) | Expr::Catch(inner, _, _) => inner,
            _ => e,
        };
        if let Expr::Call { callee, .. } = inner {
            let (base, field) = match callee.as_ref() {
                Expr::Dot { base, field, .. } | Expr::Field { base, field, .. } => {
                    (base.as_ref(), field.as_str())
                }
                _ => return,
            };
            if let Expr::Ident(n, _) = base {
                for s in scopes.iter_mut().rev() {
                    if let Some(info) = s.get_mut(n) {
                        if let Some(ts) = &mut info.thread {
                            if self.conditional_depth == 0 {
                                match field {
                                    "join" => ts.bound = true,
                                    "detach" => ts.detached = true,
                                    _ => {}
                                }
                            }
                        }
                        break;
                    }
                }
            }
        }
    }

    /// G3 Q19：写入未绑定线程按引用捕获的局部 → 冻结违例（spawn→join 窗口）。
    /// 保守即时报告：即使该线程后逃逸（另有 Q18 错误）也报——两错误独立有效。
    pub(crate) fn check_thread_freeze(
        &mut self,
        target: &Expr,
        scopes: &[HashMap<String, VarInfo>],
        span: &Span,
    ) {
        let Some(x) = self.addr_root_name(target) else {
            return;
        };
        for scope in scopes {
            for info in scope.values() {
                if let Some(ts) = &info.thread {
                    if !ts.bound && !ts.detached && ts.local_refs.iter().any(|(n, _)| *n == x) {
                        self.diags.push(Diagnostic::error(
                            span.clone(),
                            format!(
                                "cannot write `{x}` between spawn and join: captured by reference \
                                 in thread (Q19 冻结窗口)——被捕获引用目标在 `join()` 前禁止写入"
                            ),
                        ));
                    }
                }
            }
        }
    }

    /// G3 Q18：作用域退出——未 join/未 detach 的线程运行时提升到根回收队列
    /// （程序结束运行）；捕获局部引用 → 局部已随作用域销毁 → 悬垂，编译错误。
    pub(crate) fn thread_escape_sweep(&mut self, scope: &HashMap<String, VarInfo>) {
        for (name, info) in scope {
            if let Some(ts) = &info.thread {
                if !ts.bound && !ts.detached {
                    for (x, sp) in &ts.local_refs {
                        self.diags.push(Diagnostic::error(
                            sp.clone(),
                            format!(
                                "cannot capture reference to local `{x}` in escaping thread \
                                 `{name}`: thread not joined before scope exit (Q18)——延迟执行\
                                 在作用域退出时提升到根回收，局部引用悬垂；在 `{name}` 声明\
                                 作用域内 `join()` 或改用值复制 / 全局捕获"
                            ),
                        ));
                    }
                }
            }
        }
    }

    /// @ 内建返回类型（子集）
    pub(crate) fn call_at_builtin(
        &mut self,
        name: &str,
        args: &[Expr],
        span: &Span,
        scopes: &[HashMap<String, VarInfo>],
    ) -> Option<SType> {
        match name {
            "intFromEnum" => Some(SType::Int {
                width: IntWidth::USize,
            }),
            "enumFromInt" => {
                // @enumFromInt(Kind, 2)：首个参数为类型名
                if let Some(Expr::Ident(tn, _)) = args.first() {
                    Some(SType::Named(tn.clone(), vec![]))
                } else {
                    None
                }
            }
            "panic" => Some(SType::Void),
            // M4.3：@compileError = 编译期错误（强制编译失败）
            "compileError" => {
                let msg = match args.first() {
                    Some(Expr::StrLit { value, .. }) => value.clone(),
                    _ => "(no message)".to_string(),
                };
                self.diags.push(Diagnostic::error(
                    span.clone(),
                    format!("@compileError: {msg}"),
                ));
                Some(SType::Void)
            }
            "addWithOverflow" | "subWithOverflow" | "mulWithOverflow" => {
                Some(SType::Tuple(vec![SType::Unknown, SType::Bool]))
            }
            "sizeOf" | "alignOf" | "offsetOf" | "intCast" => Some(SType::Int {
                width: IntWidth::USize,
            }),
            "typeOf" => Some(SType::Str),
            "ptrCast" | "alignCast" => Some(SType::Unknown),
            // K2（ADR-0014）：@volatileLoad/@volatileStore——机制级 volatile（LLVM volatile
            // 语义，防优化掉副作用，MMIO 场景）。load 返回 pointee 类型；store 返回 void。
            "volatileLoad" => {
                if args.len() != 1 {
                    return Some(SType::Unknown);
                }
                match self.expr_ty(&args[0], scopes, None) {
                    Some(SType::Ptr(t, _)) => Some(*t),
                    Some(SType::Unknown) | None => Some(SType::Unknown),
                    Some(t) => {
                        self.diags.push(Diagnostic::error(
                            span.clone(),
                            format!(
                                "@volatileLoad expects a pointer argument (got `{}`)",
                                t.name()
                            ),
                        ));
                        Some(SType::Unknown)
                    }
                }
            }
            "volatileStore" => {
                if args.len() != 2 {
                    return Some(SType::Unknown);
                }
                let _ = self.expr_ty(&args[0], scopes, None);
                let _ = self.expr_ty(&args[1], scopes, None);
                Some(SType::Void)
            }
            // K4（ADR-0014）：@ptrFromInt(addr) → 虚拟指针（整数地址 → 指针，元素类型未知）；
            // @intFromPtr(p) → usize（指针 → 整数地址）。系统编程底层（物理地址访问，
            // 与 volatile 组合 = MMIO 真地址读写）。指针无类型化——ptrFromInt 恒返回
            // `*mut Unknown`，经 `@ptrCast`/注解定型（对齐 Zig 结果类型推断的退化形态）。
            "ptrFromInt" => {
                if args.len() != 1 {
                    return Some(SType::Unknown);
                }
                match self.expr_ty(&args[0], scopes, None) {
                    Some(SType::Int { .. }) | Some(SType::Unknown) | None => {
                        Some(SType::Ptr(Box::new(SType::Unknown), true))
                    }
                    Some(t) => {
                        self.diags.push(Diagnostic::error(
                            span.clone(),
                            format!(
                                "@ptrFromInt expects an integer argument (got `{}`)",
                                t.name()
                            ),
                        ));
                        Some(SType::Unknown)
                    }
                }
            }
            "intFromPtr" => {
                if args.len() != 1 {
                    return Some(SType::Unknown);
                }
                match self.expr_ty(&args[0], scopes, None) {
                    Some(SType::Ptr(..)) | Some(SType::Unknown) | None => Some(SType::Int {
                        width: IntWidth::USize,
                    }),
                    Some(t) => {
                        self.diags.push(Diagnostic::error(
                            span.clone(),
                            format!(
                                "@intFromPtr expects a pointer argument (got `{}`)",
                                t.name()
                            ),
                        ));
                        Some(SType::Unknown)
                    }
                }
            }
            // 组 F（Q-S3）：@atomicLoad(T, p, order)——原子读，返回 pointee 类型。
            // T 为类型名参数、order 为内存序枚举值（协作式下求值后丢弃）——均跳过检查，
            // 对齐 @volatileLoad/@sizeOf 的类型参数处理。
            "atomicLoad" => {
                if args.len() != 3 {
                    return Some(SType::Unknown);
                }
                match self.expr_ty(&args[1], scopes, None) {
                    Some(SType::Ptr(t, _)) => Some(*t),
                    Some(SType::Unknown) | None => Some(SType::Unknown),
                    Some(t) => {
                        self.diags.push(Diagnostic::error(
                            span.clone(),
                            format!(
                                "@atomicLoad expects a pointer argument (got `{}`)",
                                t.name()
                            ),
                        ));
                        Some(SType::Unknown)
                    }
                }
            }
            // 组 F（Q-S3）：@atomicStore(T, p, v, order)——原子写，返回 void。
            "atomicStore" => {
                if args.len() != 4 {
                    return Some(SType::Unknown);
                }
                let _ = self.expr_ty(&args[1], scopes, None);
                let _ = self.expr_ty(&args[2], scopes, None);
                Some(SType::Void)
            }
            // 组 F（Q-S3）：@atomicRmw(T, p, op, v, order)——读改写，返回旧值
            // （pointee 类型）。op 为内建枚举变体（.add/.sub/.exchange）。
            "atomicRmw" => {
                if args.len() != 5 {
                    return Some(SType::Unknown);
                }
                match self.expr_ty(&args[1], scopes, None) {
                    Some(SType::Ptr(t, _)) => Some(*t),
                    Some(SType::Unknown) | None => Some(SType::Unknown),
                    Some(t) => {
                        self.diags.push(Diagnostic::error(
                            span.clone(),
                            format!("@atomicRmw expects a pointer argument (got `{}`)", t.name()),
                        ));
                        Some(SType::Unknown)
                    }
                }
            }
            _ => {
                let _ = (span, scopes);
                None
            }
        }
    }

    /// 重载匹配：参数数量 + 类型兼容（含泛型具体化）+ 具体优先泛型 + 返回类型匹配期望
    pub(crate) fn match_overloads(
        &mut self,
        qname: &str,
        sigs: Option<Vec<FnSig>>,
        arg_tys: &[Option<SType>],
        args: &[Expr],
        span: &Span,
        expected: Option<&SType>,
        skip_self: bool,
    ) -> Option<SType> {
        let sigs = match sigs {
            Some(s) => s,
            None => match self.funcs.get(qname) {
                Some(s) => s.clone(),
                None => {
                    // 未登记函数：保守放行（兄弟文件/脚本生成函数，运行时诊断）
                    return None;
                }
            },
        };
        // 参数数量：精确匹配优先；否则参数多于实参且多出有默认值
        // （实例方法调用跳过第一个参数：p.dist(q) 实参不含接收者，运行时注入）
        let n = arg_tys.len();
        let skip = usize::from(skip_self);
        let arity = |s: &FnSig| s.params.len().saturating_sub(skip);
        let exact: Vec<&FnSig> = sigs.iter().filter(|s| arity(s) == n).collect();
        let pool: Vec<&FnSig> = if exact.is_empty() {
            sigs.iter()
                .filter(|s| {
                    arity(s) >= n && s.params[arity(s) - n..].iter().all(|p| p.default.is_some())
                })
                .collect()
        } else {
            exact
        };
        if pool.is_empty() {
            // 数量不匹配：函数存在但参数个数不符——报错（准确）
            self.diags.push(Diagnostic::error(
                span.clone(),
                format!(
                    "function `{qname}` takes {} argument(s) but {} given",
                    sigs[0].params.len().saturating_sub(skip),
                    n
                ),
            ));
            return None;
        }
        // 逐候选匹配（① 参数匹配精度：精确 > 兼容/强制/通配；② 具体优先泛型；
        // ③ 同具体度按返回类型匹配期望分级；④ 同具体度同期望且签名不同 → 歧义报错要求显式）
        let mut best: Option<(&FnSig, HashMap<String, SType>)> = None;
        let mut best_is_generic = false;
        let mut best_param_rank: u32 = 0;
        let mut best_rank: u8 = 0;
        let mut ambiguous_reported = false;
        for s in &pool {
            let mut map: HashMap<String, SType> = HashMap::new();
            let mut ok = true;
            let mut param_rank_total: u32 = 0;
            // 实例方法调用：跳过接收者参数（运行时注入）
            let params: Vec<&Param> = if skip_self {
                s.params[1.min(s.params.len())..].iter().collect()
            } else {
                s.params.iter().collect()
            };
            for (p, at) in params.iter().zip(arg_tys.iter()) {
                let pt = self.ty_of(&p.ty);
                let at = at.clone().unwrap_or(SType::Unknown);
                let r = self.param_rank(&pt, &at, &mut map);
                if r == 0 {
                    ok = false;
                    break;
                }
                param_rank_total += r as u32;
            }
            if !ok {
                continue;
            }
            let is_generic = s.generics.iter().any(|g| map.contains_key(g));
            let rank = self.ret_rank(&s.ret, &map, expected);
            let replace_best = match &best {
                None => true,
                Some(b) => {
                    if param_rank_total > best_param_rank {
                        true
                    } else if param_rank_total < best_param_rank {
                        false
                    } else if !is_generic && best_is_generic {
                        true
                    } else if is_generic && !best_is_generic {
                        false
                    } else if rank > best_rank {
                        true
                    } else if rank < best_rank {
                        false
                    } else {
                        // 同精度同具体度同期望匹配：签名不同 → 歧义（要求显式标注）
                        if !self.sig_same(s, b.0) && !ambiguous_reported {
                            self.diags.push(Diagnostic::error(
                                span.clone(),
                                format!(
                                    "ambiguous call to `{qname}`: multiple overloads match; \
                                     annotate the expected type or make the argument type explicit"
                                ),
                            ));
                            ambiguous_reported = true;
                        }
                        false
                    }
                }
            };
            if replace_best {
                best = Some((s, map.clone()));
                best_is_generic = is_generic;
                best_param_rank = param_rank_total;
                best_rank = rank;
            }
        }
        let Some((sig, map)) = best else {
            // 所有候选都不兼容：报参数类型不匹配（准确）
            let types: Vec<String> = arg_tys
                .iter()
                .map(|t| t.as_ref().map_or_else(|| "?".into(), |t| t.name()))
                .collect();
            self.diags.push(Diagnostic::error(
                span.clone(),
                format!(
                    "no overload of `{qname}` matches arguments ({})",
                    types.join(", ")
                ),
            ));
            return None;
        };
        // where 约束验证（调用点单态化）
        for (g, iface) in &sig.where_clause {
            if let Some(concrete) = map.get(g) {
                self.check_constraint(g, concrete, iface, span);
            }
        }
        // 组 D D4：anytype 调用点具体化——`anytype` 参数绑定实参具体类型，返回类型
        // `anytype` 解析为体 return 表达式在该绑定下的具体类型（ADR-0012 #5）。
        // 无 anytype 参数或返回类型非 `anytype` → 原路径（泛型替换 / 声明类型直用）。
        let any_bindings = self.anytype_bindings(sig, &arg_tys, skip_self);
        let ret_st = if !any_bindings.is_empty()
            && matches!(sig.ret.as_ref().map(|r| r.strip()), Some(Type::Infer))
        {
            Some(
                self.anytype_concrete_ret(qname, &any_bindings, &arg_tys)
                    .unwrap_or(SType::Infer),
            )
        } else {
            sig.ret
                .as_ref()
                .map(|r| self.substitute(&self.ty_of(r), &map))
        };
        let _ = args;
        // 组 E E1：async fn 调用点返回 `Future(R)`——R 为声明返回类型（含错误联合）。
        // 例：`async fn parse_json(data: &[u8]) !JsonValue` 调用点类型 = Future(!JsonValue)。
        if sig.is_async {
            let inner = ret_st.unwrap_or(SType::Unknown);
            return Some(SType::Named("Future".to_string(), vec![inner]));
        }
        ret_st
    }

    /// anytype 参数 → 调用点实参具体类型绑定（ADR-0012 #5：参数类型不预先绑定）。
    /// 仅绑定 `anytype` 参数（`Type::Infer`）；具体实参缺型（None）不绑定。
    pub(crate) fn anytype_bindings(
        &self,
        sig: &FnSig,
        arg_tys: &[Option<SType>],
        skip_self: bool,
    ) -> HashMap<String, SType> {
        let mut m = HashMap::new();
        let params: Vec<&Param> = if skip_self {
            sig.params[1.min(sig.params.len())..].iter().collect()
        } else {
            sig.params.iter().collect()
        };
        for (p, at) in params.iter().zip(arg_tys.iter()) {
            if matches!(p.ty.strip(), Type::Infer) {
                if let Some(t) = at {
                    m.insert(p.name.clone(), t.clone());
                }
            }
        }
        m
    }

    /// anytype 具体化返回类型：`(qname, 具体化键)` 缓存（同签名同实例复用，对齐
    /// 类型函数惰性缓存）；未命中则按体 return 表达式在具体绑定下重求值。
    /// 重入守卫：自递归 anytype 函数解析中再入 → None（回落 `Infer`）。
    pub(crate) fn anytype_concrete_ret(
        &mut self,
        qname: &str,
        bindings: &HashMap<String, SType>,
        arg_tys: &[Option<SType>],
    ) -> Option<SType> {
        let key: String = {
            let names: Vec<String> = arg_tys
                .iter()
                .filter_map(|t| t.as_ref().map(|t| self.stype_key(t)))
                .collect();
            format!("{qname}<@{}>", names.join(","))
        };
        let cache_key = (qname.to_string(), key);
        if self.anytype_resolving.contains(&cache_key) {
            return None;
        }
        if let Some(t) = self.anytype_ret_cache.get(&cache_key) {
            return Some(t.clone());
        }
        let body = self.anytype_bodies.get(qname).cloned()?;
        self.anytype_resolving.insert(cache_key.clone());
        let rt = self.retype_return(&body, bindings);
        self.anytype_resolving.remove(&cache_key);
        if let Some(t) = &rt {
            self.anytype_ret_cache.insert(cache_key, t.clone());
        }
        rt
    }

    /// 在 anytype 具体绑定下重求值函数体 return 表达式 → 具体返回类型。
    /// 多路径 return 取「首个 definite 类型」为代表，其余须 mutual-compatible
    /// （不符 → None，回落 `Infer`）。重求值产生的诊断截断（解析调用点类型不报错）。
    pub(crate) fn retype_return(
        &mut self,
        body: &Block,
        bindings: &HashMap<String, SType>,
    ) -> Option<SType> {
        let mut scopes: Vec<HashMap<String, VarInfo>> = vec![bindings
            .iter()
            .map(|(n, t)| {
                (
                    n.clone(),
                    VarInfo {
                        ty: Some(t.clone()),
                        pending_fields: None,
                        source: AllocSource::Unknown,
                        thread: None,
                    },
                )
            })
            .collect()];
        let mut collected: Vec<SType> = Vec::new();
        let diag_len = self.diags.len();
        self.collect_return_types(body, &mut scopes, &mut collected);
        self.diags.truncate(diag_len);
        let mut rep: Option<SType> = None;
        for t in &collected {
            if t.definite() {
                match &rep {
                    None => rep = Some(t.clone()),
                    Some(r) => {
                        if !self.compatible(r, t) && !self.compatible(t, r) {
                            return None;
                        }
                    }
                }
            }
        }
        rep
    }

    /// 收集函数体所有 return 表达式的静态类型（嵌套 if/while/for/switch 块递推）。
    pub(crate) fn collect_return_types(
        &mut self,
        block: &Block,
        scopes: &mut Vec<HashMap<String, VarInfo>>,
        out: &mut Vec<SType>,
    ) {
        for s in &block.stmts {
            match s {
                Stmt::Return(Some(e), _) => {
                    if let Some(t) = self.expr_ty(e, scopes, None) {
                        out.push(t);
                    }
                }
                Stmt::Block(b) => self.collect_return_types(b, scopes, out),
                Stmt::If(IfStmt { then_b, else_b, .. }) => {
                    self.collect_return_types(then_b, scopes, out);
                    if let Some(es) = else_b {
                        self.collect_stmt_returns(es, scopes, out);
                    }
                }
                Stmt::While(WhileStmt { body, .. }) => self.collect_return_types(body, scopes, out),
                Stmt::For(ForStmt { body, .. }) => self.collect_return_types(body, scopes, out),
                Stmt::Switch(SwitchStmt { arms, .. }) => {
                    for arm in arms {
                        self.collect_return_types(&arm.body, scopes, out);
                    }
                }
                _ => {}
            }
        }
    }

    /// 单语句形态的 return 收集（else 分支：块 / else-if 链递推）。
    pub(crate) fn collect_stmt_returns(
        &mut self,
        s: &Stmt,
        scopes: &mut Vec<HashMap<String, VarInfo>>,
        out: &mut Vec<SType>,
    ) {
        match s {
            Stmt::Block(b) => self.collect_return_types(b, scopes, out),
            Stmt::If(IfStmt { then_b, else_b, .. }) => {
                self.collect_return_types(then_b, scopes, out);
                if let Some(es) = else_b {
                    self.collect_stmt_returns(es, scopes, out);
                }
            }
            Stmt::Return(Some(e), _) => {
                if let Some(t) = self.expr_ty(e, scopes, None) {
                    out.push(t);
                }
            }
            _ => {}
        }
    }

    /// 静态类型规范串（anytype 具体化缓存键；区分整数宽度——诊断用 `name()` 不区分）。
    pub(crate) fn stype_key(&self, t: &SType) -> String {
        match t {
            SType::Int { width } => match width {
                IntWidth::I8 => "i8",
                IntWidth::I16 => "i16",
                IntWidth::I32 => "i32",
                IntWidth::I64 => "i64",
                IntWidth::I128 => "i128",
                IntWidth::ISize => "isize",
                IntWidth::U8 => "u8",
                IntWidth::U16 => "u16",
                IntWidth::U32 => "u32",
                IntWidth::U64 => "u64",
                IntWidth::U128 => "u128",
                IntWidth::USize => "usize",
                IntWidth::Comptime => "comptime_int",
            }
            .to_string(),
            SType::Float => "f64".into(),
            SType::Bool => "bool".into(),
            SType::Void => "void".into(),
            SType::Str => "String".into(),
            SType::Slice(inner) => format!("&[{}]", self.stype_key(inner)),
            SType::Named(n, args) => {
                if args.is_empty() {
                    n.clone()
                } else {
                    format!(
                        "{n}({})",
                        args.iter()
                            .map(|a| self.stype_key(a))
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                }
            }
            SType::Tuple(ts) => format!(
                "({})",
                ts.iter()
                    .map(|a| self.stype_key(a))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            SType::Optional(inner) => format!("?{}", self.stype_key(inner)),
            SType::ErrorUnion(e, inner) => format!(
                "{}!{}",
                e.as_ref()
                    .map_or_else(|| "anyerror".into(), |x| self.stype_key(x)),
                self.stype_key(inner)
            ),
            SType::Ptr(inner, mut_) => {
                if *mut_ {
                    format!("*mut {}", self.stype_key(inner))
                } else {
                    format!("*{}", self.stype_key(inner))
                }
            }
            SType::Array(n, inner) => format!("[{n}]{}", self.stype_key(inner)),
            SType::Infer => "anytype".into(),
            SType::Unknown => "?".into(),
            SType::Generic(n) => n.clone(),
        }
    }

    /// 泛型替换：泛型参数 → 具体化类型
    pub(crate) fn substitute(&self, t: &SType, map: &HashMap<String, SType>) -> SType {
        match t {
            SType::Generic(n) => map.get(n).cloned().unwrap_or(SType::Unknown),
            SType::Slice(inner) => SType::Slice(Box::new(self.substitute(inner, map))),
            SType::Ptr(inner, m) => SType::Ptr(Box::new(self.substitute(inner, map)), *m),
            SType::Optional(inner) => SType::Optional(Box::new(self.substitute(inner, map))),
            SType::ErrorUnion(e, inner) => SType::ErrorUnion(
                e.as_ref().map(|x| Box::new(self.substitute(x, map))),
                Box::new(self.substitute(inner, map)),
            ),
            SType::Tuple(ts) => SType::Tuple(ts.iter().map(|x| self.substitute(x, map)).collect()),
            SType::Array(n, inner) => SType::Array(*n, Box::new(self.substitute(inner, map))),
            SType::Named(n, args) => SType::Named(
                n.clone(),
                args.iter().map(|x| self.substitute(x, map)).collect(),
            ),
            other => other.clone(),
        }
    }

    /// 泛型具体化的返回类型与期望类型的匹配分级（M2.3 期望类型传播）：
    /// 0 = 不匹配，1 = 数值/兼容匹配，2 = 精确匹配（精确 > 兼容 > 不匹配）。
    pub(crate) fn ret_rank(
        &self,
        ret: &Option<Type>,
        map: &HashMap<String, SType>,
        expected: Option<&SType>,
    ) -> u8 {
        let Some(exp) = expected else {
            return 0;
        };
        let Some(r) = ret else {
            return 0;
        };
        let inner = match r.strip() {
            Type::ErrorUnion(_, inner) => inner.strip(),
            other => other,
        };
        let ret_st = self.substitute(&self.ty_of(inner), map);
        if matches!(ret_st, SType::Unknown) || matches!(exp, SType::Unknown) {
            return 0;
        }
        if &ret_st == exp {
            return 2;
        }
        if self.compatible(&ret_st, exp) {
            return 1;
        }
        0
    }

    /// 两个重载签名是否结构相同（参数类型 + 返回类型；歧义检测排除重复登记）
    pub(crate) fn sig_same(&self, a: &FnSig, b: &FnSig) -> bool {
        if a.params.len() != b.params.len() {
            return false;
        }
        a.params
            .iter()
            .zip(b.params.iter())
            .all(|(pa, pb)| self.ty_of(&pa.ty) == self.ty_of(&pb.ty))
            && self.ret_stype(&a.ret) == self.ret_stype(&b.ret)
    }

    /// 实参 → 形参匹配精度（M2.3 重载解析）：
    /// 0 = 不匹配，1 = 兼容/强制/通配，2 = 精确（整数宽度不区分；泛型 T 绑定按具体类型计 2）。
    pub(crate) fn param_rank(
        &self,
        pt: &SType,
        at: &SType,
        map: &mut HashMap<String, SType>,
    ) -> u8 {
        match pt {
            SType::Generic(n) => match map.get(n) {
                Some(prev) => {
                    if !self.compatible(prev, at) {
                        0
                    } else if self.ret_infer_unifies(prev, at) {
                        2
                    } else {
                        1
                    }
                }
                None => {
                    map.insert(n.clone(), at.clone());
                    match at {
                        SType::Unknown | SType::Infer => 1,
                        _ => 2,
                    }
                }
            },
            SType::Unknown | SType::Infer => 1,
            SType::Slice(inner) => match at {
                SType::Slice(a) => self.param_rank(inner, a, map),
                SType::Array(_, a) => self.param_rank(inner, a, map),
                // 引用默认切段：&arr → &[T]、&frame(Vec/String) → &[T]
                SType::Ptr(a, _) => match a.as_ref() {
                    SType::Array(_, elem) => self.param_rank(inner, elem, map),
                    SType::Slice(elem) => self.param_rank(inner, elem, map),
                    SType::Named(n, args) if n == "Vec" || n == "String" || n == "Deque" => {
                        // 整体匹配优先（&Vec<i32> 形参）；失败则元素级（&[u8] 形参收 Vec(u8)）
                        let whole = self.param_rank(inner, a, map);
                        let elem =
                            self.param_rank(inner, args.first().unwrap_or(&SType::Unknown), map);
                        whole.max(elem)
                    }
                    _ => self.param_rank(inner, a, map),
                },
                SType::Str => self.param_rank(
                    inner,
                    &SType::Int {
                        width: IntWidth::U8,
                    },
                    map,
                ),
                // 集合实参可作切片（Vec/String/Deque → 元素视图）
                SType::Named(n, args) if n == "Vec" || n == "String" || n == "Deque" => {
                    self.param_rank(inner, args.first().unwrap_or(&SType::Unknown), map)
                }
                SType::Unknown | SType::Infer | SType::Generic(_) => 1,
                _ => 0,
            },
            SType::Ptr(inner, _) => match at {
                SType::Ptr(a, _) => self.param_rank(inner, a, map),
                SType::Unknown | SType::Infer | SType::Generic(_) => 1,
                // 值实参自动地址（运行时指针参数宽松：pick_fn 对 Ptr 形参放行）
                other => self.param_rank(inner, other, map),
            },
            SType::Optional(inner) => match at {
                SType::Optional(a) => self.param_rank(inner, a, map),
                SType::Unknown | SType::Infer | SType::Generic(_) => 1,
                // 可选自动包装（对齐 compatible）：T → ?T 兼容——但弱于直接 Optional
                // 精确匹配（重载下 `f(x: i32)` 仍胜 `f(x: ?i32)`）
                other => match self.param_rank(inner, other, map) {
                    0 => 0,
                    _ => 1,
                },
            },
            SType::ErrorUnion(_, inner) => match at {
                SType::ErrorUnion(_, a) => self.param_rank(inner, a, map),
                SType::Unknown | SType::Infer | SType::Generic(_) => 1,
                _ => 0,
            },
            SType::Tuple(pts) => match at {
                SType::Tuple(ats) => {
                    if pts.len() != ats.len() {
                        return 0;
                    }
                    let mut min = 2u8;
                    for (p, a) in pts.iter().zip(ats.iter()) {
                        let r = self.param_rank(p, a, map);
                        if r == 0 {
                            return 0;
                        }
                        min = min.min(r);
                    }
                    min
                }
                SType::Unknown | SType::Infer | SType::Generic(_) => 1,
                _ => 0,
            },
            SType::Array(n, inner) => match at {
                SType::Array(m, a) => {
                    if n != m {
                        0
                    } else {
                        self.param_rank(inner, a, map)
                    }
                }
                SType::Unknown | SType::Infer | SType::Generic(_) => 1,
                _ => 0,
            },
            SType::Named(n, args) => match at {
                SType::Named(m, margs) => {
                    if n != m || args.len() != margs.len() {
                        return 0;
                    }
                    let mut min = 2u8;
                    for (p, a) in args.iter().zip(margs.iter()) {
                        let r = self.param_rank(p, a, map);
                        if r == 0 {
                            return 0;
                        }
                        min = min.min(r);
                    }
                    min
                }
                SType::Unknown | SType::Infer | SType::Generic(_) => 1,
                _ => 0,
            },
            SType::Int { .. } => match at {
                SType::Int { .. } => 2,
                SType::Float => 1,
                SType::Unknown | SType::Infer | SType::Generic(_) => 1,
                _ => 0,
            },
            SType::Float => match at {
                SType::Float => 2,
                SType::Int { .. } => 1,
                SType::Unknown | SType::Infer | SType::Generic(_) => 1,
                _ => 0,
            },
            SType::Bool => match at {
                SType::Bool => 2,
                SType::Unknown | SType::Infer | SType::Generic(_) => 1,
                _ => 0,
            },
            SType::Str => match at {
                SType::Str => 2,
                SType::Unknown | SType::Infer | SType::Generic(_) => 1,
                _ => 0,
            },
            SType::Void => match at {
                SType::Void => 2,
                SType::Unknown | SType::Infer | SType::Generic(_) => 1,
                _ => 0,
            },
        }
    }

    /// 类型双向宽松兼容（赋值/比较/数组元素）
    pub(crate) fn compatible(&self, a: &SType, b: &SType) -> bool {
        if a == b {
            return true;
        }
        match (a, b) {
            (SType::Unknown, _) | (_, SType::Unknown) => true,
            (SType::Infer, _) | (_, SType::Infer) => true,
            (SType::Generic(_), _) | (_, SType::Generic(_)) => true,
            (SType::Int { .. }, SType::Float) | (SType::Float, SType::Int { .. }) => true,
            (SType::Int { .. }, SType::Int { .. }) => true,
            (SType::Str, SType::Slice(inner)) | (SType::Slice(inner), SType::Str)
                if matches!(
                    inner.as_ref(),
                    SType::Int {
                        width: IntWidth::U8
                    } | SType::Unknown
                ) =>
            {
                true
            }
            (SType::Slice(a), SType::Slice(b)) => self.compatible(a, b),
            // 定长数组字面量 ↔ 切片（数组可作切片视图）
            (SType::Array(_, a), SType::Slice(b)) | (SType::Slice(b), SType::Array(_, a)) => {
                self.compatible(a, b)
            }
            // 可选自动包装：T 与 ?T 兼容（字面量/值赋给可选字段）
            (SType::Optional(a), b) | (b, SType::Optional(a)) => self.compatible(a, b),
            (SType::Ptr(a, _), SType::Ptr(b, _)) => self.compatible(a, b),
            (SType::ErrorUnion(_, a), SType::ErrorUnion(_, b)) => self.compatible(a, b),
            (SType::Tuple(a), SType::Tuple(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| self.compatible(x, y))
            }
            (SType::Array(n, a), SType::Array(m, b)) => n == m && self.compatible(a, b),
            (SType::Named(a, aa), SType::Named(b, bb)) => {
                a == b
                    && aa.len() == bb.len()
                    && aa.iter().zip(bb.iter()).all(|(x, y)| self.compatible(x, y))
            }
            _ => false,
        }
    }

    /// M2.3：return 类型多路径统一（比 `compatible` 严格——int/float 不互通；
    /// 整数宽度不区分；Unknown/Infer/泛型视为通配）
    pub(crate) fn ret_infer_unifies(&self, a: &SType, b: &SType) -> bool {
        if a == b {
            return true;
        }
        match (a, b) {
            (SType::Unknown, _) | (_, SType::Unknown) => true,
            (SType::Infer, _) | (_, SType::Infer) => true,
            (SType::Generic(_), _) | (_, SType::Generic(_)) => true,
            (SType::Int { .. }, SType::Int { .. }) => true,
            (SType::Ptr(a, _), SType::Ptr(b, _)) => self.ret_infer_unifies(a, b),
            (SType::Slice(a), SType::Slice(b)) => self.ret_infer_unifies(a, b),
            (SType::Optional(a), SType::Optional(b)) => self.ret_infer_unifies(a, b),
            (SType::ErrorUnion(ea, a), SType::ErrorUnion(eb, b)) => {
                // 错误集允许不同（推断收集归 M2.6）；payload 须统一
                let _ = (ea, eb);
                self.ret_infer_unifies(a, b)
            }
            (SType::Tuple(a), SType::Tuple(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .zip(b.iter())
                        .all(|(x, y)| self.ret_infer_unifies(x, y))
            }
            (SType::Array(na, a), SType::Array(nb, b)) => na == nb && self.ret_infer_unifies(a, b),
            (SType::Named(na, aa), SType::Named(nb, bb)) => {
                na == nb
                    && aa.len() == bb.len()
                    && aa
                        .iter()
                        .zip(bb.iter())
                        .all(|(x, y)| self.ret_infer_unifies(x, y))
            }
            _ => false,
        }
    }

    // ---------- where 约束验证 ----------

    /// 调用点约束验证：具体类型必须实现约束接口（M2.2）
    pub(crate) fn check_constraint(
        &mut self,
        g: &str,
        concrete: &SType,
        iface: &Type,
        span: &Span,
    ) {
        let iface_name = match iface.strip() {
            Type::Named(n, _) => n.clone(),
            _ => return,
        };
        let ok = match concrete {
            SType::Unknown | SType::Infer | SType::Generic(_) => true,
            SType::Int { width } => match width {
                IntWidth::I8
                | IntWidth::I16
                | IntWidth::I32
                | IntWidth::I64
                | IntWidth::I128
                | IntWidth::ISize => matches!(iface_name.as_str(), "IInt" | "INumber" | "ICompare"),
                IntWidth::U8
                | IntWidth::U16
                | IntWidth::U32
                | IntWidth::U64
                | IntWidth::U128
                | IntWidth::USize => {
                    matches!(iface_name.as_str(), "IUint" | "INumber" | "ICompare")
                }
                IntWidth::Comptime => true,
            },
            SType::Float => matches!(iface_name.as_str(), "IFloat" | "INumber" | "ICompare"),
            SType::Str => iface_name == "ICompare",
            SType::Named(n, _) => match self.types.get(n) {
                Some(TypeInfo {
                    kind: TypeKind::Class { ifaces, .. },
                    ..
                }) => ifaces.iter().any(|t| match t.strip() {
                    Type::Named(in_, _) => in_.as_str() == iface_name,
                    _ => false,
                }),
                _ => true, // 内建类型/未知：放行
            },
            _ => true,
        };
        if !ok {
            self.diags.push(Diagnostic::error(
                span.clone(),
                format!(
                    "type `{}` does not satisfy constraint `{g}: {iface_name}` \
                     (interface not implemented)",
                    concrete.name()
                ),
            ));
        }
    }

    // ---------- 既有检查（宽度 / 引用赋值 / definite assignment） ----------

    /// C3：类型是否满足 `Send` 标记接口（可跨线程安全传递）
    pub(crate) fn type_is_send(&self, t: &SType) -> bool {
        match t {
            SType::Int { .. } | SType::Float | SType::Bool | SType::Void | SType::Str => true,
            SType::Slice(inner) => self.type_is_send(inner),
            SType::Named(n, args) => {
                if is_collection(n) || is_builtin_type(n) {
                    // 集合类型（Vec/Map/Deque/Table/String）与内建类型（包括四模式共享
                    // 容器 OneToOne/OneToMany/ManyToOne/ManyToMany）均为 Send——
                    // 四模式容器是内建共享特例（Q32），内部同步，Send 由实现保证
                    args.iter().all(|a| self.type_is_send(a))
                } else {
                    self.type_has_interface(n, "Send")
                }
            }
            SType::Tuple(items) => items.iter().all(|i| self.type_is_send(i)),
            SType::Optional(inner) => self.type_is_send(inner),
            SType::ErrorUnion(err, inner) => {
                err.as_ref().map_or(true, |e| self.type_is_send(e)) && self.type_is_send(inner)
            }
            SType::Ptr(inner, _) => self.type_is_send(inner),
            SType::Array(_, inner) => self.type_is_send(inner),
            SType::Infer | SType::Generic(_) | SType::Unknown => true,
        }
    }

    /// C3：类型是否满足 `Sync` 标记接口（可跨线程安全共享引用）
    pub(crate) fn type_is_sync(&self, t: &SType) -> bool {
        match t {
            SType::Int { .. } | SType::Float | SType::Bool | SType::Void | SType::Str => true,
            SType::Slice(inner) => self.type_is_sync(inner),
            SType::Named(n, args) => {
                if is_collection(n) || is_builtin_type(n) {
                    // 集合类型与内建类型（含四模式容器）均为 Sync——
                    // 四模式容器内部同步，&T 共享引用安全
                    args.iter().all(|a| self.type_is_sync(a))
                } else {
                    self.type_has_interface(n, "Sync")
                }
            }
            SType::Tuple(items) => items.iter().all(|i| self.type_is_sync(i)),
            SType::Optional(inner) => self.type_is_sync(inner),
            SType::ErrorUnion(err, inner) => {
                err.as_ref().map_or(true, |e| self.type_is_sync(e)) && self.type_is_sync(inner)
            }
            SType::Ptr(inner, false) => self.type_is_sync(inner),
            SType::Ptr(_, true) => false, // *mut T 可变共享 → 非 Sync
            SType::Array(_, inner) => self.type_is_sync(inner),
            SType::Infer | SType::Generic(_) | SType::Unknown => true,
        }
    }

    /// C3：命名类型是否实现指定接口（如 `Send`、`Sync`、`IIterable`）
    fn type_has_interface(&self, name: &str, iface: &str) -> bool {
        match self.types.get(name) {
            Some(TypeInfo {
                kind: TypeKind::Class { ifaces, .. },
                ..
            }) => ifaces.iter().any(|t| {
                if let Type::Named(n, _) = t.strip() {
                    n == iface
                } else {
                    false
                }
            }),
            _ => false,
        }
    }

    /// 类型是否为引用类型（不可值赋值；连续类型/标量除外）
    pub(crate) fn type_is_ref_st(&self, t: &SType) -> bool {
        match t {
            SType::Named(n, _) => {
                if is_collection(n) {
                    return true;
                }
                match self.types.get(n) {
                    Some(info) => !info.continuous,
                    None => false,
                }
            }
            SType::Slice(_) | SType::Ptr(_, _) => false, // 指针/切片可复制（指针自由）
            _ => false,
        }
    }

    pub(crate) fn check_int_width_st(&mut self, ty: &SType, text: &str, span: &Span) {
        let width = match ty {
            SType::Int { width } => *width,
            SType::Float => return, // 整数转 float：允许（惰性定型）
            _ => return,
        };
        self.check_int_width_width(width, text, span);
    }

    pub(crate) fn check_int_width_str(&mut self, ty: &str, text: &str, span: &Span) {
        let width = match ty {
            "i8" => IntWidth::I8,
            "i16" => IntWidth::I16,
            "i32" => IntWidth::I32,
            "i64" => IntWidth::I64,
            "i128" => IntWidth::I128,
            "isize" => IntWidth::ISize,
            "u8" => IntWidth::U8,
            "u16" => IntWidth::U16,
            "u32" => IntWidth::U32,
            "u64" => IntWidth::U64,
            "u128" => IntWidth::U128,
            "usize" => IntWidth::USize,
            _ => return,
        };
        self.check_int_width_width(width, text, span);
    }

    pub(crate) fn check_int_width_width(&mut self, width: IntWidth, text: &str, span: &Span) {
        // 去掉后缀（i32/u8 等）与下划线
        let cleaned: String = text
            .chars()
            .take_while(|c| {
                c.is_ascii_digit()
                    || matches!(c, 'x' | 'X' | 'b' | 'B' | 'o' | 'O' | 'a'..='f' | 'A'..='F' | '_')
            })
            .collect();
        let cleaned = cleaned.replace('_', "");
        let (radix, digits) =
            if let Some(r) = cleaned.strip_prefix("0x").or(cleaned.strip_prefix("0X")) {
                (16u32, r)
            } else if let Some(r) = cleaned.strip_prefix("0b").or(cleaned.strip_prefix("0B")) {
                (2u32, r)
            } else if let Some(r) = cleaned.strip_prefix("0o").or(cleaned.strip_prefix("0O")) {
                (8u32, r)
            } else {
                (10u32, cleaned.as_str())
            };
        let Ok(v) = i128::from_str_radix(digits, radix) else {
            return; // 非法字面量由运行时/解析层处理
        };
        let (min, max) = match width {
            IntWidth::I8 => (i8::MIN as i128, i8::MAX as i128),
            IntWidth::I16 => (i16::MIN as i128, i16::MAX as i128),
            IntWidth::I32 => (i32::MIN as i128, i32::MAX as i128),
            IntWidth::I64 => (i64::MIN as i128, i64::MAX as i128),
            IntWidth::I128 => (i128::MIN, i128::MAX),
            IntWidth::ISize => (isize::MIN as i128, isize::MAX as i128),
            IntWidth::U8 => (0, u8::MAX as i128),
            IntWidth::U16 => (0, u16::MAX as i128),
            IntWidth::U32 => (0, u32::MAX as i128),
            IntWidth::U64 => (0, u64::MAX as i128),
            IntWidth::U128 => (0, u128::MAX as i128),
            IntWidth::USize => (0, usize::MAX as i128),
            IntWidth::Comptime => return,
        };
        if v < min || v > max {
            self.diags.push(Diagnostic::error(
                span.clone(),
                format!(
                    "integer literal `{text}` out of range for `{}` ({v} ∉ [{min}, {max}])",
                    width_name(width)
                ),
            ));
        }
    }

    /// alloc.init(T) 无参构造检测（C7）：返回 T 的待初始化字段集
    pub(crate) fn alloc_init_pending(
        &self,
        init: Option<&Expr>,
    ) -> Option<std::collections::HashSet<String>> {
        match init {
            Some(Expr::Call { callee, args, .. }) => {
                // callee 形如 alloc.init；args = [Ident(T)]（无字段形态）
                // 注意：`alloc.init` 第一个 `.` 解析为 Dot（parse_primary），非 Field
                if let Expr::Dot { base, field, .. } = callee.as_ref() {
                    if field == "init"
                        && matches!(base.as_ref(), Expr::Ident(b, _) if b == "alloc")
                        && args.len() == 1
                    {
                        if let Expr::Ident(tname, _) = &args[0] {
                            if let Some(TypeInfo {
                                kind: TypeKind::Class { fields, .. },
                                continuous,
                            }) = self.types.get(tname)
                            {
                                if *continuous {
                                    return None; // 连续类型：字面量构造/值语义
                                }
                                return Some(fields.iter().map(|f| f.name.clone()).collect());
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// 变量待初始化字段（无该变量或无待初始化要求 → None）
    pub(crate) fn missing_fields(
        &self,
        name: &str,
        scopes: &[HashMap<String, VarInfo>],
    ) -> Option<std::collections::HashSet<String>> {
        for s in scopes.iter().rev() {
            if let Some(info) = s.get(name) {
                return info.pending_fields.clone();
            }
        }
        None
    }

    pub(crate) fn lookup_var_ty(
        &self,
        name: &str,
        scopes: &[HashMap<String, VarInfo>],
    ) -> Option<SType> {
        for s in scopes.iter().rev() {
            if let Some(v) = s.get(name) {
                return v.ty.clone();
            }
        }
        // 全局变量
        if let Some(t) = self.globals.get(name) {
            return Some(self.ty_of(t));
        }
        // 内建注入：alloc / io / arena（放行）
        if is_builtin_ns(name) {
            return Some(SType::Unknown);
        }
        None
    }

    /// 变量是否已声明（区别于类型未知：VarInfo.ty 可能为 None）
    pub(crate) fn var_exists(&self, name: &str, scopes: &[HashMap<String, VarInfo>]) -> bool {
        scopes.iter().rev().any(|s| s.contains_key(name))
            || self.globals.contains_key(name)
            || is_builtin_ns(name)
    }

    /// 变量分配来源（M2.4：move 合法性检查）
    pub(crate) fn lookup_var_source(
        &self,
        name: &str,
        scopes: &[HashMap<String, VarInfo>],
    ) -> Option<AllocSource> {
        for s in scopes.iter().rev() {
            if let Some(v) = s.get(name) {
                return Some(v.source);
            }
        }
        if self.globals.contains_key(name) {
            return Some(AllocSource::Global);
        }
        None
    }

    /// 初始化表达式 → 分配来源（形态优先，类型兜底）
    pub(crate) fn infer_source(&self, init: Option<&Expr>, init_ty: Option<&SType>) -> AllocSource {
        if let Some(e) = init {
            match e {
                // alloc.init / alloc.alloc / arena.alloc / arena.init
                Expr::Call { callee, .. } => {
                    if let Expr::Dot { base, field, .. } = callee.as_ref() {
                        if let Expr::Ident(b, _) = base.as_ref() {
                            match (b.as_str(), field.as_str()) {
                                ("alloc", _) => return AllocSource::NonArena,
                                ("arena", _) => return AllocSource::Arena,
                                ("Arena", "init") => return AllocSource::Arena,
                                // 内建类型构造（String.from / Vec.init 等，Dot 形态）→ 新建对象
                                (b, f)
                                    if is_builtin_type(b)
                                        && matches!(f, "init" | "from" | "new") =>
                                {
                                    return AllocSource::NonArena;
                                }
                                _ => {}
                            }
                        }
                    }
                    // 集合/内建类型构造（Field 形态：Vec.init / Table.init）
                    if let Expr::Field { base, field, .. } = callee.as_ref() {
                        if let Expr::Ident(b, _) = base.as_ref() {
                            if is_builtin_type(b)
                                && matches!(field.as_str(), "init" | "from" | "new")
                            {
                                return AllocSource::NonArena;
                            }
                        }
                    }
                    // copy / box：新建对象归当前作用域
                    if let Expr::Ident(name, _) = callee.as_ref() {
                        if matches!(name.as_str(), "copy" | "box") {
                            return AllocSource::NonArena;
                        }
                    }
                }
                // 数组字面量 = 新建引用对象（作用域负责）
                Expr::ArrayLit(..) => return AllocSource::NonArena,
                _ => {}
            }
        }
        // 类型兜底：引用类型 → 新建对象；值类型 → 无所有权；未知 → 放行
        match init_ty {
            Some(t) if self.type_is_ref_st(t) => AllocSource::NonArena,
            Some(_) => AllocSource::None,
            None => AllocSource::Unknown,
        }
    }
}
