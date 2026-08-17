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
use crate::errorcodes::ErrorCodeTable;
use crate::token::Span;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, Write};

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
}

#[derive(Debug, Clone)]
pub struct IrFunc {
    pub name: String,
    /// 参数槽号（声明序）
    pub params: Vec<usize>,
    /// 参数类型（声明序，重载按实参值类型分派用；与 params 等长）
    pub param_ty: Vec<Type>,
    /// 参数是否有默认值（声明序；可选参数 = 尾部默认，对齐 ADR-0009）
    pub param_defaults: Vec<bool>,
    /// 参数默认常量值（编译期常量默认值；缺失尾参时调用点补齐）
    pub defaults: Vec<Option<IrConst>>,
    /// 槽总数（参数 + 局部变量 + 临时）
    pub n_slots: usize,
    pub body: Vec<IrInst>,
    pub is_test: bool,
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
    Err { name: String, code: u32 },
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
    /// namespace 名（扁平 + 全限定）
    pub namespaces: std::collections::HashSet<String>,
}

#[derive(Debug, Default, Clone)]
pub struct ClassInfo {
    pub fields: Vec<String>,
    pub methods: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct EnumInfo {
    /// 变体名（声明序——`@intFromEnum`/`@enumFromInt` 运行时分派按序求索引）
    pub variants: Vec<String>,
}

/// 由 `program.decls` 构建类型表（lower 阶段判型用；运行时类型名内嵌于值）。
fn build_type_table(program: &Program) -> TypeTable {
    let mut tt = TypeTable::default();
    collect_types(&program.decls, &mut tt, &[]);
    tt
}

fn collect_types(decls: &[Decl], tt: &mut TypeTable, path: &[String]) {
    for d in decls {
        match d {
            Decl::Class {
                name,
                fields,
                methods,
                ..
            } => {
                let ci = ClassInfo {
                    fields: fields.iter().map(|f| f.name.clone()).collect(),
                    methods: methods.iter().map(|m| m.name.clone()).collect(),
                };
                tt.classes.insert(name.clone(), ci);
                if !path.is_empty() {
                    let mut q = path.join(".");
                    q.push('.');
                    q.push_str(name);
                    tt.classes.insert(q, tt.classes[name].clone());
                }
            }
            Decl::Enum { name, variants, .. } => {
                let ei = EnumInfo {
                    variants: variants.iter().map(|v| v.name.clone()).collect(),
                };
                tt.enums.insert(name.clone(), ei);
                if !path.is_empty() {
                    let mut q = path.join(".");
                    q.push('.');
                    q.push_str(name);
                    tt.enums.insert(q, tt.enums[name].clone());
                }
            }
            Decl::Namespace { name, decls, .. } => {
                tt.namespaces.insert(name.clone());
                let mut p = path.to_vec();
                p.push(name.clone());
                collect_types(decls, tt, &p);
            }
            _ => {}
        }
    }
}

// ---------- AST → IR 降级 ----------

pub fn lower(program: &Program) -> Result<IrModule, IrError> {
    let errors = crate::errorcodes::collect(program, 0);
    let types = build_type_table(program);
    let funcs = collect_func_names(program);
    let mut globals = collect_globals(program);
    // Phase 7：隐式环境名（alloc/io/pi/Vec…）按全局处理——`io.print` 等限定名根标识符
    // 须经 `LoadGlobal` 解析（对齐 oracle interp.rs:1585-1595 的隐式环境注入）。
    for g in IMPLICIT_ENV {
        globals.insert((*g).to_string());
    }
    let mut module = IrModule::default();
    // 错误码表（名 → 码）：内建运行时错误值（io.fs 等）须与 `error.X` 字面量同码
    for e in errors.entries() {
        module.error_codes.insert(e.name.clone(), e.code);
    }
    // 枚举变体序（Phase 7）：`@intFromEnum`/`@enumFromInt` 运行时分派
    for (n, ei) in &types.enums {
        module.enum_variants.insert(n.clone(), ei.variants.clone());
    }
    for d in &program.decls {
        lower_decl(d, &mut module, &errors, &types, &funcs, &globals)?;
    }
    // Phase 5：合成 `@__init__` 函数（声明序初始化 global/const；多文件合并 = 各模块
    // 自带 init，运行时按 funcs 序依次执行）。不登记 func_index（不可被用户调用）。
    if let Some(init) = lower_init_func(program, &errors, &types, &funcs, &globals, &mut module.closures)? {
        module.funcs.push(init);
    }
    let mut ordered = globals_set_to_ordered(program);
    for g in IMPLICIT_ENV {
        if !ordered.iter().any(|x| x == g) {
            ordered.push((*g).to_string());
        }
    }
    module.globals = ordered;
    Ok(module)
}

/// 收集全局/常量名集合（扁平；错误集别名除外——类型级构造，非值全局）。
fn collect_globals(program: &Program) -> HashSet<String> {
    let mut set = HashSet::new();
    collect_globals_in(&program.decls, &mut set);
    set
}

/// 收集全局/常量名（声明序，供 `IrModule::globals` + `@__init__` 复用）。
fn globals_set_to_ordered(program: &Program) -> Vec<String> {
    let mut ordered = Vec::new();
    collect_globals_ordered(&program.decls, &mut ordered);
    ordered
}

fn collect_globals_in(decls: &[Decl], set: &mut HashSet<String>) {
    for d in decls {
        match d {
            Decl::Global { name, .. } => {
                set.insert(name.clone());
            }
            Decl::Const { name, ty, .. } => {
                // 错误集别名：`const X = error{...}` / `const X = A || B`——类型级构造
                if let Some(Type::Named(tn, _)) = ty {
                    if tn.starts_with("error_set:") {
                        continue;
                    }
                }
                set.insert(name.clone());
            }
            Decl::Namespace { decls: nested, .. } => collect_globals_in(nested, set),
            _ => {}
        }
    }
}

fn collect_globals_ordered(decls: &[Decl], ordered: &mut Vec<String>) {
    for d in decls {
        match d {
            Decl::Global { name, .. } => ordered.push(name.clone()),
            Decl::Const { name, ty, .. } => {
                if let Some(Type::Named(tn, _)) = ty {
                    if tn.starts_with("error_set:") {
                        continue;
                    }
                }
                ordered.push(name.clone());
            }
            Decl::Namespace { decls: nested, .. } => collect_globals_ordered(nested, ordered),
            _ => {}
        }
    }
}

/// 预收集全部函数名（扁平 + 限定 + `{Type}.{method}`），供未解析 Ident → FnRef、
/// 静态方法/namespace 调用 vs 实例方法调用的降级期判定（对齐 oracle 的
/// `funcs: HashMap<String, Vec<FnDef>>` 预建表）。
fn collect_func_names(program: &Program) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_fn_names(&program.decls, &mut names, &[]);
    names
}

fn collect_fn_names(decls: &[Decl], names: &mut HashSet<String>, path: &[String]) {
    for d in decls {
        match d {
            Decl::Fn { name, .. } => {
                names.insert(name.clone());
                if !path.is_empty() {
                    let mut q = path.join(".");
                    q.push('.');
                    q.push_str(name);
                    names.insert(q);
                }
            }
            Decl::Namespace { name, decls: nested, .. } => {
                let mut p = path.to_vec();
                p.push(name.clone());
                collect_fn_names(nested, names, &p);
            }
            Decl::Class { name, methods, .. } => {
                for m in methods {
                    let bare = format!("{name}.{}", m.name);
                    names.insert(bare.clone());
                    if !path.is_empty() {
                        let mut q = path.join(".");
                        q.push('.');
                        q.push_str(&bare);
                        names.insert(q);
                    }
                }
            }
            _ => {}
        }
    }
}

/// 构造「原生/IR 后端暂不支持」的降级错误（阶段外特性 → 硬错误而非静默丢弃）。
fn unsupported_ir_err(what: &str, span: &Span) -> IrError {
    IrError::msg(
        "Unsupported",
        format!(
            "原生/IR 后端暂不支持{what}（第 {} 行第 {} 列）——请用默认 tree-walking 模式 `hc run <file>`",
            span.line, span.col
        ),
    )
}

fn lower_decl(
    d: &Decl,
    module: &mut IrModule,
    errors: &ErrorCodeTable,
    types: &TypeTable,
    funcs: &HashSet<String>,
    globals: &HashSet<String>,
) -> Result<(), IrError> {
    match d {
        Decl::Fn {
            name,
            params,
            body,
            is_test,
            ..
        } => {
            let func = lower_func(name, params, body, *is_test, errors, types, funcs, globals, &mut module.closures)?;
            register_func(module, name, func);
        }
        Decl::Namespace { name, decls, .. } => {
            // namespace 内函数：扁平名 + 限定名双注册（与运行时/语义一致）；
            // 多级 namespace（io.net.connect）注册全限定名
            let mut inner: Vec<(String, String, IrFunc)> = Vec::new();
            collect_ns_funcs(decls, &[name.clone()], &mut inner, errors, types, funcs, globals, &mut module.closures)?;
            for (flat, qn, func) in inner {
                let idx = module.funcs.len();
                module.funcs.push(func);
                // 扁平名（using 导入后直接调用）：先到先得
                module.func_index.entry(flat).or_default().push(idx);
                // 限定名（Math.square / io.net.connect）
                module.func_index.entry(qn).or_default().push(idx);
            }
        }
        // 全局/常量声明：由合成 `@__init__` 函数处理（Phase 5）——此处跳过，
        // 启动初始化语义在 IrRuntime::init 中落地
        Decl::Global { .. } | Decl::Const { .. } => {}
        // 类型级声明（class/enum/interface/using/script）：无顶层运行时代码；
        // class 方法登记为 `{Type}.{method}`（对齐 oracle interp.rs:522-535）——IIterable
        // 用户类型的 `next()` 经此查找。方法体降级失败 → 跳过登记（调用点 NoFunction
        // 硬错误，不使整个程序降级失败——方法与调用分属 Phase 3/4 边界）。
        Decl::Class { name, methods, .. } => {
            for m in methods {
                let fname = format!("{name}.{}", m.name);
                if let Ok(func) = lower_func(&fname, &m.params, &m.body, false, errors, types, funcs, globals, &mut module.closures) {
                    register_func(module, &fname, func);
                }
            }
        }
        Decl::Enum { .. }
        | Decl::Interface { .. }
        | Decl::Using { .. }
        | Decl::Script { .. } => {}
    }
    Ok(())
}

/// 递归收集 namespace 内非测试函数：(扁平名, 全限定名, IR 函数)
fn collect_ns_funcs(
    decls: &[Decl],
    path: &[String],
    out: &mut Vec<(String, String, IrFunc)>,
    errors: &ErrorCodeTable,
    types: &TypeTable,
    funcs: &HashSet<String>,
    globals: &HashSet<String>,
    closures: &mut Vec<IrFunc>,
) -> Result<(), IrError> {
    for d in decls {
        match d {
            Decl::Fn {
                name,
                params,
                body,
                is_test,
                ..
            } if !*is_test => {
                let mut qn = path.to_vec();
                qn.push(name.clone());
                let func = lower_func(name, params, body, false, errors, types, funcs, globals, closures)?;
                out.push((name.clone(), qn.join("."), func));
            }
            Decl::Namespace {
                name,
                decls: nested,
                ..
            } => {
                let mut p = path.to_vec();
                p.push(name.clone());
                collect_ns_funcs(nested, &p, out, errors, types, funcs, globals, closures)?;
            }
            // namespace 内 global/const：扁平登记（对齐 oracle `exec_decl_top`），由 `@__init__` 处理
            Decl::Global { .. } | Decl::Const { .. } => {}
            // 类型级声明在 namespace 内：安全忽略
            _ => {}
        }
    }
    Ok(())
}

fn register_func(module: &mut IrModule, name: &str, func: IrFunc) {
    let idx = module.funcs.len();
    module.funcs.push(func);
    // 重载/可选参数：同名多候选按声明序追加（对齐 oracle funcs: HashMap<String, Vec<FnDef>>）
    module.func_index.entry(name.to_string()).or_default().push(idx);
}

fn lower_func(
    name: &str,
    params: &[Param],
    body: &Block,
    is_test: bool,
    errors: &ErrorCodeTable,
    types: &TypeTable,
    funcs: &HashSet<String>,
    globals: &HashSet<String>,
    closures: &mut Vec<IrFunc>,
) -> Result<IrFunc, IrError> {
    let mut ctx = LowerCtx::new(errors.clone(), types.clone(), funcs, globals, closures);
    ctx.push_scope();
    // 参数槽（声明序，从 0 开始）
    let param_slots: Vec<usize> = params.iter().map(|_| ctx.alloc_slot()).collect();
    // 局部变量槽（变量名 → 槽）
    for (p, slot) in params.iter().zip(param_slots.iter()) {
        ctx.bind(&p.name, *slot);
    }
    for stmt in &body.stmts {
        ctx.lower_stmt(stmt);
    }
    ctx.pop_scope();
    // 子集外特性 → 硬错误（不静默丢弃语句；降级已推进完毕以保持槽号连续）
    if let Some(e) = ctx.err {
        return Err(e);
    }
    // 隐式末尾 return void
    ctx.insts.push(IrInst::ReturnVoid);
    let n_slots = ctx.next_slot;
    // 重载/可选参数元数据（Phase 4）：类型 + 尾部默认常量（ADR-0009）
    let param_ty: Vec<Type> = params.iter().map(|p| p.ty.clone()).collect();
    let param_defaults: Vec<bool> = params.iter().map(|p| p.default.is_some()).collect();
    let defaults: Vec<Option<IrConst>> = params
        .iter()
        .map(|p| p.default.as_ref().and_then(|d| lower_default_const(d, errors)))
        .collect();
    Ok(IrFunc {
        name: name.to_string(),
        params: param_slots,
        param_ty,
        param_defaults,
        defaults,
        n_slots,
        body: ctx.insts,
        is_test,
    })
}

/// Phase 5：合成 `@__init__` 函数——声明序初始化全部 global/const（`StoreGlobal`）。
/// 错误集别名（`const X = error{...}` / `A || B`）为类型级构造，跳过。
/// 返回 None 表示无值全局（无需启动初始化）。
fn lower_init_func(
    program: &Program,
    errors: &ErrorCodeTable,
    types: &TypeTable,
    funcs: &HashSet<String>,
    globals: &HashSet<String>,
    closures: &mut Vec<IrFunc>,
) -> Result<Option<IrFunc>, IrError> {
    if globals.is_empty() {
        return Ok(None);
    }
    let mut ctx = LowerCtx::new(errors.clone(), types.clone(), funcs, globals, closures);
    ctx.push_scope();
    for d in &program.decls {
        lower_global_decl(d, &mut ctx)?;
    }
    ctx.pop_scope();
    if let Some(e) = ctx.err {
        return Err(e);
    }
    ctx.insts.push(IrInst::ReturnVoid);
    let n_slots = ctx.next_slot;
    Ok(Some(IrFunc {
        name: "@__init__".to_string(),
        params: vec![],
        param_ty: vec![],
        param_defaults: vec![],
        defaults: vec![],
        n_slots,
        body: ctx.insts,
        is_test: false,
    }))
}

/// 递归降级 global/const 声明初始化（namespace 内扁平化，对齐 oracle `exec_decl_top`）。
fn lower_global_decl(d: &Decl, ctx: &mut LowerCtx) -> Result<(), IrError> {
    match d {
        Decl::Global { name, init, .. } => {
            let t = match init {
                Some(e) => ctx.lower_expr(e),
                None => {
                    let t = ctx.alloc_slot();
                    ctx.push(IrInst::Const {
                        temp: t,
                        val: IrConst::Void,
                    });
                    t
                }
            };
            ctx.push(IrInst::StoreGlobal {
                name: name.clone(),
                value: t,
            });
        }
        Decl::Const { name, init, ty, .. } => {
            // 错误集别名跳过（类型级）
            if let Some(Type::Named(tn, _)) = ty {
                if tn.starts_with("error_set:") {
                    return Ok(());
                }
            }
            let t = ctx.lower_expr(init);
            ctx.push(IrInst::StoreGlobal {
                name: name.clone(),
                value: t,
            });
        }
        Decl::Namespace { decls, .. } => {
            for inner in decls {
                lower_global_decl(inner, ctx)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// 可选参数默认值折叠为编译期常量（ADR-0009：可选参数 = 尾部 + 编译期常量默认值）。
/// 字面量/枚举常量/错误字面量 → `IrConst`；其余（依赖参数/非常量表达式）→ None
/// （运行时按「未提供」处理——pick_func 默认回退依赖 param_defaults，padding 依赖此常量）。
fn lower_default_const(e: &Expr, errors: &ErrorCodeTable) -> Option<IrConst> {
    match e {
        Expr::IntLit { text, .. } => Some(IrConst::Int(parse_int_lit(text))),
        Expr::FloatLit { text, .. } => Some(IrConst::Float(text.parse().unwrap_or(0.0))),
        Expr::BoolLit(b, _) => Some(IrConst::Bool(*b)),
        Expr::StrLit { value, .. } => Some(IrConst::Str(value.clone())),
        Expr::CharLit(c, _) => Some(IrConst::Int(*c as i128)),
        Expr::NullLit(_) => Some(IrConst::Null),
        Expr::VoidLit(_) => Some(IrConst::Void),
        Expr::ErrorLit(name, _) => Some(IrConst::Err {
            name: name.clone(),
            code: errors.code_of(name).unwrap_or(0),
        }),
        _ => None,
    }
}

struct LowerCtx<'a> {
    /// 作用域栈：名字 → 槽（词法作用域，块退出恢复外层绑定——对齐解释器作用域）
    scopes: Vec<HashMap<String, usize>>,
    next_slot: usize,
    insts: Vec<IrInst>,
    next_label: usize,
    /// 编译期错误码表（error.Name → 码，M2.6；Err 常量携带码）
    errors: ErrorCodeTable,
    /// 类型元数据（Phase 2：NamedLit/Dot 判型 class vs enum vs namespace）
    types: TypeTable,
    /// 已知函数名集合（Phase 4）：未解析 Ident → 函数引用（FnRef）/ 静态方法调用判定
    funcs: &'a HashSet<String>,
    /// 已知全局/常量名集合（Phase 5）：未解析 Ident → LoadGlobal；赋值目标 → StoreGlobal
    globals: &'a HashSet<String>,
    /// 循环栈（Phase 3）：无标签 break/continue 定位（对齐 oracle 单级跳出；标签 → Phase 6）
    loops: Vec<LoopCtx>,
    /// 已登记 defers（Phase 6，按登记序累积；**不弹**——作用域标记划分发射范围）。
    /// 退出点（作用域自然结束 / return / break / continue / try 错误返回）按 LIFO
    /// 发射内联体（守卫 JumpIfNotDefer + PopDefer）。作用域弹栈时截断到标记。
    defers: Vec<DeferRecord>,
    /// 与 `scopes` 平行的 defer 标记：进入作用域时的 `defers.len()`。`pop_scope` 发射
    /// 从当前长度下到标记的 defers（仅该作用域登记的部分），再截断——对齐 oracle
    /// 每作用域独立 defer 列表、弹栈即运行。
    defer_markers: Vec<usize>,
    /// defer 体缓冲：非 None 时 `push`/`label` 路由到缓冲（defer 体单独降级，
    /// 退出点整体发射）。defer 语句降级完即复位为外层缓冲。
    pending: Option<Vec<IrInst>>,
    /// 下一个 defer id（函数级单调递增；每个 defer 语句唯一）。
    next_defer_id: usize,
    /// 首个子集外特性错误（降级失败信号；降级继续推进以收集更多槽号，但最终报错）
    err: Option<IrError>,
    /// 闭包函数共享缓冲（Phase 4，模块级）：`MakeClosure.func` = 追加前长度
    /// （同一 LowerCtx 内嵌套闭包也追加至此 → 全局索引稳定，无需事后重定位）
    closures: &'a mut Vec<IrFunc>,
}

impl<'a> LowerCtx<'a> {
    fn new(
        errors: ErrorCodeTable,
        types: TypeTable,
        funcs: &'a HashSet<String>,
        globals: &'a HashSet<String>,
        closures: &'a mut Vec<IrFunc>,
    ) -> Self {
        LowerCtx {
            scopes: Vec::new(),
            next_slot: 0,
            insts: Vec::new(),
            next_label: 0,
            errors,
            types,
            funcs,
            globals,
            loops: Vec::new(),
            defers: Vec::new(),
            defer_markers: Vec::new(),
            pending: None,
            next_defer_id: 0,
            err: None,
            closures,
        }
    }
}

/// 循环上下文（Phase 3）：break 目标 + continue 目标；
/// Phase 6 增补：循环标签 + 进入时 defer 深度（退出该循环须排空其体内 defers）。
struct LoopCtx {
    break_label: usize,
    continue_label: usize,
    /// 循环标签（`:label while` / `:label for`），供 `break :label` / `continue :label` 定位
    label: Option<String>,
    /// 进入循环时已登记 defers 数：break/continue 排空 [depth..len)（含嵌套作用域，
    /// 但不含循环外层的 defers——外层退出点另行处理）。
    defer_depth_at_entry: usize,
}

/// 一个 defer/errdefer 语句的编译期记录：id（PushDefer/PopDefer/守卫共用）+ 内联体 + 是否 errdefer。
/// 体已确保无控制流指令（带 label 的跳转会因重复发射而冲突——降级期硬错误）。
#[derive(Clone)]
struct DeferRecord {
    id: usize,
    body: Vec<IrInst>,
    errdefer: bool,
}

/// 退出点的 errdefer 策略（对齐 oracle `run_defers(err_path)`）：
/// - `Never`：正常路径（作用域自然结束 / break / continue）——errdefer 不运行，裸 PopDefer 清理。
/// - `Always`：错误路径（`try` 错误返回）——全部 defers（含 errdefer）运行。
/// - `Value(t)`：`return e` 按运行期值判定——错误值走 `Always` 分支，否则 `Never`。
#[derive(Clone, Copy)]
enum ErrPath {
    Never,
    Always,
    Value(usize),
}

impl<'a> LowerCtx<'a> {
    fn alloc_slot(&mut self) -> usize {
        let s = self.next_slot;
        self.next_slot += 1;
        s
    }
    fn new_label(&mut self) -> usize {
        let l = self.next_label;
        self.next_label += 1;
        l
    }
    fn push(&mut self, inst: IrInst) {
        if let Some(buf) = &mut self.pending {
            buf.push(inst);
        } else {
            self.insts.push(inst);
        }
    }
    fn label(&mut self, id: usize) {
        self.push(IrInst::Label { id });
    }
    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
        self.defer_markers.push(self.defers.len());
    }
    /// 弹作用域：先发射本作用域 defers（正常路径，仅非 errdefer；守卫 + 内联体），
    /// 截断到进入时标记，再弹作用域。对齐 oracle `pop_scope`——先跑 defers 再弹
    /// （同作用域局部变量仍可解析）。
    fn pop_scope(&mut self) {
        let marker = self
            .defer_markers
            .pop()
            .expect("defer marker underflow (push_scope/pop_scope 不配对)");
        self.emit_defers(marker, ErrPath::Never);
        self.defers.truncate(marker);
        self.scopes.pop();
    }
    /// 退出点发射 defers（LIFO：从最新登记下到 `depth`；`depth` 以下归外层退出点）。
    /// 守卫（JumpIfNotDefer）跳过未登记/已运行路径——分支 DAG 下同一退出点代码
    /// 可被多条路径到达，运行时活跃计数判定「本路径是否待运行」。
    fn emit_defers(&mut self, depth: usize, err_path: ErrPath) {
        let n = self.defers.len();
        match err_path {
            ErrPath::Never => {
                for i in (depth..n).rev() {
                    let rec = self.defers[i].clone();
                    if rec.errdefer {
                        // 正常路径：errdefer 不运行，仅清理活跃计数（防跨路径泄漏）
                        self.push(IrInst::PopDefer { id: rec.id });
                    } else {
                        self.emit_defer_guarded(&rec);
                    }
                }
            }
            ErrPath::Always => {
                for i in (depth..n).rev() {
                    let rec = self.defers[i].clone();
                    self.emit_defer_guarded(&rec);
                }
            }
            ErrPath::Value(v) => {
                // `return e`：按运行期值分派——错误 → 全 defers；否则仅非 errdefer
                let l_err = self.new_label();
                let l_done = self.new_label();
                self.push(IrInst::JumpIfErr { temp: v, label: l_err });
                for i in (depth..n).rev() {
                    let rec = self.defers[i].clone();
                    if rec.errdefer {
                        self.push(IrInst::PopDefer { id: rec.id });
                    } else {
                        self.emit_defer_guarded(&rec);
                    }
                }
                self.push(IrInst::Jump { label: l_done });
                self.label(l_err);
                for i in (depth..n).rev() {
                    let rec = self.defers[i].clone();
                    self.emit_defer_guarded(&rec);
                }
                self.label(l_done);
            }
        }
    }
    /// 单条 defer 守卫 + 内联体 + 排空。体无控制流（降级期硬错误保证），可多次安全发射。
    fn emit_defer_guarded(&mut self, rec: &DeferRecord) {
        let l_skip = self.new_label();
        self.push(IrInst::JumpIfNotDefer {
            id: rec.id,
            label: l_skip,
        });
        for inst in &rec.body {
            self.push(inst.clone());
        }
        self.push(IrInst::PopDefer { id: rec.id });
        self.label(l_skip);
    }
    /// 当前作用域绑定（遮蔽时分配新槽，旧绑定保留在外层）
    fn bind(&mut self, name: &str, slot: usize) {
        self.scopes
            .last_mut()
            .expect("bind outside any scope")
            .insert(name.to_string(), slot);
    }
    fn resolve(&self, name: &str) -> Option<usize> {
        self.scopes.iter().rev().find_map(|m| m.get(name).copied())
    }
    /// 记录「原生/IR 后端不支持」的硬错误（首个生效，避免报错刷屏）
    fn fail(&mut self, what: &str, span: &Span) {
        if self.err.is_none() {
            self.err = Some(unsupported_ir_err(what, span));
        }
    }
    /// 子集外表达式：记录硬错误 + 返回 void 占位（保持槽号连续）
    fn fail_void(&mut self, t: usize, what: &str, span: &Span) {
        self.fail(what, span);
        self.push(IrInst::Const {
            temp: t,
            val: IrConst::Void,
        });
    }
    /// 块语句序列（推/弹作用域）；空块安全
    fn lower_block(&mut self, b: &Block) {
        self.push_scope();
        for stmt in &b.stmts {
            self.lower_stmt(stmt);
        }
        self.pop_scope();
    }

    /// 表达式 → 临时槽号
    fn lower_expr(&mut self, e: &Expr) -> usize {
        let t = self.alloc_slot();
        match e {
            Expr::IntLit { text, .. } => {
                let v = parse_int_lit(text);
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Int(v),
                });
            }
            Expr::FloatLit { text, .. } => {
                let v: f64 = text.parse().unwrap_or(0.0);
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Float(v),
                });
            }
            Expr::BoolLit(b, _) => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Bool(*b),
                });
            }
            Expr::StrLit { value, .. } => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Str(value.clone()),
                });
            }
            Expr::CharLit(c, _) => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Int(*c as i128),
                });
            }
            Expr::NullLit(_) => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Null,
                });
            }
            Expr::VoidLit(_) => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Void,
                });
            }
            Expr::ErrorLit(name, _) => {
                let code = self.errors.code_of(name).unwrap_or(0);
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Err {
                        name: name.clone(),
                        code,
                    },
                });
            }
            Expr::Ident(name, span) => match self.resolve(name) {
                Some(slot) => self.push(IrInst::Load { temp: t, slot }),
                // 函数名作为值（FnRef：apply(square, 5) / var f = square）——对齐 oracle
                // interp.rs:1530-1535
                None if self.funcs.contains(name) => {
                    self.push(IrInst::FnRef { temp: t, name: name.clone() });
                }
                // 全局/常量引用（Phase 5）：`LoadGlobal`——cell 由 IrRuntime::init 预分配
                None if self.globals.contains(name) => {
                    self.push(IrInst::LoadGlobal {
                        temp: t,
                        name: name.clone(),
                    });
                }
                None => self.fail_void(t, "未知标识符", span),
            },
            Expr::Binary(op, l, r, _span) => {
                let a = self.lower_expr(l);
                match op {
                    // 短路 and/or（与运行时 eval_binary 一致）
                    BinOp::And => {
                        let l_false = self.new_label();
                        let done = self.new_label();
                        self.push(IrInst::JumpIfNot {
                            temp: a,
                            label: l_false,
                        });
                        let b = self.lower_expr(r);
                        self.push(IrInst::Load { temp: t, slot: b });
                        self.push(IrInst::Jump { label: done });
                        self.label(l_false);
                        self.push(IrInst::Const {
                            temp: t,
                            val: IrConst::Bool(false),
                        });
                        self.label(done);
                    }
                    BinOp::Or => {
                        let l_true = self.new_label();
                        let done = self.new_label();
                        self.push(IrInst::JumpIf {
                            temp: a,
                            label: l_true,
                        });
                        let b = self.lower_expr(r);
                        self.push(IrInst::Load { temp: t, slot: b });
                        self.push(IrInst::Jump { label: done });
                        self.label(l_true);
                        self.push(IrInst::Const {
                            temp: t,
                            val: IrConst::Bool(true),
                        });
                        self.label(done);
                    }
                    // 区间糖：`[lo, hi)` 整数区间数组（对齐 oracle `BinOp::Range`）
                    BinOp::Range => {
                        let b = self.lower_expr(r);
                        self.push(IrInst::MakeRange {
                            temp: t,
                            lo: a,
                            hi: b,
                        });
                    }
                    _ => {
                        let b = self.lower_expr(r);
                        self.push(IrInst::Bin {
                            op: to_ir_binop(*op),
                            temp: t,
                            a,
                            b,
                        });
                    }
                }
            }
            Expr::Unary(op, inner, _) => {
                let a = self.lower_expr(inner);
                let un = match op {
                    UnaryOp::Neg => IrUnOp::Neg,
                    UnaryOp::Not => IrUnOp::Not,
                    UnaryOp::BitNot => IrUnOp::BitNot,
                };
                self.push(IrInst::Un { op: un, temp: t, a });
            }
            Expr::Try(inner, _) => {
                // try：错误值 → 从当前函数返回（值通道）。错误路径为运行期「返回错误」，
                // errdefer 须触发（对齐 oracle `is_err_path(Err(signal(Flow::Return(err))))`）——
                // 故用 ErrPath::Always 排空函数级 defers（含 errdefer）。
                let a = self.lower_expr(inner);
                let l_ret = self.new_label();
                let done = self.new_label();
                self.push(IrInst::JumpIfErr {
                    temp: a,
                    label: l_ret,
                });
                self.push(IrInst::Load { temp: t, slot: a });
                self.push(IrInst::Jump { label: done });
                self.label(l_ret);
                self.emit_defers(0, ErrPath::Always);
                self.push(IrInst::Return { temp: a });
                self.label(done);
            }
            Expr::Catch(inner, kind, _) => {
                // catch：错误值 → 处理分支；结果统一到目标槽
                let a = self.lower_expr(inner);
                let l_catch = self.new_label();
                let done = self.new_label();
                let res_slot = self.alloc_slot();
                self.push(IrInst::JumpIfErr {
                    temp: a,
                    label: l_catch,
                });
                self.push(IrInst::Store {
                    slot: res_slot,
                    temp: a,
                });
                self.push(IrInst::Jump { label: done });
                self.label(l_catch);
                match kind.as_ref() {
                    CatchKind::Default(d) => {
                        let h = self.lower_expr(d);
                        self.push(IrInst::Store {
                            slot: res_slot,
                            temp: h,
                        });
                    }
                    CatchKind::Bind { name: bname, body } => {
                        let err_slot = self.alloc_slot();
                        self.push(IrInst::Store {
                            slot: err_slot,
                            temp: a,
                        });
                        self.push_scope();
                        self.bind(bname, err_slot);
                        // 块值：最后语句为表达式时取其值（只求值一次——对齐解释器 exec_block_inner）；
                        // 其余（赋值/return/块等作值）→ void 占位
                        let last_is_value = matches!(body.stmts.last(), Some(Stmt::Expr(_)));
                        let n = body.stmts.len() - usize::from(last_is_value);
                        for stmt in &body.stmts[..n] {
                            self.lower_stmt(stmt);
                        }
                        if last_is_value {
                            if let Some(Stmt::Expr(last)) = body.stmts.last() {
                                let h = self.lower_expr(last);
                                self.push(IrInst::Store {
                                    slot: res_slot,
                                    temp: h,
                                });
                            }
                        } else {
                            let h = self.alloc_slot();
                            self.push(IrInst::Const {
                                temp: h,
                                val: IrConst::Void,
                            });
                            self.push(IrInst::Store {
                                slot: res_slot,
                                temp: h,
                            });
                        }
                        self.pop_scope();
                    }
                }
                self.label(done);
                self.push(IrInst::Load {
                    temp: t,
                    slot: res_slot,
                });
            }
            Expr::Call { callee, args, span: _ } => {
                // `@` 内建的类型位置参数（@sizeOf(i32) 等）在调用点编码为 `Const Str(type_name)`，
                // 运行时按名解析——对齐 oracle 从 `Expr::Ident` 读类型名。
                // 限定名调用（alloc.init(ABC) 等）展平为 `"alloc.init"` 后同样适用。
                let callee_name = match callee.as_ref() {
                    Expr::Ident(n, _) => Some(n.clone()),
                    Expr::Dot { base, field, .. } | Expr::Field { base, field, .. } => {
                        let mut parts = vec![field.clone()];
                        let mut b = base.as_ref();
                        while let Expr::Dot {
                            base: b2,
                            field: f2,
                            ..
                        }
                        | Expr::Field {
                            base: b2,
                            field: f2,
                            ..
                        } = b
                        {
                            parts.push(f2.clone());
                            b = b2.as_ref();
                        }
                        if let Expr::Ident(ns, _) = b {
                            parts.push(ns.clone());
                        }
                        parts.reverse();
                        Some(parts.join("."))
                    }
                    _ => None,
                };
                let arg_ts: Vec<usize> = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        if let Some(cn) = &callee_name {
                            if is_type_arg_pos(cn, i) {
                                let name = match a {
                                    Expr::Ident(n, _) => Some(n.clone()),
                                    Expr::StrLit { value, .. } => Some(value.clone()),
                                    _ => None,
                                };
                                if let Some(n) = name {
                                    let at = self.alloc_slot();
                                    self.push(IrInst::Const {
                                        temp: at,
                                        val: IrConst::Str(n),
                                    });
                                    return at;
                                }
                            }
                        }
                        self.lower_expr(a)
                    })
                    .collect();
                match callee.as_ref() {
                    Expr::Ident(name, _) => {
                        // `@`/断言恒为内建；自由内建名被用户函数遮蔽时走用户函数
                        // （对齐 oracle eval_call：先查用户函数，后回退内建）。
                        let builtin = name.starts_with('@')
                            || is_assert_builtin(name)
                            || (is_free_builtin(name) && !self.funcs.contains(name));
                        if builtin {
                            self.push(IrInst::CallBuiltin {
                                name: name.clone(),
                                args: arg_ts,
                                temp: t,
                            });
                        } else if let Some(_slot) = self.resolve(name) {
                            // 局部变量作为调用目标（存函数/闭包值）→ 间接调用
                            let cal = self.lower_expr(callee);
                            self.push(IrInst::CallIndirect {
                                temp: t,
                                callee: cal,
                                args: arg_ts,
                            });
                        } else {
                            // 全局/namespace 函数静态调用（含重载，按名分派）
                            self.push(IrInst::Call {
                                name: name.clone(),
                                args: arg_ts,
                                temp: t,
                            });
                        }
                    }
                    Expr::Dot { base, field, .. } | Expr::Field { base, field, .. } => {
                        // 展平限定名链：io.net.double → "io.net.double"
                        // （多级限定名经后缀二次处理后外层为 Field 形态）
                        let mut parts = vec![field.clone()];
                        let mut b = base.as_ref();
                        while let Expr::Dot {
                            base: b2,
                            field: f2,
                            ..
                        }
                        | Expr::Field {
                            base: b2,
                            field: f2,
                            ..
                        } = b
                        {
                            parts.push(f2.clone());
                            b = b2.as_ref();
                        }
                        if let Expr::Ident(ns, _) = b {
                            parts.push(ns.clone());
                            parts.reverse();
                            let qn = parts.join(".");
                            // 已知静态函数（namespace 函数 / `Type.method` 静态调用）→ 直接调用；
                            // `Rect.area(&rect)` 静态调用显式传 self，无注入（对齐 oracle eval_call）
                            if self.funcs.contains(&qn) {
                                self.push(IrInst::Call {
                                    name: qn,
                                    args: arg_ts,
                                    temp: t,
                                });
                                return t;
                            }
                            // 根标识符不解析为局部变量 → 未注册限定名（io.print 等内建/未声明
                            // 函数）：静态名调用 → 运行时 NoFunction（含切片外提示）。保持
                            // Phase 4 前行为；解析为局部时才是实例方法接收者。
                            if self.resolve(ns).is_none() {
                                self.push(IrInst::Call {
                                    name: qn,
                                    args: arg_ts,
                                    temp: t,
                                });
                                return t;
                            }
                        }
                        // 实例方法调用：base 求值 + 运行时按类型名分派 `{Type}.{method}`，
                        // self 注入首参（对齐 oracle interp.rs:2405-2421）
                        let base_t = self.lower_expr(base);
                        self.push(IrInst::CallMethod {
                            temp: t,
                            base: base_t,
                            method: field.clone(),
                            args: arg_ts,
                        });
                    }
                    _ => {
                        // 其它调用形态（闭包字面量立即调用 `(|v| v+a)(5)` / 复合目标）：
                        // 求值 callee → 运行时 Fn/Closure 分派（对齐 oracle eval_call `_` 臂）
                        let cal = self.lower_expr(callee);
                        self.push(IrInst::CallIndirect {
                            temp: t,
                            callee: cal,
                            args: arg_ts,
                        });
                    }
                }
            }
            Expr::IfExpr {
                cond,
                capture,
                then_e,
                else_e,
                ..
            } => {
                // if 表达式：两分支结果统一到 res_slot（对齐解释器 IfExpr）
                let c = self.lower_expr(cond);
                let l_else = self.new_label();
                let l_done = self.new_label();
                let res_slot = self.alloc_slot();
                match capture.as_ref() {
                    Some((_, name)) => {
                        // optional 捕获：null → else；否则绑定 cond 值
                        self.push(IrInst::JumpIfNull {
                            temp: c,
                            label: l_else,
                        });
                        self.push_scope();
                        self.bind(name, c);
                        let tv = self.lower_expr(then_e);
                        self.pop_scope();
                        self.push(IrInst::Store {
                            slot: res_slot,
                            temp: tv,
                        });
                    }
                    None => {
                        self.push(IrInst::JumpIfNot {
                            temp: c,
                            label: l_else,
                        });
                        let tv = self.lower_expr(then_e);
                        self.push(IrInst::Store {
                            slot: res_slot,
                            temp: tv,
                        });
                    }
                }
                self.push(IrInst::Jump { label: l_done });
                self.label(l_else);
                let ev = self.lower_expr(else_e);
                self.push(IrInst::Store {
                    slot: res_slot,
                    temp: ev,
                });
                self.label(l_done);
                self.push(IrInst::Load {
                    temp: t,
                    slot: res_slot,
                });
            }
            Expr::Orelse(l, r, _) => {
                // orelse：null → 默认值
                let a = self.lower_expr(l);
                let l_null = self.new_label();
                let done = self.new_label();
                let res_slot = self.alloc_slot();
                self.push(IrInst::JumpIfNull {
                    temp: a,
                    label: l_null,
                });
                self.push(IrInst::Store {
                    slot: res_slot,
                    temp: a,
                });
                self.push(IrInst::Jump { label: done });
                self.label(l_null);
                let d = self.lower_expr(r);
                self.push(IrInst::Store {
                    slot: res_slot,
                    temp: d,
                });
                self.label(done);
                self.push(IrInst::Load {
                    temp: t,
                    slot: res_slot,
                });
            }
            Expr::Assign {
                target, op, value, span,
            } => match self.lower_assign(*op, target, value) {
                // 赋值表达式（while 续步 i += 1 等）：值 = 新值（对齐 eval_assign）
                Some(stored) => self.push(IrInst::Load {
                    temp: t,
                    slot: stored,
                }),
                // 目标不是局部变量（字段/索引/解构等）→ 子集外硬错误
                None => self.fail_void(t, "字段/索引/解构赋值", span),
            },
            // ---- Phase 2 聚合 ----
            // 数组/元组字面量：运行时等价（Arr），逐元素求值 + 独立共享 cell
            Expr::ArrayLit(items, _) | Expr::TupleLit(items, _) => {
                let item_ts: Vec<usize> = items.iter().map(|e| self.lower_expr(e)).collect();
                self.push(IrInst::MakeArr { temp: t, items: item_ts });
            }
            Expr::NamedLit { ty, fields, span } => {
                // struct 字面量 → MakeClass；枚举字面量（恰一个变体）→ MakeEnum（对齐 oracle）
                if self.types.classes.contains_key(ty) {
                    let f: Vec<(String, usize)> = fields
                        .iter()
                        .map(|(k, v)| (k.clone(), self.lower_expr(v)))
                        .collect();
                    self.push(IrInst::MakeClass { temp: t, ty: ty.clone(), fields: f });
                } else if self.types.enums.contains_key(ty) {
                    if fields.len() != 1 {
                        self.fail_void(t, "多字段枚举字面量（应为单变体）", span);
                        return t;
                    }
                    let (variant, payload) = &fields[0];
                    let pv = self.lower_expr(payload);
                    self.push(IrInst::MakeEnum {
                        temp: t,
                        name: ty.clone(),
                        variant: variant.clone(),
                        payload: Some(pv),
                    });
                } else {
                    self.fail_void(t, &format!("未知类型 `{ty}` 的字面量构造"), span);
                }
            }
            Expr::Dot { base, field, span } => {
                // 类型名（enum/class）限定 → 枚举常量（对齐 oracle：不做变体验证，全类型名同权）
                if let Expr::Ident(bname, _) = base.as_ref() {
                    // ExitType 内建枚举特判（对齐 oracle eval_dot）
                    if bname == "ExitType" {
                        self.push(IrInst::MakeEnum {
                            temp: t,
                            name: "ExitType".into(),
                            variant: field.clone(),
                            payload: None,
                        });
                        return t;
                    }
                    if self.types.enums.contains_key(bname) || self.types.classes.contains_key(bname)
                    {
                        self.push(IrInst::MakeEnum {
                            temp: t,
                            name: bname.clone(),
                            variant: field.clone(),
                            payload: None,
                        });
                        return t;
                    }
                    // namespace 限定的值位置（非调用位）：oracle 运行时 UndefinedName
                    if self.types.namespaces.contains(bname) {
                        self.fail_void(t, "namespace 限定的值（非调用位）", span);
                        return t;
                    }
                }
                // 推断枚举字面量 `.variant`（base=VoidLit）：L1 兜底名 __inferred__（对齐 oracle）
                if matches!(base.as_ref(), Expr::VoidLit(_)) {
                    self.push(IrInst::MakeEnum {
                        temp: t,
                        name: "__inferred__".into(),
                        variant: field.clone(),
                        payload: None,
                    });
                    return t;
                }
                let b = self.lower_expr(base);
                self.push(IrInst::Field { temp: t, base: b, field: field.clone() });
            }
            Expr::Field { base, field, .. } => {
                let b = self.lower_expr(base);
                self.push(IrInst::Field { temp: t, base: b, field: field.clone() });
            }
            Expr::Index { base, indices, span } => {
                let b = self.lower_expr(base);
                if indices.len() == 1 {
                    if let Expr::Binary(BinOp::Range, lo, hi, _) = &indices[0] {
                        // 切片 `base[lo..hi]`（hi 可为 `__end__` 开区间哨兵）
                        let lo_t = self.lower_expr(lo);
                        let hi_t = self.lower_slice_end(hi);
                        self.push(IrInst::SliceOf { temp: t, base: b, lo: lo_t, hi: hi_t });
                        return t;
                    }
                    let idx = self.lower_expr(&indices[0]);
                    self.push(IrInst::Index { temp: t, base: b, index: idx });
                } else {
                    self.fail_void(t, "多索引访问（Table 行/列）", span);
                }
            }
            // 指针（Phase 1）：`p.*` 解引用
            Expr::Deref(inner, _) => {
                let a = self.lower_expr(inner);
                self.push(IrInst::Deref { temp: t, a });
            }
            // `&x`/`&mut x` 取址：变量 → AddrSlot 别名（写穿共享 cell）；
            // 非 lvalue → AddrValue 快照（对齐 tree-walking `&expr` 兜底分支）
            Expr::AddrOf(target, _, span) => match target.as_ref() {
                Expr::Ident(name, _) => match self.resolve(name) {
                    Some(slot) => self.push(IrInst::AddrSlot { temp: t, slot }),
                    // 全局/常量（Phase 5）：`&global` 别名 cell——`IrRuntime::init` 已
                    // 预分配 cell，`Deref`/`StorePtr` 写穿回全局（对齐 oracle lookup→globals）
                    None if self.globals.contains(name) => {
                        self.push(IrInst::GlobalAddr { temp: t, name: name.clone() });
                    }
                    None => self.fail_void(t, "未知标识符取址", span),
                },
                _ => {
                    let v = self.lower_expr(target);
                    self.push(IrInst::AddrValue { temp: t, value: v });
                }
            },
            Expr::Unwrap(inner, _) => {
                let a = self.lower_expr(inner);
                self.push(IrInst::Unwrap { temp: t, a });
            }
            Expr::SwitchExpr { subject, arms, span } => {
                let has_else = arms
                    .iter()
                    .any(|a| a.patterns.iter().any(|p| matches!(p, SwitchPattern::Else)));
                self.lower_switch_inner(subject, arms, has_else, span, Some(t));
            }
            // 块表达式：值 = 最后语句（若为 Expr）的值；否则 void（对齐 exec_block_inner）
            Expr::Block(b, _) => {
                self.push_scope();
                let n = b.stmts.len();
                let last_is_value = matches!(b.stmts.last(), Some(Stmt::Expr(_)));
                let m = n - usize::from(last_is_value);
                for stmt in &b.stmts[..m] {
                    self.lower_stmt(stmt);
                }
                if last_is_value {
                    if let Some(Stmt::Expr(e)) = b.stmts.last() {
                        let v = self.lower_expr(e);
                        self.push(IrInst::Load { temp: t, slot: v });
                    }
                } else {
                    self.push(IrInst::Const { temp: t, val: IrConst::Void });
                }
                self.pop_scope();
            }
            Expr::FnRef(name, _span) => {
                self.push(IrInst::FnRef { temp: t, name: name.clone() });
            }
            // 元组解构：源求值 + Destructure（运行时 arity 检查 + 逐元素克隆绑定）
            Expr::TupleDestructure(names, e, _) => {
                let v = self.lower_expr(e);
                let mut slots = Vec::with_capacity(names.len());
                for n in names {
                    if n == "_" {
                        slots.push(None);
                    } else {
                        let slot = self.alloc_slot();
                        self.bind(n, slot);
                        slots.push(Some(slot));
                    }
                }
                self.push(IrInst::Destructure { value: v, slots });
                self.push(IrInst::Const { temp: t, val: IrConst::Void });
            }
            Expr::Move(inner, _) => {
                let a = self.lower_expr(inner);
                self.push(IrInst::Move { temp: t, a });
            }
            Expr::Closure {
                params,
                body,
                is_move,
                is_mut,
                span,
            } => {
                let ct = self.lower_closure(params, body, *is_mut, *is_move, span);
                self.push(IrInst::Load { temp: t, slot: ct });
            }
        }
        t
    }

    /// 闭包降级（对齐 oracle `Expr::Closure` interp.rs:1931-1963 + `capture_env`）：
    /// **自由变量精确分析**（Phase 8，`closure_free_vars`）——只捕获 body 实际引用、
    /// 未被体内绑定遮蔽的外部变量（`(名字, 槽号)`，最近作用域优先——遮蔽解析正确），
    /// 生成独立闭包函数（前 n_caps 个参数 = 捕获参数，之后 = 显式参数），
    /// 返回闭包值临时槽（MakeClosure 结果）。move → 运行时深拷贝捕获 cell。
    /// 块值语义（对齐 `exec_block_inner`）：末语句为表达式 → 作为返回值（单表达式
    /// 闭包 `|v| v+a` 即此形态）；否则末尾 ReturnVoid。
    fn lower_closure(
        &mut self,
        params: &[String],
        body: &Block,
        is_mut: bool,
        is_move: bool,
        _span: &Span,
    ) -> usize {
        // 捕获集合：自由变量精确集 ∩ 当前作用域链绑定（名字, 槽号），
        // 最近作用域优先（遮蔽正确）。自由集外的名字不捕获（闭包不可见）。
        let free = closure_free_vars(params, body);
        let mut captures: Vec<(String, usize)> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for scope in self.scopes.iter().rev() {
            for (name, slot) in scope {
                if free.contains(name) && seen.insert(name.clone()) {
                    captures.push((name.clone(), *slot));
                }
            }
        }
        let n_caps = captures.len();
        let temp = self.alloc_slot();
        // 闭包体用独立 LowerCtx（共享闭包缓冲、错误码表、类型表、函数名/全局名集合）
        let errors = self.errors.clone();
        let types = self.types.clone();
        let funcs = self.funcs;
        let globals = self.globals;
        let closures = &mut *self.closures;
        let mut ctx = LowerCtx::new(errors, types, funcs, globals, closures);
        ctx.push_scope();
        // 捕获参数槽（0..n_caps）与显式参数槽（n_caps..）
        for _ in 0..n_caps {
            ctx.alloc_slot();
        }
        for (i, (name, _)) in captures.iter().enumerate() {
            ctx.bind(name, i);
        }
        let param_slots: Vec<usize> = params.iter().map(|_| ctx.alloc_slot()).collect();
        for (p, slot) in params.iter().zip(param_slots.iter()) {
            ctx.bind(p, *slot);
        }
        // 块值语义：末语句为表达式 → 返回值
        let n = body.stmts.len();
        let last_is_value = matches!(body.stmts.last(), Some(Stmt::Expr(_)));
        let m = n - usize::from(last_is_value);
        for stmt in &body.stmts[..m] {
            ctx.lower_stmt(stmt);
        }
        if last_is_value {
            if let Some(Stmt::Expr(e)) = body.stmts.last() {
                let v = ctx.lower_expr(e);
                ctx.insts.push(IrInst::Return { temp: v });
            }
        } else {
            // 末语句非值表达式：上面循环已按 m = n 降级了全部语句，这里只补
            // 尾部 ReturnVoid——**不得**再 lower_stmt(last)（会重复降级末语句，
            // 非 Return 时副作用双重执行）。
            ctx.insts.push(IrInst::ReturnVoid);
        }
        ctx.pop_scope();
        // 提取闭包体结果后释放 ctx（结束对 self.closures 的重借），再操作 self
        let cerr = ctx.err.take();
        let body_insts = ctx.insts;
        let n_slots = ctx.next_slot;
        // 子集外特性传播到外层（首个生效）
        if let Some(e) = cerr {
            if self.err.is_none() {
                self.err = Some(e);
            }
        }
        // 闭包索引须在**体降级完成后**取：嵌套闭包在体降级期间已推入
        // `self.closures`（先内后外），此前快照的索引会指向错误函数。
        let func_idx = self.closures.len();
        let mut fparams: Vec<usize> = (0..n_caps).collect();
        fparams.extend(param_slots.iter().copied());
        self.closures.push(IrFunc {
            name: format!("<closure#{func_idx}>"),
            params: fparams,
            param_ty: Vec::new(),
            param_defaults: Vec::new(),
            defaults: Vec::new(),
            n_slots,
            body: body_insts,
            is_test: false,
        });
        self.push(IrInst::MakeClosure {
            temp,
            func: func_idx,
            captures,
            is_move,
            is_mut,
        });
        temp
    }

    /// 切片上界降级：`__end__` 哨兵 → End 常量；否则普通表达式（对齐 parser open-end 标记）。
    fn lower_slice_end(&mut self, hi: &Expr) -> usize {
        if let Expr::IntLit { text, .. } = hi {
            if text == "__end__" {
                let t = self.alloc_slot();
                self.push(IrInst::Const { temp: t, val: IrConst::End });
                return t;
            }
        }
        self.lower_expr(hi)
    }

    /// 赋值：返回写入目标槽的新值临时槽（目标不在 IR 范围 → None）
    /// 复合赋值 x op= v → x = x op v（对齐解释器 eval_assign）
    fn lower_assign(&mut self, op: AssignOp, target: &Expr, value: &Expr) -> Option<usize> {
        match target {
            Expr::Ident(name, _) => {
                if let Some(slot) = self.resolve(name) {
                    let v = self.lower_expr(value);
                    return Some(match op {
                        AssignOp::Set => {
                            self.push(IrInst::Store { slot, temp: v });
                            v
                        }
                        _ => {
                            let cur = self.alloc_slot();
                            self.push(IrInst::Load { temp: cur, slot });
                            let r = self.alloc_slot();
                            self.push(IrInst::Bin {
                                op: to_assign_binop(op),
                                temp: r,
                                a: cur,
                                b: v,
                            });
                            self.push(IrInst::Store { slot, temp: r });
                            r
                        }
                    });
                }
                // 全局变量赋值（Phase 5）：`StoreGlobal`（复合赋值 = LoadGlobal + Bin + StoreGlobal）
                if self.globals.contains(name) {
                    let v = self.lower_expr(value);
                    return Some(match op {
                        AssignOp::Set => {
                            self.push(IrInst::StoreGlobal {
                                name: name.clone(),
                                value: v,
                            });
                            v
                        }
                        _ => {
                            let cur = self.alloc_slot();
                            self.push(IrInst::LoadGlobal {
                                temp: cur,
                                name: name.clone(),
                            });
                            let r = self.alloc_slot();
                            self.push(IrInst::Bin {
                                op: to_assign_binop(op),
                                temp: r,
                                a: cur,
                                b: v,
                            });
                            self.push(IrInst::StoreGlobal {
                                name: name.clone(),
                                value: r,
                            });
                            r
                        }
                    });
                }
            }
            // 指针写穿（Phase 1）：`p.* = v` / `p.* op= v`（对齐 eval_assign Deref 臂）
            Expr::Deref(inner, _) => {
                let p = self.lower_expr(inner);
                let v = self.lower_expr(value);
                return Some(match op {
                    AssignOp::Set => {
                        self.push(IrInst::StorePtr { target: p, value: v });
                        v
                    }
                    _ => {
                        let cur = self.alloc_slot();
                        self.push(IrInst::Deref { temp: cur, a: p });
                        let r = self.alloc_slot();
                        self.push(IrInst::Bin {
                            op: to_assign_binop(op),
                            temp: r,
                            a: cur,
                            b: v,
                        });
                        self.push(IrInst::StorePtr { target: p, value: r });
                        r
                    }
                });
            }
            // 字段赋值：`p.x = v`（仅 Class 目标；非 Class → TypeError——对齐 eval_assign Field 臂）
            Expr::Field { base, field, .. } => {
                let b = self.lower_expr(base);
                let v = self.lower_expr(value);
                self.push(IrInst::StoreField {
                    base: b,
                    field: field.clone(),
                    value: v,
                });
                return Some(v);
            }
            Expr::Dot { base, field, .. } => {
                // `Type.x = v`：类型名限定的赋值 → 运行时 BadAssign（对齐 eval_assign Dot 臂；
                // base 保证非 Class → StoreField 抛 TypeError，错误名差异不影响 PASS/FAIL）
                if let Expr::Ident(bname, _) = base.as_ref() {
                    if self.types.enums.contains_key(bname)
                        || self.types.classes.contains_key(bname)
                        || self.types.namespaces.contains(bname)
                    {
                        let base_t = self.alloc_slot();
                        self.push(IrInst::Const {
                            temp: base_t,
                            val: IrConst::Void,
                        });
                        let v = self.lower_expr(value);
                        self.push(IrInst::StoreField {
                            base: base_t,
                            field: field.clone(),
                            value: v,
                        });
                        return Some(v);
                    }
                }
                let b = self.lower_expr(base);
                let v = self.lower_expr(value);
                self.push(IrInst::StoreField {
                    base: b,
                    field: field.clone(),
                    value: v,
                });
                return Some(v);
            }
            // 索引赋值：单索引 → StoreIndex（复合 = 读 cur + binop + 写回）；
            // 区间 → StoreSlice（仅 Set；复合/开区间 → 运行时错误）
            Expr::Index { base, indices, span } => {
                if indices.len() != 1 {
                    self.fail("多索引赋值（Table 行/列）", span);
                    return None;
                }
                if let Expr::Binary(BinOp::Range, lo, hi, _) = &indices[0] {
                    // 复合区间赋值：对齐 oracle 仅允许 Set → 运行时 BadAssign
                    if op != AssignOp::Set {
                        let base_t = self.alloc_slot();
                        self.push(IrInst::Const {
                            temp: base_t,
                            val: IrConst::Void,
                        });
                        let v = self.lower_expr(value);
                        self.push(IrInst::StoreField {
                            base: base_t,
                            field: "".to_string(),
                            value: v,
                        });
                        return Some(v);
                    }
                    let b = self.lower_expr(base);
                    let lo_t = self.lower_expr(lo);
                    let hi_t = self.lower_slice_end(hi);
                    let v = self.lower_expr(value);
                    self.push(IrInst::StoreSlice {
                        base: b,
                        lo: lo_t,
                        hi: hi_t,
                        value: v,
                    });
                    return Some(v);
                }
                let b = self.lower_expr(base);
                if op == AssignOp::Set {
                    let idx = self.lower_expr(&indices[0]);
                    let v = self.lower_expr(value);
                    self.push(IrInst::StoreIndex {
                        base: b,
                        index: idx,
                        value: v,
                    });
                    return Some(v);
                }
                // 复合：cur = base[idx]；r = cur op v；base[idx] = r（对齐 oracle 双求值 base）
                let cur = self.lower_expr(target);
                let v = self.lower_expr(value);
                let r = self.alloc_slot();
                self.push(IrInst::Bin {
                    op: to_assign_binop(op),
                    temp: r,
                    a: cur,
                    b: v,
                });
                let idx = self.lower_expr(&indices[0]);
                self.push(IrInst::StoreIndex {
                    base: b,
                    index: idx,
                    value: r,
                });
                return Some(r);
            }
            _ => {}
        }
        None
    }

    fn lower_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::VarDecl { name, init, .. } => {
                // 遮蔽时分配新槽（词法作用域，块退出恢复外层绑定）
                let slot = self.alloc_slot();
                self.bind(name, slot);
                let t = match init {
                    Some(e) => self.lower_expr(e),
                    None => {
                        let t = self.alloc_slot();
                        self.push(IrInst::Const {
                            temp: t,
                            val: IrConst::Void,
                        });
                        t
                    }
                };
                self.push(IrInst::Store { slot, temp: t });
            }
            Stmt::ConstDecl { name, init, .. } => {
                let slot = self.alloc_slot();
                self.bind(name, slot);
                let t = self.lower_expr(init);
                self.push(IrInst::Store { slot, temp: t });
            }
            Stmt::Expr(Expr::Assign {
                target, op, value, span,
            }) => {
                // 语句级赋值：副作用即可；目标不在 IR 范围（字段/索引/解构）→ 硬错误
                if self.lower_assign(*op, target, value).is_none() {
                    self.fail("字段/索引/解构赋值", span);
                }
            }
            Stmt::Expr(e) => {
                let _ = self.lower_expr(e);
            }
            Stmt::If(ifs) => {
                let c = self.lower_expr(&ifs.cond);
                let l_else = self.new_label();
                let l_end = self.new_label();
                match &ifs.capture {
                    // optional 捕获：null → else；否则绑定 cond 值到捕获名（对齐解释器 exec_if）
                    Some((_, name)) => {
                        self.push(IrInst::JumpIfNull {
                            temp: c,
                            label: l_else,
                        });
                        self.push_scope();
                        self.bind(name, c);
                        for stmt in &ifs.then_b.stmts {
                            self.lower_stmt(stmt);
                        }
                        self.pop_scope();
                    }
                    None => {
                        self.push(IrInst::JumpIfNot {
                            temp: c,
                            label: l_else,
                        });
                        // then 块是独立作用域（对齐 oracle `exec_block`）：块内变量/
                        // defer 随块结束（弹栈）——defer 时序依赖此作用域边界。
                        self.push_scope();
                        for stmt in &ifs.then_b.stmts {
                            self.lower_stmt(stmt);
                        }
                        self.pop_scope();
                    }
                }
                match &ifs.else_b {
                    Some(else_b) => {
                        self.push(IrInst::Jump { label: l_end });
                        self.label(l_else);
                        self.lower_stmt(else_b);
                    }
                    None => {
                        self.label(l_else);
                    }
                }
                self.label(l_end);
            }
            Stmt::While(w) => {
                let l_top = self.new_label();
                // continue 目标：步进（如有）→ 重测条件（对齐 oracle exec_while）
                let l_cont = self.new_label();
                let l_end = self.new_label();
                self.label(l_top);
                let c = self.lower_expr(&w.cond);
                self.push(IrInst::JumpIfNot {
                    temp: c,
                    label: l_end,
                });
                let defer_depth = self.defers.len();
                self.loops.push(LoopCtx {
                    break_label: l_end,
                    continue_label: l_cont,
                    label: w.label.clone(),
                    defer_depth_at_entry: defer_depth,
                });
                self.lower_block(&w.body);
                self.loops.pop();
                self.label(l_cont);
                if let Some(step) = &w.step {
                    let _ = self.lower_expr(step);
                }
                self.push(IrInst::Jump { label: l_top });
                self.label(l_end);
            }
            Stmt::Return(e, _) => match e {
                Some(e) => {
                    let t = self.lower_expr(e);
                    // 返回排空函数级 defers：errdefer 仅当返回值为错误值（运行期判定）触发
                    self.emit_defers(0, ErrPath::Value(t));
                    self.push(IrInst::Return { temp: t });
                }
                None => {
                    // void 返回：正常路径（无错误值），仅非 errdefer
                    self.emit_defers(0, ErrPath::Never);
                    self.push(IrInst::ReturnVoid);
                }
            },
            Stmt::Block(b) => self.lower_block(b),
            Stmt::For(f) => self.lower_for(f),
            Stmt::Switch(s) => self.lower_switch(s),
            Stmt::Break(l, span) => {
                if let Some(label) = l {
                    self.lower_labeled_exit(label, true, span);
                } else {
                    self.lower_break(span);
                }
            }
            Stmt::Continue(l, span) => {
                if let Some(label) = l {
                    self.lower_labeled_exit(label, false, span);
                } else {
                    self.lower_continue(span);
                }
            }
            // defer/errdefer（Phase 6）：体降级入缓冲 → 登记 + PushDefer；退出点排空
            Stmt::Defer(e, span) => self.lower_defer(e, false, span),
            Stmt::Errdefer(e, span) => self.lower_defer(e, true, span),
            Stmt::Empty => {}
        }
    }

    /// `for` 循环（对齐 oracle `exec_for`/`iter_items`）：
    /// IterMake 展开迭代项 → 每项 IterNext 重绑定捕获槽 → 循环体 →（Mut/Move）写回。
    /// continue → l_next（重新取下一项）；break → l_end。
    fn lower_for(&mut self, f: &ForStmt) {
        let base = self.lower_expr(&f.iter);
        let iter = self.alloc_slot();
        self.push(IrInst::IterMake { temp: iter, base });
        // 捕获槽：每个迭代由 IterNext 重绑定（Read → 值副本；Mut/Move → 共享源 cell）
        let slot = self.alloc_slot();
        let read_only = matches!(f.capture, CaptureMode::Read);

        // 捕获名作用域（循环结束后弹出，防泄漏）
        self.push_scope();
        self.bind(&f.capture_name, slot);

        let l_next = self.new_label();
        let l_body = self.new_label();
        let l_end = self.new_label();

        self.label(l_next);
        let has = self.alloc_slot();
        self.push(IrInst::IterNext {
            has,
            iter,
            slot,
            read_only,
        });
        self.push(IrInst::JumpIfNot {
            temp: has,
            label: l_end,
        });
        self.label(l_body);

        let defer_depth = self.defers.len();
        self.loops.push(LoopCtx {
            break_label: l_end,
            continue_label: l_next,
            label: f.label.clone(),
            defer_depth_at_entry: defer_depth,
        });
        self.lower_block(&f.body);
        self.loops.pop();

        // Mut/Move 捕获写回（LLVM 拷贝进出；run_ir 槽 cell 即源 cell → 无操作）
        if !read_only {
            self.push(IrInst::IterWriteBack { iter, slot });
        }
        self.push(IrInst::Jump { label: l_next });
        self.label(l_end);

        self.pop_scope();
    }

    /// 无标签 break：跳到最近循环的结束标签（对齐 oracle 单级跳出）。
    /// 排空 [循环进入时 defer 深度 .. 当前] 的 defers（含嵌套作用域内登记的），
    /// 正常路径（errdefer 不运行）——对齐 oracle `exec_while` 的 `pop_scope(is_err_path=false)`。
    fn lower_break(&mut self, span: &Span) {
        let (depth, label) = match self.loops.last() {
            Some(l) => (l.defer_depth_at_entry, l.break_label),
            None => {
                self.fail("`break` 在循环外", span);
                return;
            }
        };
        self.emit_defers(depth, ErrPath::Never);
        self.push(IrInst::Jump { label });
    }

    /// 无标签 continue：跳到最近循环的 continue 标签（排空该循环体内 defers，同 break）。
    fn lower_continue(&mut self, span: &Span) {
        let (depth, label) = match self.loops.last() {
            Some(l) => (l.defer_depth_at_entry, l.continue_label),
            None => {
                self.fail("`continue` 在循环外", span);
                return;
            }
        };
        self.emit_defers(depth, ErrPath::Never);
        self.push(IrInst::Jump { label });
    }

    /// 带标签 break/continue：从循环栈向外找最近匹配标签的循环，跳到其 break/continue
    /// 标签。排空从目标循环进入深度到当前的 defers——中间层循环体内登记的 defers
    /// （未在各自退出点运行）一并排空；外层（目标循环之外）defers 由后续退出点处理。
    fn lower_labeled_exit(&mut self, label: &str, is_break: bool, span: &Span) {
        let Some(pos) = self
            .loops
            .iter()
            .rposition(|lc| lc.label.as_deref() == Some(label))
        else {
            self.fail(&format!("未找到标签 `:{label}` 对应的循环"), span);
            return;
        };
        let depth = self.loops[pos].defer_depth_at_entry;
        self.emit_defers(depth, ErrPath::Never);
        let jump = if is_break {
            self.loops[pos].break_label
        } else {
            self.loops[pos].continue_label
        };
        self.push(IrInst::Jump { label: jump });
    }

    /// defer/errdefer 降级：体表达式降级入独立缓冲（无控制流指令——硬错误保证重复
    /// 发射安全），登记 + 主流 PushDefer。体在退出点（作用域结束/return/break/
    /// continue/try 错误）由守卫 + 内联体排空。
    fn lower_defer(&mut self, e: &Expr, errdefer: bool, span: &Span) {
        let id = self.next_defer_id;
        self.next_defer_id += 1;
        // 1) 体降级入缓冲（push/label 路由到 pending）
        let prev = self.pending.take();
        self.pending = Some(Vec::new());
        let _ = self.lower_expr(e);
        let mut body = self.pending.take().expect("pending 缓冲已初始化");
        self.pending = prev;
        // 2) 体含控制流指令 → 硬错误（带 label 指令重复发射冲突；对齐「硬错误 > 静默误编译」）
        if body.iter().any(|i| is_control_flow_inst(i)) {
            self.fail("`defer`/`errdefer` 体不允许控制流（如 `defer try f()`）", span);
            body.clear(); // 避免污染退出点发射
        }
        // 3) 主流登记
        self.push(IrInst::PushDefer { id });
        self.defers.push(DeferRecord { id, body, errdefer });
    }

    /// `switch` 语句：降级为 first-match 线性链（不穷举检查，对齐 oracle `exec_switch`）。
    fn lower_switch(&mut self, s: &SwitchStmt) {
        self.lower_switch_inner(&s.subject, &s.arms, s.has_else, &s.span, None);
    }

    /// switch 通用降级（语句 `value_slot=None`；表达式 `value_slot=Some(t)`）。
    /// 模式链：每个非 Else 模式 MatchTest → JumpIfNot 下一模式；命中 → 臂体 → 跳 l_done。
    /// 全部失败 → 兜底（has_else → else 臂；否则表达式为 Void / 语句无事发生）。
    fn lower_switch_inner(
        &mut self,
        subject: &Expr,
        arms: &[SwitchArm],
        has_else: bool,
        span: &Span,
        value_slot: Option<usize>,
    ) {
        let _ = span;
        let s = self.lower_expr(subject);
        let l_done = self.new_label();
        let l_fb = self.new_label();

        // 平坦化非 Else 模式链（顺序 = 臂序 × 臂内模式序）
        let mut flat: Vec<(&SwitchArm, IrPattern)> = Vec::new();
        for arm in arms {
            for p in &arm.patterns {
                if let Some(p) = to_ir_pattern(p) {
                    flat.push((arm, p));
                }
            }
        }
        let n = flat.len();
        for (i, (arm, p)) in flat.iter().enumerate() {
            let t_pat = self.alloc_slot();
            self.push(IrInst::MatchTest {
                temp: t_pat,
                subject: s,
                pattern: p.clone(),
            });
            let l_next = if i + 1 < n {
                self.new_label()
            } else {
                l_fb
            };
            self.push(IrInst::JumpIfNot {
                temp: t_pat,
                label: l_next,
            });
            self.emit_switch_arm_body(arm, s, value_slot);
            self.push(IrInst::Jump { label: l_done });
            if i + 1 < n {
                self.label(l_next);
            }
        }

        // 兜底：else 臂（无论其是否还带非 Else 模式——与 oracle 一致，臂体可能被发射两次）
        self.label(l_fb);
        if has_else {
            if let Some(arm) = arms
                .iter()
                .find(|a| a.patterns.iter().any(|p| matches!(p, SwitchPattern::Else)))
            {
                self.emit_switch_arm_body(arm, s, value_slot);
                self.push(IrInst::Jump { label: l_done });
            }
        }
        // 无匹配（oracle `Flow::None`）→ 表达式 Void；语句无事发生
        if let Some(t) = value_slot {
            self.push(IrInst::Const {
                temp: t,
                val: IrConst::Void,
            });
        }
        self.label(l_done);
    }

    /// 发射单臂体：捕获绑定（EnumPayload 负载或 subject 本身）+ 臂体。
    fn emit_switch_arm_body(
        &mut self,
        arm: &SwitchArm,
        subject: usize,
        value_slot: Option<usize>,
    ) {
        // 对齐 oracle `exec_switch_arm`：push_scope → bind capture → exec body → pop_scope
        self.push_scope();
        if let Some((_, name)) = &arm.capture {
            let cap = self.alloc_slot();
            self.push(IrInst::EnumPayload { temp: cap, a: subject });
            self.bind(name, cap);
        }
        match value_slot {
            Some(t) => self.lower_block_value(&arm.body, t),
            None => self.lower_block(&arm.body),
        }
        self.pop_scope();
    }

    /// 块求值（值 = 最后语句若为表达式，否则 Void；对齐 oracle `exec_block_inner`）。
    /// switch 表达式臂体专用。
    fn lower_block_value(&mut self, b: &Block, t: usize) {
        self.push_scope();
        let n = b.stmts.len();
        let last_is_value = matches!(b.stmts.last(), Some(Stmt::Expr(_)));
        let m = n - usize::from(last_is_value);
        for stmt in &b.stmts[..m] {
            self.lower_stmt(stmt);
        }
        if last_is_value {
            if let Some(Stmt::Expr(e)) = b.stmts.last() {
                let v = self.lower_expr(e);
                self.push(IrInst::Load { temp: t, slot: v });
            }
        } else {
            self.push(IrInst::Const {
                temp: t,
                val: IrConst::Void,
            });
        }
        self.pop_scope();
    }
}

/// 指令是否含控制流/登记副作用——defer 体内出现即硬错误（带 label 的跳转指令
/// 在退出点重复发射会冲突；PushDefer/PopDefer 为 defer 登记副作用）。
fn is_control_flow_inst(i: &IrInst) -> bool {
    matches!(
        i,
        IrInst::Jump { .. }
            | IrInst::JumpIf { .. }
            | IrInst::JumpIfNot { .. }
            | IrInst::JumpIfNull { .. }
            | IrInst::JumpIfErr { .. }
            | IrInst::Label { .. }
            | IrInst::Return { .. }
            | IrInst::ReturnVoid
            | IrInst::PushDefer { .. }
            | IrInst::PopDefer { .. }
            | IrInst::JumpIfNotDefer { .. }
    )
}

/// switch 模式 → IR 模式（`Else` → None：不发射 MatchTest，由兜底臂处理）。
fn to_ir_pattern(p: &SwitchPattern) -> Option<IrPattern> {
    match p {
        SwitchPattern::Error(s) => Some(IrPattern::Error(s.clone())),
        SwitchPattern::Ident(s) => Some(IrPattern::Ident(s.clone())),
        SwitchPattern::Int(s) => Some(IrPattern::Int(parse_int_lit(s))),
        SwitchPattern::Float(s) => Some(IrPattern::Float(s.parse().unwrap_or(0.0))),
        SwitchPattern::Str(s) => Some(IrPattern::Str(s.clone())),
        SwitchPattern::Char(c) => Some(IrPattern::Char(*c)),
        SwitchPattern::Else => None,
    }
}

fn to_assign_binop(op: AssignOp) -> IrBinOp {
    match op {
        AssignOp::Add => IrBinOp::Add,
        AssignOp::Sub => IrBinOp::Sub,
        AssignOp::Mul => IrBinOp::Mul,
        AssignOp::Div => IrBinOp::Div,
        AssignOp::BitOr => IrBinOp::BitOr,
        AssignOp::BitAnd => IrBinOp::BitAnd,
        AssignOp::BitXor => IrBinOp::BitXor,
        AssignOp::Set => unreachable!("Set 单独处理"),
    }
}

fn to_ir_binop(op: BinOp) -> IrBinOp {
    match op {
        BinOp::Add => IrBinOp::Add,
        BinOp::Sub => IrBinOp::Sub,
        BinOp::Mul => IrBinOp::Mul,
        BinOp::Div => IrBinOp::Div,
        BinOp::Mod => IrBinOp::Mod,
        BinOp::EucMod => IrBinOp::EucMod,
        BinOp::BitAnd => IrBinOp::BitAnd,
        BinOp::BitOr => IrBinOp::BitOr,
        BinOp::BitXor => IrBinOp::BitXor,
        BinOp::Shl => IrBinOp::Shl,
        BinOp::Shr => IrBinOp::Shr,
        BinOp::Eq => IrBinOp::Eq,
        BinOp::Ne => IrBinOp::Ne,
        BinOp::Lt => IrBinOp::Lt,
        BinOp::Le => IrBinOp::Le,
        BinOp::Gt => IrBinOp::Gt,
        BinOp::Ge => IrBinOp::Ge,
        // 短路/区间在 lower_expr 单独处理，此处为不可达兜底
        BinOp::And | BinOp::Or | BinOp::Range => IrBinOp::Eq,
    }
}

/// 断言内建（IR 参考解释器实现）
fn is_assert_builtin(name: &str) -> bool {
    matches!(
        name,
        "expect" | "expect_eq" | "expect_neq" | "expect_error" | "expect_eq_slices"
    )
}

/// 自由内建（非 `@` 前缀；测试内隐式可用，普通函数体按名路由到 `CallBuiltin`）。
/// 对齐 oracle `call_builtin`（interp.rs:2911）的用户可调内建面。
fn is_free_builtin(name: &str) -> bool {
    matches!(
        name,
        // 内存/复制
        "box" | "copy"
            // 数值工具
            | "sqrt" | "min" | "max"
            // 字节工具
            | "read_u64_le"
            // 算法
            | "sort" | "binary_search"
            // 解析器辅助（71-recursive-parser）
            | "skip_space" | "peek" | "advance" | "is_digit" | "parse_number"
            // 文本解析
            | "parse_int" | "parse_float"
    )
}

/// `@` 内建的「类型位置」参数（类型名以 `Const Str` 编码，运行时按名解析）。
/// 对齐 oracle `@sizeOf/@alignOf/@offsetOf/@intCast/@enumFromInt/@ptrCast/@alignCast`
/// 从 `Expr::Ident` 读类型名（interp.rs:3009-3030, 3068-3093）。
fn is_type_arg_pos(name: &str, i: usize) -> bool {
    match name {
        "@sizeOf" | "@alignOf" => i == 0,
        "@offsetOf" => i == 0 || i == 1,
        "@intCast" | "@enumFromInt" | "@ptrCast" | "@alignCast" => i == 0,
        // alloc.init(ABC)：类型名参数（运行时按名建空实例）
        "alloc.init" => i == 0,
        // math.nan/math.inf/math.inf_neg(f64)：类型名参数（运行时忽略，仅指示宽度）
        "math.nan" | "math.inf" | "math.inf_neg" => i == 0,
        _ => false,
    }
}

/// 整数/浮点字面量解析（后缀、下划线、进制）
fn parse_int_lit(text: &str) -> i128 {
    let cleaned: String = text
        .chars()
        .take_while(|c| {
            c.is_ascii_digit()
                || matches!(c, 'x' | 'X' | 'b' | 'B' | 'o' | 'O' | 'a'..='f' | 'A'..='F' | '_')
        })
        .collect();
    let cleaned = cleaned.replace('_', "");
    let (radix, digits) = if let Some(r) = cleaned.strip_prefix("0x").or(cleaned.strip_prefix("0X"))
    {
        (16u32, r)
    } else if let Some(r) = cleaned.strip_prefix("0b").or(cleaned.strip_prefix("0B")) {
        (2u32, r)
    } else if let Some(r) = cleaned.strip_prefix("0o").or(cleaned.strip_prefix("0O")) {
        (8u32, r)
    } else {
        (10u32, cleaned.as_str())
    };
    i128::from_str_radix(digits, radix).unwrap_or(0)
}

// ---------- IR 参考解释器（M3.1：唯一语义源的语义定义） ----------

#[derive(Debug, Clone, PartialEq)]
pub enum IrValue {
    Int(i128),
    Float(f64),
    Bool(bool),
    Str(Vec<u8>),
    /// 可选值（`null` = `Opt(None)`，对齐 tree-walking `Value::Opt`）
    Opt(Option<Box<IrValue>>),
    /// 错误值（M4.2：码 + 名字；码 = M2.6 编译期错误码表，全局唯一）
    Err { name: String, code: u32 },
    /// 指针：共享堆 cell 索引（别名装置——对齐 tree-walking `Value::Ptr(Rc<RefCell>)`）
    Ptr(usize),
    /// 数组：`Cell::Elems` 的 cell 索引（元素为共享 cell——切片/写索引别名）
    Arr(usize),
    /// 切片视图：共享底层 `Cell::Elems` + 窗口；`data` 为数组 cell 索引
    Slice {
        data: usize,
        start: usize,
        len: usize,
    },
    /// 类实例：`Cell::Class` 的 cell 索引（字段为普通值——无字段级别名）
    Class(usize),
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
    Iter {
        items: Vec<IterItem>,
        next: usize,
    },
}

/// 迭代项：共享源 cell（或新 cell）+ 是否源容器引用（对齐 oracle `iter_items` 的 `(cell, is_ref)`）。
#[derive(Debug, Clone)]
pub struct IterItem {
    pub cell: usize,
    pub is_ref: bool,
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
}

/// 解引用：Ptr → pointee（一层，对齐 tree-walking `deref_value`）；非 Ptr → 恒等
fn deref_value<'a>(ctx: &'a Ctx, v: &'a IrValue) -> &'a IrValue {
    match v {
        IrValue::Ptr(c) => match &ctx.cells[*c] {
            Cell::Value(v) => v,
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
        IrValue::Arr(_) => "[]T".into(),
        IrValue::Slice { .. } => "[]T".into(),
        IrValue::Class(_) => "class".into(),
        IrValue::Enum { name, .. } => name.clone(),
        IrValue::End => "end".into(),
        IrValue::Iter(_) => "<iter>".into(),
        IrValue::Fn(_) => "fn".into(),
        IrValue::Closure { .. } => "closure".into(),
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
        (IrValue::Bool(b), IrPattern::Ident(s)) => {
            (*b && s == "true") || (!*b && s == "false")
        }
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
fn make_iter(
    ctx: &mut Ctx,
    module: &IrModule,
    v: &IrValue,
    depth: usize,
) -> R<Vec<IterItem>> {
    let v = deref_value(ctx, v).clone();
    match v {
        IrValue::Arr(c) => match &ctx.cells[c] {
            Cell::Elems(e) => Ok(e
                .iter()
                .map(|ec| IterItem { cell: *ec, is_ref: true })
                .collect()),
            _ => Err(IrError::msg("NotIterable", "array cell is not an element list")),
        },
        IrValue::Slice { data, start, len } => match &ctx.cells[data] {
            Cell::Elems(e) => Ok(e[start..start + len]
                .iter()
                .map(|ec| IterItem { cell: *ec, is_ref: true })
                .collect()),
            _ => Err(IrError::msg("NotIterable", "slice data is not an element list")),
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
                        IterItem { cell: kv, is_ref: false }
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
        },
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
        bf.get(k)
            .map_or(false, |bc| ctx.cell_value(*fc).value_eq(ctx, ctx.cell_value(*bc)))
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
            IrValue::Arr(c) => match &ctx.cells[*c] {
                Cell::Elems(e) => {
                    let items: Vec<String> =
                        e.iter().map(|ec| ctx.cell_value(*ec).display(ctx)).collect();
                    format!("[{}]", items.join(", "))
                }
                _ => "[]".into(),
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
                        .map(|(k, fc)| {
                            format!("{k} = {}", ctx.cell_value(*fc).display(ctx))
                        })
                        .collect();
                    format!("{name} {{ {} }}", items.join(", "))
                }
                _ => "void".into(),
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
                let (a_e, b_e) = (d, match &ctx.cells[*b] {
                    Cell::Elems(x) => x.clone(),
                    _ => return false,
                });
                *len == b_e.len()
                    && (0..*len)
                        .all(|i| ctx.cell_value(a_e[*start + i]).value_eq(ctx, ctx.cell_value(b_e[i])))
            }
            (IrValue::Arr(a), IrValue::Slice { data, start, len }) => {
                let d = match &ctx.cells[*data] {
                    Cell::Elems(x) => x.clone(),
                    _ => return false,
                };
                let (a_e, d_e) = (match &ctx.cells[*a] {
                    Cell::Elems(x) => x.clone(),
                    _ => return false,
                }, d);
                a_e.len() == *len
                    && (0..*len)
                        .all(|i| ctx.cell_value(a_e[i]).value_eq(ctx, ctx.cell_value(d_e[*start + i])))
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
        // main(io: Io) !void——单参数 io 版本或零参版本（对齐 oracle `run_main`
        // interp.rs:5663-5675：候选池内有 1 参 main 时注入 io，否则传空）。
        let mut args = args.to_vec();
        if entry == "main" && args.is_empty() {
            let has_1p = module.func_index.get("main").map_or(false, |v| {
                v.iter().any(|&i| module.funcs[i].params.len() == 1)
            });
            if has_1p {
                args.push(io_value_ir(&mut self.ctx));
            }
        }
        let idx = pick_func(&self.ctx, module, entry, &args)
            .ok_or_else(|| IrError::msg("NoFunction", format!("no function `{entry}`")))?;
        exec_func(&mut self.ctx, module, idx, &args, 0)
    }
}

/// 重载/可选参数分派（对齐 oracle `pick_fn` `interp.rs:2665-2796`）：
/// ① 精确参数数（非空则用；空则全池）→ ② 按实参值类型匹配（具体优先泛型）→ ③ 尾参默认回退。
/// 返回类型匹配（`expected_ret`）IR 未跟踪，留待 Phase 7 期望类型传播补齐。
fn pick_func(ctx: &Ctx, module: &IrModule, name: &str, arg_vals: &[IrValue]) -> Option<usize> {
    let candidates = module.func_index.get(name)?;
    // ① 精确参数数量匹配
    let exact: Vec<usize> = candidates
        .iter()
        .copied()
        .filter(|&i| module.funcs[i].params.len() == arg_vals.len())
        .collect();
    let pool: Vec<usize> = if exact.is_empty() {
        candidates.clone()
    } else {
        exact
    };
    if pool.len() == 1 {
        return Some(pool[0]);
    }
    // ② 按实参值类型匹配（具体优先于泛型；返回类型匹配留待 Phase 7）
    let mut best: Option<usize> = None;
    for &fi in &pool {
        let f = &module.funcs[fi];
        let mut ok = true;
        let mut is_generic = false;
        for (p, a) in f.param_ty.iter().zip(arg_vals.iter()) {
            let pt = p.strip();
            // 指针实参解引用后匹配
            let a = match a {
                IrValue::Ptr(c) => ctx.cell_value(*c),
                other => other,
            };
            match pt {
                Type::Named(n, _) => {
                    let want_float = matches!(n.as_str(), "f32" | "f64" | "f16" | "f128");
                    let want_int = matches!(
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
                    );
                    let want_bool = n == "bool";
                    match a {
                        IrValue::Int(_) if want_float => ok = false,
                        IrValue::Float(_) if want_int => ok = false,
                        IrValue::Str(_) if want_int || want_float || want_bool => ok = false,
                        IrValue::Bool(_) if !want_bool => ok = false,
                        IrValue::Class(c) if n != "String" && class_name(ctx, *c) != *n => {
                            ok = false;
                        }
                        // 泛型 T（where T: INumber 等）：不排除（编译时验证归 M2）
                        _ if n.chars().next().map_or(false, |c| c.is_uppercase())
                            && !n.starts_with("String")
                            && !n.starts_with("Vec")
                            && !n.starts_with("Map")
                            && !n.starts_with("Deque") =>
                        {
                            is_generic = true;
                        }
                        _ => {}
                    }
                }
                Type::Slice(inner, _) => {
                    // &[u8] / &[T]：Str 或数组；泛型元素 T 标记为泛型
                    match a {
                        IrValue::Str(_) => {}
                        IrValue::Arr(_) | IrValue::Slice { .. } => {}
                        _ => ok = false,
                    }
                    if let Type::Named(n, _) = inner.strip() {
                        if n.chars().next().map_or(false, |c| c.is_uppercase())
                            && !n.starts_with("String")
                            && !n.starts_with("Vec")
                            && !n.starts_with("Map")
                            && !n.starts_with("Deque")
                        {
                            is_generic = true;
                        }
                    }
                }
                Type::Infer => {}
                _ => {}
            }
        }
        if ok {
            match &best {
                None => best = Some(fi),
                Some(b) => {
                    let b_generic = module.funcs[*b].param_ty.iter().any(type_has_generic);
                    if !is_generic && b_generic {
                        // 具体优先于泛型
                        best = Some(fi);
                    } else if is_generic && !b_generic {
                        // 保留 best（泛型不替换具体）
                    }
                    // 同具体度：保留首个注册（稳定）
                }
            }
        }
    }
    if let Some(b) = best {
        return Some(b);
    }
    // ③ 带默认参数的回退（参数数 <= 声明数且尾部默认）
    for &fi in candidates {
        let f = &module.funcs[fi];
        if f.params.len() > arg_vals.len() {
            let missing = f.params.len() - arg_vals.len();
            let tail_has_default = f.param_defaults[f.params.len() - missing..]
                .iter()
                .all(|d| *d);
            if tail_has_default {
                return Some(fi);
            }
        }
    }
    None
}

/// 类型是否含泛型参数（重载分派：具体优先泛型；对齐 oracle `type_has_generic`）
fn type_has_generic(t: &Type) -> bool {
    match t.strip() {
        Type::Named(n, args) => {
            let n = n.as_str();
            (n.chars().next().map_or(false, |c| c.is_uppercase())
                && !n.starts_with("String")
                && !n.starts_with("Vec")
                && !n.starts_with("Map")
                && !n.starts_with("Deque"))
                || args.iter().any(type_has_generic)
        }
        Type::Ptr(inner, _)
        | Type::Slice(inner, _)
        | Type::Optional(inner)
        | Type::ErrorUnion(_, inner) => type_has_generic(inner),
        Type::Tuple(items) => items.iter().any(type_has_generic),
        Type::Array(_, inner) => type_has_generic(inner),
        _ => false,
    }
}

/// Class 单元的类名（pick_func 按类名匹配；oracle 用 `Value::Class(c).borrow().name`）
fn class_name(ctx: &Ctx, cell: usize) -> String {
    match &ctx.cells[cell] {
        Cell::Class { name, .. } => name.clone(),
        _ => "<not-a-class>".into(),
    }
}

/// 深拷贝（move 捕获；对齐 oracle `deep_copy` `interp.rs:5539-5562`）：
/// Arr/Class/Ptr/Opt(Some) 递归拷贝，其余按值克隆（Str 本身是不可变字节串）。
fn deep_copy(ctx: &mut Ctx, v: IrValue) -> IrValue {
    match v {
        IrValue::Arr(c) => {
            let elems = match &ctx.cells[c] {
                Cell::Elems(e) => e.clone(),
                _ => return IrValue::Arr(c),
            };
            let new_elems: Vec<usize> = elems
                .iter()
                .map(|ec| {
                    let cv = ctx.cell_value(*ec).clone();
                    let copied = deep_copy(ctx, cv);
                    ctx.alloc(Cell::Value(copied))
                })
                .collect();
            IrValue::Arr(ctx.alloc(Cell::Elems(new_elems)))
        }
        IrValue::Class(c) => {
            let (name, fields) = match &ctx.cells[c] {
                Cell::Class { name, fields } => (name.clone(), fields.clone()),
                _ => return IrValue::Class(c),
            };
            let new_fields: HashMap<String, usize> = fields
                .iter()
                .map(|(k, vc)| {
                    let cv = ctx.cell_value(*vc).clone();
                    let copied = deep_copy(ctx, cv);
                    (k.clone(), ctx.alloc(Cell::Value(copied)))
                })
                .collect();
            IrValue::Class(ctx.alloc(Cell::Class {
                name,
                fields: new_fields,
            }))
        }
        IrValue::Ptr(c) => {
            let cv = ctx.cell_value(c).clone();
            let copied = deep_copy(ctx, cv);
            IrValue::Ptr(ctx.alloc(Cell::Value(copied)))
        }
        IrValue::Opt(Some(b)) => IrValue::Opt(Some(Box::new(deep_copy(ctx, *b)))),
        // move 捕获闭包值：捕获 cell 逐个深拷贝——闭包持有独立环境副本
        // （与原作用域/其他闭包脱离共享，对齐 oracle `deep_copy` Closure 臂）
        IrValue::Closure {
            func,
            captures,
            is_mut,
        } => {
            let new_caps: Vec<usize> = captures
                .iter()
                .map(|c| {
                    let cv = ctx.cell_value(*c).clone();
                    let copied = deep_copy(ctx, cv);
                    ctx.alloc(Cell::Value(copied))
                })
                .collect();
            IrValue::Closure {
                func,
                captures: new_caps,
                is_mut,
            }
        }
        other => other,
    }
}

/// 值类型名（方法分派 key：`"{type}.{method}"`；对齐 oracle `Value::type_name`）
fn ir_type_name(ctx: &Ctx, v: &IrValue) -> String {
    match v {
        IrValue::Int(_) => "i128".into(),
        IrValue::Float(_) => "f64".into(),
        IrValue::Bool(_) => "bool".into(),
        IrValue::Str(_) => "&[u8]".into(),
        IrValue::Arr(_) => "array".into(),
        IrValue::Slice { .. } => "slice".into(),
        IrValue::Class(c) => class_name(ctx, *c),
        IrValue::Enum { name, .. } => name.clone(),
        IrValue::Opt(_) => "optional".into(),
        IrValue::Err { .. } => "error".into(),
        IrValue::Ptr(_) => "pointer".into(),
        IrValue::Fn(_) => "fn".into(),
        IrValue::Closure { .. } => "closure".into(),
        IrValue::End => "end".into(),
        IrValue::Iter(_) => "<iter>".into(),
        IrValue::Void => "void".into(),
    }
}

/// IrConst → IrValue（默认参数常量值；与 `IrInst::Const` 执行一致）
fn const_val(c: &IrConst) -> IrValue {
    match c {
        IrConst::Int(i) => IrValue::Int(*i),
        IrConst::Float(f) => IrValue::Float(*f),
        IrConst::Bool(b) => IrValue::Bool(*b),
        IrConst::Str(s) => IrValue::Str(s.clone().into_bytes()),
        IrConst::Void => IrValue::Void,
        IrConst::Null => IrValue::Opt(None),
        IrConst::Err { name, code } => IrValue::Err {
            name: name.clone(),
            code: *code,
        },
        IrConst::End => IrValue::End,
    }
}

/// 执行一个函数：堆/单元模型（Phase 1）。每槽分配共享 cell，`&x` = `Ptr(cell)`
/// 可跨帧存活（传入函数后写穿调用方槽——别名语义对齐 tree-walking `Rc<RefCell>`）。
fn exec_func(ctx: &mut Ctx, module: &IrModule, idx: usize, args: &[IrValue], depth: usize) -> R<IrValue> {
    if depth >= MAX_CALL_DEPTH {
        return Err(IrError::msg("StackOverflow", "maximum call depth exceeded"));
    }
    let func = &module.funcs[idx];
    let mut frame = Frame {
        cells: Vec::with_capacity(func.n_slots),
        defers: Vec::new(),
        readonly: Vec::new(),
    };
    for _ in 0..func.n_slots {
        frame.cells.push(ctx.alloc(Cell::Value(IrValue::Void)));
    }
    // 绑定实参；缺失尾参用编译期常量默认值补齐（ADR-0009 / 对齐 oracle `call_fn`）
    for (i, ps) in func.params.iter().enumerate() {
        if i < args.len() {
            ctx.set(&frame, *ps, args[i].clone());
        } else if i < func.defaults.len() {
            if let Some(d) = &func.defaults[i] {
                ctx.set(&frame, *ps, const_val(d));
            }
        }
    }
    exec_body(ctx, module, func, frame, depth)
}

/// 调用闭包（对齐 oracle `call_closure` `interp.rs:1444-1494`）：
/// 捕获参数槽直接绑定捕获 cell（写穿 = 共享读/mut 语义）；显式参数绑新值。
/// 单表达式闭包体 `|v| v+a` 已在 lower 阶段降级为 `Return { temp }`。
fn call_closure_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    func_idx: usize,
    captures: &[usize],
    args: &[IrValue],
    is_mut: bool,
    depth: usize,
) -> R<IrValue> {
    if depth >= MAX_CALL_DEPTH {
        return Err(IrError::msg("StackOverflow", "maximum call depth exceeded"));
    }
    let func = &module.closures[func_idx];
    let n_caps = captures.len();
    // M2.7 只读强制（Phase 8）：非 `mut` 闭包 → 捕获参数槽只读
    // （Store 写这些槽 → ReadonlyCapture；经指针/字段/索引写穿放行）
    let readonly: Vec<usize> = if is_mut {
        Vec::new()
    } else {
        func.params.iter().take(n_caps).copied().collect()
    };
    let mut frame = Frame {
        cells: Vec::with_capacity(func.n_slots),
        defers: Vec::new(),
        readonly,
    };
    for _ in 0..func.n_slots {
        frame.cells.push(ctx.alloc(Cell::Value(IrValue::Void)));
    }
    // 捕获参数（前 n_caps 个槽）→ 直接绑定捕获 cell（写穿调用方槽）
    for (i, cap_cell) in captures.iter().enumerate() {
        if i < func.params.len() {
            frame.cells[func.params[i]] = *cap_cell;
        }
    }
    // 显式参数（捕获参数之后）
    for (i, ps) in func.params.iter().enumerate().skip(n_caps) {
        let ai = i - n_caps;
        if ai < args.len() {
            ctx.set(&frame, *ps, args[ai].clone());
        }
    }
    exec_body(ctx, module, func, frame, depth)
}

/// 执行函数/闭包体（共享循环；当前函数体在 `func`，模块其余函数在 `module.funcs`）
fn exec_body(ctx: &mut Ctx, module: &IrModule, func: &IrFunc, frame: Frame, depth: usize) -> R<IrValue> {
    let mut frame = frame;
    let mut pc = 0usize;
    let mut fail: Option<String> = None;
    loop {
        if pc >= func.body.len() {
            return Err(IrError::msg(
                "NoReturn",
                format!("function `{}` fell through", func.name),
            ));
        }
        match &func.body[pc] {
            IrInst::Const { temp, val } => {
                ctx.set(&frame, *temp, match val {
                    IrConst::Int(i) => IrValue::Int(*i),
                    IrConst::Float(f) => IrValue::Float(*f),
                    IrConst::Bool(b) => IrValue::Bool(*b),
                    IrConst::Str(s) => IrValue::Str(s.clone().into_bytes()),
                    IrConst::Void => IrValue::Void,
                    IrConst::Null => IrValue::Opt(None),
                    IrConst::Err { name, code } => IrValue::Err {
                        name: name.clone(),
                        code: *code,
                    },
                    IrConst::End => IrValue::End,
                });
            }
            IrInst::Load { temp, slot } => {
                let v = ctx.get(&frame, *slot).clone();
                ctx.set(&frame, *temp, v);
            }
            IrInst::Store { slot, temp } => {
                // M2.7 只读捕获强制（Phase 8）：非 `mut` 闭包写捕获参数槽 → 错误
                if frame.readonly.contains(slot) {
                    return Err(IrError::msg(
                        "ReadonlyCapture",
                        "cannot assign to captured variable in non-mut closure \
                         (declare the closure `mut` to capture mutably)",
                    ));
                }
                let v = ctx.get(&frame, *temp).clone();
                ctx.set(&frame, *slot, v);
            }
            IrInst::Bin { op, temp, a, b } => {
                let (av, bv) = (ctx.get(&frame, *a).clone(), ctx.get(&frame, *b).clone());
                let v = binop(*op, ctx, &av, &bv)?;
                ctx.set(&frame, *temp, v);
            }
            IrInst::Un { op, temp, a } => {
                let av = ctx.get(&frame, *a).clone();
                ctx.set(&frame, *temp, match op {
                    IrUnOp::Neg => match av {
                        IrValue::Int(i) => IrValue::Int(-i),
                        IrValue::Float(f) => IrValue::Float(-f),
                        _ => return Err(IrError::msg("TypeError", "unary -")),
                    },
                    IrUnOp::Not => IrValue::Bool(!av.as_bool()),
                    IrUnOp::BitNot => match av {
                        IrValue::Int(i) => IrValue::Int(!i),
                        _ => return Err(IrError::msg("TypeError", "~")),
                    },
                });
            }
            IrInst::Jump { label } => {
                pc = find_label(func, *label)?;
                continue;
            }
            IrInst::JumpIf { temp, label } => {
                if ctx.get(&frame, *temp).as_bool() {
                    pc = find_label(func, *label)?;
                    continue;
                }
            }
            IrInst::JumpIfNot { temp, label } => {
                if !ctx.get(&frame, *temp).as_bool() {
                    pc = find_label(func, *label)?;
                    continue;
                }
            }
            IrInst::JumpIfErr { temp, label } => {
                if ctx.get(&frame, *temp).is_err() {
                    pc = find_label(func, *label)?;
                    continue;
                }
            }
            IrInst::JumpIfNull { temp, label } => {
                if matches!(ctx.get(&frame, *temp), IrValue::Opt(None)) {
                    pc = find_label(func, *label)?;
                    continue;
                }
            }
            IrInst::Label { .. } => {}
            // ---- Phase 6：defer / errdefer ----
            IrInst::PushDefer { id } => frame.defers.push(*id),
            IrInst::JumpIfNotDefer { id, label } => {
                if !frame.defers.contains(id) {
                    pc = find_label(func, *label)?;
                    continue;
                }
            }
            IrInst::PopDefer { id } => {
                // 移除最近一次登记（rposition）；未登记（分支未达）→ 无操作
                if let Some(pos) = frame.defers.iter().rposition(|d| d == id) {
                    frame.defers.remove(pos);
                }
            }
            IrInst::Call { name, args, temp } => {
                let arg_vals: Vec<IrValue> =
                    args.iter().map(|a| ctx.get(&frame, *a).clone()).collect();
                // Phase 7：隐式环境限定名（io.print / io.fs.open / alloc.init…）与
                // 虚拟根（json.parse / csv.parse / String.from）——未登记为用户函数时按
                // 「根值 → 字段 → 方法」路由（对齐 oracle eval_call 隐式环境 + 方法分派）；
                // 登记了同名用户函数则优先用户函数。
                if !module.func_index.contains_key(name) {
                    let root = name.split('.').next().unwrap_or("");
                    if is_dotted_implicit_root(root) && name.contains('.') {
                        let v = call_dotted_implicit(ctx, module, name, &arg_vals)?;
                        ctx.set(&frame, *temp, v);
                        pc += 1;
                        continue;
                    }
                }
                let callee_idx = pick_func(ctx, module, name, &arg_vals).ok_or_else(|| {
                    IrError::msg("NoFunction", format!("no function `{name}`"))
                })?;
                let v = exec_func(ctx, module, callee_idx, &arg_vals, depth + 1)?;
                ctx.set(&frame, *temp, v);
            }
            IrInst::CallBuiltin { name, args, temp } => {
                let arg_vals: Vec<IrValue> =
                    args.iter().map(|a| ctx.get(&frame, *a).clone()).collect();
                let v = call_builtin(ctx, module, name, &arg_vals, &mut fail)?;
                ctx.set(&frame, *temp, v);
            }
            IrInst::Return { temp } => {
                let v = ctx.get(&frame, *temp).clone();
                if let Some(f) = fail {
                    return Err(IrError::msg("AssertFailed", f));
                }
                return Ok(v);
            }
            IrInst::ReturnVoid => {
                if let Some(f) = fail {
                    return Err(IrError::msg("AssertFailed", f));
                }
                return Ok(IrValue::Void);
            }
            // ---- Phase 1 指针 ----
            IrInst::AddrSlot { temp, slot } => {
                let cell = frame.cells[*slot];
                ctx.set(&frame, *temp, IrValue::Ptr(cell));
            }
            IrInst::AddrValue { temp, value } => {
                // 非 lvalue 取址快照：求值结果复制进新 cell（对齐 tree-walking `&expr` 兜底）
                let v = ctx.get(&frame, *value).clone();
                let cell = ctx.alloc(Cell::Value(v));
                ctx.set(&frame, *temp, IrValue::Ptr(cell));
            }
            IrInst::Deref { temp, a } => {
                // 解引用：Ptr → pointee；非 Ptr → 恒等（对齐 tree-walking `deref_value`）
                let v = match ctx.get(&frame, *a) {
                    IrValue::Ptr(cell) => ctx.cell_value(*cell).clone(),
                    other => other.clone(),
                };
                ctx.set(&frame, *temp, v);
            }
            IrInst::StorePtr { target, value } => {
                let t = ctx.get(&frame, *target).clone();
                let v = ctx.get(&frame, *value).clone();
                match t {
                    IrValue::Ptr(cell) => ctx.set_cell(cell, v),
                    _ => return Err(IrError::msg("BadAssign", "store to non-pointer")),
                }
            }
            // ---- Phase 2 聚合 ----
            IrInst::Field { temp, base, field } => {
                let bv = deref_value(ctx, ctx.get(&frame, *base)).clone();
                let v = field_value(ctx, &bv, field)?;
                ctx.set(&frame, *temp, v);
            }
            IrInst::StoreField { base, field, value } => {
                let bv = deref_value(ctx, ctx.get(&frame, *base)).clone();
                let v = ctx.get(&frame, *value).clone();
                store_field(ctx, &bv, field, v)?;
            }
            IrInst::Index { temp, base, index } => {
                let bv = deref_value(ctx, ctx.get(&frame, *base)).clone();
                let iv = deref_value(ctx, ctx.get(&frame, *index)).clone();
                let i = as_index(ctx, &iv)?;
                let v = index_value(ctx, &bv, i)?;
                ctx.set(&frame, *temp, v);
            }
            IrInst::StoreIndex { base, index, value } => {
                let bv = deref_value(ctx, ctx.get(&frame, *base)).clone();
                let iv = deref_value(ctx, ctx.get(&frame, *index)).clone();
                let i = as_index(ctx, &iv)?;
                let v = ctx.get(&frame, *value).clone();
                store_index(ctx, &bv, i, v)?;
            }
            IrInst::SliceOf { temp, base, lo, hi } => {
                let bv = deref_value(ctx, ctx.get(&frame, *base)).clone();
                let lo_v = deref_value(ctx, ctx.get(&frame, *lo)).clone();
                let lo_i = as_index(ctx, &lo_v)?;
                let hi_v = ctx.get(&frame, *hi).clone();
                let (hi_i, open) = match hi_v {
                    IrValue::End => (0, true),
                    other => {
                        let d = deref_value(ctx, &other).clone();
                        (as_index(ctx, &d)?, false)
                    }
                };
                let v = slice_of(ctx, &bv, lo_i, hi_i, open)?;
                ctx.set(&frame, *temp, v);
            }
            IrInst::StoreSlice { base, lo, hi, value } => {
                let bv = deref_value(ctx, ctx.get(&frame, *base)).clone();
                let lo_v = deref_value(ctx, ctx.get(&frame, *lo)).clone();
                let lo_i = as_index(ctx, &lo_v)?;
                let hi_v = ctx.get(&frame, *hi).clone();
                // 开区间 `arr[a..] = v`：对齐 oracle（eval(hi=`__end__`) 报错）→ BadIndex
                if matches!(hi_v, IrValue::End) {
                    return Err(IrError::msg("BadIndex", "open-end store slice"));
                }
                let hi_d = deref_value(ctx, &hi_v).clone();
                let hi_i = as_index(ctx, &hi_d)?;
                let v = ctx.get(&frame, *value).clone();
                store_slice(ctx, &bv, lo_i, hi_i, &v)?;
            }
            IrInst::MakeArr { temp, items } => {
                let mut cells = Vec::with_capacity(items.len());
                for it in items {
                    let v = ctx.get(&frame, *it).clone();
                    cells.push(ctx.alloc(Cell::Value(v)));
                }
                let c = ctx.alloc(Cell::Elems(cells));
                ctx.set(&frame, *temp, IrValue::Arr(c));
            }
            IrInst::MakeClass { temp, ty, fields } => {
                let mut fs = HashMap::new();
                for (k, vt) in fields {
                    let v = ctx.get(&frame, *vt).clone();
                    fs.insert(k.clone(), ctx.alloc(Cell::Value(v)));
                }
                let c = ctx.alloc(Cell::Class {
                    name: ty.clone(),
                    fields: fs,
                });
                ctx.set(&frame, *temp, IrValue::Class(c));
            }
            IrInst::MakeEnum { temp, name, variant, payload } => {
                let p = match payload {
                    Some(pt) => Some(Box::new(ctx.get(&frame, *pt).clone())),
                    None => None,
                };
                ctx.set(
                    &frame,
                    *temp,
                    IrValue::Enum {
                        name: name.clone(),
                        variant: variant.clone(),
                        payload: p,
                    },
                );
            }
            IrInst::Destructure { value, slots } => {
                let v = deref_value(ctx, ctx.get(&frame, *value)).clone();
                let elems = match v {
                    IrValue::Arr(c) => match &ctx.cells[c] {
                        Cell::Elems(e) => e.clone(),
                        _ => return Err(IrError::msg("TupleArity", "expected tuple in destructure")),
                    },
                    _ => return Err(IrError::msg("TupleArity", "expected tuple in destructure")),
                };
                if elems.len() != slots.len() {
                    return Err(IrError::msg("TupleArity", "destructure arity mismatch"));
                }
                for (slot, elem) in slots.iter().zip(elems.iter()) {
                    if let Some(s) = slot {
                        let v = ctx.cell_value(*elem).clone();
                        ctx.set(&frame, *s, v);
                    }
                }
            }
            IrInst::Move { temp, a } => {
                let v = ctx.get(&frame, *a).clone();
                ctx.set(&frame, *temp, v);
            }
            IrInst::Unwrap { temp, a } => {
                let v = deref_value(ctx, ctx.get(&frame, *a)).clone();
                let r = match v {
                    IrValue::Opt(Some(inner)) => *inner,
                    IrValue::Opt(None) => {
                        return Err(IrError::msg("NullUnwrap", "unwrap of null"));
                    }
                    other => other,
                };
                ctx.set(&frame, *temp, r);
            }
            // ---- Phase 3 switch / 区间 / for ----
            IrInst::MatchTest { temp, subject, pattern } => {
                let sv = deref_value(ctx, ctx.get(&frame, *subject)).clone();
                ctx.set(&frame, *temp, IrValue::Bool(match_pattern(&sv, pattern)));
            }
            IrInst::MakeRange { temp, lo, hi } => {
                let lo_v = deref_value(ctx, ctx.get(&frame, *lo)).clone();
                let hi_v = deref_value(ctx, ctx.get(&frame, *hi)).clone();
                let (lo_i, hi_i) = match (lo_v, hi_v) {
                    (IrValue::Int(a), IrValue::Int(b)) => (a, b),
                    _ => {
                        return Err(IrError::msg(
                            "TypeError",
                            "range bounds must be integers",
                        ))
                    }
                };
                let mut cells = Vec::new();
                let mut i = lo_i;
                while i < hi_i {
                    cells.push(ctx.alloc(Cell::Value(IrValue::Int(i))));
                    i += 1;
                }
                let c = ctx.alloc(Cell::Elems(cells));
                ctx.set(&frame, *temp, IrValue::Arr(c));
            }
            IrInst::EnumPayload { temp, a } => {
                let av = ctx.get(&frame, *a).clone();
                let v = enum_payload(ctx, &av)?;
                ctx.set(&frame, *temp, v);
            }
            IrInst::IterMake { temp, base } => {
                let bv = ctx.get(&frame, *base).clone();
                let items = make_iter(ctx, module, &bv, depth)?;
                let c = ctx.alloc(Cell::Iter { items, next: 0 });
                ctx.set(&frame, *temp, IrValue::Iter(c));
            }
            IrInst::IterNext { has, iter, slot, read_only } => {
                let iter_c = match ctx.get(&frame, *iter) {
                    IrValue::Iter(c) => *c,
                    _ => return Err(IrError::msg("NotIterable", "expected iterator")),
                };
                let item = {
                    let c = &mut ctx.cells[iter_c];
                    match c {
                        Cell::Iter { items, next } => {
                            if *next < items.len() {
                                let it = items[*next].clone();
                                *next += 1;
                                Some(it)
                            } else {
                                None
                            }
                        }
                        _ => return Err(IrError::msg("NotIterable", "corrupt iterator cell")),
                    }
                };
                match item {
                    Some(it) => {
                        if *read_only {
                            // Read 捕获：槽 cell 置为该项值副本（与容器无别名）
                            let v = ctx.cell_value(it.cell).clone();
                            ctx.set_cell(frame.cells[*slot], v);
                        } else {
                            // Mut/Move 捕获：槽 cell 绑定共享源 cell（写穿）；
                            // [IrInst::IterWriteBack] 在 run_ir 为无操作（槽 cell 即源 cell）。
                            frame.cells[*slot] = it.cell;
                        }
                        ctx.set(&frame, *has, IrValue::Bool(true));
                    }
                    None => {
                        ctx.set(&frame, *has, IrValue::Bool(false));
                    }
                }
            }
            IrInst::IterWriteBack { .. } => {}
            // ---- Phase 4 闭包 / 函数引用 / 方法 / 动态调用 ----
            IrInst::MakeClosure {
                temp,
                func,
                captures,
                is_move,
                is_mut,
            } => {
                let mut cap_cells = Vec::with_capacity(captures.len());
                for (_, slot) in captures {
                    let cell = frame.cells[*slot];
                    if *is_move {
                        // move 捕获：深拷贝到新 cell（闭包脱离原作用域生命周期）
                        let v = ctx.cell_value(cell).clone();
                        let dv = deep_copy(ctx, v);
                        let ncell = ctx.alloc(Cell::Value(dv));
                        cap_cells.push(ncell);
                    } else {
                        // 读/mut 捕获：共享源 cell（写穿）
                        cap_cells.push(cell);
                    }
                }
                ctx.set(
                    &frame,
                    *temp,
                    IrValue::Closure {
                        func: *func,
                        captures: cap_cells,
                        is_mut: *is_mut,
                    },
                );
            }
            IrInst::FnRef { temp, name } => {
                ctx.set(&frame, *temp, IrValue::Fn(name.clone()));
            }
            // ---- Phase 5：global / const ----
            IrInst::LoadGlobal { temp, name } => {
                let cell = ctx.globals.get(name).copied().ok_or_else(|| {
                    IrError::msg("NoGlobal", format!("undefined global `{name}`"))
                })?;
                let v = ctx.cell_value(cell).clone();
                ctx.set(&frame, *temp, v);
            }
            IrInst::StoreGlobal { name, value } => {
                let cell = ctx.globals.get(name).copied().ok_or_else(|| {
                    IrError::msg("NoGlobal", format!("undefined global `{name}`"))
                })?;
                let v = ctx.get(&frame, *value).clone();
                ctx.set_cell(cell, v);
            }
            // `&global`：预分配 cell 的 Ptr 别名（与局部 `AddrSlot` 同构，写穿共享 cell）
            IrInst::GlobalAddr { temp, name } => {
                let cell = ctx.globals.get(name).copied().ok_or_else(|| {
                    IrError::msg("NoGlobal", format!("undefined global `{name}`"))
                })?;
                ctx.set(&frame, *temp, IrValue::Ptr(cell));
            }
            IrInst::CallIndirect { temp, callee, args } => {
                let callee_v = ctx.get(&frame, *callee).clone();
                let arg_vals: Vec<IrValue> =
                    args.iter().map(|a| ctx.get(&frame, *a).clone()).collect();
                let v = match callee_v {
                    IrValue::Fn(fname) => {
                        let idx = pick_func(ctx, module, &fname, &arg_vals).ok_or_else(|| {
                            IrError::msg("NoFunction", format!("no function `{fname}`"))
                        })?;
                        exec_func(ctx, module, idx, &arg_vals, depth + 1)?
                    }
                    IrValue::Closure {
                        func,
                        captures,
                        is_mut,
                        ..
                    } => call_closure_ir(ctx, module, func, &captures, &arg_vals, is_mut, depth + 1)?,
                    other => {
                        return Err(IrError::msg(
                            "NotCallable",
                            format!("`{}` is not callable", type_descr(&other)),
                        ))
                    }
                };
                ctx.set(&frame, *temp, v);
            }
            IrInst::CallMethod {
                temp,
                base,
                method,
                args,
            } => {
                let self_v = deref_value(ctx, ctx.get(&frame, *base)).clone();
                let mut arg_vals = vec![self_v.clone()];
                for a in args {
                    arg_vals.push(ctx.get(&frame, *a).clone());
                }
                let v = call_method_ir(ctx, module, &self_v, method, &arg_vals)?;
                ctx.set(&frame, *temp, v);
            }
        }
        pc += 1;
    }
}

/// 方法调用（对齐 oracle `interp.rs:2405-2421`）：先试内建方法 shim（标量/Str/Arr），
/// 再 `"{type}.{method}"` 静态方法表分派（self 已注入为首参）。
fn call_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    self_v: &IrValue,
    method: &str,
    arg_vals: &[IrValue],
) -> R<IrValue> {
    if let Some(v) = call_builtin_method(ctx, module, self_v, method, &arg_vals[1..])? {
        return Ok(v);
    }
    let fname = format!("{}.{}", ir_type_name(ctx, self_v), method);
    let idx = pick_func(ctx, module, &fname, arg_vals)
        .ok_or_else(|| IrError::msg("NoMethod", format!("no method `{fname}`")))?;
    exec_func(ctx, module, idx, arg_vals, 0)
}

// ==================== Phase 7 内建运行时（对齐 oracle interp.rs call_builtin* 面） ====================

/// 隐式环境名（对齐 oracle interp.rs:1585-1595 的隐式环境注入表）
const IMPLICIT_ENV: &[&str] = &[
    "alloc", "io", "test_io", "stdout", "stderr", "pi", "Vec", "Deque", "Map", "Table",
];

/// 限定名根的隐式环境/虚拟根分派（io.*、alloc.*、json.parse、csv.parse、String.from、math.*）
fn is_dotted_implicit_root(root: &str) -> bool {
    IMPLICIT_ENV.contains(&root) || matches!(root, "json" | "csv" | "String" | "math")
}

/// 错误值（码 = 编译期错误码表；内建产生的错误与 `error.X` 字面量同码）
fn err_val(module: &IrModule, name: &str) -> IrValue {
    let code = module.error_codes.get(name).copied().unwrap_or(0);
    IrValue::Err {
        name: name.to_string(),
        code,
    }
}

fn str_val(s: &str) -> IrValue {
    IrValue::Str(s.as_bytes().to_vec())
}
fn str_bytes_val(b: Vec<u8>) -> IrValue {
    IrValue::Str(b)
}
fn opt_val(v: Option<IrValue>) -> IrValue {
    IrValue::Opt(v.map(Box::new))
}

/// 元素数组 → Arr（元素为普通值 cell）
fn make_arr(ctx: &mut Ctx, items: Vec<IrValue>) -> IrValue {
    let elems: Vec<usize> = items
        .into_iter()
        .map(|v| ctx.alloc(Cell::Value(v)))
        .collect();
    IrValue::Arr(ctx.alloc(Cell::Elems(elems)))
}

/// M5.4 Io 实例（含 fs/time/net 子模块——对齐 oracle `io_value` interp.rs:1023-1029）
fn io_value_ir(ctx: &mut Ctx) -> IrValue {
    let fs = ctx.alloc(Cell::Class {
        name: "Fs".into(),
        fields: HashMap::new(),
    });
    let fs_cell = ctx.alloc(Cell::Value(IrValue::Class(fs)));
    let time = ctx.alloc(Cell::Class {
        name: "Time".into(),
        fields: HashMap::new(),
    });
    let time_cell = ctx.alloc(Cell::Value(IrValue::Class(time)));
    let net = ctx.alloc(Cell::Class {
        name: "Net".into(),
        fields: HashMap::new(),
    });
    let net_cell = ctx.alloc(Cell::Value(IrValue::Class(net)));
    let mut fields = HashMap::new();
    fields.insert("fs".into(), fs_cell);
    fields.insert("time".into(), time_cell);
    fields.insert("net".into(), net_cell);
    IrValue::Class(ctx.alloc(Cell::Class {
        name: "Io".into(),
        fields,
    }))
}

/// 隐式环境值（对齐 oracle 隐式环境注入：alloc→Alloc、io/test_io/stdout/stderr→Io、
/// pi→Float(PI)、Vec/Deque/Table→空 Arr、Map→空 Map）
fn implicit_env_value(ctx: &mut Ctx, name: &str) -> IrValue {
    match name {
        "alloc" => IrValue::Class(ctx.alloc(Cell::Class {
            name: "Alloc".into(),
            fields: HashMap::new(),
        })),
        "io" | "test_io" | "stdout" | "stderr" => io_value_ir(ctx),
        "pi" => IrValue::Float(std::f64::consts::PI),
        "Vec" | "Deque" | "Table" => make_arr(ctx, Vec::new()),
        "Map" => IrValue::Class(ctx.alloc(Cell::Class {
            name: "Map".into(),
            fields: HashMap::new(),
        })),
        _ => IrValue::Void,
    }
}

/// 可迭代值 → 元素值数组（iter/filter/map/sort/binary_search 共用；对齐 oracle
/// `iter_to_arr` interp.rs:1307-1357 的元素浅克隆语义）
fn arr_items(ctx: &mut Ctx, v: &IrValue) -> R<Vec<IrValue>> {
    match deref_value(ctx, v).clone() {
        IrValue::Arr(c) => match &ctx.cells[c] {
            Cell::Elems(e) => Ok(e.iter().map(|ec| ctx.cell_value(*ec).clone()).collect()),
            _ => Err(IrError::msg("TypeError", "bad array")),
        },
        IrValue::Slice { data, start, len } => match &ctx.cells[data] {
            Cell::Elems(e) => Ok(e[start..start + len]
                .iter()
                .map(|ec| ctx.cell_value(*ec).clone())
                .collect()),
            _ => Err(IrError::msg("TypeError", "bad slice")),
        },
        IrValue::Str(s) => Ok(s.iter().map(|b| IrValue::Int(*b as i128)).collect()),
        IrValue::Class(c) if class_name(ctx, c) == "Map" => {
            let fields = match &ctx.cells[c] {
                Cell::Class { fields, .. } => fields.clone(),
                _ => unreachable!(),
            };
            let mut out = Vec::new();
            for (k, vc) in fields {
                let mut f = HashMap::new();
                f.insert("key".into(), ctx.alloc(Cell::Value(str_val(&k))));
                f.insert("value".into(), vc);
                out.push(IrValue::Class(ctx.alloc(Cell::Class {
                    name: "KV".into(),
                    fields: f,
                })));
            }
            Ok(out)
        }
        _ => Err(IrError::msg("NotIterable", "value is not iterable")),
    }
}

/// 任意可迭代值 → 元素数组（含用户 IIterable——复用 `make_iter` 的 next() 展开）
fn iter_to_arr_ir(ctx: &mut Ctx, module: &IrModule, v: &IrValue, depth: usize) -> R<IrValue> {
    let items = make_iter(ctx, module, v, depth)?;
    let mut out = Vec::new();
    for it in items {
        out.push(ctx.cell_value(it.cell).clone());
    }
    Ok(make_arr(ctx, out))
}

/// Str/Arr/Slice → 字节（对齐 oracle `value_bytes` interp.rs:1436-1460）
fn value_bytes_ir(ctx: &Ctx, v: &IrValue) -> Option<Vec<u8>> {
    match deref_value(ctx, v) {
        IrValue::Str(s) => Some(s.clone()),
        IrValue::Arr(c) => match &ctx.cells[*c] {
            Cell::Elems(e) => Some(
                e.iter()
                    .map(|ec| match ctx.cell_value(*ec) {
                        IrValue::Int(i) => *i as u8,
                        _ => 0,
                    })
                    .collect(),
            ),
            _ => None,
        },
        IrValue::Slice { data, start, len } => match &ctx.cells[*data] {
            Cell::Elems(e) => {
                let mut out = Vec::with_capacity(*len);
                for i in 0..*len {
                    match ctx.cell_value(e[*start + i]) {
                        IrValue::Int(n) => out.push(*n as u8),
                        _ => return None,
                    }
                }
                Some(out)
            }
            _ => None,
        },
        _ => None,
    }
}

/// 任意值 → 字节（标量/嵌套；Int 在 i32 范围用 4 字节——对齐 oracle `value_to_bytes`
/// interp.rs:5345-5369；Class 无布局表 → 空（Phase 7 取舍：堆类型请用 to_json）。
/// 当前 run_ir 侧 Str/Arr 的 to_bytes 走内联实现；本 helper 预留给 P7e LLVM 原生后端。
#[allow(dead_code)]
fn value_to_bytes_ir(ctx: &Ctx, v: &IrValue) -> Vec<u8> {
    match v {
        IrValue::Int(i) => {
            if *i >= i32::MIN as i128 && *i <= i32::MAX as i128 {
                (*i as i32).to_le_bytes().to_vec()
            } else {
                (*i as i64).to_le_bytes().to_vec()
            }
        }
        IrValue::Float(f) => f.to_le_bytes().to_vec(),
        IrValue::Bool(b) => vec![if *b { 1 } else { 0 }],
        IrValue::Str(s) => {
            let mut out = (s.len() as u64).to_le_bytes().to_vec();
            out.extend_from_slice(s);
            out
        }
        IrValue::Ptr(c) => value_to_bytes_ir(ctx, ctx.cell_value(*c)),
        _ => vec![],
    }
}

/// 任意值 → JSON 字符串（对齐 oracle `value_to_json` interp.rs:5372-5412）
fn value_to_json_ir(ctx: &Ctx, v: &IrValue) -> String {
    match v {
        IrValue::Int(i) => i.to_string(),
        IrValue::Float(f) => f.to_string(),
        IrValue::Bool(b) => b.to_string(),
        IrValue::Str(s) => format!(
            "\"{}\"",
            String::from_utf8_lossy(s)
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
        ),
        IrValue::Arr(c) => match &ctx.cells[*c] {
            Cell::Elems(e) => {
                let items: Vec<String> = e
                    .iter()
                    .map(|ec| value_to_json_ir(ctx, ctx.cell_value(*ec)))
                    .collect();
                format!("[{}]", items.join(","))
            }
            _ => "null".into(),
        },
        IrValue::Slice { data, start, len } => match &ctx.cells[*data] {
            Cell::Elems(e) => {
                let items: Vec<String> = e[*start..*start + *len]
                    .iter()
                    .map(|ec| value_to_json_ir(ctx, ctx.cell_value(*ec)))
                    .collect();
                format!("[{}]", items.join(","))
            }
            _ => "null".into(),
        },
        IrValue::Class(c) => {
            let items: Vec<String> = match &ctx.cells[*c] {
                Cell::Class { fields, .. } => fields
                    .iter()
                    .map(|(k, vc)| {
                        format!("\"{k}\":{}", value_to_json_ir(ctx, ctx.cell_value(*vc)))
                    })
                    .collect(),
                _ => Vec::new(),
            };
            format!("{{{}}}", items.join(","))
        }
        IrValue::Opt(Some(b)) => value_to_json_ir(ctx, b),
        IrValue::Opt(None) => "null".into(),
        IrValue::Ptr(c) => value_to_json_ir(ctx, ctx.cell_value(*c)),
        IrValue::Err { name, .. } => format!("\"error.{name}\""),
        _ => "null".into(),
    }
}

/// @intCast 目标宽度范围（Debug 溢出检查；对齐 oracle `int_width_bounds` interp.rs:5067-5083）
fn int_width_bounds_ir(ty: &str) -> Option<(i128, i128)> {
    match ty {
        "i8" => Some((i8::MIN as i128, i8::MAX as i128)),
        "i16" => Some((i16::MIN as i128, i16::MAX as i128)),
        "i32" => Some((i32::MIN as i128, i32::MAX as i128)),
        "i64" => Some((i64::MIN as i128, i64::MAX as i128)),
        "i128" => Some((i128::MIN, i128::MAX)),
        "isize" => Some((isize::MIN as i128, isize::MAX as i128)),
        "u8" => Some((0, u8::MAX as i128)),
        "u16" => Some((0, u16::MAX as i128)),
        "u32" => Some((0, u32::MAX as i128)),
        "u64" => Some((0, u64::MAX as i128)),
        "u128" => Some((0, u128::MAX as i128)),
        "usize" => Some((0, usize::MAX as i128)),
        _ => None,
    }
}

/// @sizeOf(T) 标量表（对齐 oracle `type_size_of` interp.rs:5086-5122 的标量/引用面；
/// 用户 class/enum 无布局表 → None）
fn scalar_size_ir(ty: &str) -> Option<usize> {
    match ty {
        "i8" | "u8" | "bool" => Some(1),
        "i16" | "u16" | "f16" => Some(2),
        "i32" | "u32" | "f32" => Some(4),
        "i64" | "u64" | "isize" | "usize" | "f64" => Some(8),
        "i128" | "u128" | "f128" => Some(16),
        "String" | "Vec" | "Map" | "Deque" | "Table" | "Allocator" => Some(8),
        _ => None,
    }
}

/// 调用函数值（Fn 引用 / Closure；对齐 oracle `call_closure_value` interp.rs:1504-1511）
fn call_closure_value_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    f: &IrValue,
    args: &[IrValue],
) -> R<IrValue> {
    match f {
        IrValue::Closure {
            func,
            captures,
            is_mut,
            ..
        } => call_closure_ir(ctx, module, *func, captures, args, *is_mut, 0),
        IrValue::Fn(name) => {
            let idx = pick_func(ctx, module, name, args)
                .ok_or_else(|| IrError::msg("NoFunction", format!("no function `{name}`")))?;
            exec_func(ctx, module, idx, args, 0)
        }
        _ => Err(IrError::msg("TypeError", "expected function")),
    }
}

fn call_closure_bool_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    f: &IrValue,
    args: &[IrValue],
) -> R<bool> {
    Ok(call_closure_value_ir(ctx, module, f, args)?.as_bool())
}

/// io.print（对齐 oracle `call_io_print` interp.rs:4029-4087）：`{}` 与 `{x}`/`{b}`/`{s}`
/// 占位符格式化；输出缓冲到 `ctx.out`（`execute_ir` 运行后冲刷）
fn call_io_print_ir(ctx: &mut Ctx, args: &[IrValue]) -> R<()> {
    if args.is_empty() {
        return Err(IrError::msg(
            "ArityMismatch",
            "io.print expects a format string",
        ));
    }
    let fmt = match deref_value(ctx, &args[0]) {
        IrValue::Str(s) => s.clone(),
        _ => return Err(IrError::msg("TypeError", "io.print expects &[u8]")),
    };
    let mut out = Vec::new();
    let mut argi = 1usize;
    let mut i = 0usize;
    while i < fmt.len() {
        if fmt[i] == b'{' && i + 1 < fmt.len() && fmt[i + 1] == b'}' {
            if argi < args.len() {
                let v = deref_value(ctx, &args[argi]);
                out.extend_from_slice(v.display(ctx).as_bytes());
                argi += 1;
            }
            i += 2;
        } else if fmt[i] == b'{'
            && i + 2 < fmt.len()
            && fmt[i + 2] == b'}'
            && (fmt[i + 1] == b'x' || fmt[i + 1] == b'b' || fmt[i + 1] == b's')
        {
            let spec = fmt[i + 1];
            if argi < args.len() {
                let v = deref_value(ctx, &args[argi]);
                match spec {
                    b'x' => match v {
                        IrValue::Int(n) => out.extend_from_slice(format!("{n:x}").as_bytes()),
                        _ => out.extend_from_slice(v.display(ctx).as_bytes()),
                    },
                    b'b' => match v {
                        IrValue::Int(n) => out.extend_from_slice(format!("{n:b}").as_bytes()),
                        _ => out.extend_from_slice(v.display(ctx).as_bytes()),
                    },
                    _ => out.extend_from_slice(v.display(ctx).as_bytes()),
                }
                argi += 1;
            }
            i += 3;
        } else {
            out.push(fmt[i]);
            i += 1;
        }
    }
    ctx.out.extend_from_slice(&out);
    Ok(())
}

/// 标量方法（ICompare/INumber 族内建：add/sub/mul/div/neg/mod/abs/eq/lt/pow；
/// 对齐 oracle `call_scalar_method` interp.rs:3408-3509）
fn call_scalar_method_ir(
    ctx: &Ctx,
    self_v: &IrValue,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    // 一元整数操作
    if args.is_empty() {
        if let IrValue::Int(a) = self_v {
            let v = match field {
                "neg" => Some(IrValue::Int(-*a)),
                "abs" => Some(IrValue::Int(a.abs())),
                _ => None,
            };
            if let Some(v) = v {
                return Ok(Some(v));
            }
        }
    }
    // 二元整数操作（保持整数语义：div 截断、mod 取余、溢出检查）
    if args.len() == 1 {
        let b = deref_value(ctx, &args[0]);
        if let (IrValue::Int(a), IrValue::Int(b)) = (self_v, b) {
            let v = match field {
                "add" => Some(IrValue::Int(
                    a.checked_add(*b)
                        .ok_or_else(|| IrError::msg("Overflow", "integer overflow"))?,
                )),
                "sub" => Some(IrValue::Int(
                    a.checked_sub(*b)
                        .ok_or_else(|| IrError::msg("Overflow", "integer overflow"))?,
                )),
                "mul" => Some(IrValue::Int(
                    a.checked_mul(*b)
                        .ok_or_else(|| IrError::msg("Overflow", "integer overflow"))?,
                )),
                "div" => {
                    if *b == 0 {
                        return Err(IrError::msg("DivisionByZero", "division by zero"));
                    }
                    Some(IrValue::Int(a / b))
                }
                "mod" => {
                    if *b == 0 {
                        return Err(IrError::msg("DivisionByZero", "modulo by zero"));
                    }
                    Some(IrValue::Int(a % b))
                }
                "eq" => Some(IrValue::Bool(a == b)),
                "lt" => Some(IrValue::Bool(a < b)),
                _ => None,
            };
            if let Some(v) = v {
                return Ok(Some(v));
            }
        }
    }
    // 浮点路径（混合 Int/Float 也走此路径）
    let v = match self_v {
        IrValue::Int(i) => *i as f64,
        IrValue::Float(f) => *f,
        _ => return Ok(None),
    };
    let arg_num = |ix: usize| -> R<f64> {
        let a = args
            .get(ix)
            .ok_or_else(|| IrError::msg("ArityMismatch", "missing argument"))?;
        match deref_value(ctx, a) {
            IrValue::Int(i) => Ok(*i as f64),
            IrValue::Float(f) => Ok(*f),
            _ => Err(IrError::msg("TypeError", "expected number")),
        }
    };
    let r = match field {
        "add" => v + arg_num(0)?,
        "sub" => v - arg_num(0)?,
        "mul" => v * arg_num(0)?,
        "div" => v / arg_num(0)?,
        "mod" => v % arg_num(0)?,
        "neg" => -v,
        "abs" => v.abs(),
        "pow" => v.powf(arg_num(0)?),
        "eq" | "lt" => {
            let other = arg_num(0)?;
            let b = match field {
                "eq" => v == other,
                _ => v < other,
            };
            return Ok(Some(IrValue::Bool(b)));
        }
        _ => return Ok(None),
    };
    // 整数保持整数（无小数部分时）
    if r.fract() == 0.0 && r.is_finite() && r.abs() < 9e18 {
        Ok(Some(IrValue::Int(r as i128)))
    } else {
        Ok(Some(IrValue::Float(r)))
    }
}

// ---- 解析器辅助内建（71：peek/advance/expect/skip_space/is_digit/parse_number）----

fn parser_bytes(ctx: &Ctx, args: &[IrValue], ix: usize) -> R<Vec<u8>> {
    let v = args
        .get(ix)
        .ok_or_else(|| IrError::msg("ArityMismatch", "missing argument"))?;
    match deref_value(ctx, v) {
        IrValue::Str(s) => Ok(s.clone()),
        IrValue::Ptr(c) => match ctx.cell_value(*c) {
            IrValue::Str(s) => Ok(s.clone()),
            _ => Err(IrError::msg("TypeError", "expected bytes")),
        },
        _ => Err(IrError::msg("TypeError", "expected bytes")),
    }
}

fn parser_pos(ctx: &Ctx, args: &[IrValue], ix: usize) -> R<usize> {
    let v = args
        .get(ix)
        .ok_or_else(|| IrError::msg("ArityMismatch", "missing argument"))?;
    match deref_value(ctx, v) {
        IrValue::Ptr(c) => Ok(*c),
        _ => Err(IrError::msg("TypeError", "expected pointer")),
    }
}

fn parser_pos_int(ctx: &Ctx, cell: usize) -> R<i128> {
    match ctx.cell_value(cell) {
        IrValue::Int(i) => Ok(*i),
        _ => Err(IrError::msg("TypeError", "expected int position")),
    }
}

/// 对齐 oracle `call_parser_builtin` interp.rs:4656-4769
fn call_parser_builtin_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    name: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let _ = module;
    match name {
        "skip_space" => {
            let data = parser_bytes(ctx, args, 0)?;
            let pc = parser_pos(ctx, args, 1)?;
            let mut i = parser_pos_int(ctx, pc)? as usize;
            while i < data.len() && data[i].is_ascii_whitespace() {
                i += 1;
            }
            ctx.set_cell(pc, IrValue::Int(i as i128));
            Ok(Some(IrValue::Void))
        }
        "peek" => {
            let data = parser_bytes(ctx, args, 0)?;
            let pc = parser_pos(ctx, args, 1)?;
            let i = parser_pos_int(ctx, pc)? as usize;
            Ok(Some(if i < data.len() {
                IrValue::Opt(Some(Box::new(IrValue::Int(data[i] as i128))))
            } else {
                IrValue::Opt(None)
            }))
        }
        "advance" => {
            let pc = parser_pos(ctx, args, 1)?;
            let i = parser_pos_int(ctx, pc)?;
            ctx.set_cell(pc, IrValue::Int(i + 1));
            Ok(Some(IrValue::Void))
        }
        "expect" => {
            let data = parser_bytes(ctx, args, 0)?;
            let pc = parser_pos(ctx, args, 1)?;
            let want = match deref_value(
                ctx,
                args.get(2)
                    .ok_or_else(|| IrError::msg("ArityMismatch", "expect"))?,
            ) {
                IrValue::Int(i) => *i as u8,
                _ => return Err(IrError::msg("TypeError", "expected byte")),
            };
            let i = parser_pos_int(ctx, pc)? as usize;
            if i < data.len() && data[i] == want {
                ctx.set_cell(pc, IrValue::Int(i as i128 + 1));
                Ok(Some(IrValue::Void))
            } else {
                Err(IrError::msg("UnexpectedToken", "expect: unexpected token"))
            }
        }
        "is_digit" => {
            let v = deref_value(
                ctx,
                args.get(0)
                    .ok_or_else(|| IrError::msg("ArityMismatch", "is_digit"))?,
            )
            .clone();
            match v {
                IrValue::Int(i) => Ok(Some(IrValue::Bool((i as u8 as char).is_ascii_digit()))),
                _ => Err(IrError::msg("TypeError", "expected int")),
            }
        }
        "parse_number" => {
            let data = parser_bytes(ctx, args, 0)?;
            let pc = parser_pos(ctx, args, 1)?;
            let mut i = parser_pos_int(ctx, pc)? as usize;
            let start = i;
            while i < data.len() && data[i].is_ascii_digit() {
                i += 1;
            }
            let n: i128 = String::from_utf8_lossy(&data[start..i]).parse().unwrap_or(0);
            ctx.set_cell(pc, IrValue::Int(i as i128));
            Ok(Some(IrValue::Int(n)))
        }
        _ => Ok(None),
    }
}

// ---- 数据/路径参数辅助 ----

fn str_arg_ir(ctx: &Ctx, args: &[IrValue], i: usize) -> R<Vec<u8>> {
    let a = args
        .get(i)
        .ok_or_else(|| IrError::msg("ArityMismatch", "missing argument"))?;
    match deref_value(ctx, a) {
        IrValue::Str(s) => Ok(s.clone()),
        _ => Err(IrError::msg("TypeError", "expected &[u8]")),
    }
}

fn path_arg_ir(ctx: &Ctx, args: &[IrValue], i: usize) -> R<String> {
    Ok(String::from_utf8_lossy(&str_arg_ir(ctx, args, i)?).into_owned())
}

fn int_arg_ir(ctx: &Ctx, args: &[IrValue], i: usize) -> R<i128> {
    let a = args
        .get(i)
        .ok_or_else(|| IrError::msg("ArityMismatch", "missing argument"))?;
    match deref_value(ctx, a) {
        IrValue::Int(n) => Ok(*n),
        _ => Err(IrError::msg("TypeError", "expected int")),
    }
}

// ---- File/网络句柄 ----

fn file_fd_ir(ctx: &Ctx, v: &IrValue) -> R<i64> {
    match deref_value(ctx, v) {
        IrValue::Class(c) if class_name(ctx, *c) == "File" => match &ctx.cells[*c] {
            Cell::Class { fields, .. } => match fields.get("_fd") {
                Some(fc) => match ctx.cell_value(*fc) {
                    IrValue::Int(fd) => Ok(*fd as i64),
                    _ => Err(IrError::msg("BadFd", "bad file descriptor")),
                },
                _ => Err(IrError::msg("BadFd", "bad file descriptor")),
            },
            _ => Err(IrError::msg("BadFd", "bad file descriptor")),
        },
        _ => Err(IrError::msg("TypeError", "expected File")),
    }
}

fn net_fd_ir(ctx: &Ctx, v: &IrValue) -> R<i64> {
    match deref_value(ctx, v) {
        IrValue::Class(c) => match &ctx.cells[*c] {
            Cell::Class { fields, .. } => match fields.get("fd") {
                Some(fc) => match ctx.cell_value(*fc) {
                    IrValue::Int(fd) => Ok(*fd as i64),
                    _ => Err(IrError::msg("BadFd", "bad net descriptor")),
                },
                _ => Err(IrError::msg("BadFd", "bad net descriptor")),
            },
            _ => Err(IrError::msg("BadFd", "bad net descriptor")),
        },
        _ => Err(IrError::msg("TypeError", "expected connection")),
    }
}

fn io_error_name_ir(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "NotFound".into(),
        std::io::ErrorKind::PermissionDenied => "PermissionDenied".into(),
        _ => "Io".into(),
    }
}

fn register_file_ir(ctx: &mut Ctx, f: std::fs::File) -> IrValue {
    let fd = ctx.next_fd;
    ctx.next_fd += 1;
    ctx.files.insert(fd, f);
    let mut fields = HashMap::new();
    fields.insert(
        "_fd".into(),
        ctx.alloc(Cell::Value(IrValue::Int(fd as i128))),
    );
    IrValue::Class(ctx.alloc(Cell::Class {
        name: "File".into(),
        fields,
    }))
}

// ---- io.fs / io.time / io.net 方法族（对齐 oracle call_fs_method 等）----

fn call_fs_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "open" => {
            let path = path_arg_ir(ctx, args, 0)?;
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
            {
                Ok(f) => Ok(Some(register_file_ir(ctx, f))),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "create" => {
            let path = path_arg_ir(ctx, args, 0)?;
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(&path)
            {
                Ok(f) => Ok(Some(register_file_ir(ctx, f))),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "read_file" => {
            let path = path_arg_ir(ctx, args, 0)?;
            match std::fs::read(&path) {
                Ok(b) => Ok(Some(str_bytes_val(b))),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "read_all" => {
            let f = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "read_all"))?;
            let fd = file_fd_ir(ctx, f)?;
            let file = ctx
                .files
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad file"))?;
            file.seek(std::io::SeekFrom::Start(0))
                .map_err(|e| IrError::msg("Io", format!("seek: {e}")))?;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)
                .map_err(|e| IrError::msg("Io", format!("read: {e}")))?;
            Ok(Some(str_bytes_val(buf)))
        }
        "write_all" => {
            let f = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "write_all"))?;
            let fd = file_fd_ir(ctx, f)?;
            let data = str_arg_ir(ctx, args, 1)?;
            let file = ctx
                .files
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad file"))?;
            file.write_all(&data)
                .map_err(|e| IrError::msg("Io", format!("write: {e}")))?;
            Ok(Some(IrValue::Void))
        }
        "append" => {
            let path = path_arg_ir(ctx, args, 0)?;
            let data = str_arg_ir(ctx, args, 1)?;
            match std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(&path)
            {
                Ok(mut f) => {
                    f.write_all(&data)
                        .map_err(|e| IrError::msg("Io", format!("append: {e}")))?;
                    Ok(Some(IrValue::Void))
                }
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "remove" => {
            let path = path_arg_ir(ctx, args, 0)?;
            match std::fs::remove_file(&path) {
                Ok(_) => Ok(Some(IrValue::Void)),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "rename" => {
            let from = path_arg_ir(ctx, args, 0)?;
            let to = path_arg_ir(ctx, args, 1)?;
            match std::fs::rename(&from, &to) {
                Ok(_) => Ok(Some(IrValue::Void)),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "read_int" => {
            let path = path_arg_ir(ctx, args, 0)?;
            match std::fs::read(&path) {
                Ok(b) => match String::from_utf8_lossy(&b).trim().parse::<i64>() {
                    Ok(n) => Ok(Some(IrValue::Int(n as i128))),
                    Err(_) => Ok(Some(err_val(module, "InvalidFormat"))),
                },
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "write_int" => {
            let path = path_arg_ir(ctx, args, 0)?;
            let v = int_arg_ir(ctx, args, 1)?;
            match std::fs::write(&path, v.to_string().as_bytes()) {
                Ok(_) => Ok(Some(IrValue::Void)),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "list_dir" => {
            let path = path_arg_ir(ctx, args, 0)?;
            match std::fs::read_dir(&path) {
                Ok(rd) => {
                    let names: Vec<IrValue> = rd
                        .flatten()
                        .map(|e| str_val(&e.file_name().to_string_lossy()))
                        .collect();
                    Ok(Some(make_arr(ctx, names)))
                }
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        _ => Ok(None),
    }
}

fn call_file_method_ir(
    ctx: &mut Ctx,
    _module: &IrModule,
    self_v: &IrValue,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let fd = file_fd_ir(ctx, self_v)?;
    match field {
        "close" => {
            ctx.files.remove(&fd);
            Ok(Some(IrValue::Void))
        }
        "write_all" => {
            let data = str_arg_ir(ctx, args, 0)?;
            let file = ctx
                .files
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad file"))?;
            file.write_all(&data)
                .map_err(|e| IrError::msg("Io", format!("write: {e}")))?;
            Ok(Some(IrValue::Void))
        }
        "read_all" => {
            let file = ctx
                .files
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad file"))?;
            file.seek(std::io::SeekFrom::Start(0))
                .map_err(|e| IrError::msg("Io", format!("seek: {e}")))?;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)
                .map_err(|e| IrError::msg("Io", format!("read: {e}")))?;
            Ok(Some(str_bytes_val(buf)))
        }
        "seek" => {
            let off = int_arg_ir(ctx, args, 0)?;
            let file = ctx
                .files
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad file"))?;
            file.seek(std::io::SeekFrom::Start(off.max(0) as u64))
                .map_err(|e| IrError::msg("Io", format!("seek: {e}")))?;
            Ok(Some(IrValue::Void))
        }
        "pos" => {
            let file = ctx
                .files
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad file"))?;
            let pos = file
                .stream_position()
                .map_err(|e| IrError::msg("Io", format!("pos: {e}")))?;
            Ok(Some(IrValue::Int(pos as i128)))
        }
        "read_at" => {
            let off = int_arg_ir(ctx, args, 0)?;
            let len = int_arg_ir(ctx, args, 1)?.max(0) as usize;
            let file = ctx
                .files
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad file"))?;
            let saved = file
                .stream_position()
                .map_err(|e| IrError::msg("Io", format!("pos: {e}")))?;
            file.seek(std::io::SeekFrom::Start(off.max(0) as u64))
                .map_err(|e| IrError::msg("Io", format!("seek: {e}")))?;
            let mut buf = vec![0u8; len];
            let k = file
                .read(&mut buf)
                .map_err(|e| IrError::msg("Io", format!("read: {e}")))?;
            buf.truncate(k);
            let _ = file.seek(std::io::SeekFrom::Start(saved));
            Ok(Some(str_bytes_val(buf)))
        }
        "write_at" => {
            let off = int_arg_ir(ctx, args, 0)?;
            let data = str_arg_ir(ctx, args, 1)?;
            let file = ctx
                .files
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad file"))?;
            let saved = file
                .stream_position()
                .map_err(|e| IrError::msg("Io", format!("pos: {e}")))?;
            file.seek(std::io::SeekFrom::Start(off.max(0) as u64))
                .map_err(|e| IrError::msg("Io", format!("seek: {e}")))?;
            file.write_all(&data)
                .map_err(|e| IrError::msg("Io", format!("write: {e}")))?;
            let _ = file.seek(std::io::SeekFrom::Start(saved));
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

fn call_time_method_ir(
    ctx: &mut Ctx,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "now" => {
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i128;
            Ok(Some(IrValue::Int(ms)))
        }
        "sleep" => {
            let ms = int_arg_ir(ctx, args, 0)?;
            std::thread::sleep(std::time::Duration::from_millis(ms.max(0) as u64));
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

fn call_net_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "connect" => {
            let host = str_arg_ir(ctx, args, 0)?;
            let port = int_arg_ir(ctx, args, 1)? as u16;
            let host = String::from_utf8_lossy(&host).to_string();
            match std::net::TcpStream::connect((host.as_str(), port)) {
                Ok(stream) => {
                    let fd = ctx.next_net_fd;
                    ctx.next_net_fd += 1;
                    let _ = stream.set_nodelay(true);
                    ctx.tcp_streams.insert(fd, stream);
                    let mut fields = HashMap::new();
                    fields.insert(
                        "fd".into(),
                        ctx.alloc(Cell::Value(IrValue::Int(fd as i128))),
                    );
                    Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                        name: "TcpConn".into(),
                        fields,
                    }))))
                }
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "listen" => {
            let host = str_arg_ir(ctx, args, 0)?;
            let port = int_arg_ir(ctx, args, 1)? as u16;
            let host = String::from_utf8_lossy(&host).to_string();
            let addr = format!("{host}:{port}");
            match std::net::TcpListener::bind(&addr) {
                Ok(listener) => {
                    let fd = ctx.next_net_fd;
                    ctx.next_net_fd += 1;
                    ctx.tcp_listeners.insert(fd, listener);
                    let mut fields = HashMap::new();
                    fields.insert(
                        "fd".into(),
                        ctx.alloc(Cell::Value(IrValue::Int(fd as i128))),
                    );
                    Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                        name: "TcpListener".into(),
                        fields,
                    }))))
                }
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        _ => Ok(None),
    }
}

fn call_conn_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    self_v: &IrValue,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let fd = net_fd_ir(ctx, self_v)?;
    match field {
        "write" => {
            let data = str_arg_ir(ctx, args, 0)?;
            let stream = ctx
                .tcp_streams
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad conn"))?;
            match stream.write_all(&data) {
                Ok(_) => Ok(Some(IrValue::Void)),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "read" => {
            let n = int_arg_ir(ctx, args, 0)?.max(0) as usize;
            let stream = ctx
                .tcp_streams
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad conn"))?;
            let mut buf = vec![0u8; n];
            match stream.read(&mut buf) {
                Ok(0) => Ok(Some(str_bytes_val(vec![]))),
                Ok(k) => {
                    buf.truncate(k);
                    Ok(Some(str_bytes_val(buf)))
                }
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "read_all" => {
            let stream = ctx
                .tcp_streams
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad conn"))?;
            let mut buf = Vec::new();
            match stream.read_to_end(&mut buf) {
                Ok(_) => Ok(Some(str_bytes_val(buf))),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "write_u32_le" => {
            let n = int_arg_ir(ctx, args, 0)?;
            let stream = ctx
                .tcp_streams
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad conn"))?;
            match stream.write_all(&(n as u32).to_le_bytes()) {
                Ok(_) => Ok(Some(IrValue::Void)),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "read_u32_le" => {
            let stream = ctx
                .tcp_streams
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad conn"))?;
            let mut buf = [0u8; 4];
            match stream.read_exact(&mut buf) {
                Ok(_) => Ok(Some(IrValue::Int(u32::from_le_bytes(buf) as i128))),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "shutdown" => {
            let stream = ctx
                .tcp_streams
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad conn"))?;
            match stream.shutdown(std::net::Shutdown::Write) {
                Ok(_) => Ok(Some(IrValue::Void)),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "close" => {
            ctx.tcp_streams.remove(&fd);
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

fn call_listener_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    self_v: &IrValue,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let _ = args;
    let fd = net_fd_ir(ctx, self_v)?;
    match field {
        "local_port" => {
            let listener = ctx
                .tcp_listeners
                .get(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad listener"))?;
            match listener.local_addr() {
                Ok(addr) => Ok(Some(IrValue::Int(addr.port() as i128))),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "accept" => {
            let listener = ctx
                .tcp_listeners
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad listener"))?;
            match listener.accept() {
                Ok((stream, _peer)) => {
                    let cfd = ctx.next_net_fd;
                    ctx.next_net_fd += 1;
                    let _ = stream.set_nodelay(true);
                    ctx.tcp_streams.insert(cfd, stream);
                    let mut fields = HashMap::new();
                    fields.insert(
                        "fd".into(),
                        ctx.alloc(Cell::Value(IrValue::Int(cfd as i128))),
                    );
                    Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                        name: "TcpConn".into(),
                        fields,
                    }))))
                }
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "close" => {
            ctx.tcp_listeners.remove(&fd);
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

fn call_map_method_ir(
    ctx: &mut Ctx,
    self_v: &IrValue,
    method: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let c = match self_v {
        IrValue::Class(c) => *c,
        _ => return Ok(None),
    };
    match method {
        "put" => {
            let k = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "put"))?;
            let v = args
                .get(1)
                .ok_or_else(|| IrError::msg("ArityMismatch", "put"))?;
            let key = deref_value(ctx, k).display(ctx);
            let nc = ctx.alloc(Cell::Value(v.clone()));
            match &mut ctx.cells[c] {
                Cell::Class { fields, .. } => {
                    fields.insert(key, nc);
                    Ok(Some(IrValue::Void))
                }
                _ => Err(IrError::msg("TypeError", "put expects Map")),
            }
        }
        "get" => {
            let k = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "get"))?;
            let key = deref_value(ctx, k).display(ctx);
            let v = match &ctx.cells[c] {
                Cell::Class { fields, .. } => fields.get(&key).map(|fc| ctx.cell_value(*fc).clone()),
                _ => None,
            };
            Ok(Some(opt_val(v)))
        }
        "contains" => {
            let k = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "contains"))?;
            let key = deref_value(ctx, k).display(ctx);
            let b = match &ctx.cells[c] {
                Cell::Class { fields, .. } => fields.contains_key(&key),
                _ => false,
            };
            Ok(Some(IrValue::Bool(b)))
        }
        "remove" => {
            let k = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "remove"))?;
            let key = deref_value(ctx, k).display(ctx);
            match &mut ctx.cells[c] {
                Cell::Class { fields, .. } => {
                    fields.remove(&key);
                    Ok(Some(IrValue::Void))
                }
                _ => Err(IrError::msg("TypeError", "remove expects Map")),
            }
        }
        "len" => {
            let n = match &ctx.cells[c] {
                Cell::Class { fields, .. } => fields.len(),
                _ => 0,
            };
            Ok(Some(IrValue::Int(n as i128)))
        }
        // Map.iter() → KV 条目数组（key/value 字段，与 for |kv| 捕获一致；
        // 对齐 oracle `call_builtin_method` 的 `(_, "iter")` 分支）
        "iter" => {
            let items = arr_items(ctx, self_v)?;
            Ok(Some(make_arr(ctx, items)))
        }
        "to_json" => Ok(Some(str_val(&value_to_json_ir(ctx, self_v)))),
        "from_json" => {
            let json = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "from_json"))?;
            let s = str_arg_ir(ctx, &[json.clone()], 0)?;
            let obj = parse_json_obj_ir(ctx, &String::from_utf8_lossy(&s))?;
            let mut fields = HashMap::new();
            for (k, v) in obj {
                fields.insert(k, ctx.alloc(Cell::Value(v)));
            }
            Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                name: "Map".into(),
                fields,
            }))))
        }
        _ => Ok(None),
    }
}

fn call_alloc_method_ir(
    ctx: &mut Ctx,
    method: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match method {
        // alloc.init(T)：类型名参数建空实例（tag1 IR 无布局表——字段型构造请用
        // 字面量 `alloc.init(T{...})`；对齐 oracle interp.rs:3865-3891 的 Ident 分支）。
        // 实参已是类实例（字面量构造）→ 原样返回（对齐 oracle 字面量分支）。
        "init" => {
            if args.len() != 1 {
                return Err(IrError::msg("ArityMismatch", "alloc.init expects 1 arg"));
            }
            match deref_value(ctx, &args[0]).clone() {
                IrValue::Str(s) => Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                    name: String::from_utf8_lossy(&s).to_string(),
                    fields: HashMap::new(),
                })))),
                IrValue::Class(c) => Ok(Some(IrValue::Class(c))),
                _ => Err(IrError::msg("TypeError", "alloc.init expects type name or literal")),
            }
        }
        "alloc" => {
            let n = int_arg_ir(ctx, args, 0)?;
            Ok(Some(str_bytes_val(vec![0u8; n.max(0) as usize])))
        }
        "deinit" => Ok(Some(IrValue::Void)),
        _ => Ok(None),
    }
}

fn call_arena_method_ir(
    ctx: &mut Ctx,
    method: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match method {
        "alloc" => {
            if let Some(a) = args.first() {
                if matches!(deref_value(ctx, a), IrValue::Int(_)) {
                    let n = int_arg_ir(ctx, args, 0)?;
                    return Ok(Some(str_bytes_val(vec![0u8; n.max(0) as usize])));
                }
            }
            Ok(args.first().cloned())
        }
        "init" => Ok(Some(IrValue::Void)),
        _ => Ok(None),
    }
}

fn call_io_method_ir(
    ctx: &mut Ctx,
    _module: &IrModule,
    method: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match method {
        "print" => {
            call_io_print_ir(ctx, args)?;
            Ok(Some(IrValue::Void))
        }
        // io.exit(ExitType, code)：正常退出信号（execute_ir 视 ExitRequested 为成功）
        "exit" => {
            if args.len() != 2 {
                return Err(IrError::msg("ArityMismatch", "io.exit expects 2 args"));
            }
            let t = deref_value(ctx, &args[0]);
            let code = match deref_value(ctx, &args[1]) {
                IrValue::Int(i) => (*i).clamp(0, 255) as u8,
                _ => return Err(IrError::msg("TypeError", "io.exit expects int code")),
            };
            let is_error = matches!(t, IrValue::Enum { variant, .. } if variant == "Error");
            if is_error {
                eprintln!("error: program exited with code {code}");
            }
            Err(IrError::msg("ExitRequested", format!("code {code}")))
        }
        "stdin" => {
            let mut line = String::new();
            match std::io::stdin().read_line(&mut line) {
                Ok(0) | Err(_) => Ok(Some(str_val(""))),
                Ok(_) => {
                    let trimmed = line.trim_end_matches(['\n', '\r']);
                    Ok(Some(str_val(trimmed)))
                }
            }
        }
        "args" => Ok(Some(make_arr(
            ctx,
            ctx.args.iter().map(|a| IrValue::Str(a.clone())).collect(),
        ))),
        "env" => {
            let name = str_arg_ir(ctx, args, 0)?;
            match std::env::var(String::from_utf8_lossy(&name).as_ref()) {
                Ok(v) => Ok(Some(opt_val(Some(str_val(&v))))),
                Err(_) => Ok(Some(IrValue::Opt(None))),
            }
        }
        _ => Ok(None),
    }
}

// ---- JSON 解析（Map.from_json / json.parse；对齐 oracle parse_json_*）----

fn parse_json_value_ir(ctx: &mut Ctx, s: &str) -> R<(IrValue, usize)> {
    let s = s.trim_start();
    match s.as_bytes().first().copied() {
        Some(b'{') => parse_json_object_ir(ctx, s),
        Some(b'[') => parse_json_array_ir(ctx, s),
        Some(b'"') => parse_json_string_ir(s),
        Some(b't') if s.starts_with("true") => Ok((IrValue::Bool(true), 4)),
        Some(b'f') if s.starts_with("false") => Ok((IrValue::Bool(false), 5)),
        Some(b'n') if s.starts_with("null") => Ok((IrValue::Opt(None), 4)),
        Some(c) if c == b'-' || c.is_ascii_digit() => parse_json_number_ir(s),
        _ => Err(IrError::msg("InvalidJson", "unexpected token")),
    }
}

fn parse_json_object_ir(ctx: &mut Ctx, s: &str) -> R<(IrValue, usize)> {
    let b = s.as_bytes();
    let mut fields: HashMap<String, usize> = HashMap::new();
    let mut pos = 1usize;
    loop {
        while pos < b.len() && b[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= b.len() {
            return Err(IrError::msg("InvalidJson", "unterminated object"));
        }
        if b[pos] == b'}' {
            let class = IrValue::Class(ctx.alloc(Cell::Class {
                name: "Map".into(),
                fields,
            }));
            return Ok((class, pos + 1));
        }
        if b[pos] != b'"' {
            return Err(IrError::msg("InvalidJson", "expected string key"));
        }
        let (key, klen) = parse_json_string_ir(&s[pos..])?;
        pos += klen;
        while pos < b.len() && b[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= b.len() || b[pos] != b':' {
            return Err(IrError::msg("InvalidJson", "expected ':'"));
        }
        pos += 1;
        let (val, vlen) = parse_json_value_ir(ctx, &s[pos..])?;
        pos += vlen;
        if let IrValue::Str(ks) = key {
            fields.insert(String::from_utf8_lossy(&ks).to_string(), ctx.alloc(Cell::Value(val)));
        }
        while pos < b.len() && b[pos].is_ascii_whitespace() {
            pos += 1;
        }
        match b.get(pos).copied() {
            Some(b',') => pos += 1,
            Some(b'}') => {
                let class = IrValue::Class(ctx.alloc(Cell::Class {
                    name: "Map".into(),
                    fields,
                }));
                return Ok((class, pos + 1));
            }
            _ => return Err(IrError::msg("InvalidJson", "expected ',' or '}'")),
        }
    }
}

fn parse_json_array_ir(ctx: &mut Ctx, s: &str) -> R<(IrValue, usize)> {
    let b = s.as_bytes();
    let mut items: Vec<IrValue> = Vec::new();
    let mut pos = 1usize;
    loop {
        while pos < b.len() && b[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= b.len() {
            return Err(IrError::msg("InvalidJson", "unterminated array"));
        }
        if b[pos] == b']' {
            return Ok((make_arr(ctx, items), pos + 1));
        }
        let (val, vlen) = parse_json_value_ir(ctx, &s[pos..])?;
        pos += vlen;
        items.push(val);
        while pos < b.len() && b[pos].is_ascii_whitespace() {
            pos += 1;
        }
        match b.get(pos).copied() {
            Some(b',') => pos += 1,
            Some(b']') => return Ok((make_arr(ctx, items), pos + 1)),
            _ => return Err(IrError::msg("InvalidJson", "expected ',' or ']'")),
        }
    }
}

fn parse_json_string_ir(s: &str) -> R<(IrValue, usize)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 1usize;
    while i < b.len() {
        match b[i] {
            b'"' => return Ok((IrValue::Str(out), i + 1)),
            b'\\' => {
                i += 1;
                if i >= b.len() {
                    return Err(IrError::msg("InvalidJson", "bad escape"));
                }
                match b[i] {
                    b'"' => out.push(b'"'),
                    b'\\' => out.push(b'\\'),
                    b'/' => out.push(b'/'),
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'r' => out.push(b'\r'),
                    _ => return Err(IrError::msg("InvalidJson", "unknown escape")),
                }
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Err(IrError::msg("InvalidJson", "unterminated string"))
}

fn parse_json_number_ir(s: &str) -> R<(IrValue, usize)> {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len()
        && (b[i].is_ascii_digit() || matches!(b[i], b'-' | b'+' | b'.' | b'e' | b'E'))
    {
        i += 1;
    }
    let text = &s[..i];
    let v = if text.contains('.') || text.contains('e') || text.contains('E') {
        IrValue::Float(
            text.parse::<f64>()
                .map_err(|_| IrError::msg("InvalidJson", "bad number"))?,
        )
    } else {
        match text.parse::<i128>() {
            Ok(n) => IrValue::Int(n),
            Err(_) => IrValue::Float(
                text.parse::<f64>()
                    .map_err(|_| IrError::msg("InvalidJson", "bad number"))?,
            ),
        }
    };
    Ok((v, i))
}

fn parse_json_obj_ir(ctx: &mut Ctx, s: &str) -> R<HashMap<String, IrValue>> {
    let (v, _) = parse_json_value_ir(ctx, s)?;
    match v {
        IrValue::Class(c) => match &ctx.cells[c] {
            Cell::Class { fields, .. } => Ok(fields
                .iter()
                .map(|(k, vc)| (k.clone(), ctx.cell_value(*vc).clone()))
                .collect()),
            _ => Ok(HashMap::new()),
        },
        _ => Ok(HashMap::new()),
    }
}

/// 内建方法（对齐 oracle `call_builtin_method` interp.rs:3511-4027 全量面：标量/Str/Arr/
/// Map/Alloc/Arena/Io/Fs/Time/Net/TcpConn/TcpListener/File + iter/filter/map + 序列化）。
/// 返回 `Ok(None)` = 非内建方法（调用方回退到 `{Type}.{method}` 用户方法表）。
fn call_builtin_method(
    ctx: &mut Ctx,
    module: &IrModule,
    self_v: &IrValue,
    method: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let self_v = deref_value(ctx, self_v).clone();
    // 标量方法（INumber/ICompare 族：a.add(b) ≡ a + b）
    if matches!(self_v, IrValue::Int(_) | IrValue::Float(_)) {
        if let Some(v) = call_scalar_method_ir(ctx, &self_v, method, args)? {
            return Ok(Some(v));
        }
    }
    match (&self_v, method) {
        (IrValue::Str(s), "concat") => {
            let other = args
                .first()
                .ok_or_else(|| IrError::msg("ArityMismatch", "concat"))?;
            match deref_value(ctx, other) {
                IrValue::Str(os) => {
                    let mut bytes = s.clone();
                    bytes.extend_from_slice(os);
                    Ok(Some(str_bytes_val(bytes)))
                }
                _ => Err(IrError::msg("TypeError", "concat expects &[u8]")),
            }
        }
        (IrValue::Str(s), "as_slice") => Ok(Some(IrValue::Str(s.clone()))),
        (IrValue::Str(s), "split") => {
            let sep_v = deref_value(
                ctx,
                args.get(0).ok_or_else(|| IrError::msg("ArityMismatch", "split"))?,
            )
            .clone();
            let sep = match sep_v {
                IrValue::Int(i) => vec![i as u8],
                IrValue::Str(ss) => ss,
                _ => return Err(IrError::msg("TypeError", "split expects byte or bytes")),
            };
            let data = s.clone();
            let mut out = Vec::new();
            if sep.is_empty() {
                return Ok(Some(make_arr(ctx, vec![str_bytes_val(data)])));
            }
            let mut start = 0usize;
            let mut i = 0usize;
            while i + sep.len() <= data.len() {
                if &data[i..i + sep.len()] == sep.as_slice() {
                    out.push(str_bytes_val(data[start..i].to_vec()));
                    i += sep.len();
                    start = i;
                } else {
                    i += 1;
                }
            }
            out.push(str_bytes_val(data[start..].to_vec()));
            Ok(Some(make_arr(ctx, out)))
        }
        (IrValue::Str(s), "to_bytes") => {
            let mut out = (s.len() as u64).to_le_bytes().to_vec();
            out.extend_from_slice(s);
            Ok(Some(str_bytes_val(out)))
        }
        (IrValue::Str(s), "find") => {
            let needle_v = deref_value(
                ctx,
                args.get(0).ok_or_else(|| IrError::msg("ArityMismatch", "find"))?,
            )
            .clone();
            let needle_bytes: Vec<u8> = match needle_v {
                IrValue::Str(n) => n,
                IrValue::Int(i) => vec![i as u8],
                _ => return Err(IrError::msg("TypeError", "find expects byte or bytes")),
            };
            let data = s.clone();
            let pos = if needle_bytes.is_empty() {
                Some(0usize)
            } else {
                data.windows(needle_bytes.len())
                    .position(|w| w == needle_bytes.as_slice())
            };
            Ok(Some(match pos {
                Some(p) => IrValue::Opt(Some(Box::new(IrValue::Int(p as i128)))),
                None => IrValue::Opt(None),
            }))
        }
        (IrValue::Str(s), "substring") => {
            let lo = int_arg_ir(ctx, args, 0)?;
            let hi = int_arg_ir(ctx, args, 1)?;
            let (lo, hi) = (lo.max(0) as usize, hi.max(0) as usize);
            let hi = hi.min(s.len());
            let sub = s[lo.min(hi)..hi].to_vec();
            Ok(Some(str_bytes_val(sub)))
        }
        (IrValue::Str(s), "replace") => {
            let from_b = str_arg_ir(ctx, args, 0)?;
            let to_b = str_arg_ir(ctx, args, 1)?;
            let data = s.clone();
            let mut out = Vec::new();
            let mut i = 0usize;
            while i < data.len() {
                if from_b.is_empty() {
                    out.push(data[i]);
                    i += 1;
                } else if i + from_b.len() <= data.len()
                    && &data[i..i + from_b.len()] == from_b.as_slice()
                {
                    out.extend_from_slice(&to_b);
                    i += from_b.len();
                } else {
                    out.push(data[i]);
                    i += 1;
                }
            }
            Ok(Some(str_bytes_val(out)))
        }
        (IrValue::Str(_), "len") => Ok(Some(IrValue::Int(self_v.display(ctx).len() as i128))),
        (IrValue::Arr(c), "len") => Ok(Some(IrValue::Int(ctx.elems_len(*c) as i128))),
        (IrValue::Arr(c), "append") => {
            let v = args
                .first()
                .ok_or_else(|| IrError::msg("ArityMismatch", "append"))?
                .clone();
            let nc = ctx.alloc(Cell::Value(v));
            match &mut ctx.cells[*c] {
                Cell::Elems(e) => e.push(nc),
                _ => return Err(IrError::msg("TypeError", "append expects array")),
            }
            Ok(Some(IrValue::Void))
        }
        (IrValue::Arr(c), "push_back") => {
            let v = args
                .first()
                .ok_or_else(|| IrError::msg("ArityMismatch", "push_back"))?
                .clone();
            let nc = ctx.alloc(Cell::Value(v));
            match &mut ctx.cells[*c] {
                Cell::Elems(e) => e.push(nc),
                _ => return Err(IrError::msg("TypeError", "push_back expects array")),
            }
            Ok(Some(IrValue::Void))
        }
        (IrValue::Arr(c), "push_front") => {
            let v = args
                .first()
                .ok_or_else(|| IrError::msg("ArityMismatch", "push_front"))?
                .clone();
            let nc = ctx.alloc(Cell::Value(v));
            match &mut ctx.cells[*c] {
                Cell::Elems(e) => e.insert(0, nc),
                _ => return Err(IrError::msg("TypeError", "push_front expects array")),
            }
            Ok(Some(IrValue::Void))
        }
        (IrValue::Arr(c), "pop_back") => {
            let popped = match &mut ctx.cells[*c] {
                Cell::Elems(e) => e.pop(),
                _ => None,
            };
            let v = popped.map(|ec| ctx.cell_value(ec).clone());
            Ok(Some(opt_val(v)))
        }
        (IrValue::Arr(c), "pop_front") => {
            let popped = match &mut ctx.cells[*c] {
                Cell::Elems(e) => {
                    if e.is_empty() {
                        None
                    } else {
                        Some(e.remove(0))
                    }
                }
                _ => None,
            };
            let v = popped.map(|ec| ctx.cell_value(ec).clone());
            Ok(Some(opt_val(v)))
        }
        (IrValue::Arr(c), "front") => {
            let v = match &ctx.cells[*c] {
                Cell::Elems(e) => e.first().map(|ec| ctx.cell_value(*ec).clone()),
                _ => None,
            };
            Ok(Some(opt_val(v)))
        }
        (IrValue::Arr(c), "back") => {
            let v = match &ctx.cells[*c] {
                Cell::Elems(e) => e.last().map(|ec| ctx.cell_value(*ec).clone()),
                _ => None,
            };
            Ok(Some(opt_val(v)))
        }
        (IrValue::Arr(c), "get") => {
            let i = as_index(
                ctx,
                args.get(0)
                    .ok_or_else(|| IrError::msg("ArityMismatch", "get"))?,
            )?;
            let v = match &ctx.cells[*c] {
                Cell::Elems(e) => e.get(i).map(|ec| ctx.cell_value(*ec).clone()),
                _ => None,
            };
            Ok(Some(opt_val(v)))
        }
        (IrValue::Arr(c), "put") => {
            let i = as_index(
                ctx,
                args.get(0)
                    .ok_or_else(|| IrError::msg("ArityMismatch", "put"))?,
            )?;
            let v = args
                .get(1)
                .ok_or_else(|| IrError::msg("ArityMismatch", "put"))?
                .clone();
            let nc = ctx.alloc(Cell::Value(v));
            match &mut ctx.cells[*c] {
                Cell::Elems(e) => {
                    if i >= e.len() {
                        return Err(IrError::msg("IndexOutOfBounds", "index out of bounds"));
                    }
                    e[i] = nc;
                    Ok(Some(IrValue::Void))
                }
                _ => Err(IrError::msg("TypeError", "put expects array")),
            }
        }
        (IrValue::Arr(c), "remove") => {
            let i = as_index(
                ctx,
                args.get(0)
                    .ok_or_else(|| IrError::msg("ArityMismatch", "remove"))?,
            )?;
            let removed = match &mut ctx.cells[*c] {
                Cell::Elems(e) => {
                    if i >= e.len() {
                        return Err(IrError::msg("IndexOutOfBounds", "index out of bounds"));
                    }
                    Some(e.remove(i))
                }
                _ => None,
            };
            match removed {
                Some(ec) => Ok(Some(ctx.cell_value(ec).clone())),
                None => Err(IrError::msg("TypeError", "remove expects array")),
            }
        }
        (IrValue::Arr(c), "extend") => {
            let v = deref_value(
                ctx,
                args.first()
                    .ok_or_else(|| IrError::msg("ArityMismatch", "extend"))?,
            )
            .clone();
            match v {
                IrValue::Arr(src) => {
                    let src_elems = match &ctx.cells[src] {
                        Cell::Elems(e) => e.clone(),
                        _ => return Err(IrError::msg("TypeError", "extend expects array")),
                    };
                    match &mut ctx.cells[*c] {
                        Cell::Elems(e) => {
                            e.extend_from_slice(&src_elems);
                            Ok(Some(IrValue::Void))
                        }
                        _ => Err(IrError::msg("TypeError", "extend expects array")),
                    }
                }
                IrValue::Str(b) => {
                    let mut new_cells = Vec::new();
                    for byte in b {
                        new_cells.push(ctx.alloc(Cell::Value(IrValue::Int(byte as i128))));
                    }
                    match &mut ctx.cells[*c] {
                        Cell::Elems(e) => {
                            e.extend_from_slice(&new_cells);
                            Ok(Some(IrValue::Void))
                        }
                        _ => Err(IrError::msg("TypeError", "extend expects array")),
                    }
                }
                _ => Err(IrError::msg("TypeError", "extend expects array or bytes")),
            }
        }
        (IrValue::Arr(c), "append_u64") => {
            let n = match deref_value(
                ctx,
                args.first()
                    .ok_or_else(|| IrError::msg("ArityMismatch", "append_u64"))?,
            ) {
                IrValue::Int(i) => *i as u64,
                _ => return Err(IrError::msg("TypeError", "append_u64 expects int")),
            };
            let mut new_cells = Vec::new();
            for byte in n.to_le_bytes() {
                new_cells.push(ctx.alloc(Cell::Value(IrValue::Int(byte as i128))));
            }
            match &mut ctx.cells[*c] {
                Cell::Elems(e) => {
                    e.extend_from_slice(&new_cells);
                    Ok(Some(IrValue::Void))
                }
                _ => Err(IrError::msg("TypeError", "append_u64 expects array")),
            }
        }
        (IrValue::Arr(_), "init") => Ok(Some(make_arr(ctx, Vec::new()))),
        (IrValue::Arr(_), "from_bytes") => {
            let b = str_arg_ir(ctx, args, 0)?;
            if b.len() < 8 {
                return Err(IrError::msg("InvalidBytes", "truncated byte data"));
            }
            let n = u64::from_le_bytes(b[0..8].try_into().unwrap()) as usize;
            let mut items = Vec::new();
            let mut pos = 8usize;
            for _ in 0..n {
                let v = if b.len() >= pos + 4 {
                    let i = i32::from_le_bytes(b[pos..pos + 4].try_into().unwrap());
                    pos += 4;
                    IrValue::Int(i as i128)
                } else {
                    break;
                };
                items.push(v);
            }
            Ok(Some(make_arr(ctx, items)))
        }
        (IrValue::Arr(c), "to_bytes") => {
            // 集合 → 字节（u64 LE 元素数前缀 + 逐元素 value_to_bytes，对齐 oracle
            // interp.rs:3959-3969）。IR 的 value_to_bytes_ir 覆盖标量/字符串子集，
            // 聚合元素序列化为空（Phase 7 取舍）。
            let elems = match &ctx.cells[*c] {
                Cell::Elems(e) => e.clone(),
                _ => return Err(IrError::msg("TypeError", "to_bytes expects array")),
            };
            let mut out = (elems.len() as u64).to_le_bytes().to_vec();
            for ec in elems {
                let v = ctx.cell_value(ec).clone();
                out.extend(value_to_bytes_ir(ctx, &v));
            }
            Ok(Some(str_bytes_val(out)))
        }
        (IrValue::Slice { len, .. }, "len") => Ok(Some(IrValue::Int(*len as i128))),
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Map" => {
            call_map_method_ir(ctx, &self_v, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Alloc" => {
            call_alloc_method_ir(ctx, m, args)
        }
        (IrValue::Class(c), m)
            if class_name(ctx, *c) == "Arena" && matches!(m, "alloc" | "init") =>
        {
            call_arena_method_ir(ctx, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Io" => {
            call_io_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Fs" => {
            call_fs_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Time" => {
            call_time_method_ir(ctx, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Net" => {
            call_net_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "TcpConn" => {
            call_conn_method_ir(ctx, module, &self_v, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "TcpListener" => {
            call_listener_method_ir(ctx, module, &self_v, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "File" => {
            call_file_method_ir(ctx, module, &self_v, m, args)
        }
        // Class to_bytes：无布局表（Phase 7 取舍——堆类型请用 to_json）
        (IrValue::Class(_), "to_bytes") => Err(IrError::msg(
            "Unsupported",
            "class to_bytes requires type layout (not in IR runtime)",
        )),
        (IrValue::Class(_), "to_json") => Ok(Some(str_val(&value_to_json_ir(ctx, &self_v)))),
        (_, "iter") => Ok(Some(iter_to_arr_ir(ctx, module, &self_v, 0)?)),
        (_, "filter") => {
            let f = deref_value(
                ctx,
                args.first()
                    .ok_or_else(|| IrError::msg("ArityMismatch", "filter"))?,
            )
            .clone();
            let src = iter_to_arr_ir(ctx, module, &self_v, 0)?;
            let mut out = Vec::new();
            for item in arr_items(ctx, &src)? {
                if call_closure_bool_ir(ctx, module, &f, &[item.clone()])? {
                    out.push(item);
                }
            }
            Ok(Some(make_arr(ctx, out)))
        }
        (_, "map") => {
            let f = deref_value(
                ctx,
                args.first()
                    .ok_or_else(|| IrError::msg("ArityMismatch", "map"))?,
            )
            .clone();
            let src = iter_to_arr_ir(ctx, module, &self_v, 0)?;
            let mut out = Vec::new();
            for item in arr_items(ctx, &src)? {
                let mapped = call_closure_value_ir(ctx, module, &f, &[item])?;
                out.push(mapped);
            }
            Ok(Some(make_arr(ctx, out)))
        }
        _ => Ok(None),
    }
}

/// 隐式环境限定名调用（io.print / io.fs.open / alloc.init…）：
/// 根值 → 中段字段访问 → 末段方法分派（对齐 oracle eval_call 的隐式环境 + 方法分派）。
/// `json.parse`/`csv.parse`/`String.from` 为虚拟根静态内建（非值对象）。
fn call_dotted_implicit(
    ctx: &mut Ctx,
    module: &IrModule,
    name: &str,
    args: &[IrValue],
) -> R<IrValue> {
    match name {
        // math.nan/inf/inf_neg/sqrt/abs/pow/floor/ceil/round（对齐 oracle call_math
        // interp.rs:4922-4960：nan/inf/inf_neg 忽略类型名参数；数值函数取 arg[0]，
        // Int 强制 f64 后计算，返回 Float）
        "math.nan" => return Ok(IrValue::Float(f64::NAN)),
        "math.inf" => return Ok(IrValue::Float(f64::INFINITY)),
        "math.inf_neg" => return Ok(IrValue::Float(f64::NEG_INFINITY)),
        "math.sqrt" | "math.abs" | "math.pow" | "math.floor" | "math.ceil" | "math.round" => {
            let field = name.strip_prefix("math.").unwrap_or(name);
            let v = deref_value(
                ctx,
                args.first()
                    .ok_or_else(|| IrError::msg("ArityMismatch", format!("math.{field}")))?,
            );
            let f = match v {
                IrValue::Int(i) => *i as f64,
                IrValue::Float(f) => *f,
                _ => {
                    return Err(IrError::msg(
                        "TypeError",
                        format!("math.{field} expects a number"),
                    ))
                }
            };
            let r = match field {
                "sqrt" => f.sqrt(),
                "abs" => f.abs(),
                "pow" => f.powf(2.0),
                "floor" => f.floor(),
                "ceil" => f.ceil(),
                "round" => f.round(),
                _ => unreachable!(),
            };
            return Ok(IrValue::Float(r));
        }
        "json.parse" => {
            let data = str_arg_ir(ctx, args, 0)?;
            let obj = parse_json_obj_ir(ctx, &String::from_utf8_lossy(&data))?;
            let mut fields = HashMap::new();
            for (k, v) in obj {
                fields.insert(k, ctx.alloc(Cell::Value(v)));
            }
            return Ok(IrValue::Class(ctx.alloc(Cell::Class {
                name: "Map".into(),
                fields,
            })));
        }
        "csv.parse" => {
            let data = str_arg_ir(ctx, args, 0)?;
            let text = String::from_utf8_lossy(&data).to_string();
            let rows: Vec<IrValue> = text
                .split('\n')
                .map(|line| line.strip_suffix('\r').unwrap_or(line))
                .filter(|line| !line.is_empty())
                .map(|line| line.split(',').map(str_val).collect::<Vec<_>>())
                .map(|cols| make_arr(ctx, cols))
                .collect();
            return Ok(make_arr(ctx, rows));
        }
        "String.from" => {
            let v = args
                .first()
                .ok_or_else(|| IrError::msg("ArityMismatch", "String.from"))?;
            let v = deref_value(ctx, v);
            let s = match v {
                IrValue::Str(s) => s.clone(),
                other => other.display(ctx).as_bytes().to_vec(),
            };
            return Ok(IrValue::Str(s));
        }
        _ => {}
    }
    let parts: Vec<&str> = name.split('.').collect();
    let root = parts[0];
    let mut self_v = implicit_env_value(ctx, root);
    for mid in &parts[1..parts.len() - 1] {
        self_v = field_value(ctx, &self_v, mid)?;
    }
    let method = parts[parts.len() - 1];
    let v = call_builtin_method(ctx, module, &self_v, method, args)?.ok_or_else(|| {
        IrError::msg(
            "NoMethod",
            format!("no method `{method}` on {}", ir_type_name(ctx, &self_v)),
        )
    })?;
    Ok(v)
}

fn find_label(func: &IrFunc, id: usize) -> R<usize> {
    func.body
        .iter()
        .position(|i| matches!(i, IrInst::Label { id: l } if *l == id))
        .ok_or_else(|| {
            IrError::msg(
                "BadLabel",
                format!("label {id} not found in `{}`", func.name),
            )
        })
}

fn binop(op: IrBinOp, ctx: &Ctx, a: &IrValue, b: &IrValue) -> R<IrValue> {
    match op {
        IrBinOp::Add
        | IrBinOp::Sub
        | IrBinOp::Mul
        | IrBinOp::Div
        | IrBinOp::Mod
        | IrBinOp::EucMod => {
            use IrValue::*;
            match (a, b) {
                // 整数：溢出 → Overflow，除/模零 → DivisionByZero（对齐 tree-walking arith）
                (Int(x), Int(y)) => {
                    let v = match op {
                        IrBinOp::Add => x.checked_add(*y),
                        IrBinOp::Sub => x.checked_sub(*y),
                        IrBinOp::Mul => x.checked_mul(*y),
                        IrBinOp::Div => {
                            if *y == 0 {
                                return R::Err(IrError::msg("DivisionByZero", "division by zero"));
                            }
                            Some(x / y)
                        }
                        IrBinOp::Mod => {
                            if *y == 0 {
                                return R::Err(IrError::msg("DivisionByZero", "modulo by zero"));
                            }
                            Some(x % y)
                        }
                        IrBinOp::EucMod => {
                            if *y == 0 {
                                return R::Err(IrError::msg(
                                    "DivisionByZero",
                                    "euclidean modulo by zero",
                                ));
                            }
                            Some(x.rem_euclid(*y))
                        }
                        _ => None,
                    };
                    match v {
                        Some(v) => Ok(Int(v)),
                        None => R::Err(IrError::msg("Overflow", "integer overflow")),
                    }
                }
                // 混合/浮点：IEEE 语义，除零 = inf（对齐 tree-walking arith Float 分支）
                (Int(x), Float(y)) | (Float(y), Int(x)) => {
                    let (x, y) = (x.clone(), y.clone());
                    let v = match op {
                        IrBinOp::Add => x as f64 + y,
                        IrBinOp::Sub => x as f64 - y,
                        IrBinOp::Mul => x as f64 * y,
                        IrBinOp::Div => x as f64 / y,
                        IrBinOp::Mod | IrBinOp::EucMod => (x as f64) % y,
                        _ => 0.0,
                    };
                    Ok(Float(v))
                }
                (Float(x), Float(y)) => {
                    let v = match op {
                        IrBinOp::Add => x + y,
                        IrBinOp::Sub => x - y,
                        IrBinOp::Mul => x * y,
                        IrBinOp::Div => x / y,
                        IrBinOp::Mod | IrBinOp::EucMod => x % y,
                        _ => 0.0,
                    };
                    Ok(Float(v))
                }
                _ => Ok(Int(0)),
            }
        }
        IrBinOp::BitAnd | IrBinOp::BitOr | IrBinOp::BitXor | IrBinOp::Shl | IrBinOp::Shr => {
            match (a, b) {
                (IrValue::Int(x), IrValue::Int(y)) => {
                    let r = match op {
                        IrBinOp::BitAnd => x & y,
                        IrBinOp::BitOr => x | y,
                        IrBinOp::BitXor => x ^ y,
                        IrBinOp::Shl => x.wrapping_shl((*y % 128).max(0) as u32),
                        IrBinOp::Shr => x.wrapping_shr((*y % 128).max(0) as u32),
                        _ => 0,
                    };
                    Ok(IrValue::Int(r))
                }
                _ => Ok(IrValue::Int(0)),
            }
        }
        IrBinOp::Eq | IrBinOp::Ne | IrBinOp::Lt | IrBinOp::Le | IrBinOp::Gt | IrBinOp::Ge => {
            let r = match op {
                IrBinOp::Eq => a.value_eq(ctx, b),
                IrBinOp::Ne => !a.value_eq(ctx, b),
                IrBinOp::Lt => value_lt(a, b),
                IrBinOp::Le => value_lt(a, b) || a.value_eq(ctx, b),
                IrBinOp::Gt => !value_lt(a, b) && !a.value_eq(ctx, b),
                IrBinOp::Ge => !value_lt(a, b),
                _ => false,
            };
            Ok(IrValue::Bool(r))
        }
    }
}

fn value_lt(a: &IrValue, b: &IrValue) -> bool {
    match (a, b) {
        (IrValue::Int(x), IrValue::Int(y)) => x < y,
        (IrValue::Int(x), IrValue::Float(y)) => (*x as f64) < *y,
        (IrValue::Float(x), IrValue::Int(y)) => *x < *y as f64,
        (IrValue::Float(x), IrValue::Float(y)) => x < y,
        (IrValue::Str(x), IrValue::Str(y)) => x < y,
        (IrValue::Bool(x), IrValue::Bool(y)) => x < y,
        // 指针序：cell 索引序（稳定全序——对齐 tree-walking 按 Rc 地址序）
        (IrValue::Ptr(x), IrValue::Ptr(y)) => x < y,
        _ => false,
    }
}

/// 断言内建（IR 参考语义：失败记 fail，返回时抛 AssertFailed）
/// 全量内建（对齐 oracle `call_builtin` interp.rs:2911-3404 全面：box/copy/@ 内建/
/// sqrt/min/max/read_u64_le/sort/binary_search/解析器/parse_int/parse_float/断言五件套）。
/// 断言失败经 `fail` 通道延迟到 `Return`（对齐 IR `AssertFailed` 通道）。
fn call_builtin(
    ctx: &mut Ctx,
    module: &IrModule,
    name: &str,
    args: &[IrValue],
    fail: &mut Option<String>,
) -> R<IrValue> {
    match name {
        "box" => {
            if args.len() != 2 {
                return Err(IrError::msg("ArityMismatch", "box expects 2 args"));
            }
            let nc = ctx.alloc(Cell::Value(args[0].clone()));
            Ok(IrValue::Ptr(nc))
        }
        "copy" => {
            if args.is_empty() {
                return Err(IrError::msg("ArityMismatch", "copy"));
            }
            // copy(&x, .shallow)（L1：CopyMode 内建枚举，.shallow 推断）
            let shallow = if args.len() > 1 {
                matches!(
                    deref_value(ctx, &args[1]),
                    IrValue::Enum { variant, .. } if variant == "shallow"
                )
            } else {
                false
            };
            let v = args[0].clone();
            Ok(if shallow { v } else { deep_copy(ctx, v) })
        }
        // ---------- @ 内建 ----------
        "@intFromEnum" => {
            let v = deref_value(ctx, &args[0]);
            match v {
                IrValue::Enum { name, variant, .. } => {
                    // 内建枚举（L3）：ExitType = [Exit, Error]
                    let idx = if name == "ExitType" {
                        match variant.as_str() {
                            "Exit" => 0,
                            "Error" => 1,
                            _ => 0,
                        }
                    } else {
                        match module.enum_variants.get(name) {
                            Some(variants) => {
                                variants.iter().position(|v| v == variant).unwrap_or(0) as i128
                            }
                            None => 0,
                        }
                    };
                    Ok(IrValue::Int(idx))
                }
                _ => Err(IrError::msg("TypeError", "@intFromEnum expects enum")),
            }
        }
        "@enumFromInt" => {
            let ty = match deref_value(ctx, &args[0]) {
                IrValue::Str(s) => String::from_utf8_lossy(s).to_string(),
                _ => return Err(IrError::msg("TypeError", "@enumFromInt expects type name")),
            };
            let i = match deref_value(ctx, &args[1]) {
                IrValue::Int(i) => *i,
                _ => return Err(IrError::msg("TypeError", "@enumFromInt expects int")),
            };
            match module.enum_variants.get(&ty) {
                Some(variants) => match variants.get(i as usize) {
                    Some(v) => Ok(IrValue::Enum {
                        name: ty.clone(),
                        variant: v.clone(),
                        payload: None,
                    }),
                    None => Err(IrError::msg("IndexOutOfBounds", "@enumFromInt: index out of bounds")),
                },
                None => Err(IrError::msg(
                    "UnknownType",
                    format!("@enumFromInt: unknown type `{ty}`"),
                )),
            }
        }
        "@panic" => {
            // Q-S2：@panic("消息", 位置) abort
            let msg = if args.is_empty() {
                "panic".to_string()
            } else {
                deref_value(ctx, &args[0]).display(ctx)
            };
            Err(IrError::msg("Panic", msg))
        }
        "@sizeOf" => {
            let ty = match deref_value(ctx, &args[0]) {
                IrValue::Str(s) => String::from_utf8_lossy(s).to_string(),
                _ => return Err(IrError::msg("TypeError", "@sizeOf expects type name")),
            };
            match scalar_size_ir(&ty) {
                Some(s) => Ok(IrValue::Int(s as i128)),
                None => Err(IrError::msg(
                    "UnknownType",
                    format!("@sizeOf: unknown type `{ty}`"),
                )),
            }
        }
        "@alignOf" => {
            let ty = match deref_value(ctx, &args[0]) {
                IrValue::Str(s) => String::from_utf8_lossy(s).to_string(),
                _ => return Err(IrError::msg("TypeError", "@alignOf expects type name")),
            };
            let align = match ty.as_str() {
                "i8" | "u8" | "bool" => 1,
                "i16" | "u16" | "f16" => 2,
                "i32" | "u32" | "f32" => 4,
                "i128" | "u128" | "f128" => 16,
                _ => scalar_size_ir(&ty).map(|s| s.min(8)).unwrap_or(8),
            };
            Ok(IrValue::Int(align as i128))
        }
        "@offsetOf" => Err(IrError::msg(
            "Unsupported",
            "@offsetOf requires type layout (not in IR runtime)",
        )),
        "@typeOf" => {
            let v = deref_value(ctx, &args[0]);
            Ok(str_val(&ir_type_name(ctx, v)))
        }
        "@intCast" => {
            let ty = match deref_value(ctx, &args[0]) {
                IrValue::Str(s) => String::from_utf8_lossy(s).to_string(),
                _ => return Err(IrError::msg("TypeError", "@intCast expects type name")),
            };
            let i = match deref_value(ctx, &args[1]) {
                IrValue::Int(i) => *i,
                _ => return Err(IrError::msg("TypeError", "@intCast expects int")),
            };
            if let Some((min, max)) = int_width_bounds_ir(&ty) {
                if i < min || i > max {
                    return Err(IrError::msg(
                        "IntCastOverflow",
                        format!("@intCast overflow to {ty}"),
                    ));
                }
            }
            Ok(IrValue::Int(i))
        }
        "@ptrCast" | "@alignCast" => {
            // tag1 指针无类型化——透传
            let v = args
                .last()
                .ok_or_else(|| IrError::msg("ArityMismatch", name))?;
            Ok(deref_value(ctx, v).clone())
        }
        "@compileError" => {
            let msg = if args.is_empty() {
                "compileError".to_string()
            } else {
                deref_value(ctx, &args[0]).display(ctx)
            };
            Err(IrError::msg("CompileError", format!("@compileError: {msg}")))
        }
        "@addWithOverflow" | "@subWithOverflow" | "@mulWithOverflow" => {
            // 返回 (T, bool) 元组；tag1 Int = i128 无溢出（标志恒 false）
            let a = match deref_value(ctx, &args[0]) {
                IrValue::Int(i) => *i,
                _ => return Err(IrError::msg("TypeError", "expected int")),
            };
            let b = match deref_value(ctx, &args[1]) {
                IrValue::Int(i) => *i,
                _ => return Err(IrError::msg("TypeError", "expected int")),
            };
            let r = match name {
                "@addWithOverflow" => a.wrapping_add(b),
                "@subWithOverflow" => a.wrapping_sub(b),
                _ => a.wrapping_mul(b),
            };
            Ok(make_arr(
                ctx,
                vec![IrValue::Int(r), IrValue::Bool(false)],
            ))
        }
        // ---------- 数值工具 ----------
        "sqrt" => {
            let v = deref_value(ctx, &args[0]);
            match v {
                IrValue::Int(i) => Ok(IrValue::Float((*i as f64).sqrt())),
                IrValue::Float(f) => Ok(IrValue::Float(f.sqrt())),
                _ => Err(IrError::msg("TypeError", "sqrt expects number")),
            }
        }
        "min" | "max" => {
            if args.len() != 2 {
                return Err(IrError::msg("ArityMismatch", name));
            }
            let a = deref_value(ctx, &args[0]).clone();
            let b = deref_value(ctx, &args[1]).clone();
            let take_a = match (&a, &b) {
                (IrValue::Int(x), IrValue::Int(y)) => {
                    if name == "min" {
                        x <= y
                    } else {
                        x >= y
                    }
                }
                (IrValue::Float(x), IrValue::Float(y)) => {
                    if name == "min" {
                        x <= y
                    } else {
                        x >= y
                    }
                }
                (IrValue::Int(x), IrValue::Float(y)) => {
                    if name == "min" {
                        (*x as f64) <= *y
                    } else {
                        (*x as f64) >= *y
                    }
                }
                (IrValue::Float(x), IrValue::Int(y)) => {
                    if name == "min" {
                        *x <= (*y as f64)
                    } else {
                        *x >= (*y as f64)
                    }
                }
                _ => return Err(IrError::msg("TypeError", "min/max expects numbers")),
            };
            Ok(if take_a { a } else { b })
        }
        // ---------- 字节/算法 ----------
        "read_u64_le" => {
            let v = deref_value(ctx, &args[0]);
            let b = value_bytes_ir(ctx, v)
                .ok_or_else(|| IrError::msg("TypeError", "read_u64_le expects bytes"))?;
            if b.len() < 8 {
                return Err(IrError::msg("IndexOutOfBounds", "read_u64_le: truncated"));
            }
            let n = u64::from_le_bytes(b[0..8].try_into().unwrap());
            Ok(IrValue::Int(n as i128))
        }
        "sort" => {
            let v = deref_value(ctx, &args[0]).clone();
            // 对齐 oracle interp.rs:3195-3205：第二参若提供必须是比较器闭包，
            // 否则 TypeError（避免静默不排序）
            let cmp_f = match args.get(1) {
                Some(a) => {
                    let f = deref_value(ctx, a).clone();
                    match &f {
                        IrValue::Closure { .. } | IrValue::Fn(_) => Some(f),
                        _ => {
                            return Err(IrError::msg(
                                "TypeError",
                                "sort comparator must be a closure",
                            ))
                        }
                    }
                }
                None => None,
            };
            match v {
                IrValue::Arr(c) => {
                    let elems = match &ctx.cells[c] {
                        Cell::Elems(e) => e.clone(),
                        _ => return Err(IrError::msg("TypeError", "sort expects array")),
                    };
                    let mut items: Vec<(usize, IrValue)> = elems
                        .iter()
                        .map(|ec| (*ec, ctx.cell_value(*ec).clone()))
                        .collect();
                    items.sort_by(|x, y| match &cmp_f {
                        Some(f) => {
                            let r = call_closure_value_ir(
                                ctx,
                                module,
                                f,
                                &[x.1.clone(), y.1.clone()],
                            );
                            match r {
                                Ok(IrValue::Int(i)) if i < 0 => std::cmp::Ordering::Less,
                                Ok(IrValue::Int(i)) if i > 0 => std::cmp::Ordering::Greater,
                                Ok(IrValue::Float(ff)) if ff < 0.0 => std::cmp::Ordering::Less,
                                Ok(IrValue::Float(ff)) if ff > 0.0 => std::cmp::Ordering::Greater,
                                _ => std::cmp::Ordering::Equal,
                            }
                        }
                        None => {
                            if value_lt(&x.1, &y.1) {
                                std::cmp::Ordering::Less
                            } else if x.1.value_eq(ctx, &y.1) {
                                std::cmp::Ordering::Equal
                            } else {
                                std::cmp::Ordering::Greater
                            }
                        }
                    });
                    let new_elems: Vec<usize> = items.iter().map(|(c, _)| *c).collect();
                    ctx.cells[c] = Cell::Elems(new_elems);
                    Ok(IrValue::Void)
                }
                _ => Err(IrError::msg("TypeError", "sort expects array")),
            }
        }
        "binary_search" => {
            let v = deref_value(ctx, &args[0]).clone();
            let target = deref_value(ctx, &args[1]).clone();
            let items: Vec<IrValue> = match &v {
                IrValue::Arr(c) => match &ctx.cells[*c] {
                    Cell::Elems(e) => e.iter().map(|ec| ctx.cell_value(*ec).clone()).collect(),
                    _ => return Err(IrError::msg("TypeError", "binary_search expects array")),
                },
                IrValue::Slice { data, start, len } => match &ctx.cells[*data] {
                    Cell::Elems(e) => e[*start..*start + *len]
                        .iter()
                        .map(|ec| ctx.cell_value(*ec).clone())
                        .collect(),
                    _ => return Err(IrError::msg("TypeError", "binary_search expects slice")),
                },
                _ => return Err(IrError::msg("TypeError", "binary_search expects array or slice")),
            };
            let mut lo = 0usize;
            let mut hi = items.len();
            while lo < hi {
                let mid = (lo + hi) / 2;
                if value_lt(&items[mid], &target) {
                    lo = mid + 1;
                } else if items[mid].value_eq(ctx, &target) {
                    return Ok(IrValue::Opt(Some(Box::new(IrValue::Int(mid as i128)))));
                } else {
                    hi = mid;
                }
            }
            Ok(IrValue::Opt(None))
        }
        // ---------- 解析器辅助（71-recursive-parser；操作 &[u8] 与 *usize）----------
        "skip_space" | "peek" | "advance" | "is_digit" | "parse_number" => {
            let r = call_parser_builtin_ir(ctx, module, name, args)?
                .ok_or_else(|| IrError::msg("NoMethod", name))?;
            Ok(r)
        }
        "parse_int" => {
            let s = str_arg_ir(ctx, args, 0)?;
            let text = String::from_utf8_lossy(&s).trim().to_string();
            let parsed = if text.is_empty() {
                None
            } else {
                text.parse::<i128>().ok()
            };
            Ok(match parsed {
                Some(n) => IrValue::Opt(Some(Box::new(IrValue::Int(n)))),
                None => IrValue::Opt(None),
            })
        }
        "parse_float" => {
            let s = str_arg_ir(ctx, args, 0)?;
            let text = String::from_utf8_lossy(&s).trim().to_string();
            let parsed = if text.is_empty() {
                None
            } else {
                text.parse::<f64>().ok()
            };
            Ok(match parsed {
                Some(n) => IrValue::Opt(Some(Box::new(IrValue::Float(n)))),
                None => IrValue::Opt(None),
            })
        }
        // ---------- 断言五件套（Q-T1）：测试函数内隐式可用；3 参 expect = 解析器 ----------
        "expect" => {
            if args.len() == 3 {
                let r = call_parser_builtin_ir(ctx, module, "expect", args)?
                    .ok_or_else(|| IrError::msg("NoMethod", "expect parser"))?;
                return Ok(r);
            }
            if args.first().map_or(false, |v| v.as_bool()) {
                Ok(IrValue::Void)
            } else {
                *fail = Some("expect failed".into());
                Ok(IrValue::Void)
            }
        }
        "expect_eq" | "expect_neq" => {
            if args.len() != 2 {
                return Err(IrError::msg("ArityMismatch", name));
            }
            let a = deref_value(ctx, &args[0]);
            let b = deref_value(ctx, &args[1]);
            let eq = a.value_eq(ctx, b);
            let want_eq = name == "expect_eq";
            if eq != want_eq {
                *fail = Some(format!(
                    "{} failed: expected {} {}, got {}",
                    name,
                    if want_eq { "=" } else { "!=" },
                    b.display(ctx),
                    a.display(ctx)
                ));
            }
            Ok(IrValue::Void)
        }
        "expect_error" => {
            if args.len() != 2 {
                return Err(IrError::msg("ArityMismatch", "expect_error"));
            }
            let want = deref_value(ctx, &args[0]);
            let got = deref_value(ctx, &args[1]);
            match (want, got) {
                // M4.2：错误码比较（码全局唯一）
                (IrValue::Err { name: w, .. }, IrValue::Err { name: g, .. }) if w == g => {
                    Ok(IrValue::Void)
                }
                (IrValue::Err { name: w, .. }, IrValue::Err { name: g, .. }) => {
                    *fail = Some(format!("expect_error failed: expected error.{w}, got error.{g}"));
                    Ok(IrValue::Void)
                }
                (_, g) => {
                    *fail = Some(format!(
                        "expect_error failed: expected error, got {}",
                        ir_type_name(ctx, g)
                    ));
                    Ok(IrValue::Void)
                }
            }
        }
        "expect_eq_slices" => {
            if args.len() != 2 {
                return Err(IrError::msg("ArityMismatch", "expect_eq_slices"));
            }
            let a = deref_value(ctx, &args[0]);
            let b = deref_value(ctx, &args[1]);
            if a.value_eq(ctx, b) {
                Ok(IrValue::Void)
            } else {
                *fail = Some(format!(
                    "expect_eq_slices failed: {} != {}",
                    a.display(ctx),
                    b.display(ctx)
                ));
                Ok(IrValue::Void)
            }
        }
        _ => Ok(IrValue::Void),
    }
}
