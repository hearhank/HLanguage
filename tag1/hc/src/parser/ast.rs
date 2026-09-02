//! AST 类型定义：程序、声明、表达式、语句、类型等抽象语法树节点
//!
//! 定义：枚举：Decl, TestMode, Trait, Type, Stmt, CaptureMode, SwitchPattern, Expr, UnaryOp, BinOp, AssignOp, CatchKind
//! 定义：结构体：Program, Param, FieldDecl, Method, EnumVariant, Block, IfStmt, WhileStmt, ForStmt, SwitchStmt, SwitchArm

use std::collections::HashSet;

use crate::token::Span;

#[derive(Debug, Clone)]
pub struct Program {
    pub decls: Vec<Decl>,
}

#[derive(Debug, Clone)]
pub enum Decl {
    Global {
        name: String,
        ty: Option<Type>,
        init: Option<Expr>,
        pub_: bool,
        span: Span,
    },
    Const {
        name: String,
        ty: Option<Type>,
        init: Expr,
        pub_: bool,
        span: Span,
    },
    Fn {
        name: String,
        /// 泛型参数表（`fn swap<T>(...)`）：显式声明的类型参数名（<T> 尖括号表）
        type_params: Vec<String>,
        params: Vec<Param>,
        ret: Option<Type>,
        /// where 子句（M2.2：泛型约束）：(泛型参数名, 约束接口)
        where_clause: Vec<(String, Type)>,
        body: Block,
        span: Span,
        is_test: bool,
        /// `[test("名称")]` 特性：测试显示名（可省，省略时显示函数名）
        test_name: Option<String>,
        /// D1：`[test(async)]` / `[test(thread)]` 测试模式
        test_mode: TestMode,
        /// D1：`[test(timeout=5)]` 测试超时（秒）
        test_timeout: Option<u64>,
        /// 跨包导出（默认私有；`pub` 管包边界）
        pub_: bool,
        /// 组 E：`async fn`——调用点返回 `Future(R)`（R = 声明返回类型，含错误联合）
        is_async: bool,
        /// K5（ADR-0014）：`export fn`——原生符号级导出（链接器可见干净符号，
        /// 生成外部 thunk；`_start` 导出 = 入口钩子）。与 `pub_` 正交（语言可见性 vs 符号导出）。
        exported: bool,
        /// A1（ADR-0020）：`extern fn`——纯声明（无 body，链接期解析外部 C 符号）。
        /// 语义层应跳过 body 检查，注册为外部符号；解释器拒绝调用；LLVM 生成 `declare`。
        is_extern: bool,
        /// Q8：`[Extension(Type)]`——扩展方法，附着到指定类型上
        extension_of: Option<String>,
    },
    Class {
        name: String,
        ifaces: Vec<Type>,
        traits: Vec<Trait>,
        fields: Vec<FieldDecl>,
        methods: Vec<Method>,
        pub_: bool,
        span: Span,
    },
    Struct {
        name: String,
        traits: Vec<Trait>,
        fields: Vec<FieldDecl>,
        pub_: bool,
        span: Span,
    },
    Enum {
        name: String,
        variants: Vec<EnumVariant>,
        pub_: bool,
        span: Span,
    },
    /// K1（ADR-0014）：无标签 union——字段内存重叠、无判别标签，语义对齐 C 风格 union。
    /// 大小 = 最大字段（对齐 = 最大对齐）；仅标量字段（内存双关工具，引用类型编译错误）。
    Union {
        name: String,
        fields: Vec<FieldDecl>,
        pub_: bool,
        span: Span,
    },
    Interface {
        name: String,
        supers: Vec<Type>,
        methods: Vec<Method>,
        pub_: bool,
        span: Span,
    },
    Namespace {
        name: String,
        decls: Vec<Decl>,
        pub_: bool,
        /// `[module]` 特性标注（2026-08-17）：模块——内容与其它命名空间隔离
        is_module: bool,
        span: Span,
    },
    /// `import` 语句；导入对象 = 模块 `[module]` 标注的命名空间/包）
    ///
    /// 三种形态：
    /// ```hc
    /// import pkg.mod;                    // 整模块导入（绑定名 = 末段 `mod`）
    /// import pkg.mod as m;               // 整模块导入 + 别名（绑定名 = `m`）
    /// import pkg.mod.{a, b as c};        // 符号选择（多符号 + as 重命名）
    /// ```
    Import {
        /// 完整路径（`H.std` / `pkg.mod` / `H.std.net`）
        path: Vec<String>,
        /// 整模块导入别名（`import pkg.mod as m;`）；None = 用路径末段
        alias: Option<String>,
        /// 符号选择：`(原名, 别名)`；None = 整模块导入
        select: Option<Vec<(String, Option<String>)>>,
        span: Span,
    },
    /// E1.2（组 D D2）：`comptime { ... }` 块——编译期求值（装载期受限 Interp，
    /// 结果丢弃、失败 = 编译错误）。仅编译期存在，不产生运行时代码、不替换源码。
    Comptime { body: Block, span: Span },
    /// `.hs` 脚本文件引用：`import "path/to/file.hc"`（B6-2：脚本用文件引用而非命名空间）。
    /// 文件路径解析顺序：SDK 目录 → 当前项目目录。
    Include {
        /// 文件路径（相对或绝对；字符串字面量）
        path: String,
        /// 别名（可选，省略时用文件名）
        alias: Option<String>,
        span: Span,
    },
}

impl Decl {
    /// 跨包导出标志（`script` 无包边界概念，恒 false）
    pub fn is_pub(&self) -> bool {
        match self {
            Decl::Global { pub_, .. }
            | Decl::Const { pub_, .. }
            | Decl::Fn { pub_, .. }
            | Decl::Class { pub_, .. }
            | Decl::Struct { pub_, .. }
            | Decl::Enum { pub_, .. }
            | Decl::Union { pub_, .. }
            | Decl::Interface { pub_, .. }
            | Decl::Namespace { pub_, .. } => *pub_,
            Decl::Import { .. } | Decl::Comptime { .. } | Decl::Include { .. } => false,
        }
    }
}

/// D1 并发测试 runner：测试执行模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TestMode {
    #[default]
    Serial,
    Async,
    Thread,
}

pub enum Trait {
    Pad,
    Align(u32),
    Test {
        name: Option<String>,
        /// D1 测试模式：`[test]` = Serial（默认）、`[test(async)]` = Async、`[test(thread)]` = Thread
        mode: TestMode,
        /// D1 测试超时：`[test(timeout=5)]` = 5 秒（默认 None = 5s）
        timeout: Option<u64>,
    },
    /// `[module]`（2026-08-17 定案）：命名空间 = 模块——内容与其它命名空间隔离
    /// （不参与同包共享命名空间），需要其它库的数据经上下文（init 参数列表）注入
    Module,
    /// Q8：`[Extension(Type)]`——扩展方法，附着到指定类型上（Q15：不能访问私有字段）
    Extension(String),
}

impl std::fmt::Debug for Trait {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Trait::Pad => write!(f, "[pad]"),
            Trait::Align(n) => write!(f, "[align({n})]"),
            Trait::Module => write!(f, "[module]"),
            Trait::Extension(ty) => write!(f, "[Extension({ty})]"),
            Trait::Test {
                name,
                mode,
                timeout,
            } => {
                let mut s = match name {
                    Some(n) => format!("[test({n:?})"),
                    None => format!("[test"),
                };
                if *mode != TestMode::Serial {
                    s.push_str(&format!(", {mode:?}"));
                }
                if let Some(t) = timeout {
                    s.push_str(&format!(", timeout={t}"));
                }
                s.push(']');
                write!(f, "{s}")
            }
        }
    }
}

impl Clone for Trait {
    fn clone(&self) -> Self {
        match self {
            Trait::Pad => Trait::Pad,
            Trait::Align(n) => Trait::Align(*n),
            Trait::Module => Trait::Module,
            Trait::Extension(ty) => Trait::Extension(ty.clone()),
            Trait::Test {
                name,
                mode,
                timeout,
            } => Trait::Test {
                name: name.clone(),
                mode: *mode,
                timeout: *timeout,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    pub default: Option<Expr>,
    pub span: Span,
    pub mut_: bool,
    /// K1/ADR-0036：`owned` 名称前缀（`owned args: *mut Vec<String>`）——
    /// 参数位置拥有标注（与 var 声明的类型前缀 `owned T` 两位置两语法）
    pub owned: bool,
}

#[derive(Debug, Clone)]
pub struct FieldDecl {
    pub name: String,
    pub ty: Type,
    /// 跨包导出（Q3：属性默认私有，`pub` 显式导出）
    pub pub_: bool,
    /// K1/ADR-0036：`owned` 名称前缀（`pub owned x: *mut T`）——字段拥有标注
    pub owned: bool,
    /// 字段级特性（如 `[Align(n)]`）
    pub traits: Vec<Trait>,
    /// 字段默认值（如 `x: i32 = 42`）
    pub default: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Method {
    pub name: String,
    /// 泛型参数表（`fn save<T>(...)`）：显式声明的类型参数名
    pub type_params: Vec<String>,
    pub params: Vec<Param>,
    pub ret: Option<Type>,
    /// where 子句（M2.2：泛型约束）：(泛型参数名, 约束接口)
    pub where_clause: Vec<(String, Type)>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub payload: Option<Type>,
    pub span: Span,
}

// ---------- 类型 ----------

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// 简单命名类型：i32 / String / Point / Vec<i32> / I(T1,T2)
    Named(String, Vec<Type>),
    /// 只读指针 *T / 可写指针 *mut T
    Ptr(Box<Type>, bool),
    /// 切片 &[T] / &mut [T]
    Slice(Box<Type>, bool),
    /// 可选 ?T
    Optional(Box<Type>),
    /// 错误联合 E!T；E 可为 void 表示 anyerror
    ErrorUnion(Option<Box<Type>>, Box<Type>),
    /// 元组 (T1, T2)
    Tuple(Vec<Type>),
    /// 定长数组 [N]T
    Array(usize, Box<Type>),
    /// comptime_int 字面量（组 D：类型实参位置的整数字面量，`ArrayLen(i32, 3)` 的 `3`）。
    /// 编译期任意精度字面量，实例化时按上下文收窄（ADR-0012）；无运行时表示。
    ComptimeInt(usize),
    /// 推断（省略标注）
    Infer,
    /// 所有权标注包装：o T（仅记录形态）
    Owned(Box<Type>),
    /// K1/ADR-0036：可写值形态 `mut T`（类型位置的 mut）——必定拥有。
    /// 权限标注非类型身份：strip() 后与 T 同型（签名比较不区分）
    MutValue(Box<Type>),
}

impl Type {
    pub fn is_infer(&self) -> bool {
        matches!(self, Type::Infer)
    }
    /// 移除所有权形态标注，用于签名比较
    pub fn strip(&self) -> &Type {
        match self {
            Type::Owned(inner) => inner.strip(),
            Type::MutValue(inner) => inner.strip(),
            other => other,
        }
    }
}

// ---------- 语句 ----------

#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    VarDecl {
        name: String,
        mut_: bool,
        ty: Option<Type>,
        init: Option<Expr>,
        span: Span,
    },
    ConstDecl {
        name: String,
        init: Expr,
        span: Span,
    },
    Expr(Expr),
    If(IfStmt),
    While(WhileStmt),
    For(ForStmt),
    Switch(SwitchStmt),
    Return(Option<Expr>, Span),
    Break(Option<String>, Span),
    Continue(Option<String>, Span),
    Defer(Expr, Span),
    Errdefer(Expr, Span),
    Block(Block),
    /// 空语句
    Empty,
}

#[derive(Debug, Clone)]
pub struct IfStmt {
    pub cond: Expr,
    /// optional 捕获：if (maybe) |v| { ... }
    pub capture: Option<(CaptureMode, String)>,
    /// 错误捕获：if (e!T) |v| { ... } else |err| { ... }
    pub err_capture: Option<(CaptureMode, String)>,
    pub then_b: Block,
    pub else_b: Option<Box<Stmt>>, // Block 或 If（else if）
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct WhileStmt {
    /// 循环标签（`:label while`），供 `break :label` / `continue :label` 定位
    pub label: Option<String>,
    pub cond: Expr,
    /// optional 捕获：while (maybe) |v| { ... }——Some 绑定 v 并循环，None 退出
    pub capture: Option<(CaptureMode, String)>,
    pub step: Option<Expr>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ForStmt {
    /// 循环标签（`:label for`），供 `break :label` / `continue :label` 定位
    pub label: Option<String>,
    pub iter: Expr,
    pub capture: CaptureMode,
    pub capture_name: String,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    Read,
    Mut,
    Move,
}

#[derive(Debug, Clone)]
pub struct SwitchStmt {
    pub subject: Expr,
    pub arms: Vec<SwitchArm>,
    pub has_else: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct SwitchArm {
    pub patterns: Vec<SwitchPattern>,
    /// C3：switch 守卫——`pattern if guard => expr`，守卫失败继续下一分支
    pub guard: Option<Expr>,
    pub capture: Option<(CaptureMode, String)>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum SwitchPattern {
    /// error.NotFound
    Error(String),
    /// 标识符 / 枚举变体 / 字面量
    Ident(String),
    Int(String),
    Float(String),
    Str(String),
    Char(u32),
    /// 穷举检查用（tag1：不强制穷举）
    Else,
}

// ---------- 表达式 ----------

#[derive(Debug, Clone)]
pub enum Expr {
    IntLit {
        text: String,
        span: Span,
    },
    FloatLit {
        text: String,
        span: Span,
    },
    StrLit {
        value: String,
        raw: bool,
        span: Span,
    },
    CharLit(u32, Span),
    BoolLit(bool, Span),
    NullLit(Span),
    VoidLit(Span),
    Ident(String, Span),
    /// 数组/元组/struct/枚举字面量统一容器
    ArrayLit(Vec<Expr>, Span),
    TupleLit(Vec<Expr>, Span),
    /// Type<T>[item, ...] — 容器字面量（Vec/Deque 等，ADR-0027）
    ContainerLit {
        ty: String,
        ty_args: Vec<Type>,
        items: Vec<Expr>,
        span: Span,
    },
    /// Type{field = value, ...}（struct/enum 字面量）。
    /// `ty_args` = 泛型实参（`Pair<i32>{...}` 的 `[i32]`；E1.2 组 D comptime 类型应用，
    /// 无泛型 = 空）。类型函数名 + 实参 → 具体化（monomorphization）后登记具体类型。
    NamedLit {
        ty: String,
        ty_args: Vec<Type>,
        fields: Vec<(String, Expr)>,
        span: Span,
    },
    /// struct 类型字面量（E1.2 组 D type-as-value）：`struct { name: Type, ... }`。
    /// 出现在类型函数体内（`fn Pair(T: type) type { return struct { ... }; }`），
    /// 编译期求值 = 具体化后登记为 class。tag1 仅 `name: Type` 形态（值字段不在此）。
    StructType {
        fields: Vec<(String, Type)>,
        span: Span,
    },
    /// 数组类型值（组 D 类型函数）：`[n]T`（`fn ArrayLen(T: type, n: comptime_int) type`）。
    /// `len` = 编译期整数表达式（标识符/字面量）；`elem` = 元素类型值表达式。编译期求值 =
    /// 长度收窄 + 元素替换后具体化为 `Type::Array`。
    ArrayType {
        len: Box<Expr>,
        elem: Box<Expr>,
        span: Span,
    },
    /// Type.name（枚举常量 / 命名空间限定）
    Dot {
        base: Box<Expr>,
        field: String,
        span: Span,
    },
    /// 字段访问 p.x / 方法调用 p.dist(q)
    Field {
        base: Box<Expr>,
        field: String,
        span: Span,
    },
    Index {
        base: Box<Expr>,
        indices: Vec<Expr>,
        span: Span,
    },
    /// p.* 显式解引用
    Deref(Box<Expr>, Span),
    /// &x / &mut x
    AddrOf(Box<Expr>, bool, Span),
    Unary(UnaryOp, Box<Expr>, Span),
    Binary(BinOp, Box<Expr>, Box<Expr>, Span),
    /// x orelse 默认值
    Orelse(Box<Expr>, Box<Expr>, Span),
    /// x.? 断言解包
    Unwrap(Box<Expr>, Span),
    /// try expr
    Try(Box<Expr>, Span),
    /// 组 E：`await expr`——Future(R) 值 → R（协作式 Future，对齐 ADR-0011）
    Await(Box<Expr>, Span),
    /// expr catch 默认 / expr catch |err| { ... }
    Catch(Box<Expr>, Box<CatchKind>, Span),
    /// 调用 f(args)
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    /// if 表达式
    IfExpr {
        cond: Box<Expr>,
        /// optional 捕获：if (maybe) |v| v else 0
        capture: Option<(CaptureMode, String)>,
        then_e: Box<Expr>,
        else_e: Box<Expr>,
        span: Span,
    },
    /// switch 表达式（tag1：解释器统一 switch 为表达式）
    SwitchExpr {
        subject: Box<Expr>,
        arms: Vec<SwitchArm>,
        span: Span,
    },
    /// 块表达式（值 = 最后语句的表达式；tag1 简化）
    Block(Block, Span),
    /// 赋值 a = b / a += b（语句级）
    Assign {
        target: Box<Expr>,
        op: AssignOp,
        value: Box<Expr>,
        span: Span,
    },
    /// 错误字面量 error.NotFound
    ErrorLit(String, Span),
    /// 函数指针/闭包引用（tag1：仅标识符形式，用于函数作为值）
    FnRef(String, Span),
    /// 多值返回/解构 var (a, b) = f()（tag1：以元组处理）
    TupleDestructure(Vec<String>, Box<Expr>, Span),
    /// move x：所有权转移标记（M2.4——调用点显式；原绑定仍可访问，悬垂用户负责）
    Move(Box<Expr>, Span),
    /// 闭包：|v| expr / mut |v| { ... } / move |v| expr（move 捕获）/ |v, w| expr
    Closure {
        params: Vec<String>,
        body: Block,
        is_mut: bool,
        is_move: bool,
        span: Span,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    EucMod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Range, // .. 区间
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
    BitOr,
    BitAnd,
    BitXor,
}

#[derive(Debug, Clone)]
pub enum CatchKind {
    /// catch 默认值
    Default(Box<Expr>),
    /// catch |err| { block }
    Bind { name: String, body: Block },
}

// ---------- 闭包自由变量分析（M2.7 捕获精确化，Phase 8） ----------

/// 闭包自由变量：body 实际引用、且未被体内部署绑定遮蔽的外部变量名。
///
/// 语义依据（对齐 tree-walking 运行时）：
/// - **作用域栈感知**：嵌套块/捕获绑定只在其作用域内遮蔽；外层块对同名的引用
///   仍是自由变量（`{ var x = 1; print(x); } print(x);` → x 自由）。
/// - **流敏感**：`var` 绑定自其语句位置起生效（运行时「声明时绑定」，非提升）——
///   `print(x); var x = 5;` 的 x 为自由变量（引用先于声明）。
/// - **嵌套闭包传递**：内层闭包体对未被遮蔽名的引用归入外层自由集（外层必须捕获
///   才能在内层创建时提供）；内层参数不泄漏到外层。
/// - 函数名（`FnRef`/`Dot` base 类型名/`SwitchPattern::Ident` 枚举变体）非变量，
///   但 `Dot` base 表达式照常访问——多捕获无害（`capture_env` 只对作用域内名字生效）。
pub fn closure_free_vars(params: &[String], body: &Block) -> HashSet<String> {
    let mut fv = HashSet::new();
    // 参数作用域在最底；闭包体块压栈其下
    let mut scopes: Vec<HashSet<String>> = vec![params.iter().cloned().collect()];
    visit_block(body, &mut scopes, &mut fv);
    fv
}

/// 块级访问：新作用域压栈；语句按序访问（绑定自出现位置生效）；退出弹栈。
fn visit_block(b: &Block, scopes: &mut Vec<HashSet<String>>, fv: &mut HashSet<String>) {
    scopes.push(HashSet::new());
    for stmt in &b.stmts {
        visit_stmt(stmt, scopes, fv);
    }
    scopes.pop();
}

/// 带捕获绑定的块访问（`if (m) |v| {...}`、`for |v| in ...`、`catch |err| {...}`、
/// switch 臂捕获）：种子名在块作用域内最先绑定。
fn visit_block_seeded(
    seed: &str,
    b: &Block,
    scopes: &mut Vec<HashSet<String>>,
    fv: &mut HashSet<String>,
) {
    let mut scope = HashSet::new();
    scope.insert(seed.to_string());
    scopes.push(scope);
    for stmt in &b.stmts {
        visit_stmt(stmt, scopes, fv);
    }
    scopes.pop();
}

fn visit_stmt(s: &Stmt, scopes: &mut Vec<HashSet<String>>, fv: &mut HashSet<String>) {
    match s {
        Stmt::VarDecl { name, init, .. } => {
            if let Some(init) = init {
                visit_expr(init, scopes, fv);
            }
            scopes.last_mut().unwrap().insert(name.clone());
        }
        Stmt::ConstDecl { name, init, .. } => {
            visit_expr(init, scopes, fv);
            scopes.last_mut().unwrap().insert(name.clone());
        }
        Stmt::Expr(e) => visit_expr(e, scopes, fv),
        Stmt::If(IfStmt {
            cond,
            capture,
            then_b,
            else_b,
            ..
        }) => {
            visit_expr(cond, scopes, fv);
            match capture {
                Some((_, name)) => visit_block_seeded(name, then_b, scopes, fv),
                None => visit_block(then_b, scopes, fv),
            }
            if let Some(e) = else_b {
                visit_stmt(e, scopes, fv);
            }
        }
        Stmt::While(WhileStmt {
            cond, step, body, ..
        }) => {
            visit_expr(cond, scopes, fv);
            if let Some(st) = step {
                visit_expr(st, scopes, fv);
            }
            visit_block(body, scopes, fv);
        }
        Stmt::For(ForStmt {
            iter,
            capture_name,
            body,
            ..
        }) => {
            visit_expr(iter, scopes, fv);
            let mut scope = HashSet::new();
            scope.insert(capture_name.clone());
            scopes.push(scope);
            for stmt in &body.stmts {
                visit_stmt(stmt, scopes, fv);
            }
            scopes.pop();
        }
        Stmt::Switch(SwitchStmt { subject, arms, .. }) => {
            visit_expr(subject, scopes, fv);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    visit_expr(guard, scopes, fv);
                }
                match &arm.capture {
                    Some((_, name)) => visit_block_seeded(name, &arm.body, scopes, fv),
                    None => visit_block(&arm.body, scopes, fv),
                }
            }
        }
        Stmt::Return(e, _) => {
            if let Some(e) = e {
                visit_expr(e, scopes, fv);
            }
        }
        Stmt::Defer(e, _) | Stmt::Errdefer(e, _) => visit_expr(e, scopes, fv),
        Stmt::Block(b) => visit_block(b, scopes, fv),
        Stmt::Break(..) | Stmt::Continue(..) | Stmt::Empty => {}
    }
}

fn visit_expr(e: &Expr, scopes: &mut Vec<HashSet<String>>, fv: &mut HashSet<String>) {
    match e {
        Expr::Ident(name, _) => {
            if !scopes.iter().any(|s| s.contains(name)) {
                fv.insert(name.clone());
            }
        }
        Expr::ArrayLit(items, _) | Expr::TupleLit(items, _) => {
            for it in items {
                visit_expr(it, scopes, fv);
            }
        }
        Expr::NamedLit { fields, .. } => {
            for (_, v) in fields {
                visit_expr(v, scopes, fv);
            }
        }
        Expr::ContainerLit { items, .. } => {
            for it in items {
                visit_expr(it, scopes, fv);
            }
        }
        // struct 类型字面量：字段为类型标注（无运行时值/自由变量）
        Expr::StructType { .. } => {}
        // 数组类型值 `[n]T`：长度与元素类型值表达式均可能引用 comptime 参数
        Expr::ArrayType { len, elem, .. } => {
            visit_expr(len, scopes, fv);
            visit_expr(elem, scopes, fv);
        }
        Expr::Dot { base, .. } | Expr::Field { base, .. } => visit_expr(base, scopes, fv),
        Expr::Index { base, indices, .. } => {
            visit_expr(base, scopes, fv);
            for i in indices {
                visit_expr(i, scopes, fv);
            }
        }
        Expr::Deref(inner, _) | Expr::AddrOf(inner, _, _) => visit_expr(inner, scopes, fv),
        Expr::Unary(_, inner, _) => visit_expr(inner, scopes, fv),
        Expr::Binary(_, a, b, _) | Expr::Orelse(a, b, _) => {
            visit_expr(a, scopes, fv);
            visit_expr(b, scopes, fv);
        }
        Expr::Unwrap(inner, _)
        | Expr::Try(inner, _)
        | Expr::Move(inner, _)
        | Expr::Await(inner, _) => visit_expr(inner, scopes, fv),
        Expr::Catch(e, kind, _) => {
            visit_expr(e, scopes, fv);
            match kind.as_ref() {
                CatchKind::Default(d) => visit_expr(d, scopes, fv),
                CatchKind::Bind { name, body } => visit_block_seeded(name, body, scopes, fv),
            }
        }
        Expr::Call { callee, args, .. } => {
            visit_expr(callee, scopes, fv);
            for a in args {
                visit_expr(a, scopes, fv);
            }
        }
        Expr::IfExpr {
            cond,
            capture,
            then_e,
            else_e,
            ..
        } => {
            visit_expr(cond, scopes, fv);
            match capture {
                Some((_, name)) => {
                    let mut scope = HashSet::new();
                    scope.insert(name.clone());
                    scopes.push(scope);
                    visit_expr(then_e, scopes, fv);
                    scopes.pop();
                }
                None => visit_expr(then_e, scopes, fv),
            }
            visit_expr(else_e, scopes, fv);
        }
        Expr::SwitchExpr { subject, arms, .. } => {
            visit_expr(subject, scopes, fv);
            for arm in arms {
                match &arm.capture {
                    Some((_, name)) => visit_block_seeded(name, &arm.body, scopes, fv),
                    None => visit_block(&arm.body, scopes, fv),
                }
            }
        }
        Expr::Block(b, _) => visit_block(b, scopes, fv),
        Expr::Assign { target, value, .. } => {
            visit_expr(target, scopes, fv);
            visit_expr(value, scopes, fv);
        }
        Expr::TupleDestructure(names, e, _) => {
            visit_expr(e, scopes, fv);
            scopes.last_mut().unwrap().extend(names.iter().cloned());
        }
        Expr::Closure { params, body, .. } => {
            // 嵌套闭包：参数入栈（不泄漏到外层）；体内引用与当前作用域链解析
            scopes.push(params.iter().cloned().collect());
            visit_block(body, scopes, fv);
            scopes.pop();
        }
        // 无变量引用：字面量 / 错误字面量 / 函数名
        Expr::IntLit { .. }
        | Expr::FloatLit { .. }
        | Expr::StrLit { .. }
        | Expr::CharLit(..)
        | Expr::BoolLit(..)
        | Expr::NullLit(..)
        | Expr::VoidLit(..)
        | Expr::ErrorLit(..)
        | Expr::FnRef(..) => {}
    }
}

/// K2：类型调试输出——返回类型名的简洁字符串表示（供 `hc parse` dump 使用）
pub fn fmt_type_debug(t: &Type) -> String {
    match t {
        Type::Named(name, args) => {
            if args.is_empty() {
                name.to_string()
            } else {
                let args_str: Vec<String> = args.iter().map(fmt_type_debug).collect();
                format!("{}({})", name, args_str.join(", "))
            }
        }
        Type::Ptr(inner, mut_) => {
            if *mut_ {
                format!("*mut {}", fmt_type_debug(inner))
            } else {
                format!("*{}", fmt_type_debug(inner))
            }
        }
        Type::Slice(inner, mut_) => {
            if *mut_ {
                format!("&mut [{}]", fmt_type_debug(inner))
            } else {
                format!("&[{}]", fmt_type_debug(inner))
            }
        }
        Type::Optional(inner) => format!("?{}", fmt_type_debug(inner)),
        Type::ErrorUnion(e, t) => match e {
            Some(e) => format!("{}!{}", fmt_type_debug(e), fmt_type_debug(t)),
            None => format!("!{}", fmt_type_debug(t)),
        },
        Type::Tuple(items) => {
            let items_str: Vec<String> = items.iter().map(fmt_type_debug).collect();
            format!("({})", items_str.join(", "))
        }
        Type::Array(n, inner) => format!("[{}]{}", n, fmt_type_debug(inner)),
        Type::ComptimeInt(n) => format!("comptime_int({})", n),
        Type::Infer => "_infer_".to_string(),
        Type::Owned(inner) => format!("o {}", fmt_type_debug(inner)),
        Type::MutValue(inner) => format!("mut {}", fmt_type_debug(inner)),
    }
}
