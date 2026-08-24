//! M3.1 共享 IR（唯一语义源，ADR-0004）
//!
//! 线性指令 + 标签形态——字节码 VM（M3.2）与 LLVM 原生后端（M3.3）共用，
//! 禁止各后端私语义。覆盖：标量运算 / 控制流 / 函数调用 / **错误值通道**
//! （M2.6 传播模型：错误是值，`try`/`catch` 降级为错误值检查 + 分支）。
//!
//! 垂直切片范围（tag1）：标量 + bool + 字符串 + 函数/参数/局部变量 +
//! if（语句/表达式/else-if/optional 捕获）+ while（含续步）+ return +
//! error 字面量 + try/catch + orelse + 全局函数调用（含多级限定名）+
//! 断言内建 + **指针**（Phase 1：`&`/`&mut` 取址、`p.*` 解引用、写穿别名）+
//! **聚合**（Phase 2：数组/元组字面量、struct/枚举字面量与常量、字段/索引/切片
//! 读写、`.?` 断言解包、元组解构、`move`）+
//! **switch + range + for**（Phase 3：`MatchTest`/`IrPattern` first-match 线性链、
//! `0..n` 区间糖、`IterMake/IterNext/IterWriteBack` 迭代含 mut 写回、无标签
//! break/continue）。
//! **不做**（硬错误拒绝，不静默丢弃）：defer/errdefer、带标签 break/continue
//! （Phase 6 起）。
//! 复杂库操作 = `CallBuiltin` 原子指令。

use crate::ast::*;
use crate::comptime::{self, Instantiated};
use crate::errorcodes::ErrorCodeTable;
use crate::regex::{parse_regex, RegexMatcher};
use crate::rng::xorshift64;
use crate::token::Span;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

// ---------- 子模块 ----------
mod builtin;
mod json;
mod lower_impl;
mod method;
mod ops;
mod runtime;
mod types;

pub use self::builtin::*;
pub use self::json::*;
pub use self::lower_impl::*;
pub use self::method::*;
pub use self::ops::*;
pub use self::runtime::*;
pub use self::types::*;

// ---------- IR 结构 ----------

#[derive(Debug, Clone, Default)]
pub struct IrModule {
    pub funcs: Vec<IrFunc>,
    /// 闭包函数表（Phase 4）：`IrValue::Closure.func` / `MakeClosure.func` 索引。
    /// 与 funcs 同构（body/params/n_slots），但绝不参与 `func_index` 按名分派。
    pub closures: Vec<IrFunc>,
    /// 函数名（扁平 + 限定）→ 索引表（声明序，支持重载/可选参数多候选；
    /// 对齐 oracle `pick_fn` interp.rs:2665-2796 的候选池）
    pub func_index: HashMap<String, Vec<usize>>,
    /// 全局/常量名（声明序，扁平——namespace 内 global 扁平化对齐 oracle
    /// `exec_decl_top` 无前缀登记；错误集别名除外）。运行时 `IrRuntime::init`
    /// 预分配 cell 后执行全部 `@__init__` 函数（多文件合并 = 多个 init 依次运行）。
    pub globals: Vec<String>,
    /// 错误名 → 码（M2.6 编译期错误码表；运行时内建产生的错误值携带与
    /// `error.X` 字面量一致的码——`value_eq` 按码比较，须同一张表）。
    pub error_codes: HashMap<String, u32>,
    /// 枚举名（扁平 + 全限定）→ 变体名（声明序；Phase 7 `@intFromEnum`/`@enumFromInt`
    /// 运行时分派按序求索引，对齐 oracle `TypeDef::Enum`）。
    pub enum_variants: HashMap<String, Vec<String>>,
    /// [continuous] 类名（扁平 + 全限定）：`DeepCopy` 指令运行时门——仅连续类值
    /// 语义深拷贝，标量/数组/非连续类恒等（引用别名）。由 `TypeTable.classes` 汇集。
    pub continuous: HashSet<String>,
    /// K1 无标签 union（扁平 + 全限定）→ 字段声明（名 + 类型，声明序）：`UnionSync`
    /// 与 `store_field` 写路径字节重解释同步用（ADR-0014）。
    pub unions: HashMap<String, Vec<(String, Type)>>,
}

#[derive(Debug, Clone)]
pub struct IrFunc {
    pub name: String,
    /// 参数槽号（声明序）
    pub params: Vec<usize>,
    /// 参数类型（声明序，重载按实参值类型分派用；与 params 等长）
    pub param_ty: Vec<Type>,
    /// 返回类型（None → void，即 Type::Named("void", vec![])）
    pub ret_ty: Type,
    /// 参数是否有默认值（声明序；可选参数 = 尾部默认，对齐 ADR-0009）
    pub param_defaults: Vec<bool>,
    /// 参数默认常量值（编译期常量默认值；缺失尾参时调用点补齐）
    pub defaults: Vec<Option<IrConst>>,
    /// 槽总数（参数 + 局部变量 + 临时）
    pub n_slots: usize,
    pub body: Vec<IrInst>,
    pub is_test: bool,
    /// K5（ADR-0014）：`export fn`——原生符号级导出（LLVM 外部 thunk 生成依据）
    pub exported: bool,
    /// A1（ADR-0020）：`extern fn`——纯声明（无 body，链接期解析外部 C 符号）。
    /// LLVM 后端生成 `declare` 而非 `define`；解释器拒绝调用。
    pub is_extern: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IrConst {
    Int(i128),
    Float(f64),
    Bool(bool),
    Str(String),
    Void,
    Null,
    /// error.Name（错误值 = 普通值，走值通道；code = M2.6 编译期错误码）
    Err {
        name: String,
        code: u32,
    },
    /// 开区间切片 `arr[a..]` 的上界哨兵（Phase 2）
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    EucMod,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrUnOp {
    Neg,
    Not,
    BitNot,
}

#[derive(Debug, Clone)]
pub enum IrInst {
    /// temp = 常量
    Const {
        temp: usize,
        val: IrConst,
    },
    /// temp = slot
    Load {
        temp: usize,
        slot: usize,
    },
    /// slot = temp
    Store {
        slot: usize,
        temp: usize,
    },
    /// temp = a op b
    Bin {
        op: IrBinOp,
        temp: usize,
        a: usize,
        b: usize,
    },
    /// temp = op a
    Un {
        op: IrUnOp,
        temp: usize,
        a: usize,
    },
    Jump {
        label: usize,
    },
    JumpIf {
        temp: usize,
        label: usize,
    },
    JumpIfNot {
        temp: usize,
        label: usize,
    },
    /// temp 是 null → 跳转（orelse / optional 捕获降级）
    JumpIfNull {
        temp: usize,
        label: usize,
    },
    Label {
        id: usize,
    },
    /// temp = call name(args...)（错误值经值通道返回）
    Call {
        name: String,
        args: Vec<usize>,
        temp: usize,
    },
    /// temp = builtin(args...)（断言 / @ 内建）
    CallBuiltin {
        name: String,
        args: Vec<usize>,
        temp: usize,
    },
    /// temp 是错误值 → 跳转（try/catch 降级）
    JumpIfErr {
        temp: usize,
        label: usize,
    },
    Return {
        temp: usize,
    },
    ReturnVoid,
    /// temp = &slot（变量别名：指向该槽的共享 cell——写穿别名关键装置）
    AddrSlot {
        temp: usize,
        slot: usize,
    },
    /// temp = &expr（非 lvalue 快照：求值到临时槽后复制进新 cell——对齐
    /// tree-walking `AddrOf` 兜底分支 `Value::Ptr(Rc::new(RefCell::new(v)))`）
    AddrValue {
        temp: usize,
        value: usize,
    },
    /// temp = *a（解引用：Ptr → pointee；非 Ptr → 恒等——对齐 `deref_value`）
    Deref {
        temp: usize,
        a: usize,
    },
    /// *target = value（写穿 pointee cell；target 非 Ptr → BadAssign）
    StorePtr {
        target: usize,
        value: usize,
    },
    // ---- Phase 2 聚合 ----
    /// temp = base.field（Class 字段 / Str/Arr/Slice/Map .len 内建字段；无字段 → NoField）
    Field {
        temp: usize,
        base: usize,
        field: String,
    },
    /// base.field = value（写穿 class 字段 cell；base 非 Class → TypeError）
    StoreField {
        base: usize,
        field: String,
        value: usize,
    },
    /// temp = base[index]（Arr/Slice/Str；越界 → IndexOutOfBounds；非整 → BadIndex；非可索引 → NotIndexable）
    Index {
        temp: usize,
        base: usize,
        index: usize,
    },
    /// base[index] = value（写穿元素 cell——切片/别名共享底层；base 非 Arr → TypeError）
    StoreIndex {
        base: usize,
        index: usize,
        value: usize,
    },
    /// temp = base[lo..hi]（Arr → 共享视图；Str → 拷贝字节；Slice → 重切片；hi=End 哨兵 → 到末尾）
    SliceOf {
        temp: usize,
        base: usize,
        lo: usize,
        hi: usize,
    },
    /// base[lo..hi] = value（切片写回：源 Arr 元素逐一复制到目标槽；base 非 Arr 静默无操作）
    StoreSlice {
        base: usize,
        lo: usize,
        hi: usize,
        value: usize,
    },
    /// temp = 数组/元组字面量 [e1, e2, ...]（每元素独立共享 cell）
    MakeArr {
        temp: usize,
        items: Vec<usize>,
    },
    /// temp = struct 字面量 Type{ f1 = v1, ... }
    MakeClass {
        temp: usize,
        ty: String,
        fields: Vec<(String, usize)>,
    },
    /// K1 无标签 union（ADR-0014）：union 字面量构造后，把 `written` 字段字节
    /// 重解释同步其余字段（对齐 interp `union_sync_fields`）。
    UnionSync {
        class: usize,
        written: String,
    },
    /// temp = 枚举值（Type.variant 常量 或 Type{variant = payload}）
    MakeEnum {
        temp: usize,
        name: String,
        variant: String,
        payload: Option<usize>,
    },
    /// 元组解构 `var (a, b) = e`：源须为 Arr 且元素数与 slots 一致（_ 跳过）；
    /// 逐元素克隆绑定。slots = (槽号 or None=_)
    Destructure {
        value: usize,
        slots: Vec<Option<usize>>,
    },
    /// temp = move a（所有权转移标记；运行时恒等——对齐 tree-walking M2.4）
    Move {
        temp: usize,
        a: usize,
    },
    /// temp = a.?（Opt(Some) → 内值；Opt(None) → NullUnwrap；非 Opt → 恒等）
    Unwrap {
        temp: usize,
        a: usize,
    },
    // ---- Phase 3：switch / 区间 / for ----
    /// temp = 模式匹配（对齐 oracle `match_pattern`：subject 先 deref 一次）
    MatchTest {
        temp: usize,
        subject: usize,
        pattern: IrPattern,
    },
    /// temp = [lo, hi) 整数区间数组（对齐 oracle `BinOp::Range`；lo/hi 须为 Int，否则 TypeError）
    MakeRange {
        temp: usize,
        lo: usize,
        hi: usize,
    },
    /// temp = 枚举负载（subject 为 `Enum{payload:Some(p)}` → p；否则 → subject 本身）。
    /// switch 臂捕获专用（对齐 oracle `exec_switch_arm` 的负载捕获分支）。
    EnumPayload {
        temp: usize,
        a: usize,
    },
    /// temp = 迭代器（`iter_items` 语义：Arr/Slice 共享元素 cell `is_ref=true`；
    /// Map→KV 新 cell；Str→字节 Int；用户 IIterable→`next()` 至 Opt(None)）
    IterMake {
        temp: usize,
        base: usize,
    },
    /// 取下一项并绑定捕获槽：`has` = 是否还有下一项；有则
    /// `read_only`（Read 捕获）→ 槽 cell 置为「该项值副本」；
    /// 否则（Mut/Move 捕获）→ 槽 cell 绑定为「共享源 cell」（写穿；LLVM 侧为拷贝进出）。
    /// 迭代器内部记录「当前项」供 [`IrInst::IterWriteBack`] 写回。
    IterNext {
        has: usize,
        iter: usize,
        slot: usize,
        read_only: bool,
    },
    /// 把捕获槽的 cell 内容写回迭代器「当前项」的源 cell（Mut/Move 捕获循环体末尾发射；
    /// run_ir 因槽 cell 即源 cell 而为无操作；LLVM 侧为拷贝进出写回）。
    IterWriteBack {
        iter: usize,
        slot: usize,
    },
    // ---- Phase 4 闭包 / 函数引用 / 方法 / 动态调用 ----
    /// temp = 闭包值（Phase 8 起只捕获**自由变量**——body 实际引用且未被体内绑定
    /// 遮蔽的名字，与 oracle `closure_free_vars` + `capture_env` 对齐；
    /// `is_move` → 深拷贝独立 cell；`is_mut` → 闭包内可重绑定捕获槽，否则只读）
    MakeClosure {
        temp: usize,
        /// 索引 [`IrModule::closures`]（与 captures 长度一致的闭包函数）
        func: usize,
        /// (变量名, 封闭帧槽号)：闭包函数的前导捕获参数与之逐位对齐
        captures: Vec<(String, usize)>,
        is_move: bool,
        is_mut: bool,
    },
    /// temp = 调用 callee（`Fn` 名 → 按名分派；`Closure` → 绑定捕获 cell + 显式参数）
    CallIndirect {
        temp: usize,
        callee: usize,
        args: Vec<usize>,
    },
    /// temp = base.method(args...)（运行时按 base 实际类型名分派 `{Type}.{method}` +
    /// self 注入首参；对齐 oracle eval_call Field 臂 interp.rs:2350-2421）
    CallMethod {
        temp: usize,
        base: usize,
        method: String,
        args: Vec<usize>,
    },
    /// temp = 函数引用（name → `Fn(name)`；未注册 → 运行时 UndefinedName）
    FnRef {
        temp: usize,
        name: String,
    },
    // ---- Phase 5：global / const ----
    /// temp = 全局变量值（运行时按名查 [`Ctx::globals`] cell；未初始化 → NoGlobal）
    LoadGlobal {
        temp: usize,
        name: String,
    },
    /// global = value（写穿全局 cell；对齐 oracle `lookup` → `Rc<RefCell>` 写回）
    StoreGlobal {
        name: String,
        value: usize,
    },
    /// temp = 全局变量 cell 指针（`&global`/`&mut global`；`Ptr(cell)` 与局部
    /// `AddrSlot` 同构——写穿经 `Deref`/`StorePtr` 回全局。对齐 oracle `AddrOf(Ident)`
    /// 对全局名走 `lookup` → `Value::Ptr(global_cell)`）
    GlobalAddr {
        temp: usize,
        name: String,
    },
    // ---- Phase 6：defer / errdefer ----
    /// 登记 defer（运行时活跃计数 +1；`id` 为该 defer 语句的编译期唯一编号）。
    /// 在 defer 语句处发射；退出点用守卫（JumpIfNotDefer）+ 内联体 + PopDefer 排空。
    /// 对齐 oracle `exec_stmt` 的 `Stmt::Defer`（`interp.rs`）——defer 求值推迟到作用域退出。
    PushDefer {
        id: usize,
    },
    /// 该 defer 未登记于当前动态路径（活跃计数为 0）→ 跳过内联体（分支/已运行路径）。
    /// 运行时 LIFO 顺序由发射顺序（编译期）保证，计数仅做「是否待运行」判定。
    JumpIfNotDefer {
        id: usize,
        label: usize,
    },
    /// 排空该 defer（活跃计数 -1）。正常路径上 errdefer 由裸 PopDefer 清理（不运行）；
    /// 运行后紧随 PopDefer 同步移除。计数减法（非栈顶弹出）天然支持 errdefer 穿插。
    PopDefer {
        id: usize,
    },
    // ---- P11d [continuous] 值语义 ----
    /// temp = deep_copy(a)（[continuous] 连续类赋值即复制：`var p2: Point = p`
    /// 复制独立副本而非共享 cell 别名）。运行时仅当 a 为连续类（类名 ∈
    /// [`IrModule::continuous`]）才深拷贝，否则恒等（标量/数组/非连续类 = 引用别名，
    /// 与 tree-walking 一致——数组 var 复制仍共享底层）。对齐 oracle VarDecl
    /// `interp.rs:926-949` + `deep_copy`。
    DeepCopy {
        temp: usize,
        a: usize,
    },
}

/// switch 模式（对齐 AST [`crate::ast::SwitchPattern`]；`Else` 不发射 MatchTest——
/// 在 lower 阶段识别为兜底臂，其余模式全部失败后落入）。
#[derive(Debug, Clone)]
pub enum IrPattern {
    /// error.NotFound → 主题为 `Err{name}` 且 name 相等
    Error(String),
    /// 标识符 / 枚举变体 / true/false / null
    Ident(String),
    Int(i128),
    Float(f64),
    Str(String),
    Char(u8),
}

// ---------- 类型元数据（Phase 2：class/enum/namespace 判型） ----------

#[derive(Debug, Default, Clone)]
pub struct TypeTable {
    /// class 名（扁平 + 全限定）→ 元数据
    pub classes: HashMap<String, ClassInfo>,
    /// enum 名（扁平 + 全限定）→ 变体集
    pub enums: HashMap<String, EnumInfo>,
    /// K1 无标签 union 名（扁平 + 全限定）→ 字段声明（ADR-0014）
    pub unions: HashMap<String, UnionInfo>,
    /// namespace 名（扁平 + 全限定）
    pub namespaces: std::collections::HashSet<String>,
}

/// K1 无标签 union（ADR-0014）：字段（名 + 标量类型，声明序）——字段内存重叠，
/// size = 最大字段宽度；`@union` 标记 + 写字段字节重解释同步其余字段。
#[derive(Debug, Default, Clone)]
pub struct UnionInfo {
    pub fields: Vec<(String, Type)>,
}

#[derive(Debug, Default, Clone)]
pub struct ClassInfo {
    /// 字段（名 + 类型，声明序）——`alloc.init(T)` 默认字段构造用（对齐 oracle
    /// `default_value`：无参构造 = 类型空实例，字段逐默认值）。
    pub fields: Vec<(String, Type)>,
    pub methods: Vec<String>,
    /// [continuous] 连续内存值类型（H1 特性标注）：赋值即复制（值语义），非别名。
    /// 驱动 `Stmt::VarDecl` 降级发射 `DeepCopy`（对齐 oracle `type_is_continuous`）。
    pub continuous: bool,
}

#[derive(Debug, Default, Clone)]
pub struct EnumInfo {
    /// 变体名（声明序——`@intFromEnum`/`@enumFromInt` 运行时分派按序求索引）
    pub variants: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum IrValue {
    Int(i128),
    Float(f64),
    Bool(bool),
    Str(Vec<u8>),
    /// 可选值（`null` = `Opt(None)`，对齐 tree-walking `Value::Opt`）
    Opt(Option<Box<IrValue>>),
    /// 错误值（M4.2：码 + 名字；码 = M2.6 编译期错误码表，全局唯一）
    Err {
        name: String,
        code: u32,
    },
    /// 指针：共享堆 cell 索引（别名装置——对齐 tree-walking `Value::Ptr(Rc<RefCell>)`）
    Ptr(usize),
    /// 装箱/接口胖指针（G3：三字宽 data + vtbl + alloc；指向 `Cell::Boxed`）
    Boxed(usize),
    /// 数组：`Cell::Elems` 的 cell 索引（元素为共享 cell——切片/写索引别名）
    Arr(usize),
    /// 集合 Vec（G4：持分配器引用的集合；指向 `Cell::Vec`。deref peel 到 Arr）
    Vec(usize),
    /// 集合 Map（G4：持分配器引用的 Map；指向 `Cell::Map`）
    Map(usize),
    /// 切片视图：共享底层 `Cell::Elems` + 窗口；`data` 为数组 cell 索引
    Slice {
        data: usize,
        start: usize,
        len: usize,
    },
    /// 类实例：`Cell::Class` 的 cell 索引（字段为普通值——无字段级别名）
    Class(usize),
    /// Arena 分配器句柄（G1：真实 bump + 块链表；指向 `Cell::Arena`）
    Arena(usize),
    /// 互斥锁（E4：真 OS 并行——Mutex.init(v) 构造，.lock()/.try_lock() 访问）
    Mutex(Arc<std::sync::Mutex<IrValue>>),
    /// 枚举值（`Type.variant` 常量 或 `Type{variant = payload}`）
    Enum {
        name: String,
        variant: String,
        payload: Option<Box<IrValue>>,
    },
    /// 开区间切片 `arr[a..]` 的上界哨兵
    End,
    /// 迭代器值（Phase 3）：指向 `Cell::Iter` 的 cell 索引
    Iter(usize),
    /// 函数引用（Phase 4）：名字在调用点经 `pick_func` 按 arity/类型分派
    Fn(String),
    /// 闭包值（Phase 4）：func = [`IrModule::closures`] 索引；
    /// captures = 捕获变量 cell 索引（共享读/mut → 原 cell；move → 深拷贝新 cell）。
    /// 别名语义：闭包帧捕获参数槽直接绑定 captures[i] cell → 写穿对齐 oracle Rc<RefCell>。
    Closure {
        func: usize,
        captures: Vec<usize>,
        is_mut: bool,
    },
    Void,
}

impl PartialEq for IrValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (IrValue::Int(a), IrValue::Int(b)) => a == b,
            (IrValue::Float(a), IrValue::Float(b)) => a == b,
            (IrValue::Bool(a), IrValue::Bool(b)) => a == b,
            (IrValue::Str(a), IrValue::Str(b)) => a == b,
            (IrValue::Opt(a), IrValue::Opt(b)) => a == b,
            (IrValue::Err { name: an, code: ac }, IrValue::Err { name: bn, code: bc }) => {
                an == bn && ac == bc
            }
            (IrValue::Ptr(a), IrValue::Ptr(b)) => a == b,
            (IrValue::Boxed(a), IrValue::Boxed(b)) => a == b,
            (IrValue::Arr(a), IrValue::Arr(b)) => a == b,
            (IrValue::Vec(a), IrValue::Vec(b)) => a == b,
            (IrValue::Map(a), IrValue::Map(b)) => a == b,
            (
                IrValue::Slice {
                    data: da,
                    start: sa,
                    len: la,
                },
                IrValue::Slice {
                    data: db,
                    start: sb,
                    len: lb,
                },
            ) => da == db && sa == sb && la == lb,
            (IrValue::Class(a), IrValue::Class(b)) => a == b,
            (IrValue::Arena(a), IrValue::Arena(b)) => a == b,
            (IrValue::Mutex(a), IrValue::Mutex(b)) => Arc::ptr_eq(a, b),
            (
                IrValue::Enum {
                    name: an,
                    variant: av,
                    payload: ap,
                },
                IrValue::Enum {
                    name: bn,
                    variant: bv,
                    payload: bp,
                },
            ) => an == bn && av == bv && ap == bp,
            (IrValue::End, IrValue::End) => true,
            (IrValue::Iter(a), IrValue::Iter(b)) => a == b,
            (IrValue::Fn(a), IrValue::Fn(b)) => a == b,
            (
                IrValue::Closure {
                    func: af,
                    captures: ac,
                    is_mut: am,
                },
                IrValue::Closure {
                    func: bf,
                    captures: bc,
                    is_mut: bm,
                },
            ) => af == bf && ac == bc && am == bm,
            (IrValue::Void, IrValue::Void) => true,
            _ => false,
        }
    }
}

// ---------- 堆/单元模型（Phase 1：别名与 tree-walking `Rc<RefCell<Value>>` 对齐） ----------

/// 堆单元（cell）：槽持有的共享可变数据。槽 → cell 索引（[`Frame`]），
/// 指针 = `IrValue::Ptr(cell)`——多槽/多指针可共享同一 cell，写穿即别名。
#[derive(Debug, Clone)]
pub enum Cell {
    /// 普通值单元
    Value(IrValue),
    /// 数组底层（Phase 2）：元素 cell 索引（共享——切片/写索引/别名共用底层）
    Elems(Vec<usize>),
    /// 类实例（Phase 2）：类型名 + 字段 → 字段 cell 索引（字段为普通值，无别名）
    Class {
        name: String,
        fields: HashMap<String, usize>,
    },
    /// 迭代器（Phase 3）：`iter_items` 展开结果 + 前进游标。
    /// `items[i].cell` 为第 i 项的共享源 cell（Arr/Slice）或新 cell（Map/Str/用户迭代）；
    /// `is_ref` 表示是否与源容器共享（Mut/Move 捕获可写穿）。
    Iter { items: Vec<IterItem>, next: usize },
    /// Arena 分配器状态（G1：真实 bump + 块链表；deinit 批量归还 backing）
    Arena(ArenaStateIr),
    /// 装箱/接口胖指针（G3：data + vtbl + alloc 三字宽；对齐 tree-walking `BoxedData`）。
    /// data = pointee 的 cell 索引（`Cell::Value`）；vtbl = 具体类型名（tag1 静态标注）；
    /// alloc = 分配器引用（全局 alloc 或 Arena 句柄）。
    Boxed {
        data: usize,
        vtbl: String,
        alloc: IrValue,
    },
    /// 集合 Vec（G4：`arr` 恒为 `IrValue::Arr(items_cell)`——deref peel 共享底层
    /// `Cell::Elems`；`alloc` = 构造 `init(alloc)` 时携带的分配器引用）
    Vec { arr: IrValue, alloc: IrValue },
    /// 集合 Map（G4：键 → 字段 cell 索引；`alloc` = 构造时携带的分配器引用）
    Map {
        fields: HashMap<String, usize>,
        alloc: IrValue,
    },
}

/// Arena 默认块大小（IR 侧；对齐 tree-walking `value::ARENA_BLOCK_SIZE`）
const ARENA_BLOCK_SIZE_IR: usize = 1024;

/// 分配器对齐下限（G5/§2.3：H 值为 i128/f64 承载，对齐 ≥ 16；对齐 tree-walking `ALLOC_ALIGN`）
const ALLOC_ALIGN_IR: usize = 16;

/// 对齐到 `a` 倍数（向上圆整）
fn align_up_ir(x: usize, a: usize) -> usize {
    (x + a - 1) & !(a - 1)
}

/// Arena 分配器状态（G1：真实 bump + 块链表）
#[derive(Debug, Clone, Default)]
pub struct ArenaStateIr {
    /// 已提交块（真实 backing 内存；bump 从当前块切，不足时申请新块）
    pub blocks: Vec<Vec<u8>>,
    /// 当前块内游标（下一分配起点）
    pub cursor: usize,
    /// 累计分配字节（统计；`arena.bytes()`）
    pub total: usize,
    /// 可用标志（`deinit` 后 false → `alloc` 抛 `ArenaDeinitialized`）
    pub live: bool,
}

impl ArenaStateIr {
    pub fn new() -> Self {
        Self {
            blocks: vec![],
            cursor: 0,
            total: 0,
            live: true,
        }
    }

    /// bump 分配 `n` 字节零初始化内存；不足时申请新块（大小 = `max(ARENA_BLOCK_SIZE_IR, n)`）。
    /// 返回（块索引, 块内偏移）；失败（deinit / OOM）返回 Err。
    ///
    /// **对齐（G5/§2.3）**：切出前把游标圆整到 `ALLOC_ALIGN_IR`（16）的倍数，保证
    /// 返回区域起始相对块起点 16 对齐；对齐填充计入 `total`（对齐 tree-walking `bump`）。
    fn bump(&mut self, n: usize) -> Result<(usize, usize), ArenaAllocErrIr> {
        if !self.live {
            return Err(ArenaAllocErrIr::Deinit);
        }
        let aligned = align_up_ir(self.cursor, ALLOC_ALIGN_IR);
        let need_new = self.blocks.is_empty() || aligned + n > self.blocks.last().unwrap().len();
        if need_new {
            let size = n.max(ARENA_BLOCK_SIZE_IR);
            let mut block = Vec::new();
            // 优雅失败（`vec![0u8; size]` 对超大 size 会中止进程）
            block
                .try_reserve_exact(size)
                .map_err(|_| ArenaAllocErrIr::Oom)?;
            block.resize(size, 0u8);
            self.blocks.push(block);
            self.cursor = 0;
        }
        let idx = self.blocks.len() - 1;
        let off = align_up_ir(self.cursor, ALLOC_ALIGN_IR);
        self.total += off + n - self.cursor;
        self.cursor = off + n;
        Ok((idx, off))
    }

    /// deinit：清空全部块（归还 backing）、重置统计、标记不可用
    fn deinit(&mut self) {
        self.blocks.clear();
        self.cursor = 0;
        self.total = 0;
        self.live = false;
    }
}

/// bump 分配失败原因（调用方映射为 IR 错误 / `error.OutOfMemory`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArenaAllocErrIr {
    Deinit,
    Oom,
}

/// 迭代项：共享源 cell（或新 cell）+ 是否源容器引用（对齐 oracle `iter_items` 的 `(cell, is_ref)`）。
#[derive(Debug, Clone)]
pub struct IterItem {
    pub cell: usize,
    pub is_ref: bool,
}

/// 线程运行结果（跨线程传递，IR 版本）
#[derive(Debug)]
pub(crate) enum ThreadResultIr {
    Ok(IrValue),
    Err(IrError),
}

/// OS 线程控制块（IR 版本）
#[derive(Debug)]
pub(crate) struct ThreadStateIr {
    join_handle: Option<thread::JoinHandle<()>>,
    result: Arc<Mutex<Option<ThreadResultIr>>>,
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
}

/// 通道状态（IR 版本，E4：四模式容器真并行）
#[derive(Debug)]
pub(crate) enum ChannelStateIr {
    Pipe {
        sender: std::sync::mpsc::Sender<IrValue>,
        receiver: std::sync::mpsc::Receiver<IrValue>,
    },
}

/// 运行时堆：跨帧共享的 cell 池（指针可跨帧存活——如传入函数后写穿调用方槽）。
#[derive(Debug, Default)]
pub struct Ctx {
    pub cells: Vec<Cell>,
    /// 全局/常量名 → cell 索引（Phase 5）：cell 由 [`IrRuntime::init`] 预分配，
    /// `@__init__`（`StoreGlobal`）写入初值；`LoadGlobal`/`StoreGlobal` 读写写穿。
    pub globals: HashMap<String, usize>,
    /// io.print/printErr 输出缓冲（Phase 7）：`execute_ir` 运行后冲刷到 stdout。
    pub out: Vec<u8>,
    /// 程序参数（io.args()；由 `hc run`/`hc test` 注入，对齐 oracle `Interp.args`）
    pub args: Vec<Vec<u8>>,
    /// io.exit 请求的退出码（F2：对齐 oracle `Interp.exit_code`；`execute_ir` 遇
    /// ExitRequested 时读取并映射进程退出码）
    pub exit_code: Option<u8>,
    /// io.fs 真实文件句柄表（Phase 7）：File 值 = `Class{_fd}`，fd 索引本表。
    pub files: HashMap<i64, std::fs::File>,
    /// 下一文件描述符（自增分配）
    pub next_fd: i64,
    /// io.net TCP 连接表（fd → TcpStream）
    pub tcp_streams: HashMap<i64, std::net::TcpStream>,
    /// io.net TCP 监听器表（fd → TcpListener）
    pub tcp_listeners: HashMap<i64, std::net::TcpListener>,
    /// 下一网络描述符（自增分配）
    pub next_net_fd: i64,
    /// G5/§8.3 Debug 泄漏检测：全局 alloc 分配记录表（(size, line)；IR 无行号 → line 0）。
    /// IR 值无引用计数，分配登记不自动注销——`leaks()`/`leak_report()` 反映本 run 内
    /// 已分配数（对齐 oracle 语义的 Debug 簿记可观测面；tree-walking 侧用弱引用精确跟踪）。
    pub alloc_tracker: Vec<(usize, u32)>,
    /// 组 G（Q8）：当前线程子任务的每线程 alloc 覆盖。协作式单线程执行下，线程 fn
    /// 运行期间置 Some(每线程 Arena)，`alloc` 解析（LoadGlobal / implicit_env_value）
    /// 优先返回该值——对齐 oracle `Interp` 的 `push_scope` + `bind("alloc", 每线程 arena)`。
    pub current_alloc: Option<IrValue>,
    /// 组 G：当前执行深度（`exec_body` 每次进入时刷新）。线程 fn 以 `cur_depth + 1`
    /// 起步，对齐 oracle 共享 `call_depth` 的 StackOverflow 防护（非独立栈）。
    pub cur_depth: usize,
    /// G1（E3.1）：UDP socket 注册表（UdpSocket 值持 fd；对齐 oracle udp_sockets）
    pub udp_sockets: HashMap<i64, std::net::UdpSocket>,
    /// G2（io 差异项）：Dir 句柄注册表（fd → 目录路径；Dir 值持 `_fd`，list_dir 按路径重读）
    pub dirs: HashMap<i64, String>,
    /// G3（E3.2 ipc）：管道注册表（pid → 共享缓冲 + 写端开标志；PipeReader/PipeWriter
    /// 共享同一 pid，协作式模型下读写均不阻塞）
    pub pipes: HashMap<i64, PipeIr>,
    /// G3（E3.2 ipc）：共享内存注册表（id → 定长字节区；Shm 值持 `shm` id）
    pub shms: HashMap<i64, Vec<u8>>,
    /// 下一管道/共享内存/目录/存储描述符（对齐 oracle 计数器从 1 起步）
    pub next_pipe_fd: i64,
    pub next_shm_fd: i64,
    pub next_dir_fd: i64,
    pub next_store_fd: i64,
    /// G4（E3.3 storage）：键值存储注册表（id → (路径, 键值)；KvStore 值持 `store` id）
    pub stores: HashMap<i64, (String, HashMap<Vec<u8>, Vec<u8>>)>,
    /// G5（E3.3 rng）：全局伪随机数状态（xorshift64；`io.rng.seed` 重置——协作式
    /// 单线程执行下全局态安全；默认种子常量对齐 oracle）
    pub rng_state: u64,
    /// K4（ADR-0014）：`@intFromPtr` 登记的整数地址 → 原值（Ptr/Boxed，round-trip 重建用）。
    /// `@ptrFromInt` 依此重建原指针；未登记地址合成匿名槽（同地址幂等——对齐 interp
    /// 合成 cell 与原生 inttoptr 虚拟指针语义）。
    pub addr_registry: HashMap<i128, IrValue>,
    /// E4：OS 线程控制表（tid → ThreadStateIr）
    pub thread_handles: HashMap<i64, ThreadStateIr>,
    /// E4：下一线程 ID（自增分配）
    pub next_tid: i64,
    /// E4：通道注册表（通道 ID → 通道状态，Pipe 使用 mpsc）
    pub channels: HashMap<i64, ChannelStateIr>,
    /// E4：下一通道 ID（自增分配）
    pub next_channel_id: i64,
    /// E4：当前模块引用（供 spawn 新线程克隆以访问函数定义）
    pub module: Option<Arc<IrModule>>,
}

/// io.ipc 管道共享态（协作式：读写均不阻塞；writer_open=false 且空缓冲 = 读端空切片）
#[derive(Debug, Default)]
pub struct PipeIr {
    pub buf: Vec<u8>,
    pub writer_open: bool,
}

impl Ctx {
    fn alloc(&mut self, cell: Cell) -> usize {
        self.cells.push(cell);
        self.cells.len() - 1
    }
    /// 读槽值（槽 → cell → value；槽/元素/字段 cell 恒为 `Cell::Value`——不变量）
    fn get(&self, frame: &Frame, slot: usize) -> &IrValue {
        match &self.cells[frame.cells[slot]] {
            Cell::Value(v) => v,
            _ => unreachable!("slot cell is not a value cell"),
        }
    }
    /// 写槽值
    fn set(&mut self, frame: &Frame, slot: usize, v: IrValue) {
        self.cells[frame.cells[slot]] = Cell::Value(v);
    }
    /// 读 cell 值（指针目标/数组元素/类字段）
    fn cell_value(&self, cell: usize) -> &IrValue {
        match &self.cells[cell] {
            Cell::Value(v) => v,
            _ => unreachable!("cell is not a value cell"),
        }
    }
    /// 读 cell 为值：Value 直接克隆，非 Value cell（Class/Map/Arena/Boxed）还原为
    /// 对应句柄值（遍历产物如 Map 的 KV 条目即以 Class cell 承载，捕获/收集时用）
    fn read_cell(&self, cell: usize) -> IrValue {
        match &self.cells[cell] {
            Cell::Value(v) => v.clone(),
            Cell::Class { .. } => IrValue::Class(cell),
            Cell::Map { .. } => IrValue::Map(cell),
            Cell::Arena(_) => IrValue::Arena(cell),
            Cell::Boxed { .. } => IrValue::Boxed(cell),
            other => unreachable!("cell is not a value or handle cell: {other:?}"),
        }
    }
    /// 写 cell 值（写穿）
    fn set_cell(&mut self, cell: usize, v: IrValue) {
        self.cells[cell] = Cell::Value(v);
    }
    /// 数组底层长度（Phase 2）
    fn elems_len(&self, cell: usize) -> usize {
        match &self.cells[cell] {
            Cell::Elems(e) => e.len(),
            _ => 0,
        }
    }

    /// G5/§8.3 Debug 泄漏检测：分配清单文本（供程序退出报告 / 测试断言）
    pub fn leak_report(&self) -> String {
        let mut out = String::new();
        for (size, line) in &self.alloc_tracker {
            out.push_str(&format!("leak: line {line}: {size} bytes\n"));
        }
        out
    }

    /// G5/§8.3 Debug 泄漏检测：当前已登记分配数
    pub fn leak_count(&self) -> usize {
        self.alloc_tracker.len()
    }
}

/// 解引用：Ptr/Boxed → pointee（对齐 tree-walking `deref_value`）；pointee 为 Vec 时
/// 一并剥为共享 Arr；非 Ptr → 恒等。tree-walking 递归解引用，此处引用返回无法递归，
/// 故在 Ptr/Boxed/Vec 三处显式 peel（一层 `Ptr(Vec)`/`Boxed(Vec)` 即达 Arr）。
fn deref_value<'a>(ctx: &'a Ctx, v: &'a IrValue) -> &'a IrValue {
    match v {
        IrValue::Ptr(c) => match &ctx.cells[*c] {
            Cell::Value(v) => peel_vec(ctx, v),
            _ => v,
        },
        IrValue::Boxed(c) => match &ctx.cells[*c] {
            Cell::Boxed { data, .. } => match &ctx.cells[*data] {
                Cell::Value(v) => peel_vec(ctx, v),
                _ => v,
            },
            _ => v,
        },
        // G4：Vec peel → 共享底层的 Arr（对齐 tree-walking `Value::Vec => Value::Arr`）
        IrValue::Vec(c) => match &ctx.cells[*c] {
            Cell::Vec { arr, .. } => arr,
            _ => v,
        },
        other => other,
    }
}

/// G4：`IrValue::Vec` 剥为其底层 Arr 的引用；非 Vec 恒等（peel 辅助）
fn peel_vec<'a>(ctx: &'a Ctx, v: &'a IrValue) -> &'a IrValue {
    match v {
        IrValue::Vec(c) => match &ctx.cells[*c] {
            Cell::Vec { arr, .. } => arr,
            _ => v,
        },
        other => other,
    }
}

/// 索引值 → usize（负/非整 → BadIndex，对齐 tree-walking `as_index`）
fn as_index(ctx: &Ctx, v: &IrValue) -> R<usize> {
    match deref_value(ctx, v) {
        IrValue::Int(i) if *i >= 0 => Ok(*i as usize),
        _ => Err(IrError::msg("BadIndex", "bad index")),
    }
}

/// 值形态描述（`NotIterable` 错误消息；对齐 tree-walking `type_name` 的通俗面）
fn type_descr(v: &IrValue) -> String {
    match v {
        IrValue::Int(_) => "i32".into(),
        IrValue::Float(_) => "f64".into(),
        IrValue::Bool(_) => "bool".into(),
        IrValue::Str(_) => "&[u8]".into(),
        IrValue::Opt(_) => "?T".into(),
        IrValue::Err { name, .. } => format!("error.{name}"),
        IrValue::Ptr(_) => "*T".into(),
        IrValue::Boxed(_) => "*T".into(),
        IrValue::Arr(_) => "[]T".into(),
        IrValue::Vec(_) => "[]T".into(),
        IrValue::Map(_) => "Map".into(),
        IrValue::Slice { .. } => "[]T".into(),
        IrValue::Class(_) => "class".into(),
        IrValue::Arena(_) => "Arena".into(),
        IrValue::Enum { name, .. } => name.clone(),
        IrValue::End => "end".into(),
        IrValue::Iter(_) => "<iter>".into(),
        IrValue::Fn(_) => "fn".into(),
        IrValue::Closure { .. } => "closure".into(),
        IrValue::Mutex(_) => "Mutex".into(),
        IrValue::Void => "void".into(),
    }
}

// ---------- Phase 3 运行时语义（switch 模式匹配 / 迭代器；对齐 oracle） ----------

/// 模式匹配（对齐 oracle `match_pattern`，`interp.rs:1342-1361`）：
/// subject 已 deref 一次；`Else` 不在此处理（lower 阶段识别为兜底臂）。
fn match_pattern(subject: &IrValue, pat: &IrPattern) -> bool {
    match (subject, pat) {
        (IrValue::Enum { variant, .. }, IrPattern::Ident(s)) => variant == s,
        (IrValue::Int(i), IrPattern::Int(s)) => *i == *s,
        (IrValue::Float(f), IrPattern::Float(s)) => *f == *s,
        (IrValue::Str(st), IrPattern::Str(s)) => *st == s.as_bytes(),
        (IrValue::Int(c), IrPattern::Char(pc)) => *c == *pc as i128,
        (IrValue::Err { name, .. }, IrPattern::Error(pe)) => name == pe,
        (IrValue::Bool(b), IrPattern::Ident(s)) => (*b && s == "true") || (!*b && s == "false"),
        (IrValue::Opt(None), IrPattern::Ident(s)) => s == "null",
        _ => false,
    }
}

/// 枚举负载捕获：subject 为 `Enum{payload:Some(p)}` → p；否则 → subject 本身
/// （对齐 oracle `exec_switch_arm` 的负载捕获分支，`interp.rs:1318-1323`）。
fn enum_payload(ctx: &Ctx, v: &IrValue) -> R<IrValue> {
    let v = deref_value(ctx, v).clone();
    match v {
        IrValue::Enum {
            payload: Some(p), ..
        } => Ok(*p),
        other => Ok(other),
    }
}

/// 展开可迭代对象为迭代项列表（对齐 oracle `iter_items`，`interp.rs:1162-1217`）：
/// - Arr/Slice：共享元素 cell，`is_ref=true`（写穿别名）
/// - Class "Map"：KV 条目新 cell（`key` 为新建 Str cell、`value` 共享源字段 cell），`is_ref=false`
/// - 其它 Class：用户 IIterable——循环调用 `{Type}.next(self)` 至 `Opt(None)`/`Void`
/// - Str：字节 Int 新 cell，`is_ref=false`
/// - 其余 → NotIterable
fn make_iter(ctx: &mut Ctx, module: &IrModule, v: &IrValue, depth: usize) -> R<Vec<IterItem>> {
    let v = deref_value(ctx, v).clone();
    match v {
        IrValue::Arr(c) => match &ctx.cells[c] {
            Cell::Elems(e) => Ok(e
                .iter()
                .map(|ec| IterItem {
                    cell: *ec,
                    is_ref: true,
                })
                .collect()),
            _ => Err(IrError::msg(
                "NotIterable",
                "array cell is not an element list",
            )),
        },
        // 集合（G4）：Vec 句柄遍历（Ptr(Vec) 一层 deref 后为 Vec——共享 Elems）
        IrValue::Vec(c) => match &ctx.cells[c] {
            Cell::Vec {
                arr: IrValue::Arr(ac),
                ..
            } => match &ctx.cells[*ac] {
                Cell::Elems(e) => Ok(e
                    .iter()
                    .map(|ec| IterItem {
                        cell: *ec,
                        is_ref: true,
                    })
                    .collect()),
                _ => Err(IrError::msg(
                    "NotIterable",
                    "vec cell is not an element list",
                )),
            },
            _ => Err(IrError::msg("NotIterable", "vec cell is corrupt")),
        },
        IrValue::Slice { data, start, len } => match &ctx.cells[data] {
            Cell::Elems(e) => Ok(e[start..start + len]
                .iter()
                .map(|ec| IterItem {
                    cell: *ec,
                    is_ref: true,
                })
                .collect()),
            _ => Err(IrError::msg(
                "NotIterable",
                "slice data is not an element list",
            )),
        },
        IrValue::Class(c) => {
            // 先克隆字段表，释放 `ctx.cells` 借用（Map 分支内需可变借用 ctx.alloc）
            let (name, fields) = match &ctx.cells[c] {
                Cell::Class { name, fields } => (name.clone(), fields.clone()),
                _ => return Err(IrError::msg("NotIterable", "class cell is corrupt")),
            };
            if name == "Map" {
                // Map 遍历：KV 条目（key/value 字段，value 共享源字段 cell——与 for |kv| 一致）
                let items: Vec<IterItem> = fields
                    .iter()
                    .map(|(k, vc)| {
                        let mut fs = HashMap::new();
                        fs.insert(
                            "key".into(),
                            ctx.alloc(Cell::Value(IrValue::Str(k.clone().into_bytes()))),
                        );
                        fs.insert("value".into(), *vc);
                        let kv = ctx.alloc(Cell::Class {
                            name: "KV".into(),
                            fields: fs,
                        });
                        IterItem {
                            cell: kv,
                            is_ref: false,
                        }
                    })
                    .collect();
                Ok(items)
            } else {
                // 用户 IIterable：next() 直到 Opt(None)/Void（tag1：next → ?T）
                let fname = format!("{name}.next");
                let idx = pick_func(ctx, module, &fname, &[v.clone()]).ok_or_else(|| {
                    IrError::msg(
                        "NotIterable",
                        format!("type `{name}` has no `next` method (IIterable)"),
                    )
                })?;
                let self_v = v.clone();
                let mut items = Vec::new();
                loop {
                    let nv = exec_func(ctx, module, idx, &[self_v.clone()], depth + 1)?;
                    match nv {
                        IrValue::Opt(Some(inner)) => items.push(IterItem {
                            cell: ctx.alloc(Cell::Value(*inner)),
                            is_ref: false,
                        }),
                        IrValue::Opt(None) | IrValue::Void => break,
                        other => items.push(IterItem {
                            cell: ctx.alloc(Cell::Value(other)),
                            is_ref: false,
                        }),
                    }
                }
                Ok(items)
            }
        }
        // 集合（G4）：Map 句柄遍历 → KV 条目（key/value 字段，value 共享源字段 cell）
        IrValue::Map(c) => {
            let fields = match &ctx.cells[c] {
                Cell::Map { fields, .. } => fields.clone(),
                _ => return Err(IrError::msg("NotIterable", "map cell is corrupt")),
            };
            let items: Vec<IterItem> = fields
                .iter()
                .map(|(k, vc)| {
                    let mut fs = HashMap::new();
                    fs.insert(
                        "key".into(),
                        ctx.alloc(Cell::Value(IrValue::Str(k.clone().into_bytes()))),
                    );
                    fs.insert("value".into(), *vc);
                    let kv = ctx.alloc(Cell::Class {
                        name: "KV".into(),
                        fields: fs,
                    });
                    IterItem {
                        cell: kv,
                        is_ref: false,
                    }
                })
                .collect();
            Ok(items)
        }
        IrValue::Str(s) => Ok(s
            .iter()
            .map(|b| IterItem {
                cell: ctx.alloc(Cell::Value(IrValue::Int(*b as i128))),
                is_ref: false,
            })
            .collect()),
        other => Err(IrError::msg(
            "NotIterable",
            format!("value of type `{}` is not iterable", type_descr(&other)),
        )),
    }
}

// ---------- Phase 2 聚合运行时语义（对齐 tree-walking eval_field/eval_index/等） ----------

/// 字段读取：Class 字段 / Str/Arr/Slice/Map `.len` 内建字段；无字段 → NoField
fn field_value(ctx: &Ctx, b: &IrValue, field: &str) -> R<IrValue> {
    match b {
        IrValue::Class(c) => match &ctx.cells[*c] {
            Cell::Class { name, fields } => {
                // Map 内建字段：len
                if name == "Map" && field == "len" {
                    return Ok(IrValue::Int(fields.len() as i128));
                }
                match fields.get(field) {
                    Some(fc) => Ok(ctx.cell_value(*fc).clone()),
                    None => Err(IrError::msg("NoField", format!("no field `{field}`"))),
                }
            }
            _ => Err(IrError::msg("NoField", format!("no field `{field}`"))),
        },
        IrValue::Str(s) => {
            if field == "len" {
                Ok(IrValue::Int(s.len() as i128))
            } else {
                Err(IrError::msg("NoField", format!("no field `{field}`")))
            }
        }
        IrValue::Arr(c) => match &ctx.cells[*c] {
            Cell::Elems(e) => {
                if field == "len" {
                    Ok(IrValue::Int(e.len() as i128))
                } else {
                    Err(IrError::msg("NoField", format!("no field `{field}`")))
                }
            }
            _ => Err(IrError::msg("NoField", format!("no field `{field}`"))),
        },
        // 集合（G4）：Vec 委托 Arr 字段读；Map 字段读（.len）
        IrValue::Vec(c) => match &ctx.cells[*c] {
            Cell::Vec { arr, .. } => field_value(ctx, arr, field),
            _ => Err(IrError::msg("NoField", format!("no field `{field}`"))),
        },
        IrValue::Map(c) => match &ctx.cells[*c] {
            Cell::Map { fields, .. } => {
                if field == "len" {
                    Ok(IrValue::Int(fields.len() as i128))
                } else {
                    Err(IrError::msg("NoField", format!("no field `{field}`")))
                }
            }
            _ => Err(IrError::msg("NoField", format!("no field `{field}`"))),
        },
        IrValue::Slice { len, .. } => {
            if field == "len" {
                Ok(IrValue::Int(*len as i128))
            } else {
                Err(IrError::msg("NoField", format!("no field `{field}`")))
            }
        }
        _ => Err(IrError::msg("NoField", format!("no field `{field}`"))),
    }
}

/// 字段写入：仅 Class 目标（非 Class → TypeError）；字段为普通值——写入即替换
fn store_field(ctx: &mut Ctx, b: &IrValue, field: &str, v: IrValue) -> R<()> {
    let c = match b {
        IrValue::Class(c) => *c,
        _ => return Err(IrError::msg("TypeError", "store to non-class")),
    };
    // 先分配新字段 cell，避免在 cells 的可变借用内再次借用 ctx
    let nc = ctx.alloc(Cell::Value(v));
    match &mut ctx.cells[c] {
        Cell::Class { fields, .. } => {
            fields.insert(field.to_string(), nc);
            Ok(())
        }
        _ => Err(IrError::msg("TypeError", "store to non-class")),
    }
}

/// 索引读取：Arr/Slice 元素（克隆值）、Str 字节；越界 → IndexOutOfBounds；非可索引 → NotIndexable
fn index_value(ctx: &Ctx, b: &IrValue, i: usize) -> R<IrValue> {
    match b {
        IrValue::Arr(c) => match &ctx.cells[*c] {
            Cell::Elems(e) => {
                if i >= e.len() {
                    return Err(IrError::msg("IndexOutOfBounds", "index out of bounds"));
                }
                Ok(ctx.cell_value(e[i]).clone())
            }
            _ => Err(IrError::msg("NotIndexable", "not indexable")),
        },
        IrValue::Str(s) => {
            if i >= s.len() {
                return Err(IrError::msg("IndexOutOfBounds", "index out of bounds"));
            }
            Ok(IrValue::Int(s[i] as i128))
        }
        IrValue::Slice { data, start, len } => {
            if i >= *len {
                return Err(IrError::msg("IndexOutOfBounds", "index out of bounds"));
            }
            match &ctx.cells[*data] {
                Cell::Elems(e) => Ok(ctx.cell_value(e[*start + i]).clone()),
                _ => Err(IrError::msg("NotIndexable", "not indexable")),
            }
        }
        _ => Err(IrError::msg("NotIndexable", "not indexable")),
    }
}

/// 索引写入：仅 Arr 目标（非 Arr → TypeError）；写穿元素 cell（切片/视图共享）
fn store_index(ctx: &mut Ctx, b: &IrValue, i: usize, v: IrValue) -> R<()> {
    let ec = match b {
        IrValue::Arr(c) => match &ctx.cells[*c] {
            Cell::Elems(e) => {
                if i >= e.len() {
                    return Err(IrError::msg("IndexOutOfBounds", "index out of bounds"));
                }
                e[i]
            }
            _ => return Err(IrError::msg("TypeError", "store to non-array")),
        },
        _ => return Err(IrError::msg("TypeError", "store to non-array")),
    };
    ctx.set_cell(ec, v);
    Ok(())
}

/// 切片：Arr → 共享视图；Str → 字节拷贝；Slice → 重切片；越界 → IndexOutOfBounds
fn slice_of(ctx: &Ctx, b: &IrValue, lo: usize, hi: usize, open_end: bool) -> R<IrValue> {
    match b {
        IrValue::Arr(c) => match &ctx.cells[*c] {
            Cell::Elems(e) => {
                let total = e.len();
                let h = if open_end { total } else { hi };
                if h > total || lo > total {
                    return Err(IrError::msg("IndexOutOfBounds", "slice out of bounds"));
                }
                Ok(IrValue::Slice {
                    data: *c,
                    start: lo,
                    len: h.saturating_sub(lo),
                })
            }
            _ => Err(IrError::msg("NotIndexable", "not indexable")),
        },
        IrValue::Str(s) => {
            let bytes = s.clone();
            let h = if open_end { bytes.len() } else { hi };
            if h > bytes.len() || lo > bytes.len() {
                return Err(IrError::msg("IndexOutOfBounds", "slice out of bounds"));
            }
            Ok(IrValue::Str(bytes[lo..h].to_vec()))
        }
        IrValue::Slice { data, start, len } => {
            let total = *len;
            let h = if open_end { total } else { hi };
            if h > total || lo > total {
                return Err(IrError::msg("IndexOutOfBounds", "slice out of bounds"));
            }
            Ok(IrValue::Slice {
                data: *data,
                start: *start + lo,
                len: h.saturating_sub(lo),
            })
        }
        _ => Err(IrError::msg("NotIndexable", "not indexable")),
    }
}

/// 切片写回：仅 Arr 目标且仅 Set（其余 → TypeError/BadAssign，由调用方判定）；
/// 源元素从 lo 起写入目标，受目标长度约束（非 Arr 源静默无操作——对齐 oracle）。
fn store_slice(ctx: &mut Ctx, b: &IrValue, lo: usize, hi: usize, v: &IrValue) -> R<()> {
    let c = match b {
        IrValue::Arr(c) => *c,
        _ => return Err(IrError::msg("TypeError", "store to non-array")),
    };
    let total = ctx.elems_len(c);
    if hi > total || lo > total {
        return Err(IrError::msg("IndexOutOfBounds", "slice out of bounds"));
    }
    // 源元素值快照（先克隆，避免可变借用冲突）
    let src_vals: Vec<IrValue> = match v {
        IrValue::Arr(sc) => match &ctx.cells[*sc] {
            Cell::Elems(e) => e.iter().map(|ec| ctx.cell_value(*ec).clone()).collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    let target_cells: Vec<usize> = match &ctx.cells[c] {
        Cell::Elems(e) => e.clone(),
        _ => return Err(IrError::msg("TypeError", "store to non-array")),
    };
    for (k, sv) in src_vals.iter().enumerate() {
        if lo + k < total {
            ctx.set_cell(target_cells[lo + k], sv.clone());
        }
    }
    Ok(())
}

/// 数组深比较（元素按值比较，递归）
fn arr_eq(ctx: &Ctx, a: usize, b: usize) -> bool {
    let (ae, be) = match (&ctx.cells[a], &ctx.cells[b]) {
        (Cell::Elems(x), Cell::Elems(y)) => (x.clone(), y.clone()),
        _ => return false,
    };
    ae.len() == be.len()
        && ae
            .iter()
            .zip(be.iter())
            .all(|(x, y)| ctx.cell_value(*x).value_eq(ctx, ctx.cell_value(*y)))
}

/// 集合 Map 深比较（G4）：键值表按值相等（键集相同 + 每键字段值相等）
fn map_eq(ctx: &Ctx, a: usize, b: usize) -> bool {
    let (af, bf) = match (&ctx.cells[a], &ctx.cells[b]) {
        (Cell::Map { fields: x, .. }, Cell::Map { fields: y, .. }) => (x.clone(), y.clone()),
        _ => return false,
    };
    if af.len() != bf.len() {
        return false;
    }
    af.iter().all(|(k, fc)| {
        bf.get(k).map_or(false, |bc| {
            ctx.cell_value(*fc).value_eq(ctx, ctx.cell_value(*bc))
        })
    })
}

/// 集合 Map 与 Class("Map") 深比较（G4）：Map 字段表 vs class 字段表
fn map_class_eq(ctx: &Ctx, m: usize, c: usize) -> bool {
    let (fm, fc) = match (&ctx.cells[m], &ctx.cells[c]) {
        (Cell::Map { fields: x, .. }, Cell::Class { fields: y, .. }) => (x.clone(), y.clone()),
        _ => return false,
    };
    if fm.len() != fc.len() {
        return false;
    }
    fm.iter().all(|(k, mc)| {
        fc.get(k).map_or(false, |cc| {
            ctx.cell_value(*mc).value_eq(ctx, ctx.cell_value(*cc))
        })
    })
}

/// 类深比较：类型名相同 + 字段数相同 + 每字段按值相等
fn class_eq(ctx: &Ctx, a: usize, b: usize) -> bool {
    let (an, af) = match &ctx.cells[a] {
        Cell::Class { name, fields } => (name.clone(), fields.clone()),
        _ => return false,
    };
    let (bn, bf) = match &ctx.cells[b] {
        Cell::Class { name, fields } => (name.clone(), fields.clone()),
        _ => return false,
    };
    if an != bn || af.len() != bf.len() {
        return false;
    }
    af.iter().all(|(k, fc)| {
        bf.get(k).map_or(false, |bc| {
            ctx.cell_value(*fc).value_eq(ctx, ctx.cell_value(*bc))
        })
    })
}

/// 帧：槽 → cell 索引（别名关键装置——`&x` 即 `Ptr(frame.cells[slot_of_x])`）。
/// `defers`：本调用内待运行 defer 的多重集（PushDefer 增 / PopDefer 减；守卫判成员）。
/// 运行时 LIFO 顺序由编译期发射顺序保证，故此处仅需「是否待运行」判定，无需栈序。
#[derive(Debug, Clone)]
pub struct Frame {
    pub cells: Vec<usize>,
    pub defers: Vec<usize>,
    /// M2.7 只读捕获强制（Phase 8）：非 `mut` 闭包帧中**只读**的捕获参数槽号。
    /// [`IrInst::Store`] 写这些槽 → ReadonlyCapture（对齐 oracle `readonly_caps`）。
    /// 普通函数/`mut` 闭包恒空。
    pub readonly: Vec<usize>,
    /// Q14：Boxed 值 cell 索引集（`box(v)` 产生的 `Cell::Boxed` 索引）。
    /// 离开作用域时自动释放（`Return`/`ReturnVoid`/`Err` 退出前清理）。
    /// 返回值若为 Boxed，所有权转移至调用方（从本集移除），不释放。
    pub boxed: HashSet<usize>,
}

#[derive(Debug, Clone)]
pub struct IrError {
    pub name: String,
    pub message: String,
}

impl IrError {
    pub fn msg(name: &str, message: impl Into<String>) -> Self {
        IrError {
            name: name.to_string(),
            message: message.into(),
        }
    }
}

type R<T> = std::result::Result<T, IrError>;

impl IrValue {
    fn as_bool(&self) -> bool {
        match self {
            IrValue::Bool(b) => *b,
            IrValue::Int(i) => *i != 0,
            IrValue::Float(f) => *f != 0.0,
            IrValue::Str(s) => !s.is_empty(),
            IrValue::Opt(Some(v)) => v.as_bool(),
            // 指针恒真（对齐 tree-walking `Value::Ptr(_) => true`）
            IrValue::Ptr(_) => true,
            IrValue::Boxed(_) => true,
            _ => true,
        }
    }
    fn is_err(&self) -> bool {
        matches!(self, IrValue::Err { .. })
    }
    fn display(&self, ctx: &Ctx) -> String {
        match self {
            IrValue::Int(i) => i.to_string(),
            IrValue::Float(f) => {
                if f.fract() == 0.0 && f.is_finite() && f.abs() < 1e15 {
                    format!("{f:.1}")
                } else {
                    f.to_string()
                }
            }
            IrValue::Bool(b) => b.to_string(),
            IrValue::Str(s) => String::from_utf8_lossy(s).to_string(),
            IrValue::Opt(Some(v)) => format!("?{}", v.display(ctx)),
            IrValue::Opt(None) => "null".into(),
            IrValue::Err { name, .. } => format!("error.{name}"),
            // 指针显示 pointee（对齐 tree-walking `Value::Ptr(p) => p.borrow().display()`）
            IrValue::Ptr(c) => ctx.cell_value(*c).display(ctx),
            IrValue::Boxed(c) => match &ctx.cells[*c] {
                Cell::Boxed { data, .. } => ctx.cell_value(*data).display(ctx),
                _ => "boxed".into(),
            },
            IrValue::Arr(c) => match &ctx.cells[*c] {
                Cell::Elems(e) => {
                    let items: Vec<String> = e
                        .iter()
                        .map(|ec| ctx.cell_value(*ec).display(ctx))
                        .collect();
                    format!("[{}]", items.join(", "))
                }
                _ => "[]".into(),
            },
            // 集合（G4）：Vec 委托 Arr 显示；Map 显示 `Map { k = v, ... }`
            IrValue::Vec(c) => match &ctx.cells[*c] {
                Cell::Vec { arr, .. } => arr.display(ctx),
                _ => "[]".into(),
            },
            IrValue::Map(c) => match &ctx.cells[*c] {
                Cell::Map { fields, .. } => {
                    let items: Vec<String> = fields
                        .iter()
                        .map(|(k, fc)| format!("{k} = {}", ctx.cell_value(*fc).display(ctx)))
                        .collect();
                    format!("Map {{ {} }}", items.join(", "))
                }
                _ => "Map {  }".into(),
            },
            IrValue::Slice { data, start, len } => match &ctx.cells[*data] {
                Cell::Elems(e) => {
                    let items: Vec<String> = e[*start..*start + *len]
                        .iter()
                        .map(|ec| ctx.cell_value(*ec).display(ctx))
                        .collect();
                    format!("[{}]", items.join(", "))
                }
                _ => "[]".into(),
            },
            IrValue::Class(c) => match &ctx.cells[*c] {
                Cell::Class { name, fields } => {
                    let items: Vec<String> = fields
                        .iter()
                        .map(|(k, fc)| format!("{k} = {}", ctx.cell_value(*fc).display(ctx)))
                        .collect();
                    format!("{name} {{ {} }}", items.join(", "))
                }
                _ => "void".into(),
            },
            IrValue::Arena(c) => match &ctx.cells[*c] {
                Cell::Arena(st) => {
                    format!("Arena(bytes={}, blocks={})", st.total, st.blocks.len())
                }
                _ => "Arena".into(),
            },
            IrValue::Enum {
                name,
                variant,
                payload,
            } => match payload {
                Some(p) => format!("{name}.{variant} = {}", p.display(ctx)),
                None => format!("{name}.{variant}"),
            },
            IrValue::End => "end".into(),
            IrValue::Iter(_) => "<iter>".into(),
            IrValue::Fn(name) => name.clone(),
            IrValue::Closure { .. } => "<closure>".into(),
            IrValue::Mutex(m) => match m.lock() {
                Ok(v) => format!("Mutex({})", v.display(ctx)),
                Err(_) => "Mutex(<poisoned>)".to_string(),
            },
            IrValue::Void => "void".into(),
        }
    }
    fn value_eq(&self, ctx: &Ctx, other: &IrValue) -> bool {
        match (self, other) {
            (IrValue::Int(a), IrValue::Int(b)) => a == b,
            (IrValue::Int(a), IrValue::Float(b)) => *a as f64 == *b,
            (IrValue::Float(a), IrValue::Int(b)) => *a == *b as f64,
            (IrValue::Float(a), IrValue::Float(b)) => a == b,
            (IrValue::Bool(a), IrValue::Bool(b)) => a == b,
            (IrValue::Str(a), IrValue::Str(b)) => a == b,
            (IrValue::Opt(a), IrValue::Opt(b)) => match (a, b) {
                (Some(x), Some(y)) => x.value_eq(ctx, y),
                (None, None) => true,
                _ => false,
            },
            // M4.2：错误按「码」比较（全局唯一），非名字
            (IrValue::Err { code: a, .. }, IrValue::Err { code: b, .. }) => a == b,
            // 指针：同 cell = 同一目标（身份——对齐 tree-walking `Rc::ptr_eq`）；
            // Ptr 与普通值比较时解引用后比较（对齐 `(Ptr, b) => deref(a).value_eq(b)`）
            (IrValue::Ptr(a), IrValue::Ptr(b)) => a == b,
            (IrValue::Ptr(a), b) => ctx.cell_value(*a).value_eq(ctx, b),
            (a, IrValue::Ptr(b)) => a.value_eq(ctx, ctx.cell_value(*b)),
            // 装箱胖指针：同 cell 索引 = 同一目标（身份）；与普通值比较时解引用 pointee
            (IrValue::Boxed(a), IrValue::Boxed(b)) => a == b,
            (IrValue::Boxed(a), b) => match &ctx.cells[*a] {
                Cell::Boxed { data, .. } => ctx.cell_value(*data).value_eq(ctx, b),
                _ => false,
            },
            (a, IrValue::Boxed(b)) => match &ctx.cells[*b] {
                Cell::Boxed { data, .. } => a.value_eq(ctx, ctx.cell_value(*data)),
                _ => false,
            },
            // 集合（G4）：Vec 按内容比较（委托 Arr）；Map 按键值表比较（含 Class("Map")）
            (IrValue::Vec(a), IrValue::Vec(b)) => match (&ctx.cells[*a], &ctx.cells[*b]) {
                (
                    Cell::Vec {
                        arr: IrValue::Arr(aa),
                        ..
                    },
                    Cell::Vec {
                        arr: IrValue::Arr(bb),
                        ..
                    },
                ) => arr_eq(ctx, *aa, *bb),
                _ => a == b,
            },
            (IrValue::Vec(a), b) => match &ctx.cells[*a] {
                Cell::Vec { arr, .. } => arr.value_eq(ctx, b),
                _ => false,
            },
            (a, IrValue::Vec(b)) => match &ctx.cells[*b] {
                Cell::Vec { arr, .. } => a.value_eq(ctx, arr),
                _ => false,
            },
            (IrValue::Map(a), IrValue::Map(b)) => map_eq(ctx, *a, *b),
            (IrValue::Map(a), IrValue::Class(b)) if class_name(ctx, *b) == "Map" => {
                map_class_eq(ctx, *a, *b)
            }
            (IrValue::Class(a), IrValue::Map(b)) if class_name(ctx, *a) == "Map" => {
                map_class_eq(ctx, *b, *a)
            }
            // ---- Phase 2 聚合 ----
            (IrValue::Arr(a), IrValue::Arr(b)) => arr_eq(ctx, *a, *b),
            (
                IrValue::Slice {
                    data: da,
                    start: sa,
                    len: la,
                },
                IrValue::Slice {
                    data: db,
                    start: sb,
                    len: lb,
                },
            ) => {
                if la != lb {
                    return false;
                }
                let (da_e, db_e) = match (&ctx.cells[*da], &ctx.cells[*db]) {
                    (Cell::Elems(x), Cell::Elems(y)) => (x.clone(), y.clone()),
                    _ => return false,
                };
                (0..*la).all(|i| {
                    ctx.cell_value(da_e[*sa + i])
                        .value_eq(ctx, ctx.cell_value(db_e[*sb + i]))
                })
            }
            (IrValue::Slice { data, start, len }, IrValue::Arr(b)) => {
                let d = match &ctx.cells[*data] {
                    Cell::Elems(x) => x.clone(),
                    _ => return false,
                };
                let (a_e, b_e) = (
                    d,
                    match &ctx.cells[*b] {
                        Cell::Elems(x) => x.clone(),
                        _ => return false,
                    },
                );
                *len == b_e.len()
                    && (0..*len).all(|i| {
                        ctx.cell_value(a_e[*start + i])
                            .value_eq(ctx, ctx.cell_value(b_e[i]))
                    })
            }
            (IrValue::Arr(a), IrValue::Slice { data, start, len }) => {
                let d = match &ctx.cells[*data] {
                    Cell::Elems(x) => x.clone(),
                    _ => return false,
                };
                let (a_e, d_e) = (
                    match &ctx.cells[*a] {
                        Cell::Elems(x) => x.clone(),
                        _ => return false,
                    },
                    d,
                );
                a_e.len() == *len
                    && (0..*len).all(|i| {
                        ctx.cell_value(a_e[i])
                            .value_eq(ctx, ctx.cell_value(d_e[*start + i]))
                    })
            }
            (IrValue::Class(a), IrValue::Class(b)) => class_eq(ctx, *a, *b),
            (
                IrValue::Enum {
                    name: an,
                    variant: av,
                    payload: ap,
                },
                IrValue::Enum {
                    name: bn,
                    variant: bv,
                    payload: bp,
                },
            ) => {
                an == bn
                    && av == bv
                    && match (ap, bp) {
                        (Some(x), Some(y)) => x.value_eq(ctx, y),
                        (None, None) => true,
                        _ => false,
                    }
            }
            (IrValue::Fn(a), IrValue::Fn(b)) => a == b,
            (IrValue::Closure { .. }, _) | (_, IrValue::Closure { .. }) => false,
            (IrValue::End, IrValue::End) => true,
            (IrValue::Void, IrValue::Void) => true,
            (IrValue::Mutex(a), IrValue::Mutex(b)) => match (a.lock(), b.lock()) {
                (Ok(av), Ok(bv)) => av.value_eq(ctx, &bv),
                _ => false,
            },
            _ => false,
        }
    }
}

/// 递归深度上限（对齐 tree-walking `MAX_CALL_DEPTH`——双模式一致）
pub const MAX_CALL_DEPTH: usize = 1000;

/// 一次性执行模块中名为 entry 的函数（测试/入口）——建独立 [`IrRuntime`]（含全局初始化）。
pub fn run_ir(module: &IrModule, entry: &str, args: &[IrValue]) -> R<IrValue> {
    let mut rt = IrRuntime::new();
    rt.call(module, entry, args)
}

/// 运行时实例（Phase 5）：共享堆 + 全局 cell + `@__init__` 一次性初始化。
/// 多测试/多函数共用同一实例时，全局只初始化一次、跨调用可见（对齐 oracle
/// `Interp` 的 `globals: HashMap`）。一致性套件与 `hc run --ir`/字节码 VM 走此路径。
#[derive(Debug, Default)]
pub struct IrRuntime {
    pub ctx: Ctx,
    inited: bool,
}

impl IrRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// 启动初始化（幂等）：预分配全部全局 cell（声明序）→ 按 funcs 序执行所有
    /// `@__init__` 函数（多文件合并 = 各模块 init 依次运行，entry 在前）。
    pub fn init(&mut self, module: &IrModule) -> R<()> {
        if self.inited {
            return Ok(());
        }
        self.inited = true;
        // E4：存储模块引用供 spawn 新线程使用
        self.ctx.module = Some(Arc::new(module.clone()));
        // G5：rng 默认状态（对齐 oracle Interp::new——seed(0) 亦回退该常量）
        self.ctx.rng_state = 0x9e37_79b9_7f4a_7c15;
        // 预分配全部全局 cell（声明序）——即使无全局也继续（保险：`@__init__` 仍须执行）。
        // Phase 7：隐式环境名（alloc/io/pi/Vec…）预置内建值（对齐 oracle 隐式环境注入）。
        for name in &module.globals {
            let v = if IMPLICIT_ENV.iter().any(|e| *e == name) {
                implicit_env_value(&mut self.ctx, name)
            } else {
                IrValue::Void
            };
            let cell = self.ctx.alloc(Cell::Value(v));
            self.ctx.globals.insert(name.clone(), cell);
        }
        for (idx, f) in module.funcs.iter().enumerate() {
            if f.name == "@__init__" {
                exec_func(&mut self.ctx, module, idx, &[], 0)?;
            }
        }
        Ok(())
    }

    /// 调用模块函数（自动先初始化全局）。
    pub fn call(&mut self, module: &IrModule, entry: &str, args: &[IrValue]) -> R<IrValue> {
        self.init(module)?;
        // main(args: owned Vec(String))——单参数 = 命令行参数（0 号 = 程序名）；或零参版本。
        // 2026-08-17 定案（ADR-0010）：main 不再注入 io（io 经 `import H.std.{io}` 引入）。
        let mut args = args.to_vec();
        if entry == "main" && args.is_empty() {
            let has_1p = module.func_index.get("main").map_or(false, |v| {
                v.iter().any(|&i| module.funcs[i].params.len() == 1)
            });
            if has_1p {
                let items = self
                    .ctx
                    .args
                    .iter()
                    .map(|a| IrValue::Str(a.clone()))
                    .collect();
                let alloc = implicit_env_value(&mut self.ctx, "alloc");
                args.push(make_vec_with(&mut self.ctx, items, alloc));
            }
        }
        let idx = pick_func(&self.ctx, module, entry, &args)
            .ok_or_else(|| IrError::msg("NoFunction", format!("no function `{entry}`")))?;
        exec_func(&mut self.ctx, module, idx, &args, 0)
    }
}
