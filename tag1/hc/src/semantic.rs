//! 语义检查（M2.2 完整：表达式级类型检查 / 期望类型传播 / 字段与索引校验 /
//! 存储形态验证 / 泛型 where 约束验证）
//!
//! tag1 静态 pass：在解释器 load 之前运行。检查策略：**能精确判定才报错**（准确
//! 可靠——调试友好语言要求不误报）；类型信息不足（Unknown / 泛型未单态化）时
//! 保守放行，交由运行时诊断。

use crate::ast::*;
use crate::diag::Diagnostic;
use crate::token::Span;
use std::collections::{HashMap, HashSet};

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

/// 变量声明类型（静态推断 / definite assignment 跟踪 / 分配来源）
#[derive(Clone)]
struct VarInfo {
    ty: Option<SType>,
    pending_fields: Option<std::collections::HashSet<String>>,
    /// 分配来源（M2.4 所有权：move 唯一约束 = 拥有所有权）
    source: AllocSource,
}

/// 分配来源（M2.4：谁负责销毁 / 可否 move）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AllocSource {
    /// 无所有权（标量/连续类型/指针/切片/字面量值）——禁止 move
    None,
    /// 非 Arena 分配（alloc.init / copy / 集合构造 / 函数返回引用类型）→ 当前作用域负责
    NonArena,
    /// Arena 分配（arena.alloc / arena.init / Arena.init）→ 归 Arena 统一回收——禁止 move
    Arena,
    /// global 声明 → 归根作用域——禁止 move
    Global,
    /// 参数 / 未判定——保守放行
    Unknown,
}

pub fn check(program: &Program) -> Vec<Diagnostic> {
    check_with_extern_deps(program, &[], &[])
}

/// M1.4：跨文件语义检查——外部（兄弟文件）符号并入登记，
/// 使限定名（`Orders.Line`）与 using 导入（`square(5)`）可被准确检查
pub fn check_with_extern(program: &Program, externs: &[&Program]) -> Vec<Diagnostic> {
    check_with_extern_deps(program, externs, &[])
}

/// M7.2：主程序 + 同包兄弟 + 依赖包（包名前缀 + 仅 pub 可见）的联合语义检查
pub fn check_with_extern_deps(
    program: &Program,
    externs: &[&Program],
    deps: &[(&str, &Program)],
) -> Vec<Diagnostic> {
    let mut checker = Checker {
        types: HashMap::new(),
        funcs: HashMap::new(),
        globals: HashMap::new(),
        error_sets: HashMap::new(),
        namespaces: std::collections::HashSet::new(),
        collect_infer_ret: false,
        infer_ret: None,
        infer_ret_conflict: false,
        imported: HashSet::new(),
        diags: Vec::new(),
    };
    // 先收集外部符号（只登记不检查——诊断归属主文件）；兄弟文件按文件私有规则收集
    for ext in externs {
        checker.collect_sibling(ext);
    }
    // 依赖包：包名前缀 + pub 过滤
    for (name, dep) in deps {
        checker.collect_dep(dep, name);
    }
    checker.collect(program);
    checker.apply_usings(program);
    checker.apply_imports(program);
    checker.validate_continuous();
    checker.validate_interface_impls(program);
    checker.check_program(program);
    // M2.6 Q-S8：!T 推断错误集——递归无法收集 → 退化为 anyerror + 提示显式标注（warning）
    let inferred = infer_error_sets(program);
    for name in &inferred.incomplete {
        if let Some(span) = find_fn_span(program, name) {
            checker.diags.push(Diagnostic::warning(
                span,
                format!(
                    "cannot infer error set for `{name}` (recursive): `!T` degrades to \
                     `anyerror`; annotate explicitly with a named error set (`E!T`)"
                ),
            ));
        }
    }
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
    /// M2.3：当前函数未标注返回类型 → 收集 return 表达式类型（多路径统一推断）
    collect_infer_ret: bool,
    /// M2.3：已统一（首个 return 类型）或 None
    infer_ret: Option<SType>,
    /// M2.3：已报过多路径推断冲突（避免重复报错）
    infer_ret_conflict: bool,
    /// ADR-0010：import 登记过的符号（限定名）——整模块导入冲突检测用
    /// （通配/整模块遇同名冲突 → 编译错误；文件自身定义优先，不被导入覆盖）
    imported: HashSet<String>,
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
                | "spawn"
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

/// 递归收集 class 声明（namespace 内展平），供接口实现验证用
fn collect_class_decls<'a>(
    decls: &'a [Decl],
    out: &mut Vec<(&'a str, &'a Vec<Type>, &'a Vec<Method>, &'a Span)>,
) {
    for d in decls {
        match d {
            Decl::Class {
                name,
                ifaces,
                methods,
                span,
                ..
            } => out.push((name.as_str(), ifaces, methods, span)),
            Decl::Namespace { decls, .. } => collect_class_decls(decls, out),
            _ => {}
        }
    }
}

impl Checker {
    // ---------- 第一遍：收集元数据 ----------

    fn collect(&mut self, program: &Program) {
        for d in &program.decls {
            self.collect_decl_prefixed(d, "");
        }
    }

    /// M1.4：兄弟文件声明收集——对齐运行时 `load_siblings` 的文件私有规则：
    /// 类型/全局/错误集扁平+限定双登记（共享）；顶层函数文件私有不登记（避免跨文件
    /// 污染同名重载池，如 25/26 各自 load_config）；命名空间函数只登记限定名（扁平名
    /// 由目标文件 `using NS;` 导入）。
    fn collect_sibling(&mut self, program: &Program) {
        for d in &program.decls {
            self.collect_decl_prefixed_filter(d, "", false, false, true);
        }
    }

    /// M1.4：声明收集（Q21 命名空间）——扁平名 + 限定名双登记
    /// （`square` 供包内直接引用 / using 导入；`Math.square` 供限定访问）
    fn collect_decl_prefixed(&mut self, d: &Decl, prefix: &str) {
        self.collect_decl_prefixed_filter(d, prefix, false, false, false);
    }

    /// M7.2：依赖包收集——包名前缀 + 仅 `pub` + 不登记扁平名
    fn collect_dep(&mut self, program: &Program, prefix: &str) {
        let p = format!("{prefix}.");
        for d in &program.decls {
            self.collect_decl_prefixed_filter(d, &p, true, true, true);
        }
    }

    /// 声明收集核心：`skip_flat` 抑制扁平名（依赖包类型/全局/错误集）；`pub_only` 只登记 `pub` 项（跨包边界）；
    /// `skip_entry` 对齐运行时文件私有规则（函数）：跳过 main 与顶层函数，命名空间函数只登记限定名。
    fn collect_decl_prefixed_filter(
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
                let continuous = traits.iter().any(|t| matches!(t, Trait::Continuous));
                let info = TypeInfo {
                    kind: TypeKind::Class {
                        fields: fields.clone(),
                        ifaces: ifaces.clone(),
                        methods: methods.clone(),
                        traits: traits.clone(),
                    },
                    continuous,
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
                params,
                ret,
                where_clause,
                is_test,
                ..
            } => {
                let sig = self.make_sig(params.clone(), ret.clone(), where_clause.clone());
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
    fn apply_usings(&mut self, program: &Program) {
        for d in &program.decls {
            self.collect_using_decl(d);
        }
    }

    fn collect_using_decl(&mut self, d: &Decl) {
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
    fn apply_imports(&mut self, program: &Program) {
        for d in &program.decls {
            self.collect_import_decl(d);
        }
    }

    fn collect_import_decl(&mut self, d: &Decl) {
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
    fn import_explicit_symbol(&mut self, target_prefix: &str, sym: &str, bound: &str, span: &Span) {
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
    fn import_whole_module(&mut self, target_prefix: &str, bound: &str, span: &Span) {
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

    /// M2.1 接口三用途真实实现（去占位）：验证 class 的 implements 冒号标注实际满足
    /// 接口方法契约（① 标记 class 功能 → 须有对应方法实现；③ 类型参数编译可验证 →
    /// 单态化检查实现：方法名 + 参数/返回签名兼容）。内建接口（ICompare/INumber/
    /// IIterable/Io 等）不在 types 表登记 → 跳过（编译器内建实现）。
    fn validate_interface_impls(&mut self, program: &Program) {
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
    fn check_iface_impl(
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
    fn method_satisfies(&self, iface_m: &Method, class_m: &Method, class_name: &str) -> bool {
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
    fn sig_type_compat(&self, a: &Type, b: &Type, class_name: &str) -> bool {
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
    fn subst_self_ty(&self, t: &Type, class_name: &str) -> Type {
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
    fn method_sig_display(&self, m: &Method) -> String {
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
                            // 参数来源由调用点决定（o T 拥有 / 借用）——保守放行
                            source: AllocSource::Unknown,
                        },
                    );
                }
                // M2.3：未标注返回类型 → 从 return 表达式收集（多路径统一推断）
                self.collect_infer_ret = ret_ty.is_none();
                self.infer_ret = None;
                self.infer_ret_conflict = false;
                self.check_block(body, &mut scopes, constraint, ret_ty);
                self.collect_infer_ret = false;
            }
            Decl::Class { name, methods, .. } => {
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
                    source: AllocSource::Unknown,
                },
            );
        }
    }

    /// 函数返回类型 → 静态类型（供 return 期望类型传播）
    /// 未标注返回类型 → None（M2.3：从 return 表达式推断，多路径一致性检查）
    fn ret_stype(&self, ret: &Option<Type>) -> Option<SType> {
        match ret {
            Some(t) => Some(self.ty_of(t)),
            None => None,
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
                // M2.4：分配来源（move 合法性用）
                let source = self.infer_source(init.as_ref(), init_ty.as_ref());
                let var_ty = declared.or(init_ty);
                scopes.last_mut().unwrap().insert(
                    name.clone(),
                    VarInfo {
                        ty: var_ty,
                        pending_fields: pending,
                        source,
                    },
                );
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
                            source: AllocSource::Unknown,
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
                        source: AllocSource::Unknown,
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
                                source: AllocSource::Unknown,
                            },
                        );
                    }
                    self.check_block(&arm.body, scopes, err_constraint.clone(), ret_ty.clone());
                    scopes.pop();
                }
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
                    if !ret_is_error_union {
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
                            let source = self.infer_source(init.as_ref(), init_ty.as_ref());
                            let var_ty = declared.or(init_ty);
                            sc2.last_mut().unwrap().insert(
                                name.clone(),
                                VarInfo {
                                    ty: var_ty,
                                    pending_fields: pending,
                                    source,
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

    /// M2.3 指针形态：写目标若解引用只读指针（`*T`）→ 编译错误（须 `*mut T`）。
    /// `p.* = v` / `p.field = v` / `p[i] = v` 中基座为只读指针即拦截。
    fn check_ptr_write(&mut self, target: &Expr, scopes: &[HashMap<String, VarInfo>], span: &Span) {
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
            // G3（设计文档 §6）：`box(v, alloc)` 返回拥有/可变指针 `o *mut T`
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
            // G1：`spawn(f, args...) o Thread(T)` 返回线程句柄（协作式延迟执行）。
            // G3 精化：提取 callee 返回类型 T 作泛型实参并加捕获/冻结窗口检查。
            "spawn" => SType::Named("Thread".to_string(), vec![]),
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

    /// 泛型具体化的返回类型与期望类型的匹配分级（M2.3 期望类型传播）：
    /// 0 = 不匹配，1 = 数值/兼容匹配，2 = 精确匹配（精确 > 兼容 > 不匹配）。
    fn ret_rank(
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
    fn sig_same(&self, a: &FnSig, b: &FnSig) -> bool {
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
    fn param_rank(&self, pt: &SType, at: &SType, map: &mut HashMap<String, SType>) -> u8 {
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
                        // 整体匹配优先（&Vec(i32) 形参）；失败则元素级（&[u8] 形参收 Vec(u8)）
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

    /// M2.3：return 类型多路径统一（比 `compatible` 严格——int/float 不互通；
    /// 整数宽度不区分；Unknown/Infer/泛型视为通配）
    fn ret_infer_unifies(&self, a: &SType, b: &SType) -> bool {
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

    /// 变量分配来源（M2.4：move 合法性检查）
    fn lookup_var_source(
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
    fn infer_source(&self, init: Option<&Expr>, init_ty: Option<&SType>) -> AllocSource {
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

// ---------- M2.6 Q-S8：!T 推断错误集收集 ----------

/// !T 推断错误集收集结果（Q-S8）
pub struct InferredErrorSets {
    /// 函数名（含命名空间 / `Type.method` 前缀）→ 推断错误集成员。
    /// 仅覆盖可完整收集的 `!T` 函数；显式 `E!T` 与 `anyerror!T` 不在此列。
    pub sets: HashMap<String, HashSet<String>>,
    /// 无法完整收集（递归自调用）的 `!T` 函数名 → Q-S8 退化为 anyerror（提示显式标注）。
    pub incomplete: Vec<String>,
}

/// 返回类型的错误联合形态（`fn`/方法）
enum FnErrorForm {
    /// E!T：显式命名错误集（const 别名）
    Explicit(String),
    /// !T：推断错误集（从函数体收集）
    Infer,
    /// anyerror!T：接口契约，不静态约束
    Anyerror,
    /// 非错误联合
    None,
}

fn fn_error_form(ret: &Option<Type>) -> FnErrorForm {
    match ret {
        Some(Type::ErrorUnion(Some(err), _)) => match err.strip() {
            Type::Named(n, _) if n == "anyerror" => FnErrorForm::Anyerror,
            Type::Named(n, _) => FnErrorForm::Explicit(n.clone()),
            _ => FnErrorForm::Anyerror,
        },
        Some(Type::ErrorUnion(None, _)) => FnErrorForm::Infer,
        _ => FnErrorForm::None,
    }
}

/// 单个函数体的错误收集：direct = `return error.X`；propagates = `try g()` / `return g()`
struct BodyErrors {
    direct: HashSet<String>,
    propagates: Vec<String>,
}

/// 解析被调名（`g` / `ns.g`）——方法调用（`obj.m()`）无法静态判定类型，跳过
fn callee_name(e: &Expr) -> Option<String> {
    match e {
        Expr::Ident(name, _) => Some(name.clone()),
        Expr::Dot { base, field, .. } => match base.as_ref() {
            Expr::Ident(ns, _) => Some(format!("{ns}.{field}")),
            _ => None,
        },
        _ => None,
    }
}

fn collect_body_errors(b: &Block, out: &mut BodyErrors) {
    for s in &b.stmts {
        collect_stmt_errors(s, out);
    }
}

fn collect_stmt_errors(s: &Stmt, out: &mut BodyErrors) {
    match s {
        Stmt::VarDecl { init, .. } => {
            if let Some(e) = init {
                collect_expr_errors(e, out);
            }
        }
        Stmt::ConstDecl { init, .. } => collect_expr_errors(init, out),
        Stmt::Expr(e) => collect_expr_errors(e, out),
        Stmt::If(ifs) => {
            collect_expr_errors(&ifs.cond, out);
            collect_body_errors(&ifs.then_b, out);
            if let Some(eb) = &ifs.else_b {
                collect_stmt_errors(eb, out);
            }
        }
        Stmt::While(w) => {
            collect_expr_errors(&w.cond, out);
            if let Some(step) = &w.step {
                collect_expr_errors(step, out);
            }
            collect_body_errors(&w.body, out);
        }
        Stmt::For(f) => {
            collect_expr_errors(&f.iter, out);
            collect_body_errors(&f.body, out);
        }
        Stmt::Switch(sw) => {
            collect_expr_errors(&sw.subject, out);
            for arm in &sw.arms {
                collect_body_errors(&arm.body, out);
            }
        }
        Stmt::Return(e, _) => {
            if let Some(e) = e {
                // return g()：g 返回错误联合 → 其错误集随联合直接传递
                if let Expr::Call { callee, .. } = e {
                    if let Some(n) = callee_name(callee) {
                        out.propagates.push(n);
                    }
                }
                collect_expr_errors(e, out);
            }
        }
        Stmt::Defer(e, _) | Stmt::Errdefer(e, _) => collect_expr_errors(e, out),
        Stmt::Block(b) => collect_body_errors(b, out),
        _ => {}
    }
}

fn collect_expr_errors(e: &Expr, out: &mut BodyErrors) {
    match e {
        Expr::ErrorLit(name, _) => {
            out.direct.insert(name.clone());
        }
        Expr::Try(inner, _) => {
            // try g()：g 的错误集传播到当前函数
            if let Expr::Call { callee, .. } = inner.as_ref() {
                if let Some(n) = callee_name(callee) {
                    out.propagates.push(n);
                }
            }
            collect_expr_errors(inner, out);
        }
        Expr::ArrayLit(items, _) | Expr::TupleLit(items, _) => {
            for it in items {
                collect_expr_errors(it, out);
            }
        }
        Expr::NamedLit { fields, .. } => {
            for (_, v) in fields {
                collect_expr_errors(v, out);
            }
        }
        Expr::Dot { base, .. } | Expr::Field { base, .. } | Expr::Deref(base, _) => {
            collect_expr_errors(base, out);
        }
        Expr::Index { base, indices, .. } => {
            collect_expr_errors(base, out);
            for i in indices {
                collect_expr_errors(i, out);
            }
        }
        Expr::AddrOf(inner, _, _)
        | Expr::Unary(_, inner, _)
        | Expr::Unwrap(inner, _)
        | Expr::Orelse(inner, _, _)
        | Expr::Move(inner, _) => {
            collect_expr_errors(inner, out);
        }
        Expr::Binary(_, l, r, _) => {
            collect_expr_errors(l, out);
            collect_expr_errors(r, out);
        }
        Expr::Catch(inner, kind, _) => {
            collect_expr_errors(inner, out);
            match kind.as_ref() {
                CatchKind::Default(d) => collect_expr_errors(d, out),
                CatchKind::Bind { body, .. } => collect_body_errors(body, out),
            }
        }
        Expr::Call { callee, args, .. } => {
            collect_expr_errors(callee, out);
            for a in args {
                collect_expr_errors(a, out);
            }
        }
        Expr::IfExpr {
            cond,
            then_e,
            else_e,
            ..
        } => {
            collect_expr_errors(cond, out);
            collect_expr_errors(then_e, out);
            collect_expr_errors(else_e, out);
        }
        Expr::SwitchExpr { subject, arms, .. } => {
            collect_expr_errors(subject, out);
            for arm in arms {
                collect_body_errors(&arm.body, out);
            }
        }
        Expr::Block(b, _) => collect_body_errors(b, out),
        Expr::Assign { target, value, .. } => {
            collect_expr_errors(target, out);
            collect_expr_errors(value, out);
        }
        Expr::TupleDestructure(_, value, _) => collect_expr_errors(value, out),
        Expr::Closure { body, .. } => collect_body_errors(body, out),
        _ => {}
    }
}

/// 登记全部函数/方法（命名空间前缀展平）与显式错误集 const 别名
fn collect_fn_table<'a>(
    decls: &'a [Decl],
    prefix: &str,
    form: &mut HashMap<String, FnErrorForm>,
    bodies: &mut HashMap<String, &'a Block>,
    explicit: &mut HashMap<String, HashSet<String>>,
) {
    for d in decls {
        match d {
            Decl::Fn {
                name, ret, body, ..
            } => {
                let key = format!("{prefix}{name}");
                form.insert(key.clone(), fn_error_form(ret));
                bodies.insert(key, body);
            }
            Decl::Class { name, methods, .. } => {
                for m in methods {
                    let key = format!("{prefix}{name}.{}", m.name);
                    form.insert(key.clone(), fn_error_form(&m.ret));
                    bodies.insert(key, &m.body);
                }
            }
            Decl::Const { name, ty, .. } => {
                if let Some(Type::Named(tn, _)) = ty {
                    if let Some(rest) = tn.strip_prefix("error_set:") {
                        let members: HashSet<String> =
                            rest.split(',').map(|s| s.trim().to_string()).collect();
                        explicit.insert(format!("{prefix}{name}"), members);
                    }
                }
            }
            Decl::Namespace { name, decls, .. } => {
                collect_fn_table(decls, &format!("{prefix}{name}."), form, bodies, explicit);
            }
            _ => {}
        }
    }
}

/// 递归可达检测：start 经 propagate 边可达自身
fn reaches_self(start: &str, edges: &HashMap<String, Vec<String>>) -> bool {
    fn dfs(
        cur: &str,
        start: &str,
        edges: &HashMap<String, Vec<String>>,
        visited: &mut HashSet<String>,
    ) -> bool {
        if let Some(nexts) = edges.get(cur) {
            for next in nexts {
                if next == start {
                    return true;
                }
                if visited.insert(next.clone()) && dfs(next, start, edges, visited) {
                    return true;
                }
            }
        }
        false
    }
    let mut visited = HashSet::new();
    visited.insert(start.to_string());
    dfs(start, start, edges, &mut visited)
}

/// 定位函数/方法的 span（供诊断定位；命名空间前缀匹配）
fn find_fn_span(program: &Program, key: &str) -> Option<Span> {
    fn walk(decls: &[Decl], prefix: &str, key: &str) -> Option<Span> {
        for d in decls {
            match d {
                Decl::Fn { name, span, .. } => {
                    if format!("{prefix}{name}") == key {
                        return Some(span.clone());
                    }
                }
                Decl::Class { name, methods, .. } => {
                    for m in methods {
                        if format!("{prefix}{name}.{}", m.name) == key {
                            return Some(m.span.clone());
                        }
                    }
                }
                Decl::Namespace { name, decls, .. } => {
                    if let Some(s) = walk(decls, &format!("{prefix}{name}."), key) {
                        return Some(s);
                    }
                }
                _ => {}
            }
        }
        None
    }
    walk(&program.decls, "", key)
}

/// Q-S8：`!T` 推断错误集——从函数体收集 `return error.X` + `try`/`return` 传播的
/// 实际返回集（固定点闭包）。递归自调用无法收集 → 退化为 anyerror（incomplete）。
pub fn infer_error_sets(program: &Program) -> InferredErrorSets {
    let mut form: HashMap<String, FnErrorForm> = HashMap::new();
    let mut bodies: HashMap<String, &Block> = HashMap::new();
    let mut explicit: HashMap<String, HashSet<String>> = HashMap::new();
    collect_fn_table(&program.decls, "", &mut form, &mut bodies, &mut explicit);

    // 每个函数体：直接返回的错误 + 传播的被调名
    let mut direct: HashMap<String, HashSet<String>> = HashMap::new();
    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    for (name, body) in &bodies {
        let mut be = BodyErrors {
            direct: HashSet::new(),
            propagates: Vec::new(),
        };
        collect_body_errors(body, &mut be);
        direct.insert(name.clone(), be.direct);
        edges.insert(name.clone(), be.propagates);
    }

    // 固定点：!T 函数集合 = 直接错误 ∪ 被调已知集（显式 const / 其它 !T）
    let mut sets: HashMap<String, HashSet<String>> = HashMap::new();
    for (name, f) in &form {
        if matches!(f, FnErrorForm::Infer) {
            sets.insert(name.clone(), direct.get(name).cloned().unwrap_or_default());
        }
    }
    for _ in 0..64 {
        let mut changed = false;
        let names: Vec<String> = sets.keys().cloned().collect();
        for f in names {
            let mut new: HashSet<String> = direct.get(&f).cloned().unwrap_or_default();
            if let Some(callees) = edges.get(&f) {
                for g in callees {
                    match form.get(g) {
                        Some(FnErrorForm::Explicit(name)) => {
                            if let Some(m) = explicit.get(name) {
                                new.extend(m.iter().cloned());
                            }
                        }
                        Some(FnErrorForm::Infer) => {
                            if let Some(s) = sets.get(g) {
                                new.extend(s.iter().cloned());
                            }
                        }
                        Some(FnErrorForm::Anyerror) | Some(FnErrorForm::None) | None => {
                            // 接口契约 / 非错误联合 / 内建·外部未知被调：不传播（保守 best-effort）
                        }
                    }
                }
            }
            if new != sets[&f] {
                sets.insert(f, new);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // 递归自调用 → 无法完整收集 → 退化为 anyerror（Q-S8）
    let mut incomplete: Vec<String> = Vec::new();
    for (name, f) in &form {
        if matches!(f, FnErrorForm::Infer) && reaches_self(name, &edges) {
            incomplete.push(name.clone());
            sets.remove(name);
        }
    }
    incomplete.sort();

    InferredErrorSets { sets, incomplete }
}
