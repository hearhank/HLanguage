//! 存储形态验证：[continuous] 值类型 / 接口实现契约。

use super::*;
use crate::ast::*;
use crate::diag::Diagnostic;
use crate::token::Span;

impl Checker {
    // ---------- 存储形态验证（[continuous] 字段全为值类型） ----------

    pub(crate) fn validate_continuous(&mut self) {
        let names: Vec<String> = self
            .types
            .iter()
            .filter_map(|(n, info)| {
                if info.continuous {
                    Some(n.clone())
                } else {
                    None
                }
            })
            .collect();
        for n in names {
            let (fields, traits) = match &self.types[&n].kind {
                TypeKind::Class { fields, traits, .. } => (fields.clone(), traits.clone()),
                TypeKind::Struct { fields, traits, .. } => (fields.clone(), traits.clone()),
                _ => continue,
            };
            // [align(N)] 值校验：必须是 2 的幂，范围 1..=128
            for t in &traits {
                if let Trait::Align(a) = t {
                    let a = *a;
                    if a == 0 || !a.is_power_of_two() || a > 128 {
                        self.diags.push(Diagnostic::error(
                            Span::new(0, 0, 0, 0),
                            format!(
                                "invalid alignment `{a}` for struct `{n}`; alignment must be a power of 2 in range 1..=128"
                            ),
                        ));
                    }
                }
            }
            for f in &fields {
                if !self.type_is_value(&f.ty) {
                    self.diags.push(Diagnostic::error(
                        f.span.clone(),
                        format!(
                            "struct `{n}` has non-value field `{}` of type `{}`; \
                             struct fields must be value types (scalar, fixed array, or nested struct)",
                            f.name,
                            self.ty_display(&f.ty)
                        ),
                    ));
                }
            }
        }
    }

    /// M2.1 接口三用途真实实现（去占位）：验证 class 的 implements 冒号标注实际满足
    /// 接口方法契约（① 标记 class 功能 → 须有对应方法实现；③ 类型参数编译可验证 →
    /// 单态化检查实现：方法名 + 参数/返回签名兼容）。内建接口（ICompare/INumber/
    /// IIterable/Io 等）不在 types 表登记 → 跳过（编译器内建实现）。
    pub(crate) fn validate_interface_impls(&mut self, program: &Program) {
        // 快照 class 声明（含 span），避免与 self.types 借用冲突
        let mut classes: Vec<(&str, &Vec<Type>, &Vec<Method>, &Span)> = Vec::new();
        collect_class_decls(&program.decls, &mut classes);
        for (name, ifaces, methods, span) in classes {
            for iface in ifaces {
                let iname = match iface.strip() {
                    Type::Named(n, _) => n.clone(),
                    _ => continue,
                };
                let mut visited: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                self.check_iface_impl(name, &iname, methods, span, &mut visited);
            }
        }
    }

    /// 递归收集 class 声明（namespace 内展平）
    pub(crate) fn check_iface_impl(
        &mut self,
        class_name: &str,
        iface_name: &str,
        class_methods: &[Method],
        span: &Span,
        visited: &mut std::collections::HashSet<String>,
    ) {
        if !visited.insert(iface_name.to_string()) {
            return; // 循环继承防护
        }
        let (iface_methods, supers) = match self.types.get(iface_name).map(|i| &i.kind) {
            Some(TypeKind::Interface { methods, supers }) => (methods.clone(), supers.clone()),
            _ => return, // 内建/未知接口：放行（编译器内建实现）
        };
        for im in &iface_methods {
            let satisfied = class_methods
                .iter()
                .any(|cm| cm.name == im.name && self.method_satisfies(im, cm, class_name));
            if !satisfied {
                self.diags.push(Diagnostic::error(
                    span.clone(),
                    format!(
                        "class `{class_name}` does not implement method `{}` required by interface `{iface_name}`",
                        self.method_sig_display(im)
                    ),
                ));
            }
        }
        for s in &supers {
            if let Type::Named(sn, _) = s.strip() {
                self.check_iface_impl(class_name, sn, class_methods, span, visited);
            }
        }
    }

    /// 接口方法契约是否被 class 方法满足（方法名相同 + 参数个数与类型兼容 + 返回类型兼容；
    /// Self 在两侧均替换为实现类型）
    pub(crate) fn method_satisfies(
        &self,
        iface_m: &Method,
        class_m: &Method,
        class_name: &str,
    ) -> bool {
        if iface_m.params.len() != class_m.params.len() {
            return false;
        }
        for (ip, cp) in iface_m.params.iter().zip(class_m.params.iter()) {
            if !self.sig_type_compat(&ip.ty, &cp.ty, class_name) {
                return false;
            }
        }
        match (&iface_m.ret, &class_m.ret) {
            (None, None) => true,
            (Some(ir), Some(cr)) => self.sig_type_compat(ir, cr, class_name),
            _ => false,
        }
    }

    /// 接口/实现签名类型兼容：Self → 实现类型替换后静态类型相等；
    /// 任一为未知/推断/泛型时宽松放行（能精确判定才报错）
    pub(crate) fn sig_type_compat(&self, a: &Type, b: &Type, class_name: &str) -> bool {
        let a = self.subst_self_ty(a, class_name);
        let b = self.subst_self_ty(b, class_name);
        let (sa, sb) = (self.ty_of(&a), self.ty_of(&b));
        if sa == sb {
            return true;
        }
        matches!(sa, SType::Unknown | SType::Infer | SType::Generic(_))
            || matches!(sb, SType::Unknown | SType::Infer | SType::Generic(_))
    }

    /// 类型中 `Self` 替换为实现类型（接口/class 方法签名比较用）
    pub(crate) fn subst_self_ty(&self, t: &Type, class_name: &str) -> Type {
        match t {
            Type::Named(n, args) if n == "Self" => Type::Named(class_name.to_string(), vec![]),
            Type::Named(n, args) => Type::Named(
                n.clone(),
                args.iter()
                    .map(|x| self.subst_self_ty(x, class_name))
                    .collect(),
            ),
            Type::Ptr(inner, m) => Type::Ptr(Box::new(self.subst_self_ty(inner, class_name)), *m),
            Type::Slice(inner, m) => {
                Type::Slice(Box::new(self.subst_self_ty(inner, class_name)), *m)
            }
            Type::Optional(inner) => {
                Type::Optional(Box::new(self.subst_self_ty(inner, class_name)))
            }
            Type::ErrorUnion(e, inner) => Type::ErrorUnion(
                e.as_ref()
                    .map(|x| Box::new(self.subst_self_ty(x, class_name))),
                Box::new(self.subst_self_ty(inner, class_name)),
            ),
            Type::Tuple(ts) => Type::Tuple(
                ts.iter()
                    .map(|x| self.subst_self_ty(x, class_name))
                    .collect(),
            ),
            Type::Array(n, inner) => {
                Type::Array(*n, Box::new(self.subst_self_ty(inner, class_name)))
            }
            other => other.clone(),
        }
    }

    /// 接口方法签名展示（错误信息用）：`name(a: T, ...) -> ret`
    pub(crate) fn method_sig_display(&self, m: &Method) -> String {
        let params: Vec<String> = m
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name, self.ty_display(&p.ty)))
            .collect();
        match &m.ret {
            Some(r) => format!(
                "{}({}) -> {}",
                m.name,
                params.join(", "),
                self.ty_display(r)
            ),
            None => format!("{}({})", m.name, params.join(", ")),
        }
    }

    /// K1 标量类型名（union 字段限定；与运行时 scalar_size 宽度表同源）
    pub(crate) fn is_scalar_ty_name(n: &str) -> bool {
        matches!(
            n,
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
        )
    }

    /// 类型是否为值类型（标量/连续 class/元组/纯常量枚举/定长数组？——数组为引用类型）
    pub(crate) fn type_is_value(&self, t: &Type) -> bool {
        match t.strip() {
            Type::Named(n, _) => {
                if matches!(
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
                        | "f16"
                        | "f32"
                        | "f64"
                        | "f128"
                        | "bool"
                ) {
                    return true;
                }
                match self.types.get(n) {
                    Some(info) => {
                        if info.continuous {
                            return true;
                        }
                        // 纯常量枚举（变体均无负载）= 值类型
                        if let TypeKind::Enum { variants } = &info.kind {
                            return variants.iter().all(|v| v.payload.is_none());
                        }
                        // K1 union（仅标量字段）= 值类型（无堆内容，赋值即复制）
                        if let TypeKind::Union { .. } = &info.kind {
                            return true;
                        }
                        false
                    }
                    None => {
                        // 内建引用类型（String/Vec/Map/Deque/Table）：非值类型
                        if is_builtin_type(n) || is_collection(n) {
                            return false;
                        }
                        true // 未知类型：放行
                    }
                }
            }
            Type::Tuple(ts) => ts.iter().all(|t| self.type_is_value(t)),
            Type::Optional(_) | Type::Slice(_, _) | Type::Ptr(_, _) => false,
            Type::Array(_, inner) => self.type_is_value(inner), // 定长数组的元素为值类型则可内联
            _ => false,
        }
    }

    pub(crate) fn ty_display(&self, t: &Type) -> String {
        match t.strip() {
            Type::Named(n, args) => {
                if args.is_empty() {
                    n.clone()
                } else {
                    let a = args
                        .iter()
                        .map(|x| self.ty_display(x))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{n}<{a}>")
                }
            }
            Type::Ptr(inner, true) => format!("*mut {}", self.ty_display(inner)),
            Type::Ptr(inner, false) => format!("*{}", self.ty_display(inner)),
            Type::Slice(inner, _) => format!("&[{}]", self.ty_display(inner)),
            Type::Optional(inner) => format!("?{}", self.ty_display(inner)),
            Type::ErrorUnion(e, inner) => match e {
                Some(es) => format!("{}!{}", self.ty_display(es), self.ty_display(inner)),
                None => format!("!{}", self.ty_display(inner)),
            },
            Type::Tuple(ts) => {
                let a = ts
                    .iter()
                    .map(|x| self.ty_display(x))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({a})")
            }
            Type::Array(n, inner) => format!("[{n}]{}", self.ty_display(inner)),
            _ => "?".into(),
        }
    }
}
