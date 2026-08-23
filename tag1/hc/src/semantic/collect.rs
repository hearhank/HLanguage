//! 第一遍收集：类型/函数/全局/错误集/命名空间 + using/import 导入。

use super::*;
use crate::ast::*;
use crate::diag::Diagnostic;
use crate::token::Span;

impl Checker {
    pub(crate) fn collect(&mut self, program: &Program) {
        for d in &program.decls {
            self.collect_decl_prefixed(d, "");
        }
    }

    /// M1.4：兄弟文件声明收集——对齐运行时 `load_siblings` 的文件私有规则：
    /// 类型/全局/错误集扁平+限定双登记（共享）；顶层函数文件私有不登记（避免跨文件
    /// 污染同名重载池，如 25/26 各自 load_config）；命名空间函数只登记限定名（扁平名
    /// 由目标文件 `using NS;` 导入）。
    pub(crate) fn collect_sibling(&mut self, program: &Program) {
        for d in &program.decls {
            self.collect_decl_prefixed_filter(d, "", false, false, true);
        }
    }

    /// M1.4：声明收集（Q21 命名空间）——扁平名 + 限定名双登记
    /// （`square` 供包内直接引用 / using 导入；`Math.square` 供限定访问）
    pub(crate) fn collect_decl_prefixed(&mut self, d: &Decl, prefix: &str) {
        self.collect_decl_prefixed_filter(d, prefix, false, false, false);
    }

    /// M7.2：依赖包收集——包名前缀 + 仅 `pub` + 不登记扁平名
    pub(crate) fn collect_dep(&mut self, program: &Program, prefix: &str) {
        let p = format!("{prefix}.");
        for d in &program.decls {
            self.collect_decl_prefixed_filter(d, &p, true, true, true);
        }
    }

    /// 声明收集核心：`skip_flat` 抑制扁平名（依赖包类型/全局/错误集）；`pub_only` 只登记 `pub` 项（跨包边界）；
    /// `skip_entry` 对齐运行时文件私有规则（函数）：跳过 main 与顶层函数，命名空间函数只登记限定名。
    pub(crate) fn collect_decl_prefixed_filter(
        &mut self,
        d: &Decl,
        prefix: &str,
        skip_flat: bool,
        pub_only: bool,
        skip_entry: bool,
    ) {
        // 跨包边界：非 pub 顶层声明（含非 pub namespace 整体）不可见
        if pub_only && !d.is_pub() {
            return;
        }
        match d {
            Decl::Class {
                name,
                ifaces,
                traits,
                fields,
                methods,
                span,
                ..
            } => {
                // 命名规范（Q22）：类型名 PascalCase（首字母大写）
                if !name.chars().next().map_or(true, |c| c.is_ascii_uppercase()) {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        format!(
                            "class `{name}` 命名必须首字母大写（PascalCase，如 `{}{}`）",
                            name[..1].to_uppercase(),
                            &name[1..]
                        ),
                    ));
                }
                let info = TypeInfo {
                    kind: TypeKind::Class {
                        fields: fields.clone(),
                        ifaces: ifaces.clone(),
                        methods: methods.clone(),
                        traits: traits.clone(),
                    },
                    continuous: false,
                };
                if skip_flat {
                    if !prefix.is_empty() {
                        self.types.insert(format!("{prefix}{name}"), info);
                    }
                } else if prefix.is_empty() {
                    self.types.insert(name.clone(), info);
                } else {
                    self.types.insert(format!("{prefix}{name}"), info.clone());
                    self.types.insert(name.clone(), info);
                }
                for m in methods {
                    if !skip_flat {
                        self.register_sig(&format!("{name}.{}", m.name), m);
                    }
                    if !prefix.is_empty() {
                        self.register_sig(&format!("{prefix}{name}.{}", m.name), m);
                    }
                }
            }
            Decl::Struct {
                name,
                traits,
                fields,
                span,
                ..
            } => {
                // 命名规范：类型名 PascalCase（首字母大写）
                if !name.chars().next().map_or(true, |c| c.is_ascii_uppercase()) {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        format!(
                            "struct `{name}` 命名必须首字母大写（PascalCase，如 `{}{}`）",
                            name[..1].to_uppercase(),
                            &name[1..]
                        ),
                    ));
                }
                let info = TypeInfo {
                    kind: TypeKind::Class {
                        fields: fields.clone(),
                        ifaces: vec![],
                        methods: vec![],
                        traits: traits.clone(),
                    },
                    continuous: true,
                };
                if skip_flat {
                    if !prefix.is_empty() {
                        self.types.insert(format!("{prefix}{name}"), info);
                    }
                } else if prefix.is_empty() {
                    self.types.insert(name.clone(), info);
                } else {
                    self.types.insert(format!("{prefix}{name}"), info.clone());
                    self.types.insert(name.clone(), info);
                }
            }
            Decl::Enum {
                name,
                variants,
                span,
                ..
            } => {
                // 命名规范（Q22）：类型名 PascalCase（首字母大写）
                if !name.chars().next().map_or(true, |c| c.is_ascii_uppercase()) {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        format!(
                            "enum `{name}` 命名必须首字母大写（PascalCase，如 `{}{}`）",
                            name[..1].to_uppercase(),
                            &name[1..]
                        ),
                    ));
                }
                let info = TypeInfo {
                    kind: TypeKind::Enum {
                        variants: variants.clone(),
                    },
                    continuous: false,
                };
                if skip_flat {
                    if !prefix.is_empty() {
                        self.types.insert(format!("{prefix}{name}"), info);
                    }
                } else if prefix.is_empty() {
                    self.types.insert(name.clone(), info);
                } else {
                    self.types.insert(format!("{prefix}{name}"), info.clone());
                    self.types.insert(name.clone(), info);
                }
            }
            Decl::Union {
                name, fields, span, ..
            } => {
                // 命名规范（Q22）：类型名 PascalCase（首字母大写）
                if !name.chars().next().map_or(true, |c| c.is_ascii_uppercase()) {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        format!(
                            "union `{name}` 命名必须首字母大写（PascalCase，如 `{}{}`）",
                            name[..1].to_uppercase(),
                            &name[1..]
                        ),
                    ));
                }
                // K1 语义（ADR-0014）：仅标量字段——内存双关工具，引用/堆字段编译错误
                for fd in fields {
                    if let Type::Named(n, _) = fd.ty.strip() {
                        if !Self::is_scalar_ty_name(n) {
                            self.diags.push(Diagnostic::error(
                                fd.span.clone(),
                                format!(
                                    "union `{name}` 字段 `{}` 必须为标量类型（i8..u128/f32/f64/bool，得 `{n}`）",
                                    fd.name
                                ),
                            ));
                        }
                    } else {
                        self.diags.push(Diagnostic::error(
                            fd.span.clone(),
                            format!(
                                "union `{name}` 字段 `{}` 必须为标量类型（内存双关工具；得非标量形态）",
                                fd.name
                            ),
                        ));
                    }
                }
                let info = TypeInfo {
                    kind: TypeKind::Union {
                        fields: fields.clone(),
                    },
                    continuous: false,
                };
                if skip_flat {
                    if !prefix.is_empty() {
                        self.types.insert(format!("{prefix}{name}"), info);
                    }
                } else if prefix.is_empty() {
                    self.types.insert(name.clone(), info);
                } else {
                    self.types.insert(format!("{prefix}{name}"), info.clone());
                    self.types.insert(name.clone(), info);
                }
            }
            Decl::Interface {
                name,
                supers,
                methods,
                span,
                ..
            } => {
                // 接口命名约定：必须以 I 开头（如 `IShape` / `IParse`）
                if !name.starts_with('I') {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        format!("interface `{name}` 必须以 I 开头（如 `I{name}`）"),
                    ));
                }
                let info = TypeInfo {
                    kind: TypeKind::Interface {
                        supers: supers.clone(),
                        methods: methods.clone(),
                    },
                    continuous: false,
                };
                if skip_flat {
                    if !prefix.is_empty() {
                        self.types.insert(format!("{prefix}{name}"), info);
                    }
                } else if prefix.is_empty() {
                    self.types.insert(name.clone(), info);
                } else {
                    self.types.insert(format!("{prefix}{name}"), info.clone());
                    self.types.insert(name.clone(), info);
                }
            }
            Decl::Fn {
                name,
                type_params,
                params,
                ret,
                where_clause,
                is_test,
                is_async,
                ..
            } => {
                let sig = self.make_sig(
                    type_params.clone(),
                    params.clone(),
                    ret.clone(),
                    where_clause.clone(),
                    *is_async,
                );
                // [test] fn 不进重载池（运行时按 test 名收集）
                if !is_test {
                    // 兄弟文件（skip_entry）：跳过 main 与顶层函数（文件私有——对齐运行时
                    // register_fn_decl_prefixed_filter 的 skip_entry 规则）；命名空间函数
                    // 只登记限定名（扁平名由 `using` 导入）。
                    if !(skip_entry && (name == "main" || prefix.is_empty())) {
                        // 模块隔离（A2b）：`[module]` 成员不登记扁平名（仅限定名，供 import 复制）
                        if !skip_entry && !skip_flat {
                            self.funcs
                                .entry(name.clone())
                                .or_default()
                                .push(sig.clone());
                        }
                        if !prefix.is_empty() {
                            self.funcs
                                .entry(format!("{prefix}{name}"))
                                .or_default()
                                .push(sig);
                        }
                    }
                }
            }
            Decl::Global { name, ty, .. } => {
                if let Some(t) = ty {
                    if !skip_flat {
                        self.globals.insert(name.clone(), t.clone());
                    }
                    if !prefix.is_empty() {
                        self.globals.insert(format!("{prefix}{name}"), t.clone());
                    }
                }
            }
            Decl::Const { name, ty, .. } => {
                // 错误集别名：const FileError = error{ NotFound, ... }
                if let Some(Type::Named(tn, _)) = ty {
                    if let Some(rest) = tn.strip_prefix("error_set:") {
                        let members: ErrorSet =
                            rest.split(',').map(|s| s.trim().to_string()).collect();
                        if !skip_flat {
                            self.error_sets.insert(name.clone(), members.clone());
                        }
                        if !prefix.is_empty() {
                            self.error_sets.insert(format!("{prefix}{name}"), members);
                        }
                    }
                }
            }
            Decl::Namespace {
                name,
                decls,
                is_module,
                span,
                ..
            } => {
                // 命名规范（Q22）：命名空间名 PascalCase（首字母大写）
                if !name.chars().next().map_or(true, |c| c.is_ascii_uppercase()) {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        format!(
                            "namespace `{name}` 命名必须首字母大写（PascalCase，如 `{}{}`）",
                            name[..1].to_uppercase(),
                            &name[1..]
                        ),
                    ));
                }
                if *is_module {
                    // `[module]` 隔离（2026-08-17 定案）：模块名不登记入命名空间——
                    // 裸限定访问（`M.f`）不可达，仅经 `import` 授予访问；
                    // 成员仅登记限定名（`M.f`，供 import_whole_module 复制）。
                    let np = format!("{prefix}{name}.");
                    for inner in decls {
                        self.collect_decl_prefixed_filter(inner, &np, true, pub_only, skip_entry);
                    }
                } else if skip_flat {
                    if !prefix.is_empty() {
                        self.namespaces.insert(format!("{prefix}{name}"));
                    }
                    let np = format!("{prefix}{name}.");
                    for inner in decls {
                        self.collect_decl_prefixed_filter(
                            inner, &np, skip_flat, pub_only, skip_entry,
                        );
                    }
                } else {
                    self.namespaces.insert(name.clone());
                    let np = format!("{prefix}{name}.");
                    for inner in decls {
                        self.collect_decl_prefixed_filter(
                            inner, &np, skip_flat, pub_only, skip_entry,
                        );
                    }
                }
            }
            _ => {}
        }
    }

    /// M1.4：语义层 `using NS;` 导入——限定名（函数/类型/全局）复制为扁平名
    /// （与运行时 apply_usings 对齐；文件自身定义优先）
    pub(crate) fn apply_usings(&mut self, program: &Program) {
        for d in &program.decls {
            self.collect_using_decl(d);
        }
    }

    pub(crate) fn collect_using_decl(&mut self, d: &Decl) {
        match d {
            Decl::Using { path, alias, .. } => {
                let prefix = format!("{}.{}", path.join("."), "");
                let flat_of = |member: &str| match alias {
                    Some(a) => format!("{a}.{member}"),
                    None => member.to_string(),
                };
                // 函数（跳过方法：成员名不含 `.`）
                let fkeys: Vec<String> = self
                    .funcs
                    .keys()
                    .filter(|k| k.starts_with(&prefix) && !k[prefix.len()..].contains('.'))
                    .cloned()
                    .collect();
                for k in fkeys {
                    let member = k[prefix.len()..].to_string();
                    let flat = flat_of(&member);
                    if !self.funcs.contains_key(&flat) {
                        let defs = self.funcs.get(&k).cloned().unwrap_or_default();
                        if !defs.is_empty() {
                            self.funcs.entry(flat).or_default().extend(defs);
                        }
                    }
                }
                // 类型
                let tkeys: Vec<String> = self
                    .types
                    .keys()
                    .filter(|k| k.starts_with(&prefix))
                    .cloned()
                    .collect();
                for k in tkeys {
                    let member = k[prefix.len()..].to_string();
                    let flat = flat_of(&member);
                    if !self.types.contains_key(&flat) {
                        if let Some(info) = self.types.get(&k) {
                            self.types.insert(flat, info.clone());
                        }
                    }
                }
                // 全局
                let gkeys: Vec<String> = self
                    .globals
                    .keys()
                    .filter(|k| k.starts_with(&prefix))
                    .cloned()
                    .collect();
                for k in gkeys {
                    let member = k[prefix.len()..].to_string();
                    let flat = flat_of(&member);
                    if !self.globals.contains_key(&flat) {
                        if let Some(t) = self.globals.get(&k) {
                            self.globals.insert(flat, t.clone());
                        }
                    }
                }
            }
            Decl::Namespace { decls, .. } => {
                for inner in decls {
                    self.collect_using_decl(inner);
                }
            }
            _ => {}
        }
    }

    // ---------- ADR-0010：import 语义（A2a） ----------

    /// 文件级 `import` 语句语义——符号登记（前缀 + 别名）与冲突规则。
    /// 三种形态（06-08-modules.md §import）：
    /// - `import pkg.mod;`：整模块导入——绑定名 = 末段（或 `as` 别名），
    ///   全部成员以 `{绑定}.{member}` 复制（命名空间前缀 + 限定名可用）
    /// - `import pkg.mod as m;`：整模块 + 别名
    /// - `import pkg.mod.{a, b as c};`：符号选择——函数/类型/全局直接可用；
    ///   命名空间成员以前缀绑定（`my.print` 形态）
    ///
    /// 冲突规则（06-08）：显式符号选择（非通配）优先于通配；整模块导入遇
    /// 同名冲突 → 编译错误；文件自身定义优先（不被导入覆盖）。
    pub(crate) fn apply_imports(&mut self, program: &Program) {
        for d in &program.decls {
            self.collect_import_decl(d);
        }
    }

    pub(crate) fn collect_import_decl(&mut self, d: &Decl) {
        match d {
            Decl::Import {
                path,
                alias,
                select,
                span,
            } => {
                let target_prefix = format!("{}.", path.join("."));
                match select {
                    Some(syms) => {
                        // 显式符号选择（优先级高于通配）
                        for (sym, sym_alias) in syms {
                            let bound = sym_alias.clone().unwrap_or_else(|| sym.clone());
                            self.import_explicit_symbol(&target_prefix, sym, &bound, span);
                        }
                    }
                    None => {
                        let bound = alias
                            .clone()
                            .unwrap_or_else(|| path.last().cloned().unwrap_or_default());
                        self.import_whole_module(&target_prefix, &bound, span);
                    }
                }
            }
            Decl::Namespace { decls, .. } => {
                for inner in decls {
                    self.collect_import_decl(inner);
                }
            }
            _ => {}
        }
    }

    /// 显式符号选择导入：`import pkg.mod.{sym as bound}`——
    /// 函数/类型/全局复制到绑定名（直接可用）；命名空间成员 → 前缀绑定。
    pub(crate) fn import_explicit_symbol(
        &mut self,
        target_prefix: &str,
        sym: &str,
        bound: &str,
        span: &Span,
    ) {
        let q = format!("{target_prefix}{sym}");
        // 命名空间成员（有子成员或内建命名空间）→ 前缀绑定（`my.print` 形态）
        let is_ns = self.namespaces.contains(&q)
            || self.funcs.keys().any(|k| k.starts_with(&format!("{q}.")))
            || self.types.keys().any(|k| k.starts_with(&format!("{q}.")))
            || is_builtin_ns(&q)
            || is_builtin_ns(sym);
        if is_ns {
            self.namespaces.insert(bound.to_string());
        }
        // 函数符号 → 绑定名直调（`sq(4)`）
        if let Some(defs) = self.funcs.get(&q) {
            if !defs.is_empty() && !self.funcs.contains_key(bound) {
                self.funcs.insert(bound.to_string(), defs.clone());
                self.imported.insert(bound.to_string());
            }
        }
        // 类型符号 → 绑定名直接引用（`Line{...}`）
        if let Some(info) = self.types.get(&q) {
            if !self.types.contains_key(bound) {
                self.types.insert(bound.to_string(), info.clone());
                self.imported.insert(bound.to_string());
            }
        }
        // 全局符号 → 绑定名
        if let Some(t) = self.globals.get(&q) {
            if !self.globals.contains_key(bound) {
                self.globals.insert(bound.to_string(), t.clone());
                self.imported.insert(bound.to_string());
            }
        }
        // 未解析符号：保守放行（库/兄弟文件未知，运行时诊断）——不报错
        let _ = span;
    }

    /// 整模块导入：`import pkg.mod;`——绑定名前缀登记 + 全部成员复制为 `{bound}.{member}`。
    /// 冲突：同名成员已被另一整模块导入登记 → 编译错误（通配冲突，06-08）。
    pub(crate) fn import_whole_module(&mut self, target_prefix: &str, bound: &str, span: &Span) {
        self.namespaces.insert(bound.to_string());
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
                // 同名已被整模块导入 → 通配冲突（文件自身定义优先，不覆盖）
                if self.imported.contains(&flat) {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        format!("import 冲突：整模块导入 `{bound}` 的成员 `{member}` 与已导入符号重名（用 `as 别名` 显式改名）"),
                    ));
                }
                continue;
            }
            let defs = self.funcs.get(&k).cloned().unwrap_or_default();
            if !defs.is_empty() {
                self.funcs.insert(flat.clone(), defs);
                self.imported.insert(flat);
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
            if self.types.contains_key(&flat) {
                if self.imported.contains(&flat) {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        format!(
                            "import 冲突：整模块导入 `{bound}` 的类型 `{member}` 与已导入类型重名"
                        ),
                    ));
                }
                continue;
            }
            if let Some(info) = self.types.get(&k) {
                self.types.insert(flat.clone(), info.clone());
                self.imported.insert(flat);
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
            if self.globals.contains_key(&flat) {
                continue;
            }
            if let Some(t) = self.globals.get(&k) {
                self.globals.insert(flat.clone(), t.clone());
                self.imported.insert(flat);
            }
        }
    }

    pub(crate) fn make_sig(
        &self,
        type_params: Vec<String>,
        params: Vec<Param>,
        ret: Option<Type>,
        where_clause: Vec<(String, Type)>,
        is_async: bool,
    ) -> FnSig {
        // 泛型参数名 = 显式 <T> 表 + where 键 + 参数/返回类型中出现的泛型标识符
        let mut generics: Vec<String> = where_clause.iter().map(|(t, _)| t.clone()).collect();
        generics.extend(type_params);
        for p in &params {
            collect_generic_names(&p.ty, &mut generics);
        }
        if let Some(r) = &ret {
            collect_generic_names(r, &mut generics);
        }
        generics.sort();
        generics.dedup();
        FnSig {
            params,
            ret,
            where_clause,
            generics,
            is_async,
        }
    }

    pub(crate) fn register_sig(&mut self, qname: &str, m: &Method) {
        let sig = self.make_sig(
            m.type_params.clone(),
            m.params.clone(),
            m.ret.clone(),
            m.where_clause.clone(),
            false,
        );
        self.funcs.entry(qname.to_string()).or_default().push(sig);
    }
}
