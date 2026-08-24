//! 语义分析模块根：类型检查器主入口、SType 类型系统
//!
//! 定义：枚举：SType, IntWidth, TypeKind, AllocSource
//! 定义：结构体：TypeInfo, FnSig, VarInfo, ThreadState, Checker

mod check;
mod collect;
mod error_infer;
mod infer;
mod resolve;
pub(crate) mod trait_registry;
mod validate;

use crate::ast::*;
use crate::diag::Diagnostic;
use crate::token::Span;
use std::collections::{HashMap, HashSet};

pub use self::error_infer::infer_error_sets;
pub use self::error_infer::InferredErrorSets;

// ---------- 静态类型 ----------

/// 编译期静态类型（表达式类型检查用）
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SType {
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
pub(crate) enum IntWidth {
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
                    format!("{n}<{a}>")
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
    /// Struct：连续内存值类型，字段必须为标量或定长标量数组
    Struct {
        fields: Vec<FieldDecl>,
        traits: Vec<Trait>,
    },
    Enum {
        variants: Vec<EnumVariant>,
    },
    /// K1（ADR-0014）：无标签 union——字段内存重叠、无判别标签。
    /// 语义对齐 C 风格 union：大小 = 最大字段、对齐 = 最大对齐；仅标量字段。
    Union {
        fields: Vec<FieldDecl>,
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
pub(crate) struct FnSig {
    params: Vec<Param>,
    ret: Option<Type>,
    where_clause: Vec<(String, Type)>,
    /// 泛型参数名（where 键 + 参数/返回类型中的泛型标识符）
    generics: Vec<String>,
    /// 组 E E1：`async fn`——调用点返回 `Future(R)`（R = 声明返回类型，含错误联合）
    is_async: bool,
}

/// 变量声明类型（静态推断 / definite assignment 跟踪 / 分配来源）
#[derive(Clone)]
struct VarInfo {
    ty: Option<SType>,
    pending_fields: Option<std::collections::HashSet<String>>,
    /// 分配来源（M2.4 所有权：move 唯一约束 = 拥有所有权）
    source: AllocSource,
    /// 组 G：`spawn(...)` 线程句柄状态（Q18 绑定/逃逸 + Q19 冻结窗口）
    thread: Option<ThreadState>,
}

/// 线程句柄静态跟踪（协作式延迟执行：spawn 立即返回、join/detach/程序结束运行）
#[derive(Clone)]
struct ThreadState {
    /// Q18 绑定：声明作用域线性路径上已 `join()`（可捕引用；冻结窗口闭合）
    bound: bool,
    /// 已 `detach()`（运行点 = 调用处，引用在该作用域内存活 → 允许）
    detached: bool,
    /// 捕获的局部引用（`&local`/`&local.f`/`&local[i]` 根变量名 + 位置）——
    /// 线程逃逸（未 join 作用域退出 → 根回收运行到程序结束）时局部已死 → 悬垂，编译错误
    local_refs: Vec<(String, Span)>,
}

/// 分配来源（M2.4：谁负责销毁 / 可否 move）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AllocSource {
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
        conditional_depth: 0,
        in_comptime_block: false,
        anytype_bodies: HashMap::new(),
        anytype_ret_cache: HashMap::new(),
        anytype_resolving: HashSet::new(),
        extension_of: None,
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
    /// 组 G Q18：if/while/for/switch 条件体内 = 非直线路径（join 不保证执行 →
    /// 不视为绑定；冻结违例仍报）。0 = 直线/块级
    conditional_depth: usize,
    /// 组 D D4：`comptime { }` 块类型检查中——放宽「`return error.X` 需错误联合」
    /// （comptime 块失败机制 = `return error.X`，非函数错误联合语义）
    in_comptime_block: bool,
    /// 组 D D4：anytype 函数体缓存（声明名 → 体）——调用点按实参具体类型解析
    /// `anytype` 返回类型时重求值 return 表达式（ADR-0012 #5）
    anytype_bodies: HashMap<String, Block>,
    /// 组 D D4：anytype 具体化返回类型缓存（(qname, 具体化键) → 返回类型）——
    /// 同签名同实例复用（对齐类型函数惰性缓存）
    anytype_ret_cache: HashMap<(String, String), SType>,
    /// 组 D D4：anytype 具体化解析中守卫（自递归 anytype 函数终止 → 回落 Infer）
    anytype_resolving: HashSet<(String, String)>,
    /// Q15：当前正在检查的扩展方法的目标类型名（None = 普通函数或类方法）
    extension_of: Option<String>,
    diags: Vec<Diagnostic>,
}

/// 内建函数名（test 注入断言 / @ 内建 / 标准库工具）——放行不做重载匹配
pub(crate) fn is_builtin_fn(name: &str) -> bool {
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
pub(crate) fn is_builtin_ns(name: &str) -> bool {
    matches!(
        name,
        "io" | "alloc" | "arena" | "math" | "debug" | "utf8" | "test_io"
    )
}

/// 序列化内建方法（Type.from_bytes/to_json 等，编译器内建契约）——不要求类型登记
pub(crate) fn is_serialize_builtin(field: &str) -> bool {
    matches!(field, "from_json" | "to_json" | "from_bytes" | "to_bytes")
}

/// 内建类型（编译器内建实现；方法放行）
pub(crate) fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        // 组 F（Q32 内建共享特例）：四模式共享容器类型——Pipe/Tee/Funnel/
        // Hub 为内建泛型共享特例（方法取 *Self；不占用唯一写者槽）
        "Pipe"
            | "Tee"
            | "Funnel"
            | "Hub"
            | "String"
            | "Vec"
            | "Map"
            | "Deque"
            | "Table"
            | "Allocator"
            | "Arena"
            | "ExitType"
    )
}

/// 内建集合类型（可迭代 / 引用语义）
pub(crate) fn is_collection(name: &str) -> bool {
    matches!(name, "Vec" | "Map" | "Deque" | "Table" | "String")
}

/// 递归收集 class 声明（namespace 内展平），供接口实现验证用
pub(crate) fn collect_class_decls<'a>(
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

/// 泛型参数名收集（make_sig 用；大写未登记标识符 = 泛型参数）
pub(crate) fn collect_generic_names(t: &Type, out: &mut Vec<String>) {
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

pub(crate) fn op_name(op: BinOp) -> &'static str {
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

pub(crate) fn width_name(w: IntWidth) -> &'static str {
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
