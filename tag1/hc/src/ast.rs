//! AST（M1.2 Parser 产物）
//!
//! tag1 垂直切片覆盖核心语法子集。注释中标注「E1/E3」等表示完整功能归后续里程碑。

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
        params: Vec<Param>,
        ret: Option<Type>,
        /// where 子句（M2.2：泛型约束）：(泛型参数名, 约束接口)
        where_clause: Vec<(String, Type)>,
        body: Block,
        span: Span,
        is_test: bool,
        /// `[test("名称")]` 特性：测试显示名（可省，省略时显示函数名）
        test_name: Option<String>,
        /// 跨包导出（默认私有；`pub` 管包边界）
        pub_: bool,
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
    Enum {
        name: String,
        variants: Vec<EnumVariant>,
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
        span: Span,
    },
    Using {
        path: Vec<String>,
        alias: Option<String>,
        span: Span,
    },
    Script {
        body: Block,
        span: Span,
    },
}

impl Decl {
    /// 跨包导出标志（`using`/`script` 无包边界概念，恒 false）
    pub fn is_pub(&self) -> bool {
        match self {
            Decl::Global { pub_, .. }
            | Decl::Const { pub_, .. }
            | Decl::Fn { pub_, .. }
            | Decl::Class { pub_, .. }
            | Decl::Enum { pub_, .. }
            | Decl::Interface { pub_, .. }
            | Decl::Namespace { pub_, .. } => *pub_,
            Decl::Using { .. } | Decl::Script { .. } => false,
        }
    }
}

pub enum Trait {
    Continuous,
    Pad,
    Align(String),
    Test {
        name: Option<String>,
    },
}

impl std::fmt::Debug for Trait {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Trait::Continuous => write!(f, "[continuous]"),
            Trait::Pad => write!(f, "[pad]"),
            Trait::Align(s) => write!(f, "[align({s})]"),
            Trait::Test { name } => match name {
                Some(n) => write!(f, "[test({n:?})]"),
                None => write!(f, "[test]"),
            },
        }
    }
}

impl Clone for Trait {
    fn clone(&self) -> Self {
        match self {
            Trait::Continuous => Trait::Continuous,
            Trait::Pad => Trait::Pad,
            Trait::Align(s) => Trait::Align(s.clone()),
            Trait::Test { name } => Trait::Test {
                name: name.clone(),
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
}

#[derive(Debug, Clone)]
pub struct FieldDecl {
    pub name: String,
    pub ty: Type,
    /// 跨包导出（Q3：属性默认私有，`pub` 显式导出）
    pub pub_: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Method {
    pub name: String,
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
    /// 简单命名类型：i32 / String / Point / Vec(i32) / I(T1,T2)
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
    /// 推断（省略标注）
    Infer,
    /// 所有权标注包装：o T（仅记录形态）
    Owned(Box<Type>),
}

impl Type {
    pub fn is_infer(&self) -> bool {
        matches!(self, Type::Infer)
    }
    /// 移除所有权形态标注，用于签名比较
    pub fn strip(&self) -> &Type {
        match self {
            Type::Owned(inner) => inner.strip(),
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
    pub then_b: Block,
    pub else_b: Option<Box<Stmt>>, // Block 或 If（else if）
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct WhileStmt {
    pub cond: Expr,
    pub step: Option<Expr>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ForStmt {
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
    Char(u8),
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
    CharLit(u8, Span),
    BoolLit(bool, Span),
    NullLit(Span),
    VoidLit(Span),
    Ident(String, Span),
    /// 数组/元组/struct/枚举字面量统一容器
    ArrayLit(Vec<Expr>, Span),
    TupleLit(Vec<Expr>, Span),
    /// Type{field = value, ...}（struct/enum 字面量）
    NamedLit {
        ty: String,
        fields: Vec<(String, Expr)>,
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
