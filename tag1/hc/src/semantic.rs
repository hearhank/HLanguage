//! 语义检查（M2.2 完整：表达式级类型检查 / 期望类型传播 / 字段与索引校验 /
//! 存储形态验证 / 泛型 where 约束验证）
//!
//! tag1 静态 pass：在解释器 load 之前运行。检查策略：**能精确判定才报错**（准确
//! 可靠——调试友好语言要求不误报）；类型信息不足（Unknown / 泛型未单态化）时
//! 保守放行，交由运行时诊断。

use crate::ast::*;
use crate::diag::Diagnostic;
use crate::token::Span;
use std::collections::HashMap;

// ---------- 静态类型 ----------

/// 编译期静态类型（表达式类型检查用）
#[derive(Debug, Clone, PartialEq)]
enum SType {
    /// 整数（含惰性宽度：字面量在期望类型处定型）
    Int {
        width: IntWidth,
    },
    Float,
    Bool,
    Void,
    /// String 或字符串字面量（&[u8] 静态只读切片）
    Str,
    /// 切片 &[T]
    Slice(Box<SType>),
    /// 用户类型 / 内建泛型集合 Vec/Map/Deque/Table（含类型实参）
    Named(String, Vec<SType>),
    Tuple(Vec<SType>),
    Optional(Box<SType>),
    /// E!T；None = anyerror
    ErrorUnion(Option<Box<SType>>, Box<SType>),
    /// *T / *mut T
    Ptr(Box<SType>, bool),
    /// 定长数组 [N]T
    Array(usize, Box<SType>),
    /// 省略标注（推断）
    Infer,
    /// 泛型参数 T（大写未登记标识符，与运行时启发式一致）
    Generic(String),
    /// 无法判定——保守放行
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntWidth {
    I8,
    I16,
    I32,
    I64,
    I128,
    ISize,
    U8,
    U16,
    U32,
    U64,
    U128,
    USize,
    /// comptime_int：字面量惰性宽度，使用处定型
    Comptime,
}

impl SType {
    fn numeric(&self) -> bool {
        matches!(self, SType::Int { .. } | SType::Float)
    }
    fn integer(&self) -> bool {
        matches!(self, SType::Int { .. })
    }
    fn definite(&self) -> bool {
        !matches!(self, SType::Infer | SType::Unknown)
    }
    /// 静态类型显示名（诊断文案）
    fn name(&self) -> String {
        match self {
            SType::Int { width } => match width {
                IntWidth::Comptime => "integer".into(),
                _ => "integer".into(),
            },
            SType::Float => "float".into(),
            SType::Bool => "bool".into(),
            SType::Void => "void".into(),
            SType::Str => "String".into(),
            SType::Slice(t) => format!("&[{}]", t.name()),
            SType::Named(n, args) => {
                if args.is_empty() {
                    n.clone()
                } else {
                    let a = args.iter().map(|t| t.name()).collect::<Vec<_>>().join(", ");
                    format!("{n}({a})")
                }
            }
            SType::Tuple(ts) => {
                let a = ts.iter().map(|t| t.name()).collect::<Vec<_>>().join(", ");
                format!("({a})")
            }
            SType::Optional(t) => format!("?{}", t.name()),
            SType::ErrorUnion(e, t) => match e {
                Some(es) => format!("{}!{}", es.name(), t.name()),
                None => format!("!{}", t.name()),
            },
            SType::Ptr(t, mut_) => {
                if *mut_ {
                    format!("*mut {}", t.name())
                } else {
                    format!("*{}", t.name())
                }
            }
            SType::Array(n, t) => format!("[{n}]{}", t.name()),
            SType::Infer => "_".into(),
            SType::Generic(n) => n.clone(),
            SType::Unknown => "?".into(),
        }
    }
}

// ---------- 类型登记 ----------

/// 编译时类型元数据（M2.2：存储形态 + 字段/变体/接口）
#[derive(Clone)]
pub struct TypeInfo {
    pub kind: TypeKind,
    pub continuous: bool,
}

#[derive(Clone)]
pub enum TypeKind {
    Class {
        fields: Vec<FieldDecl>,
        ifaces: Vec<Type>,
        methods: Vec<Method>,
        traits: Vec<Trait>,
    },
    Enum {
        variants: Vec<EnumVariant>,
    },
    Interface {
        supers: Vec<Type>,
        methods: Vec<Method>,
    },
}

/// 错误集（M2.6）：error{ NotFound, ... } 成员集合
pub type ErrorSet = std::collections::HashSet<String>;

/// 函数签名（M2.2 重载匹配 + where 约束验证）
#[derive(Clone)]
struct FnSig {
    params: Vec<Param>,
    ret: Option<Type>,
    where_clause: Vec<(String, Type)>,
    /// 泛型参数名（where 键 + 参数/返回类型中的泛型标识符）
    generics: Vec<String>,
}

/// 变量声明类型（静态推断 / definite assignment 跟踪）
#[derive(Clone)]
struct VarInfo {
    ty: Option<SType>,
    pending_fields: Option<std::collections::HashSet<String>>,
}

pub fn check(program: &Program) -> Vec<Diagnostic> {
    let mut checker = Checker {
        types: HashMap::new(),
        funcs: HashMap::new(),
        globals: HashMap::new(),
        error_sets: HashMap::new(),
        namespaces: std::collections::HashSet::new(),
        diags: Vec::new(),
    };
    checker.collect(program);
    checker.validate_continuous();
    checker.check_program(program);
    checker.diags
}

struct Checker {
    types: HashMap<String, TypeInfo>,
    /// 函数名 → 重载签名池（含类型方法 "Type.method"）
    funcs: HashMap<String, Vec<FnSig>>,
    globals: HashMap<String, Type>,
    /// 错误集：const 名 → 成员（M2.6）
    error_sets: HashMap<String, ErrorSet>,
    /// 命名空间声明（M1.4：namespace NS { ... }）
    namespaces: std::collections::HashSet<String>,
    diags: Vec<Diagnostic>,
}

/// 内建函数名（test 注入断言 / @ 内建 / 标准库工具）——放行不做重载匹配
fn is_builtin_fn(name: &str) -> bool {
    name.starts_with('@')
        || matches!(
            name,
            "expect"
                | "expect_eq"
                | "expect_neq"
                | "expect_error"
                | "expect_eq_slices"
                | "copy"
                | "box"
                | "min"
                | "max"
                | "parse_int"
                | "parse_float"
                | "parse_bool"
                | "parse_char"
                | "read_u64_le"
                | "read_u32_le"
                | "read_u16_le"
                | "read_i64_le"
                | "sort"
                | "binary_search"
                | "sqrt"
                | "fmt_int"
                | "fmt_float"
        )
}

/// 内建命名空间（io / alloc / arena / math / utf8 / test_io 等）——放行
fn is_builtin_ns(name: &str) -> bool {
    matches!(
        name,
        "io" | "alloc" | "arena" | "math" | "debug" | "utf8" | "test_io"
    )
}

/// 序列化内建方法（Type.from_bytes/to_json 等，编译器内建契约）——不要求类型登记
fn is_serialize_builtin(field: &str) -> bool {
    matches!(field, "from_json" | "to_json" | "from_bytes" | "to_bytes")
}

/// 内建类型（编译器内建实现；方法放行）
fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "String" | "Vec" | "Map" | "Deque" | "Table" | "Allocator" | "Arena" | "ExitType"
    )
}

/// 内建集合类型（可迭代 / 引用语义）
fn is_collection(name: &str) -> bool {
    matches!(name, "Vec" | "Map" | "Deque" | "Table" | "String")
}

impl Checker {
    // ---------- 第一遍：收集元数据 ----------

    fn collect(&mut self, program: &Program) {
        for d in &program.decls {
            self.collect_decl(d);
        }
    }

    fn collect_decl(&mut self, d: &Decl) {
        match d {
            Decl::Class {
                name,
                ifaces,
                traits,
                fields,
                methods,
                ..
            } => {
                let continuous = traits.iter().any(|t| matches!(t, Trait::Continuous));
                self.types.insert(
                    name.clone(),
                    TypeInfo {
                        kind: TypeKind::Class {
                            fields: fields.clone(),
                            ifaces: ifaces.clone(),
                            methods: methods.clone(),
                            traits: traits.clone(),
                        },
                        continuous,
                    },
                );
                for m in methods {
                    self.register_sig(&format!("{name}.{}", m.name), m);
                }
            }
            Decl::Enum { name, variants, .. } => {
                self.types.insert(
                    name.clone(),
                    TypeInfo {
                        kind: TypeKind::Enum {
                            variants: variants.clone(),
                        },
                        continuous: false,
                    },
                );
            }
            Decl::Interface {
                name,
                supers,
                methods,
                ..
            } => {
                self.types.insert(
                    name.clone(),
                    TypeInfo {
                        kind: TypeKind::Interface {
                            supers: supers.clone(),
                            methods: methods.clone(),
                        },
                        continuous: false,
                    },
                );
            }
            Decl::Fn {
                name,
                params,
                ret,
                where_clause,
                is_test,
                ..
            } => {
                let sig = self.make_sig(params.clone(), ret.clone(), where_clause.clone());
                // test fn 不进重载池（运行时按 test 名收集）
                if !is_test {
                    self.funcs.entry(name.clone()).or_default().push(sig);
                }
            }
            Decl::Global { name, ty, .. } => {
                if let Some(t) = ty {
                    self.globals.insert(name.clone(), t.clone());
                }
            }
            Decl::Const { name, ty, .. } => {
                // 错误集别名：const FileError = error{ NotFound, ... }
                if let Some(Type::Named(tn, _)) = ty {
                    if let Some(rest) = tn.strip_prefix("error_set:") {
                        let members: ErrorSet =
                            rest.split(',').map(|s| s.trim().to_string()).collect();
                        self.error_sets.insert(name.clone(), members);
                    }
                }
            }
            Decl::Namespace { name, decls, .. } => {
                self.namespaces.insert(name.clone());
                for inner in decls {
                    self.collect_decl(inner);
                }
            }
            _ => {}
        }
    }

    fn make_sig(
        &self,
        params: Vec<Param>,
        ret: Option<Type>,
        where_clause: Vec<(String, Type)>,
    ) -> FnSig {
        // 泛型参数名 = where 键 + 参数/返回类型中出现的泛型标识符
        let mut generics: Vec<String> = where_clause.iter().map(|(t, _)| t.clone()).collect();
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
        }
    }

    fn register_sig(&mut self, qname: &str, m: &Method) {
        let sig = self.make_sig(m.params.clone(), m.ret.clone(), m.where_clause.clone());
        self.funcs.entry(qname.to_string()).or_default().push(sig);
    }

    // ---------- 存储形态验证（[continuous] 字段全为值类型） ----------

    fn validate_continuous(&mut self) {
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
            let fields = match &self.types[&n].kind {
                TypeKind::Class { fields, .. } => fields.clone(),
                _ => continue,
            };
            for f in &fields {
                if !self.type_is_value(&f.ty) {
                    self.diags.push(Diagnostic::error(
                        f.span.clone(),
                        format!(
                            "`[continuous]` type `{n}` has non-value field `{}` of type `{}`; \
                             continuous types require all-value fields (use a heap class instead)",
                            f.name,
                            self.ty_display(&f.ty)
                        ),
                    ));
                }
            }
        }
    }

    /// 类型是否为值类型（标量/连续 class/元组/纯常量枚举/定长数组？——数组为引用类型）
    fn type_is_value(&self, t: &Type) -> bool {
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
            Type::Array(_, _) => false, // 定长数组 = 引用类型（06-02）
            _ => false,
        }
    }

    fn ty_display(&self, t: &Type) -> String {
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
                    format!("{n}({a})")
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

    // ---------- 第二遍：检查 ----------

    fn check_program(&mut self, program: &Program) {
        for d in &program.decls {
            self.check_decl(d);
        }
    }

    fn check_decl(&mut self, d: &Decl) {
        match d {
            Decl::Fn {
                name,
                ret,
                body,
                is_test,
                span,
                params,
                ..
            } => {
                let _ = (name, is_test, span);
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
                        },
                    );
                }
                self.check_block(body, &mut scopes, constraint, ret_ty);
            }
            Decl::Class { name, methods, .. } => {
                for m in methods {
                    let ret_ty = self.ret_stype(&m.ret);
                    let constraint = self.fn_error_constraint(&m.ret);
                    let mut scopes: Vec<HashMap<String, VarInfo>> = Vec::new();
                    // self 参数注入：self: *Self
                    let self_ty = SType::Ptr(Box::new(SType::Named(name.clone(), vec![])), false);
                    scopes.push(HashMap::new());
                    scopes.last_mut().unwrap().insert(
                        "self".into(),
                        VarInfo {
                            ty: Some(self_ty),
                            pending_fields: None,
                        },
                    );
                    // 方法参数（self 显式声明时已含；此处避免重复登记由 check_block 内 params
                    // 处理——方法参数在 body 检查时按 params 登记，见 check_method_params）
                    let _ = constraint;
                    self.check_method_params(m, &mut scopes);
                    self.check_block(&m.body, &mut scopes, constraint, ret_ty);
                }
            }
            Decl::Global {
                name,
                ty,
                init,
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
            _ => {}
        }
    }

    /// 方法体检查前登记显式参数（不含 self——self 已在作用域）
    fn check_method_params(&mut self, m: &Method, scopes: &mut Vec<HashMap<String, VarInfo>>) {
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
                },
            );
        }
    }

    /// 函数返回类型 → 静态类型（供 return 期望类型传播）
    fn ret_stype(&self, ret: &Option<Type>) -> Option<SType> {
        match ret {
            Some(t) => Some(self.ty_of(t)),
            None => Some(SType::Void),
        }
    }

    /// 当前函数返回的错误集约束（Some(集合名)）；None = anyerror/无约束
    fn fn_error_constraint(&self, ret: &Option<Type>) -> Option<String> {
        match ret {
            Some(Type::ErrorUnion(Some(err), _)) => match err.strip() {
                Type::Named(n, _) => Some(n.clone()),
                _ => None,
            },
            Some(Type::ErrorUnion(None, _)) => None, // anyerror：不检查
            _ => None,
        }
    }

    // ---------- 类型解析 ----------

    /// AST Type → 静态类型（大写未登记标识符 → 泛型参数，与运行时启发式一致）
    fn ty_of(&self, t: &Type) -> SType {
        match t.strip() {
            Type::Named(n, args) => {
                match n.as_str() {
                    "i8" => {
                        return SType::Int {
                            width: IntWidth::I8,
                        }
                    }
                    "i16" => {
                        return SType::Int {
                            width: IntWidth::I16,
                        }
                    }
                    "i32" => {
                        return SType::Int {
                            width: IntWidth::I32,
                        }
                    }
                    "i64" => {
                        return SType::Int {
                            width: IntWidth::I64,
                        }
                    }
                    "i128" => {
                        return SType::Int {
                            width: IntWidth::I128,
                        }
                    }
                    "isize" => {
                        return SType::Int {
                            width: IntWidth::ISize,
                        }
                    }
                    "u8" => {
                        return SType::Int {
                            width: IntWidth::U8,
                        }
                    }
                    "u16" => {
                        return SType::Int {
                            width: IntWidth::U16,
                        }
                    }
                    "u32" => {
                        return SType::Int {
                            width: IntWidth::U32,
                        }
                    }
                    "u64" => {
                        return SType::Int {
                            width: IntWidth::U64,
                        }
                    }
                    "u128" => {
                        return SType::Int {
                            width: IntWidth::U128,
                        }
                    }
                    "usize" => {
                        return SType::Int {
                            width: IntWidth::USize,
                        }
                    }
                    "f16" | "f32" | "f64" | "f128" => return SType::Float,
                    "bool" => return SType::Bool,
                    "void" => return SType::Void,
                    "String" => return SType::Str,
                    "Allocator" | "ExitType" => return SType::Named(n.clone(), vec![]),
                    _ => {}
                }
                if is_builtin_type(n) {
                    return SType::Named(n.clone(), args.iter().map(|a| self.ty_of(a)).collect());
                }
                match self.types.get(n) {
                    Some(_) => {
                        SType::Named(n.clone(), args.iter().map(|a| self.ty_of(a)).collect())
                    }
                    None => {
                        // 大写未登记 → 泛型参数（启发式）；小写未登记 → 未知
                        if n.chars().next().map_or(false, |c| c.is_uppercase()) {
                            SType::Generic(n.clone())
                        } else {
                            SType::Unknown
                        }
                    }
                }
            }
            Type::Ptr(inner, mut_) => SType::Ptr(Box::new(self.ty_of(inner)), *mut_),
            Type::Slice(inner, _) => SType::Slice(Box::new(self.ty_of(inner))),
            Type::Optional(inner) => SType::Optional(Box::new(self.ty_of(inner))),
            Type::ErrorUnion(e, inner) => SType::ErrorUnion(
                e.as_ref().map(|x| Box::new(self.ty_of(x))),
                Box::new(self.ty_of(inner)),
            ),
            Type::Tuple(ts) => SType::Tuple(ts.iter().map(|x| self.ty_of(x)).collect()),
            Type::Array(n, inner) => SType::Array(*n, Box::new(self.ty_of(inner))),
            Type::Infer => SType::Infer,
            Type::Owned(inner) => self.ty_of(inner),
        }
    }

    /// 成员访问/索引前自动解引用（评审 A3：p.x、s[i]）
    fn deref_member<'a>(&self, t: &'a SType) -> &'a SType {
        match t {
            SType::Ptr(inner, _) => inner,
            other => other,
        }
    }

    /// 容器/切片/字符串的 `.len` 字段 → usize
    fn len_field_ty(&self, t: &SType) -> Option<SType> {
        match t {
            SType::Slice(_) | SType::Str | SType::Array(_, _) => Some(SType::Int {
                width: IntWidth::USize,
            }),
            SType::Named(n, _) if is_collection(n) => Some(SType::Int {
                width: IntWidth::USize,
            }),
            _ => None,
        }
    }

    /// 类字段类型查询（已解引用）
    fn class_field_ty(&self, t: &SType, field: &str) -> Option<SType> {
        match t {
            SType::Named(cn, _) => {
                if let Some(TypeKind::Class { fields, .. }) = self.types.get(cn).map(|i| &i.kind) {
                    if let Some(fd) = fields.iter().find(|f| f.name == *field) {
                        return Some(self.ty_of(&fd.ty));
                    }
                }
                None
            }
            _ => None,
        }
    }

    // ---------- 语句检查 ----------

    fn check_block(
        &mut self,
        b: &Block,
        scopes: &mut Vec<HashMap<String, VarInfo>>,
        err_constraint: Option<String>,
        ret_ty: Option<SType>,
    ) {
        scopes.push(HashMap::new());
        for stmt in &b.stmts {
            self.check_stmt(stmt, scopes, err_constraint.clone(), ret_ty.clone());
        }
        scopes.pop();
    }

    fn check_stmt(
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
                let init_ty = match init {
                    Some(e) => self.expr_ty(e, scopes, declared.as_ref()),
                    None => None,
                };
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
                let var_ty = declared.or(init_ty);
                scopes.last_mut().unwrap().insert(
                    name.clone(),
                    VarInfo {
                        ty: var_ty,
                        pending_fields: pending,
                    },
                );
            }
            Stmt::ConstDecl { name, init, .. } => {
                let t = self.expr_ty(init, scopes, None);
                scopes.last_mut().unwrap().insert(
                    name.clone(),
                    VarInfo {
                        ty: t,
                        pending_fields: None,
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
                let _ = self.expr_ty(e, scopes, None);
            }
            Stmt::If(ifs) => {
                let ct = self.expr_ty(&ifs.cond, scopes, None);
                self.check_condition(ct.as_ref(), &ifs.cond.span());
                // optional 捕获：if (maybe) |v|
                if let Some((_, n)) = &ifs.capture {
                    let cap_ty = match &ct {
                        Some(SType::Optional(inner)) => Some(inner.as_ref().clone()),
                        _ => None,
                    };
                    scopes.push(HashMap::new());
                    scopes.last_mut().unwrap().insert(
                        n.clone(),
                        VarInfo {
                            ty: cap_ty,
                            pending_fields: None,
                        },
                    );
                    self.check_block(&ifs.then_b, scopes, err_constraint.clone(), ret_ty.clone());
                    scopes.pop();
                } else {
                    self.check_block(&ifs.then_b, scopes, err_constraint.clone(), ret_ty.clone());
                }
                if let Some(else_b) = &ifs.else_b {
                    self.check_stmt(else_b, scopes, err_constraint, ret_ty);
                }
            }
            Stmt::While(w) => {
                let ct = self.expr_ty(&w.cond, scopes, None);
                self.check_condition(ct.as_ref(), &w.cond.span());
                self.check_block(&w.body, scopes, err_constraint, ret_ty);
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
                    },
                );
                self.check_block(&f.body, scopes, err_constraint, ret_ty);
                scopes.pop();
            }
            Stmt::Switch(sw) => {
                let st = self.expr_ty(&sw.subject, scopes, None);
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
                            },
                        );
                    }
                    self.check_block(&arm.body, scopes, err_constraint.clone(), ret_ty.clone());
                    scopes.pop();
                }
            }
            Stmt::Return(e, span) => {
                // M2.6：错误集成员检查——return error.X 必须属于函数返回的错误集
                if let Some(constraint) = &err_constraint {
                    if let Some(Expr::ErrorLit(ename, _)) = e {
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
                // （return error.X = 返回错误值，非 payload——错误集成员检查已单独处理）
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
                        self.check_assignable(&Some(payload.clone()), &et, span, "return");
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
            }
            Stmt::Errdefer(expr, _) => {
                let _ = self.expr_ty(expr, scopes, None);
            }
            _ => {}
        }
    }

    // ---------- 表达式类型推断（M2.2 核心） ----------

    fn expr_ty(
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
            Expr::NamedLit { ty, fields, span } => {
                self.check_named_lit(ty, fields, span, scopes);
                match self.types.get(ty) {
                    Some(_) => SType::Named(ty.clone(), vec![]),
                    None => SType::Unknown,
                }
            }
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
                        args.first().cloned().unwrap_or(SType::Unknown)
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
                            let var_ty = declared.or(init_ty);
                            sc2.last_mut().unwrap().insert(
                                name.clone(),
                                VarInfo {
                                    ty: var_ty,
                                    pending_fields: pending,
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
            Expr::Closure { .. } => SType::Unknown,
        };
        Some(t)
    }

    /// switch 臂体（Block 值）类型
    fn expr_ty_arm(&mut self, b: &Block, scopes: &[HashMap<String, VarInfo>]) -> Option<SType> {
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
    fn check_assignable(
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

    fn check_condition(&mut self, t: Option<&SType>, span: &Span) {
        if let Some(t) = t {
            match t {
                SType::Bool
                | SType::Int { .. }
                | SType::Float
                | SType::Str
                | SType::Ptr(_, _)
                | SType::Optional(_)
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

    fn check_iterable(&mut self, t: Option<&SType>, span: &Span) {
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
    fn check_binary(&mut self, op: BinOp, lt: Option<&SType>, rt: Option<&SType>, span: &Span) {
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
    fn implements(&self, t: &SType, iface: &str) -> bool {
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
    fn check_field_access(&mut self, bt: Option<&SType>, field: &str, span: &Span) {
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

    /// 索引校验：Table 双整数索引；其余单索引
    fn check_index(&mut self, bt: Option<&SType>, indices: &[Expr], span: &Span) {
        match bt {
            Some(SType::Named(n, _)) if n == "Table" => {
                if indices.len() != 2 {
                    self.diags.push(Diagnostic::error(
                        span.clone(),
                        format!(
                            "`Table` requires exactly 2 indices `t[i, j]` (got {})",
                            indices.len()
                        ),
                    ));
                }
            }
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
    fn check_named_lit(
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
        } else {
            // 未登记类型（内建/未知）：放行
            let _ = span;
        }
    }

    // ---------- 调用检查（重载匹配 + where 约束验证） ----------

    fn check_call(
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
                // 类型方法：Vec(i32).init / Table(i32).init / JsonParser.parse
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
    fn builtin_fn_ret(&self, name: &str) -> SType {
        match name {
            "box" => SType::Ptr(Box::new(SType::Unknown), false),
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
            _ => SType::Void,
        }
    }

    /// @ 内建返回类型（子集）
    fn call_at_builtin(
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
            "panic" | "compileError" => Some(SType::Void),
            "addWithOverflow" | "subWithOverflow" | "mulWithOverflow" => {
                Some(SType::Tuple(vec![SType::Unknown, SType::Bool]))
            }
            "sizeOf" | "alignOf" | "offsetOf" => Some(SType::Int {
                width: IntWidth::USize,
            }),
            _ => {
                let _ = (span, scopes);
                None
            }
        }
    }

    /// 重载匹配：参数数量 + 类型兼容（含泛型具体化）+ 具体优先泛型 + 返回类型匹配期望
    fn match_overloads(
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
        // 逐候选匹配（具体优先泛型；同具体度返回类型匹配期望）
        let mut best: Option<(&FnSig, HashMap<String, SType>)> = None;
        let mut best_is_generic = false;
        for s in &pool {
            let mut map: HashMap<String, SType> = HashMap::new();
            let mut ok = true;
            // 实例方法调用：跳过接收者参数（运行时注入）
            let params: Vec<&Param> = if skip_self {
                s.params[1.min(s.params.len())..].iter().collect()
            } else {
                s.params.iter().collect()
            };
            for (p, at) in params.iter().zip(arg_tys.iter()) {
                let pt = self.ty_of(&p.ty);
                let at = at.clone().unwrap_or(SType::Unknown);
                if !self.param_matches(&pt, &at, &mut map) {
                    ok = false;
                    break;
                }
            }
            if !ok {
                continue;
            }
            let is_generic = s.generics.iter().any(|g| map.contains_key(g));
            let replace_best = match &best {
                None => true,
                Some(_) => {
                    if !is_generic && best_is_generic {
                        true
                    } else if is_generic && !best_is_generic {
                        false
                    } else {
                        // 同具体度：返回类型匹配期望
                        let f_ret = self.ret_matches(&s.ret, &map, expected);
                        let b_ret = self.ret_matches(&best.as_ref().unwrap().0.ret, &map, expected);
                        f_ret && !b_ret
                    }
                }
            };
            if replace_best {
                best = Some((s, map.clone()));
                best_is_generic = is_generic;
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
        // 返回类型（具体化泛型）
        let ret_st = sig
            .ret
            .as_ref()
            .map(|r| self.substitute(&self.ty_of(r), &map));
        let _ = args;
        ret_st
    }

    /// 泛型替换：泛型参数 → 具体化类型
    fn substitute(&self, t: &SType, map: &HashMap<String, SType>) -> SType {
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

    /// 泛型具体化的返回类型是否匹配期望（与运行时 ret_matches_expected 对齐）
    fn ret_matches(
        &self,
        ret: &Option<Type>,
        map: &HashMap<String, SType>,
        expected: Option<&SType>,
    ) -> bool {
        let Some(exp) = expected else {
            return false;
        };
        let Some(r) = ret else {
            return false;
        };
        let inner = match r.strip() {
            Type::ErrorUnion(_, inner) => inner.strip(),
            other => other,
        };
        let ret_st = self.substitute(&self.ty_of(inner), map);
        if matches!(ret_st, SType::Unknown) || matches!(exp, SType::Unknown) {
            return false;
        }
        match exp {
            SType::Named(expn, _) => match &ret_st {
                SType::Named(retn, _) => expn == retn,
                _ => false,
            },
            SType::Str => matches!(ret_st, SType::Str),
            SType::Int { .. } | SType::Float => ret_st.numeric(),
            _ => false,
        }
    }

    /// 实参 → 形参兼容（含泛型 T 绑定/统一）
    fn param_matches(&self, pt: &SType, at: &SType, map: &mut HashMap<String, SType>) -> bool {
        match pt {
            SType::Generic(n) => match map.get(n) {
                Some(prev) => self.compatible(prev, at),
                None => {
                    map.insert(n.clone(), at.clone());
                    true
                }
            },
            SType::Unknown | SType::Infer => true,
            SType::Slice(inner) => match at {
                SType::Slice(a) => self.param_matches(inner, a, map),
                SType::Array(_, a) => self.param_matches(inner, a, map),
                // 引用默认切段：&arr → &[T]、&frame(Vec/String) → &[T]
                SType::Ptr(a, _) => match a.as_ref() {
                    SType::Array(_, elem) => self.param_matches(inner, elem, map),
                    SType::Slice(elem) => self.param_matches(inner, elem, map),
                    SType::Named(n, args) if n == "Vec" || n == "String" || n == "Deque" => {
                        // 整体匹配优先（&Vec(i32) 形参）；失败则元素级（&[u8] 形参收 Vec(u8)）
                        self.param_matches(inner, a, map)
                            || self.param_matches(
                                inner,
                                args.first().unwrap_or(&SType::Unknown),
                                map,
                            )
                    }
                    _ => self.param_matches(inner, a, map),
                },
                SType::Str => self.param_matches(
                    inner,
                    &SType::Int {
                        width: IntWidth::U8,
                    },
                    map,
                ),
                // 集合实参可作切片（Vec/String/Deque → 元素视图）
                SType::Named(n, args) if n == "Vec" || n == "String" || n == "Deque" => {
                    self.param_matches(inner, args.first().unwrap_or(&SType::Unknown), map)
                }
                SType::Unknown | SType::Infer | SType::Generic(_) => true,
                _ => false,
            },
            SType::Ptr(inner, _) => match at {
                SType::Ptr(a, _) => self.param_matches(inner, a, map),
                SType::Unknown | SType::Infer | SType::Generic(_) => true,
                // 值实参自动地址（运行时指针参数宽松：pick_fn 对 Ptr 形参放行）
                other => self.param_matches(inner, other, map),
            },
            SType::Optional(inner) => match at {
                SType::Optional(a) => self.param_matches(inner, a, map),
                SType::Unknown | SType::Infer | SType::Generic(_) => true,
                _ => false,
            },
            SType::ErrorUnion(_, inner) => match at {
                SType::ErrorUnion(_, a) => self.param_matches(inner, a, map),
                SType::Unknown | SType::Infer | SType::Generic(_) => true,
                _ => false,
            },
            SType::Tuple(pts) => match at {
                SType::Tuple(ats) => {
                    pts.len() == ats.len()
                        && pts
                            .iter()
                            .zip(ats.iter())
                            .all(|(p, a)| self.param_matches(p, a, map))
                }
                SType::Unknown | SType::Infer | SType::Generic(_) => true,
                _ => false,
            },
            SType::Array(n, inner) => match at {
                SType::Array(m, a) => n == m && self.param_matches(inner, a, map),
                SType::Unknown | SType::Infer | SType::Generic(_) => true,
                _ => false,
            },
            SType::Named(n, args) => match at {
                SType::Named(m, margs) => {
                    n == m
                        && args.len() == margs.len()
                        && args
                            .iter()
                            .zip(margs.iter())
                            .all(|(p, a)| self.param_matches(p, a, map))
                }
                SType::Unknown | SType::Infer | SType::Generic(_) => true,
                _ => false,
            },
            SType::Int { width: _ } => match at {
                SType::Int { .. }
                | SType::Float
                | SType::Unknown
                | SType::Infer
                | SType::Generic(_) => true,
                _ => false,
            },
            SType::Float => match at {
                SType::Int { .. }
                | SType::Float
                | SType::Unknown
                | SType::Infer
                | SType::Generic(_) => true,
                _ => false,
            },
            SType::Bool => matches!(
                at,
                SType::Bool | SType::Unknown | SType::Infer | SType::Generic(_)
            ),
            SType::Str => matches!(
                at,
                SType::Str | SType::Unknown | SType::Infer | SType::Generic(_)
            ),
            SType::Void => matches!(
                at,
                SType::Void | SType::Unknown | SType::Infer | SType::Generic(_)
            ),
        }
    }

    /// 类型双向宽松兼容（赋值/比较/数组元素）
    fn compatible(&self, a: &SType, b: &SType) -> bool {
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

    // ---------- where 约束验证 ----------

    /// 调用点约束验证：具体类型必须实现约束接口（M2.2）
    fn check_constraint(&mut self, g: &str, concrete: &SType, iface: &Type, span: &Span) {
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

    /// 类型是否为引用类型（不可值赋值；连续类型/标量除外）
    fn type_is_ref_st(&self, t: &SType) -> bool {
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

    fn check_int_width_st(&mut self, ty: &SType, text: &str, span: &Span) {
        let width = match ty {
            SType::Int { width } => *width,
            SType::Float => return, // 整数转 float：允许（惰性定型）
            _ => return,
        };
        self.check_int_width_width(width, text, span);
    }

    fn check_int_width_str(&mut self, ty: &str, text: &str, span: &Span) {
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

    fn check_int_width_width(&mut self, width: IntWidth, text: &str, span: &Span) {
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
    fn alloc_init_pending(&self, init: Option<&Expr>) -> Option<std::collections::HashSet<String>> {
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
    fn missing_fields(
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

    fn lookup_var_ty(&self, name: &str, scopes: &[HashMap<String, VarInfo>]) -> Option<SType> {
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
    fn var_exists(&self, name: &str, scopes: &[HashMap<String, VarInfo>]) -> bool {
        scopes.iter().rev().any(|s| s.contains_key(name))
            || self.globals.contains_key(name)
            || is_builtin_ns(name)
    }
}

// ---------- 自由函数 ----------

fn collect_generic_names(t: &Type, out: &mut Vec<String>) {
    match t.strip() {
        Type::Named(n, args) => {
            // 大写未登记标识符 = 泛型参数（与运行时启发式一致）
            if n.chars().next().map_or(false, |c| c.is_uppercase())
                && !is_builtin_type(n)
                && !matches!(
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
                        | "void"
                )
            {
                out.push(n.clone());
            }
            for a in args {
                collect_generic_names(a, out);
            }
        }
        Type::Ptr(inner, _) | Type::Slice(inner, _) | Type::Optional(inner) => {
            collect_generic_names(inner, out);
        }
        Type::ErrorUnion(e, inner) => {
            if let Some(x) = e {
                collect_generic_names(x, out);
            }
            collect_generic_names(inner, out);
        }
        Type::Tuple(ts) => {
            for x in ts {
                collect_generic_names(x, out);
            }
        }
        Type::Array(_, inner) => collect_generic_names(inner, out),
        Type::Owned(inner) => collect_generic_names(inner, out),
        _ => {}
    }
}

fn op_name(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::EucMod => "%%",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => "and",
        BinOp::Or => "or",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::Range => "..",
    }
}

fn width_name(w: IntWidth) -> &'static str {
    match w {
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
}
