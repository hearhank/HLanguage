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
use crate::rle::{decode_rle, encode_rle};
use crate::rng::xorshift64;
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
                traits,
                fields,
                methods,
                ..
            } => {
                let ci = ClassInfo {
                    fields: fields
                        .iter()
                        .map(|f| (f.name.clone(), f.ty.clone()))
                        .collect(),
                    methods: methods.iter().map(|m| m.name.clone()).collect(),
                    continuous: traits.iter().any(|t| matches!(t, Trait::Continuous)),
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
            // K1 无标签 union（ADR-0014）：登记字段声明（扁平 + 全限定）
            Decl::Union { name, fields, .. } => {
                let ui = UnionInfo {
                    fields: fields
                        .iter()
                        .map(|f| (f.name.clone(), f.ty.clone()))
                        .collect(),
                };
                tt.unions.insert(name.clone(), ui);
                if !path.is_empty() {
                    let mut q = path.join(".");
                    q.push('.');
                    q.push_str(name);
                    tt.unions.insert(q, tt.unions[name].clone());
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
    // E1.2 组 D：类型函数定义表（comptime-only，体降级跳过；类型应用点惰性具体化）
    let type_fns = collect_type_fns(program);
    // E1.2 组 D D5：comptime 值函数定义表（运行时调用点常量折叠，IR 无调用残留）
    let value_fns = collect_value_fns(program);
    let mut module = IrModule::default();
    // C3：文件级 import 展开表（bound → 完整限定名 / 模块前缀）——原生链接与 IR 调用名对齐
    let (import_syms, import_mods) = collect_imports(program);
    // 错误码表（名 → 码）：内建运行时错误值（io.fs 等）须与 `error.X` 字面量同码
    for e in errors.entries() {
        module.error_codes.insert(e.name.clone(), e.code);
    }
    // 枚举变体序（Phase 7）：`@intFromEnum`/`@enumFromInt` 运行时分派
    for (n, ei) in &types.enums {
        module.enum_variants.insert(n.clone(), ei.variants.clone());
    }
    // K1 无标签 union（ADR-0014）：字段声明表（扁平 + 全限定）→ 写路径字节重解释同步
    for (n, ui) in &types.unions {
        module.unions.insert(n.clone(), ui.fields.clone());
    }
    // [continuous] 类名集（扁平 + 全限定）：DeepCopy 指令运行时门
    for (n, ci) in &types.classes {
        if ci.continuous {
            module.continuous.insert(n.clone());
        }
    }
    for d in &program.decls {
        lower_decl(
            d,
            &mut module,
            &errors,
            &types,
            &funcs,
            &globals,
            &type_fns,
            &value_fns,
            &import_syms,
            &import_mods,
        )?;
    }
    // Phase 5：合成 `@__init__` 函数（声明序初始化 global/const；多文件合并 = 各模块
    // 自带 init，运行时按 funcs 序依次执行）。不登记 func_index（不可被用户调用）。
    if let Some(init) = lower_init_func(
        program,
        &errors,
        &types,
        &funcs,
        &globals,
        &type_fns,
        &value_fns,
        &mut module.closures,
        &import_syms,
        &import_mods,
    )? {
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

/// C3：文件级 import 展开表——(符号选择 bound → 完整限定名, 整模块 bound → 包路径)。
/// `H.std` 根跳过（内建虚拟根，`io.print` 等走 CallBuiltin 路由，不展开）。
fn collect_imports(program: &Program) -> (HashMap<String, String>, HashMap<String, String>) {
    let mut syms = HashMap::new();
    let mut mods = HashMap::new();
    for d in &program.decls {
        if let Decl::Import {
            path,
            alias,
            select,
            ..
        } = d
        {
            if path.first().map_or(false, |p| p == "H") {
                continue;
            }
            let base = path.join(".");
            match select {
                Some(syms_sel) => {
                    for (sym, sym_alias) in syms_sel {
                        let bound = sym_alias.clone().unwrap_or_else(|| sym.clone());
                        syms.insert(bound, format!("{base}.{sym}"));
                    }
                }
                None => {
                    let bound = alias
                        .clone()
                        .unwrap_or_else(|| path.last().cloned().unwrap_or_default());
                    mods.insert(bound, base);
                }
            }
        }
    }
    (syms, mods)
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
            Decl::Namespace {
                name,
                decls: nested,
                ..
            } => {
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

/// E1.2 组 D：收集类型函数定义（name → params+body），供 NamedLit 惰性具体化。
/// 顶层 + namespace 内均收集；键 = 扁平名 + 限定名（对齐 `collect_fn_names`）。
/// 类型函数体本身由降级**跳过**（comptime-only，运行时不执行），仅在类型应用点
/// （`Pair(i32)`）经 `comptime::instantiate` 编译期求值。
fn collect_type_fns(program: &Program) -> HashMap<String, (Vec<Param>, Block)> {
    let mut map = HashMap::new();
    collect_type_fns_in(&program.decls, &mut map, &[]);
    map
}

fn collect_type_fns_in(
    decls: &[Decl],
    map: &mut HashMap<String, (Vec<Param>, Block)>,
    path: &[String],
) {
    for d in decls {
        match d {
            Decl::Fn {
                name,
                params,
                body,
                ret,
                ..
            } => {
                if comptime::is_type_fn(params, ret) {
                    let def = (params.clone(), body.clone());
                    map.insert(name.clone(), def.clone());
                    if !path.is_empty() {
                        let mut q = path.join(".");
                        q.push('.');
                        q.push_str(name);
                        map.insert(q, def);
                    }
                }
            }
            Decl::Namespace {
                name,
                decls: nested,
                ..
            } => {
                let mut p = path.to_vec();
                p.push(name.clone());
                collect_type_fns_in(nested, map, &p);
            }
            _ => {}
        }
    }
}

/// E1.2 组 D D5：收集 comptime 值函数定义（name → params+body），供运行时调用点折叠。
/// 顶层 + namespace 内均收集；键 = 扁平名 + 限定名（对齐 `collect_type_fns`）。
/// 与类型函数不同（体降级跳过），值函数体为普通常量表达式，**运行时不执行**——
/// 调用点（`var n = array_len(i32);`）经常量求值折叠为 `IrConst`，IR 中无调用残留。
fn collect_value_fns(program: &Program) -> HashMap<String, (Vec<Param>, Block)> {
    let mut map = HashMap::new();
    collect_value_fns_in(&program.decls, &mut map, &[]);
    map
}

fn collect_value_fns_in(
    decls: &[Decl],
    map: &mut HashMap<String, (Vec<Param>, Block)>,
    path: &[String],
) {
    for d in decls {
        match d {
            Decl::Fn {
                name,
                params,
                body,
                ret,
                ..
            } => {
                if comptime::is_comptime_value_fn(params, ret) {
                    let def = (params.clone(), body.clone());
                    map.insert(name.clone(), def.clone());
                    if !path.is_empty() {
                        let mut q = path.join(".");
                        q.push('.');
                        q.push_str(name);
                        map.insert(q, def);
                    }
                }
            }
            Decl::Namespace {
                name,
                decls: nested,
                ..
            } => {
                let mut p = path.to_vec();
                p.push(name.clone());
                collect_value_fns_in(nested, map, &p);
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
    type_fns: &HashMap<String, (Vec<Param>, Block)>,
    value_fns: &HashMap<String, (Vec<Param>, Block)>,
    import_syms: &HashMap<String, String>,
    import_mods: &HashMap<String, String>,
) -> Result<(), IrError> {
    match d {
        Decl::Fn {
            name,
            params,
            body,
            is_test,
            ret,
            ..
        } => {
            // E1.2 组 D：类型函数（返回 `type`）= comptime-only，跳过体降级
            // （体含 `struct { ... }` 类型值，运行时后端不可求值；类型应用点
            // 经 `comptime::instantiate` 编译期求值）。函数名已在 funcs 集合，
            // 调用位判定不受影响。
            if comptime::is_type_fn(params, ret) {
                return Ok(());
            }
            let func = lower_func(
                name,
                params,
                body,
                *is_test,
                errors,
                types,
                funcs,
                globals,
                type_fns,
                value_fns,
                &mut module.closures,
                import_syms,
                import_mods,
            )?;
            register_func(module, name, func);
        }
        Decl::Namespace { name, decls, .. } => {
            // namespace 内函数：扁平名 + 限定名双注册（与运行时/语义一致）；
            // 多级 namespace（io.net.connect）注册全限定名
            let mut inner: Vec<(String, String, IrFunc)> = Vec::new();
            collect_ns_funcs(
                decls,
                &[name.clone()],
                &mut inner,
                errors,
                types,
                funcs,
                globals,
                type_fns,
                value_fns,
                &mut module.closures,
                import_syms,
                import_mods,
            )?;
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
                if let Ok(func) = lower_func(
                    &fname,
                    &m.params,
                    &m.body,
                    false,
                    errors,
                    types,
                    funcs,
                    globals,
                    type_fns,
                    value_fns,
                    &mut module.closures,
                    import_syms,
                    import_mods,
                ) {
                    register_func(module, &fname, func);
                }
            }
        }
        Decl::Enum { .. }
        | Decl::Union { .. }
        | Decl::Interface { .. }
        | Decl::Using { .. }
        | Decl::Import { .. }
        | Decl::Script { .. }
        | Decl::Comptime { .. } => {}
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
    type_fns: &HashMap<String, (Vec<Param>, Block)>,
    value_fns: &HashMap<String, (Vec<Param>, Block)>,
    closures: &mut Vec<IrFunc>,
    import_syms: &HashMap<String, String>,
    import_mods: &HashMap<String, String>,
) -> Result<(), IrError> {
    for d in decls {
        match d {
            Decl::Fn {
                name,
                params,
                body,
                is_test,
                ret,
                ..
            } if !*is_test => {
                // E1.2 组 D：类型函数跳过体降级（comptime-only）
                if comptime::is_type_fn(params, ret) {
                    continue;
                }
                let mut qn = path.to_vec();
                qn.push(name.clone());
                let func = lower_func(
                    name,
                    params,
                    body,
                    false,
                    errors,
                    types,
                    funcs,
                    globals,
                    type_fns,
                    value_fns,
                    closures,
                    import_syms,
                    import_mods,
                )?;
                out.push((name.clone(), qn.join("."), func));
            }
            Decl::Namespace {
                name,
                decls: nested,
                ..
            } => {
                let mut p = path.to_vec();
                p.push(name.clone());
                collect_ns_funcs(
                    nested,
                    &p,
                    out,
                    errors,
                    types,
                    funcs,
                    globals,
                    type_fns,
                    value_fns,
                    closures,
                    import_syms,
                    import_mods,
                )?;
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
    module
        .func_index
        .entry(name.to_string())
        .or_default()
        .push(idx);
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
    type_fns: &HashMap<String, (Vec<Param>, Block)>,
    value_fns: &HashMap<String, (Vec<Param>, Block)>,
    closures: &mut Vec<IrFunc>,
    import_syms: &HashMap<String, String>,
    import_mods: &HashMap<String, String>,
) -> Result<IrFunc, IrError> {
    let mut ctx = LowerCtx::new(
        errors.clone(),
        types.clone(),
        funcs,
        globals,
        type_fns,
        value_fns,
        closures,
        import_syms.clone(),
        import_mods.clone(),
    );
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
        .map(|p| {
            p.default
                .as_ref()
                .and_then(|d| lower_default_const(d, errors))
        })
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
    type_fns: &HashMap<String, (Vec<Param>, Block)>,
    value_fns: &HashMap<String, (Vec<Param>, Block)>,
    closures: &mut Vec<IrFunc>,
    import_syms: &HashMap<String, String>,
    import_mods: &HashMap<String, String>,
) -> Result<Option<IrFunc>, IrError> {
    if globals.is_empty() {
        return Ok(None);
    }
    let mut ctx = LowerCtx::new(
        errors.clone(),
        types.clone(),
        funcs,
        globals,
        type_fns,
        value_fns,
        closures,
        import_syms.clone(),
        import_mods.clone(),
    );
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
    /// E1.2 组 D：类型函数定义表（name → params+body，comptime-only）。`Pair(i32)`
    /// 类型应用点惰性具体化：instantiate → 具体化 Class 登记进 `self.types`。
    type_fns: &'a HashMap<String, (Vec<Param>, Block)>,
    /// E1.2 组 D D5：comptime 值函数定义表（name → params+body）。运行时调用点
    /// （`var n = array_len(i32);`）常量折叠为 `IrConst`，IR 中无调用残留。
    value_fns: &'a HashMap<String, (Vec<Param>, Block)>,
    /// E1.2 组 D D3：具体化登记期进行中的具体化名集合（`Pair<@i32>` 键）。
    /// 自/互递归类型函数（`LinkedList(T) { next: ?LinkedList(T) }`）在登记期重入时
    /// 命中即返回键本身（叶），防止无限实例化。
    instantiating: Vec<String>,
    /// C3：文件级 import 符号选择展开表（bound 名 → 完整限定名 `jsonlib.parse`）
    import_syms: HashMap<String, String>,
    /// C3：整模块 import 前缀展开表（bound 模块名 → 包路径 `pkg.mod`）
    import_mods: HashMap<String, String>,
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
        type_fns: &'a HashMap<String, (Vec<Param>, Block)>,
        value_fns: &'a HashMap<String, (Vec<Param>, Block)>,
        closures: &'a mut Vec<IrFunc>,
        import_syms: HashMap<String, String>,
        import_mods: HashMap<String, String>,
    ) -> Self {
        LowerCtx {
            scopes: Vec::new(),
            next_slot: 0,
            insts: Vec::new(),
            next_label: 0,
            errors,
            types,
            funcs,
            type_fns,
            value_fns,
            instantiating: Vec::new(),
            import_syms,
            import_mods,
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
                self.push(IrInst::JumpIfErr {
                    temp: v,
                    label: l_err,
                });
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
                    self.push(IrInst::FnRef {
                        temp: t,
                        name: name.clone(),
                    });
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
            Expr::Await(inner, _) => {
                // 组 E E2 子集边界：IR 无 Future 延迟任务抽象，async fn 调用降级为同步
                // 执行（lower 普通 Call），await 透传内层值——纯函数下与 interp lazy 语义
                // 结果一致（consistency e2_async_await_consistent）；副作用时序/取消为
                // interp 特有（E4 原生异步落地后对齐）。interp 侧见 future_run。
                let a = self.lower_expr(inner);
                self.push(IrInst::Load { temp: t, slot: a });
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
            Expr::Call {
                callee,
                args,
                span: _,
            } => {
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
                // `Type.new(args, alloc)` 构造器（对齐 oracle `call_new_builtin` interp.rs:
                // 4661-4695）：已知 class 类型名 → MakeClass 按字段声明序填位置参数，alloc
                // first/last 跳过，缺省字段落默认值。用户静态函数 `Type.new` 优先。
                if let Some(qn) = &callee_name {
                    if let Some((ns, method)) = qn.rsplit_once('.') {
                        if method == "new"
                            && !self.funcs.contains(qn)
                            && self.types.classes.contains_key(ns)
                        {
                            return self.lower_new_constructor(ns, args);
                        }
                    }
                }
                // E1.2 组 D D5：comptime 值函数运行时调用点折叠（`var n = array_len(i32);`）
                // ——类型实参收已知类型表达式、值实参常量求值、体常量求值 → `Const`，
                // 类型值仅编译期存在，IR 中无调用/类型值残留。折叠失败回落既有调用路径。
                if let Some(qn) = &callee_name {
                    if self.try_fold_comptime_value_call(qn, args, t) {
                        return t;
                    }
                }
                let arg_ts: Vec<usize> = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        if let Some(cn) = &callee_name {
                            // alloc.init(SomeClass)：已知 class 类型名 → 默认字段 MakeClass
                            // （对齐 oracle 无参构造 = 类型空实例，字段逐默认值；未知/枚举
                            // 类型名回退 Const Str——运行时建空实例）。
                            if matches!(cn.as_str(), "alloc.init" | "arena.init") && i == 0 {
                                if let Expr::Ident(n, _) = a {
                                    if self.types.classes.contains_key(n) {
                                        return self.lower_alloc_init_defaults(n);
                                    }
                                }
                            }
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
                            // 全局/namespace 函数静态调用（含重载，按名分派）；
                            // C3：import 符号选择 bound → 展开完整限定名（`parse` → `jsonlib.parse`，
                            // 原生经 extern links 链接 / IR 运行时 NoFunction）
                            let qn = self
                                .import_syms
                                .get(name)
                                .cloned()
                                .unwrap_or_else(|| name.clone());
                            self.push(IrInst::Call {
                                name: qn,
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
                            // C3：整模块 import 前缀替换——`import jsonlib;` 后 `jsonlib.f` →
                            // `{包路径}.f`（原生经 extern links 链接）
                            let qn = if let Some(base) = self.import_mods.get(ns) {
                                format!("{base}.{field}")
                            } else {
                                qn
                            };
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
                // orelse：null → 默认值；非 null → 解包负载（Opt(Some(x)) → x，
                // 对齐 oracle interp.rs Orelse——此前直接存 a 导致 Opt 泄漏）
                let a = self.lower_expr(l);
                let l_null = self.new_label();
                let done = self.new_label();
                let res_slot = self.alloc_slot();
                let unwrapped = self.alloc_slot();
                self.push(IrInst::JumpIfNull {
                    temp: a,
                    label: l_null,
                });
                self.push(IrInst::Unwrap {
                    temp: unwrapped,
                    a,
                });
                self.push(IrInst::Store {
                    slot: res_slot,
                    temp: unwrapped,
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
                target,
                op,
                value,
                span,
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
                self.push(IrInst::MakeArr {
                    temp: t,
                    items: item_ts,
                });
            }
            Expr::NamedLit { ty, ty_args, fields, span, .. } => {
                // E1.2 组 D：泛型应用 `Pair(i32){...}` → 惰性具体化后按具体化名构造。
                // 具体化失败（实参个数/形态不符）→ 硬错误。
                let ty = if ty_args.is_empty() {
                    ty.clone()
                } else {
                    match self.concrete_type_name(ty, ty_args) {
                        Ok(cn) => cn,
                        Err(msg) => {
                            self.fail_void(t, &msg, span);
                            return t;
                        }
                    }
                };
                // struct 字面量 → MakeClass；枚举字面量（恰一个变体）→ MakeEnum（对齐 oracle）
                if self.types.classes.contains_key(&ty) {
                    let f: Vec<(String, usize)> = fields
                        .iter()
                        .map(|(k, v)| (k.clone(), self.lower_expr(v)))
                        .collect();
                    self.push(IrInst::MakeClass {
                        temp: t,
                        ty,
                        fields: f,
                    });
                } else if self.types.unions.contains_key(&ty) {
                    // K1 union 字面量（ADR-0014）：`Foo { field = v }`——单字段。
                    // 运行时形态 = `Cell::Class` + `@union` 标记；缺省字段落标量零值，
                    // 构造后 `UnionSync` 把 `written` 字段字节重解释同步其余字段。
                    if fields.len() != 1 {
                        self.fail_void(t, "union 字面量应为单字段（K1）", span);
                        return t;
                    }
                    let (fname, fval) = &fields[0];
                    let fvt = self.lower_expr(fval);
                    // 先克隆字段表释放 `self.types` 借用，再可变借用 `self` 降级默认值
                    let ufields: Vec<(String, Type)> = self
                        .types
                        .unions
                        .get(&ty)
                        .map(|u| u.fields.clone())
                        .unwrap_or_default();
                    let mut fs: Vec<(String, usize)> = Vec::with_capacity(ufields.len() + 2);
                    for (fdname, fdty) in &ufields {
                        let dt = self.lower_default_value(fdty);
                        fs.push((fdname.clone(), dt));
                    }
                    let mk = self.alloc_slot();
                    self.push(IrInst::Const {
                        temp: mk,
                        val: IrConst::Bool(true),
                    });
                    fs.push(("@union".to_string(), mk));
                    fs.push((fname.clone(), fvt));
                    self.push(IrInst::MakeClass {
                        temp: t,
                        ty: ty.clone(),
                        fields: fs,
                    });
                    self.push(IrInst::UnionSync {
                        class: t,
                        written: fname.clone(),
                    });
                } else if self.types.enums.contains_key(&ty) {
                    if fields.len() != 1 {
                        self.fail_void(t, "多字段枚举字面量（应为单变体）", span);
                        return t;
                    }
                    let (variant, payload) = &fields[0];
                    let pv = self.lower_expr(payload);
                    self.push(IrInst::MakeEnum {
                        temp: t,
                        name: ty,
                        variant: variant.clone(),
                        payload: Some(pv),
                    });
                } else {
                    self.fail_void(t, &format!("未知类型 `{ty}` 的字面量构造"), span);
                }
            }
            // struct 类型字面量（E1.2 组 D）：类型值——仅 comptime 类型函数体内求值；
            // 运行时表达式位置 = 用法错误（类型函数体由 IR 降级跳过，不会到达这里）
            Expr::StructType { span, .. } => {
                self.fail_void(t, "类型值 `struct { ... }`（仅 comptime 类型函数内可求值）", span);
            }
            // 数组类型值 `[n]T`（组 D）：同 struct 类型字面量——仅 comptime 类型函数
            // 体内编译期求值；运行时表达式位置 = 用法错误（类型函数体降级跳过）
            Expr::ArrayType { span, .. } => {
                self.fail_void(t, "类型值 `[n]T`（仅 comptime 类型函数内可求值）", span);
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
                    if self.types.enums.contains_key(bname)
                        || self.types.classes.contains_key(bname)
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
                self.push(IrInst::Field {
                    temp: t,
                    base: b,
                    field: field.clone(),
                });
            }
            Expr::Field { base, field, .. } => {
                let b = self.lower_expr(base);
                self.push(IrInst::Field {
                    temp: t,
                    base: b,
                    field: field.clone(),
                });
            }
            Expr::Index {
                base,
                indices,
                span,
            } => {
                let b = self.lower_expr(base);
                if indices.len() == 1 {
                    if let Expr::Binary(BinOp::Range, lo, hi, _) = &indices[0] {
                        // 切片 `base[lo..hi]`（hi 可为 `__end__` 开区间哨兵）
                        let lo_t = self.lower_expr(lo);
                        let hi_t = self.lower_slice_end(hi);
                        self.push(IrInst::SliceOf {
                            temp: t,
                            base: b,
                            lo: lo_t,
                            hi: hi_t,
                        });
                        return t;
                    }
                    let idx = self.lower_expr(&indices[0]);
                    self.push(IrInst::Index {
                        temp: t,
                        base: b,
                        index: idx,
                    });
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
                        self.push(IrInst::GlobalAddr {
                            temp: t,
                            name: name.clone(),
                        });
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
            Expr::SwitchExpr {
                subject,
                arms,
                span,
            } => {
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
                    self.push(IrInst::Const {
                        temp: t,
                        val: IrConst::Void,
                    });
                }
                self.pop_scope();
            }
            Expr::FnRef(name, _span) => {
                self.push(IrInst::FnRef {
                    temp: t,
                    name: name.clone(),
                });
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
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Void,
                });
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
        let type_fns = self.type_fns;
        let value_fns = self.value_fns;
        let closures = &mut *self.closures;
        let import_syms = self.import_syms.clone();
        let import_mods = self.import_mods.clone();
        let mut ctx = LowerCtx::new(
            errors,
            types,
            funcs,
            globals,
            type_fns,
            value_fns,
            closures,
            import_syms,
            import_mods,
        );
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
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::End,
                });
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
                        self.push(IrInst::StorePtr {
                            target: p,
                            value: v,
                        });
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
                        self.push(IrInst::StorePtr {
                            target: p,
                            value: r,
                        });
                        r
                    }
                });
            }
            // 字段赋值：`p.x = v`（仅 Class 目标；非 Class → TypeError——对齐 eval_assign Field 臂）。
            // 复合（`p.x += v`）：cur = 字段读 + binop + 写回（对齐 oracle eval_assign 先
            // eval(target) 求当前值再 binop；base 双求值语义与 oracle 一致）。
            Expr::Field { base, field, .. } => {
                let b = self.lower_expr(base);
                let v = match op {
                    AssignOp::Set => self.lower_expr(value),
                    _ => {
                        let cur = self.lower_expr(target);
                        let rhs = self.lower_expr(value);
                        let r = self.alloc_slot();
                        self.push(IrInst::Bin {
                            op: to_assign_binop(op),
                            temp: r,
                            a: cur,
                            b: rhs,
                        });
                        r
                    }
                };
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
                let v = match op {
                    AssignOp::Set => self.lower_expr(value),
                    // 复合（`p.x += v`）：cur = 字段读 + binop + 写回（对齐 eval_assign）
                    _ => {
                        let cur = self.lower_expr(target);
                        let rhs = self.lower_expr(value);
                        let r = self.alloc_slot();
                        self.push(IrInst::Bin {
                            op: to_assign_binop(op),
                            temp: r,
                            a: cur,
                            b: rhs,
                        });
                        r
                    }
                };
                self.push(IrInst::StoreField {
                    base: b,
                    field: field.clone(),
                    value: v,
                });
                return Some(v);
            }
            // 索引赋值：单索引 → StoreIndex（复合 = 读 cur + binop + 写回）；
            // 区间 → StoreSlice（仅 Set；复合/开区间 → 运行时错误）
            Expr::Index {
                base,
                indices,
                span,
            } => {
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

    /// [continuous] 值语义判定（P11d，对齐 oracle VarDecl `interp.rs:926-949`）：
    /// 声明类型 `ty` 为连续类（`Type::Named` 且 `TypeTable.continuous`），或
    /// 未标注类型且初始值为标识符（`var p2 = p`——运行时门按值实际类名判定，
    /// 标量/数组/非连续类恒等 = 引用别名）。
    fn needs_deep_copy(&self, ty: Option<&Type>, init: Option<&Expr>) -> bool {
        if let Some(t) = ty {
            return match t.strip() {
                Type::Named(tn, _) => self
                    .types
                    .classes
                    .get(tn)
                    .map(|c| c.continuous)
                    .unwrap_or(false),
                _ => false,
            };
        }
        matches!(init, Some(Expr::Ident(..)))
    }

    /// `alloc.init(SomeClass)` 的默认字段构造：MakeClass + 逐字段默认值（对齐 oracle
    /// 无参构造 `alloc.init(T)` interp.rs:3912-3919——字段逐 `default_value`）。
    /// 运行时 `call_alloc_method_ir("init")` 对 `IrValue::Class` 实参原样返回，语义等价。
    fn lower_alloc_init_defaults(&mut self, ty_name: &str) -> usize {
        // 先克隆字段表释放 `self.types` 借用，再可变借用 `self` 递归降级默认值。
        let fields: Vec<(String, Type)> = self
            .types
            .classes
            .get(ty_name)
            .map(|c| c.fields.clone())
            .unwrap_or_default();
        let mut field_temps = Vec::with_capacity(fields.len());
        for (fname, fty) in &fields {
            let v = self.lower_default_value(fty);
            field_temps.push((fname.clone(), v));
        }
        let t = self.alloc_slot();
        self.push(IrInst::MakeClass {
            temp: t,
            ty: ty_name.to_string(),
            fields: field_temps,
        });
        t
    }

    /// `Type.new(args, alloc)` 构造器降级（对齐 oracle `call_new_builtin` interp.rs:
    /// 4661-4695）：位置参数按字段声明序填充，alloc-first/alloc-last 跳过分配器实参，
    /// 缺省字段落默认值（Vec 字段 → 空 Arr，余同 `lower_default_value`）。发射 `MakeClass`
    /// —— run_ir/字节码/LLVM 三后端经既有指令语义对齐 tree-walking oracle。
    fn lower_new_constructor(&mut self, ty_name: &str, args: &[Expr]) -> usize {
        // 先克隆字段表释放 `self.types` 借用，再可变借用 `self` 递归降级默认值。
        let fields: Vec<(String, Type)> = self
            .types
            .classes
            .get(ty_name)
            .map(|c| c.fields.clone())
            .unwrap_or_default();
        let (vals_start, vals_end) = if args.len() > 1 {
            let is_alloc_first = matches!(&args[0], Expr::Ident(n, _) if n == "alloc");
            let is_alloc_last = matches!(args.last(), Some(Expr::Ident(n, _)) if n == "alloc");
            if is_alloc_first {
                (1usize, args.len())
            } else if is_alloc_last {
                (0usize, args.len() - 1)
            } else {
                (0usize, args.len())
            }
        } else {
            (0usize, args.len())
        };
        let mut ai = vals_start;
        let mut field_temps = Vec::with_capacity(fields.len());
        for (fname, fty) in &fields {
            if ai < vals_end {
                let t = self.lower_expr(&args[ai]);
                field_temps.push((fname.clone(), t));
                ai += 1;
            } else {
                let t = self.lower_default_value(fty);
                field_temps.push((fname.clone(), t));
            }
        }
        let t = self.alloc_slot();
        self.push(IrInst::MakeClass {
            temp: t,
            ty: ty_name.to_string(),
            fields: field_temps,
        });
        t
    }

    /// 类型默认值（对齐 oracle `default_value` interp.rs:1036-1080）：标量零值 /
    /// 空字符串 / 空集合 / `?T`→Opt(None) / 命名 class 递归默认字段 / 枚举空变体。
    fn lower_default_value(&mut self, ty: &Type) -> usize {
        let t = self.alloc_slot();
        match ty.strip() {
            Type::Named(n, args) => {
                // E1.2 组 D D3：类型函数应用（`Pair(i32)`）声明式无初值 → 惰性具体化后
                // 递归（对齐 oracle default_value interp.rs:1438-1440，消除 `__none__`
                // 静默损坏）。具体化失败（类型函数体形状非法）→ 降级硬错误 + void 占位。
                if !args.is_empty() {
                    match self.concrete_type_name(n, args) {
                        Ok(cn) => {
                            let inner = self.lower_default_value(&Type::Named(cn, vec![]));
                            self.push(IrInst::Move { temp: t, a: inner });
                            return t;
                        }
                        Err(msg) => {
                            self.fail_void(t, &msg, &Span::new(0, 0, 0, 0));
                            return t;
                        }
                    }
                }
                match n.as_str() {
                "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64"
                | "u128" | "usize" => {
                    self.push(IrInst::Const {
                        temp: t,
                        val: IrConst::Int(0),
                    });
                }
                "f32" | "f64" | "f16" | "f128" => {
                    self.push(IrInst::Const {
                        temp: t,
                        val: IrConst::Float(0.0),
                    });
                }
                "bool" => {
                    self.push(IrInst::Const {
                        temp: t,
                        val: IrConst::Bool(false),
                    });
                }
                "void" => {
                    self.push(IrInst::Const {
                        temp: t,
                        val: IrConst::Void,
                    });
                }
                "String" | "&[u8]" => {
                    self.push(IrInst::Const {
                        temp: t,
                        val: IrConst::Str(String::new()),
                    });
                }
                // G4：集合默认值 = 隐式环境空容器（持全局 alloc）
                "Vec" | "Deque" | "Table" => {
                    self.push(IrInst::LoadGlobal {
                        temp: t,
                        name: "Vec".into(),
                    });
                }
                "Map" => {
                    self.push(IrInst::LoadGlobal {
                        temp: t,
                        name: "Map".into(),
                    });
                }
                _ => {
                    // Vec(T) / Map(K,V) 泛型集合形态
                    if n == "Vec" || n == "Deque" {
                        self.push(IrInst::LoadGlobal {
                            temp: t,
                            name: "Vec".into(),
                        });
                    } else if n == "Map" {
                        self.push(IrInst::LoadGlobal {
                            temp: t,
                            name: "Map".into(),
                        });
                    } else if let Some(ci) = self.types.classes.get(n) {
                        // 命名 class：递归默认字段（先克隆字段表释放 `self.types` 借用）
                        let cls_fields = ci.fields.clone();
                        let mut fields = Vec::with_capacity(cls_fields.len());
                        for (fname, fty) in &cls_fields {
                            let v = self.lower_default_value(fty);
                            fields.push((fname.clone(), v));
                        }
                        self.push(IrInst::MakeClass {
                            temp: t,
                            ty: n.clone(),
                            fields,
                        });
                    } else {
                        // 未知命名类型（enum 等）：空变体（对齐 oracle default_value Enum 臂）
                        self.push(IrInst::MakeEnum {
                            temp: t,
                            name: n.clone(),
                            variant: "__none__".into(),
                            payload: None,
                        });
                    }
                }
                }
            },
            Type::Optional(_) => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Null,
                });
            }
            Type::Ptr(_, _) | Type::Infer | Type::Owned(_) => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Void,
                });
            }
            Type::Slice(_, _) => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Str(String::new()),
                });
            }
            _ => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Void,
                });
            }
        }
        t
    }

    /// E1.2 组 D：惰性具体化——`Pair(i32)` → 具体化名 `Pair<@i32>`。
    ///
    /// `self.types` 缓存命中即回；未命中则查类型函数定义表（`type_fns`，comptime-only）
    /// → `comptime::instantiate` → 以具体化名登记 `ClassInfo` → 返回具体化名。
    /// `args` 为空 / 非类型函数（内建泛型 `Vec(T)` 等）→ 回退基础名，由调用方既有路径处理。
    ///
    /// 透传形态（`return T;`）产物是**实参类型自身**：返回其规范名（`type_key`），
    /// 使 `Pair(i32)` 与 `i32` 同义。
    fn concrete_type_name(&mut self, name: &str, args: &[Type]) -> Result<String, String> {
        if args.is_empty() {
            return Ok(name.to_string());
        }
        // E1.2 组 D D3：预解析实参——内层类型函数应用先具体化登记（返回具体化键）。
        // 自/互递归类型函数经 `instantiating` 守卫终止（见下）。
        let mut resolved: Vec<Type> = Vec::with_capacity(args.len());
        for a in args {
            resolved.push(self.resolve_nested_types(a)?);
        }
        let cname = comptime::concrete_name(name, &resolved);
        if self.types.classes.contains_key(&cname) || self.types.enums.contains_key(&cname) {
            return Ok(cname);
        }
        // 自/互递归守卫：`LinkedList(i32)` 字段内自引用在登记期重入 → 返回键本身（叶）。
        if self.instantiating.contains(&cname) {
            return Ok(cname);
        }
        if let Some((params, body)) = self.type_fns.get(name) {
            self.instantiating.push(cname.clone());
            let inst = comptime::instantiate(name, params, body, &resolved);
            let result = match inst {
                Ok(Instantiated::Class(mut decl)) => {
                    match self.normalize_decl_fields(&mut decl) {
                        Ok(()) => {
                            if let Decl::Class {
                                name: cn,
                                traits,
                                fields,
                                methods,
                                ..
                            } = &decl
                            {
                                let ci = ClassInfo {
                                    fields: fields
                                        .iter()
                                        .map(|f| (f.name.clone(), f.ty.clone()))
                                        .collect(),
                                    methods: methods.iter().map(|m| m.name.clone()).collect(),
                                    continuous: traits
                                        .iter()
                                        .any(|t| matches!(t, Trait::Continuous)),
                                };
                                self.types.classes.insert(cn.clone(), ci);
                            }
                            Ok(cname)
                        }
                        Err(msg) => Err(msg),
                    }
                }
                Ok(Instantiated::Type(t)) => Ok(comptime::type_key(&t)),
                Err(msg) => Err(msg),
            };
            self.instantiating.pop();
            return result;
        }
        // 非类型函数（内建泛型 `Vec(T)`/`Map(K,V)` 等）：回退基础名，由调用方
        // 既有路径处理（空集合 / 类型未登记 → 未知类型，保持原语义）。
        Ok(name.to_string())
    }

    /// E1.2 组 D D3：深度解析类型中的嵌套类型函数应用（`Pair(i32)` → `Pair<@i32>`）。
    /// 内层先具体化登记；自/互递归经 `instantiating` 守卫返回键（叶）。
    fn resolve_nested_types(&mut self, ty: &Type) -> Result<Type, String> {
        comptime::map_type_apps(ty, &mut |n, a| self.concrete_type_name(n, a))
    }

    /// E1.2 组 D D3：把具体化 Class 声明的字段类型深度规范化——嵌套类型函数应用
    /// （`Pair(i32)`）替换为具体化键（`Pair<@i32>`）；自/互递归经守卫终止。
    fn normalize_decl_fields(&mut self, decl: &mut Decl) -> Result<(), String> {
        if let Decl::Class { fields, .. } = decl {
            for fd in fields.iter_mut() {
                fd.ty = self.resolve_nested_types(&fd.ty)?;
            }
        }
        Ok(())
    }

    // ---------- E1.2 组 D D5：comptime 值函数运行时调用点折叠 ----------

    /// 已知类型名判定（对齐 oracle interp.rs `is_known_type_name`）：基础类型 + 内建
    /// 容器 + 已登记 class/enum + 类型函数。值函数 `T: type` 实参须为已知类型表达式。
    fn is_known_type_name(&self, name: &str) -> bool {
        if matches!(
            name,
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
                | "String"
                | "comptime_int"
                | "comptime_float"
        ) {
            return true;
        }
        if matches!(
            name,
            "Vec" | "Map" | "Deque" | "Table" | "Allocator" | "Arena" | "ExitType"
        ) {
            return true;
        }
        if self.types.classes.contains_key(name) || self.types.enums.contains_key(name) {
            return true;
        }
        // 类型函数名（`fn X(...) type`）
        if self.type_fns.contains_key(name) {
            return true;
        }
        false
    }

    /// 折叠 comptime 值函数调用（`array_len(i32)`）→ `IrConst` 并发射 `Const`。
    /// 类型实参经 `comptime::expr_to_type` 收已知类型表达式（编译期类型值，无运行时
    /// 残留）；值实参常量求值入 bindings；体常量求值取最后 return。任一失败回退
    /// `false` → 调用方走既有路径（未知类型实参等错误在实参降级处报告）。
    fn try_fold_comptime_value_call(&mut self, name: &str, args: &[Expr], t: usize) -> bool {
        let Some((params, body)) = self.value_fns.get(name).cloned() else {
            return false;
        };
        if params.len() != args.len() {
            return false;
        }
        let mut bindings: HashMap<String, IrConst> = HashMap::new();
        for (p, a) in params.iter().zip(args.iter()) {
            if comptime::is_type_param(p) {
                match comptime::expr_to_type(a) {
                    Some(Type::Named(n, _)) if self.is_known_type_name(&n) => {}
                    _ => return false,
                }
            } else {
                match self.eval_const_expr(a, &bindings) {
                    Some(v) => {
                        bindings.insert(p.name.clone(), v);
                    }
                    None => return false,
                }
            }
        }
        if let Ok(Some(v)) = self.eval_const_block(&body, &mut bindings) {
            self.push(IrInst::Const { temp: t, val: v });
            return true;
        }
        false
    }

    /// 常量表达式求值（编译期纯函数）：字面量、值参数引用、一元/二元、if 分支折叠、
    /// 块（委托 `eval_const_block`）。不支持 → None（回退既有路径）。
    fn eval_const_expr(&self, e: &Expr, bindings: &HashMap<String, IrConst>) -> Option<IrConst> {
        match e {
            Expr::IntLit { text, .. } => Some(IrConst::Int(parse_int_lit(text))),
            Expr::FloatLit { text, .. } => {
                let t = text.trim_end_matches(|c: char| c.is_alphabetic());
                let f: f64 = t.replace('_', "").parse().ok()?;
                Some(IrConst::Float(f))
            }
            Expr::BoolLit(b, _) => Some(IrConst::Bool(*b)),
            Expr::StrLit { value, .. } => Some(IrConst::Str(value.clone())),
            Expr::CharLit(c, _) => Some(IrConst::Int(*c as i128)),
            Expr::Ident(n, _) => bindings.get(n).cloned(),
            Expr::Unary(op, inner, _) => {
                let v = self.eval_const_expr(inner, bindings)?;
                const_unary(*op, &v)
            }
            Expr::Binary(BinOp::And, a, b, _) => match self.eval_const_expr(a, bindings)? {
                IrConst::Bool(false) => Some(IrConst::Bool(false)),
                IrConst::Bool(true) => self.eval_const_expr(b, bindings),
                _ => None,
            },
            Expr::Binary(BinOp::Or, a, b, _) => match self.eval_const_expr(a, bindings)? {
                IrConst::Bool(true) => Some(IrConst::Bool(true)),
                IrConst::Bool(false) => self.eval_const_expr(b, bindings),
                _ => None,
            },
            Expr::Binary(op, a, b, _) => {
                let av = self.eval_const_expr(a, bindings)?;
                let bv = self.eval_const_expr(b, bindings)?;
                const_binop(*op, &av, &bv)
            }
            Expr::IfExpr {
                cond,
                then_e,
                else_e,
                ..
            } => match self.eval_const_expr(cond, bindings)? {
                IrConst::Bool(true) => self.eval_const_expr(then_e, bindings),
                IrConst::Bool(false) => self.eval_const_expr(else_e, bindings),
                _ => None,
            },
            Expr::Block(b, _) => {
                let mut b2 = bindings.clone();
                self.eval_const_block(b, &mut b2).ok().flatten()
            }
            _ => None,
        }
    }

    /// 块常量执行（comptime 值函数体求值，对齐 oracle 顺序语义）：
    /// - 语句按序执行；`var`/`const` 初始化并入 bindings；`return` 即返回其值。
    /// - `Stmt::If` 常量条件折叠分支（then/else/else-if）；分支块**未返回**则继续后续语句。
    /// - `Err(())` = 块含无法常量求值的语句（while/for/switch/丢弃调用等）→ 折叠回退；
    ///   `Ok(None)` = 块正常执行完（未返回）；`Ok(Some(v))` = 块返回 v。
    fn eval_const_block(
        &self,
        body: &Block,
        bindings: &mut HashMap<String, IrConst>,
    ) -> Result<Option<IrConst>, ()> {
        for stmt in &body.stmts {
            match stmt {
                Stmt::VarDecl {
                    name, init: Some(e), ..
                } => {
                    let v = self.eval_const_expr(e, bindings).ok_or(())?;
                    bindings.insert(name.clone(), v);
                }
                Stmt::ConstDecl { name, init, .. } => {
                    let v = self.eval_const_expr(init, bindings).ok_or(())?;
                    bindings.insert(name.clone(), v);
                }
                Stmt::Return(Some(e), _) => {
                    return Ok(Some(self.eval_const_expr(e, bindings).ok_or(())?))
                }
                Stmt::Return(None, _) => return Ok(Some(IrConst::Void)),
                Stmt::If(ifst) => {
                    let c = self.eval_const_expr(&ifst.cond, bindings).ok_or(())?;
                    match c {
                        IrConst::Bool(true) => {
                            let r = self.eval_const_block(&ifst.then_b, bindings)?;
                            if r.is_some() {
                                return Ok(r);
                            }
                        }
                        IrConst::Bool(false) => {
                            if let Some(else_b) = &ifst.else_b {
                                let r = match else_b.as_ref() {
                                    Stmt::Block(b2) => self.eval_const_block(b2, bindings)?,
                                    // else-if 链：伪块包一层继续求值
                                    Stmt::If(inner) => {
                                        let pseudo = Block {
                                            stmts: vec![Stmt::If(inner.clone())],
                                            span: inner.span.clone(),
                                        };
                                        self.eval_const_block(&pseudo, bindings)?
                                    }
                                    _ => None,
                                };
                                if r.is_some() {
                                    return Ok(r);
                                }
                            }
                        }
                        _ => return Err(()),
                    }
                }
                Stmt::Block(b2) => {
                    let r = self.eval_const_block(b2, bindings)?;
                    if r.is_some() {
                        return Ok(r);
                    }
                }
                // while/for/switch/丢弃调用等不可常量求值 → 折叠回退
                _ => return Err(()),
            }
        }
        Ok(None)
    }

    fn lower_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::VarDecl { name, init, ty, .. } => {
                // 遮蔽时分配新槽（词法作用域，块退出恢复外层绑定）
                let slot = self.alloc_slot();
                self.bind(name, slot);
                let t = match init {
                    Some(e) => self.lower_expr(e),
                    // 声明式无初值：有类型标注 → 类型默认值（对齐 oracle `default_value`，
                    // 含 D3 类型函数应用的惰性具体化）；无标注 → Void 占位（原行为）。
                    None => match ty {
                        Some(ty) => self.lower_default_value(ty),
                        None => {
                            let t = self.alloc_slot();
                            self.push(IrInst::Const {
                                temp: t,
                                val: IrConst::Void,
                            });
                            t
                        }
                    },
                };
                // [continuous] 值语义（P11d）：声明类型为连续类，或未标注类型且初始
                // 值为标识符 → 赋值前 DeepCopy（后者由运行时门判定，仅连续类深拷贝）。
                if self.needs_deep_copy(ty.as_ref(), init.as_ref()) {
                    let t2 = self.alloc_slot();
                    self.push(IrInst::DeepCopy { temp: t2, a: t });
                    self.push(IrInst::Store { slot, temp: t2 });
                } else {
                    self.push(IrInst::Store { slot, temp: t });
                }
            }
            Stmt::ConstDecl { name, init, .. } => {
                let slot = self.alloc_slot();
                self.bind(name, slot);
                let t = self.lower_expr(init);
                self.push(IrInst::Store { slot, temp: t });
            }
            Stmt::Expr(Expr::Assign {
                target,
                op,
                value,
                span,
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
                // 错误捕获：if (e!T) |v| else |err|——错误值走 l_err（绑定 err）。
                // 仅当存在 then 捕获才分流（`if (x) else |err|` 无捕获 → 维持普通 if 语义，
                // 对齐解释器 exec_if）。
                let l_err = if ifs.capture.is_some() && ifs.err_capture.is_some() {
                    Some(self.new_label())
                } else {
                    None
                };
                match &ifs.capture {
                    // 捕获：if (maybe) |v| / if (e!T) |v|——错误 → l_err；
                    // null → else；否则解包负载绑定捕获名（对齐解释器 exec_if）
                    Some((_, name)) => {
                        if let Some(le) = l_err {
                            self.push(IrInst::JumpIfErr { temp: c, label: le });
                        }
                        self.push(IrInst::JumpIfNull {
                            temp: c,
                            label: l_else,
                        });
                        self.push_scope();
                        let u = self.alloc_slot();
                        self.push(IrInst::Unwrap { temp: u, a: c });
                        self.bind(name, u);
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
                        if let Some(le) = l_err {
                            // err_capture：null 非错误路径不进入 else（else 体仅在
                            // 错误路径执行，err 绑定在作用域内）
                            self.push(IrInst::Jump { label: l_end });
                            self.label(le);
                            if let Some((_, en)) = &ifs.err_capture {
                                self.push_scope();
                                self.bind(en, c);
                            }
                            self.lower_stmt(else_b);
                            if ifs.err_capture.is_some() {
                                self.pop_scope();
                            }
                        } else {
                            self.lower_stmt(else_b);
                        }
                    }
                    None => {
                        self.label(l_else);
                        if let Some(le) = l_err {
                            // err_capture 但无 else（解析器不会产生，防御兜底）
                            self.label(le);
                        }
                    }
                }
                self.label(l_end);
            }
            Stmt::While(w) => {
                let l_top = self.new_label();
                // continue 目标：步进（如有）→ 重测条件（对齐 oracle exec_while）
                let l_cont = self.new_label();
                let l_end = self.new_label();
                // optional 捕获：错误值沿调用链传播（对齐 oracle exec_while `Flow::Return`）
                let l_err = if w.capture.is_some() {
                    Some(self.new_label())
                } else {
                    None
                };
                self.label(l_top);
                let c = self.lower_expr(&w.cond);
                if let Some((_, name)) = &w.capture {
                    if let Some(le) = l_err {
                        self.push(IrInst::JumpIfErr { temp: c, label: le });
                    }
                    // null → 退出循环；否则解包负载绑定捕获名
                    self.push(IrInst::JumpIfNull {
                        temp: c,
                        label: l_end,
                    });
                    self.push_scope();
                    let u = self.alloc_slot();
                    self.push(IrInst::Unwrap { temp: u, a: c });
                    self.bind(name, u);
                    let defer_depth = self.defers.len();
                    self.loops.push(LoopCtx {
                        break_label: l_end,
                        continue_label: l_cont,
                        label: w.label.clone(),
                        defer_depth_at_entry: defer_depth,
                    });
                    self.lower_block(&w.body);
                    self.loops.pop();
                    self.pop_scope();
                } else {
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
                }
                self.label(l_cont);
                if let Some(step) = &w.step {
                    let _ = self.lower_expr(step);
                }
                self.push(IrInst::Jump { label: l_top });
                if let Some(le) = l_err {
                    // 错误传播：return 错误值（errdefer 按值判定）
                    self.label(le);
                    self.emit_defers(0, ErrPath::Value(c));
                    self.push(IrInst::Return { temp: c });
                }
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
            self.fail(
                "`defer`/`errdefer` 体不允许控制流（如 `defer try f()`）",
                span,
            );
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
            let l_next = if i + 1 < n { self.new_label() } else { l_fb };
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
    fn emit_switch_arm_body(&mut self, arm: &SwitchArm, subject: usize, value_slot: Option<usize>) {
        // 对齐 oracle `exec_switch_arm`：push_scope → bind capture → exec body → pop_scope
        self.push_scope();
        if let Some((_, name)) = &arm.capture {
            let cap = self.alloc_slot();
            self.push(IrInst::EnumPayload {
                temp: cap,
                a: subject,
            });
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
            // 格式辅助（M5.3 serialize）
            | "fmt_int" | "fmt_float"
            // 字节工具
            | "read_u64_le"
            // 算法
            | "sort" | "binary_search"
            // 解析器辅助（71-recursive-parser）
            | "skip_space" | "peek" | "advance" | "is_digit" | "parse_number"
            // 文本解析
            | "parse_int" | "parse_float"
            // 组 G 线程（E2.2）：spawn(f, args...) o Thread(T)——协作式延迟执行
            | "spawn"
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
        // alloc.init(ABC) / arena.init(ABC)：类型名参数（运行时按名建空实例）
        "alloc.init" | "arena.init" => i == 0,
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

/// E1.2 组 D D5：一元常量运算（comptime 值函数体折叠）。对齐 oracle 语义
/// （interp.rs:2495-2510）：Neg 仅数值、Not 任意值转布尔、BitNot 仅整数。
/// 不支持 → None（折叠回退既有调用路径）。
fn const_unary(op: UnaryOp, v: &IrConst) -> Option<IrConst> {
    match op {
        UnaryOp::Neg => match v {
            IrConst::Int(i) => Some(IrConst::Int(-i)),
            IrConst::Float(f) => Some(IrConst::Float(-f)),
            _ => None,
        },
        UnaryOp::Not => match v {
            IrConst::Bool(b) => Some(IrConst::Bool(!b)),
            IrConst::Int(i) => Some(IrConst::Bool(*i == 0)),
            IrConst::Float(f) => Some(IrConst::Bool(*f == 0.0)),
            _ => None,
        },
        UnaryOp::BitNot => match v {
            IrConst::Int(i) => Some(IrConst::Int(!i)),
            _ => None,
        },
    }
}

/// E1.2 组 D D5：二元常量运算（comptime 值函数体折叠）。对齐 oracle `binop_values`/
/// `arith`（interp.rs:2811-2933）：Int 溢出回 None（回退）、除零回 None、Int/Float
/// 混算提升 Int→Float、比较按值序。不支持 → None。
fn const_binop(op: BinOp, l: &IrConst, r: &IrConst) -> Option<IrConst> {
    use IrConst::{Bool, Float, Int};
    match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::EucMod => {
            match (l, r) {
                (Int(a), Int(b)) => {
                    let v = match op {
                        BinOp::Add => a.checked_add(*b),
                        BinOp::Sub => a.checked_sub(*b),
                        BinOp::Mul => a.checked_mul(*b),
                        BinOp::Div => (*b != 0).then(|| a / b),
                        BinOp::Mod => (*b != 0).then(|| a % b),
                        BinOp::EucMod => (*b != 0).then(|| a.rem_euclid(*b)),
                        _ => None,
                    };
                    v.map(Int)
                }
                (Float(a), Float(b)) => {
                    let v = match op {
                        BinOp::Add => a + b,
                        BinOp::Sub => a - b,
                        BinOp::Mul => a * b,
                        BinOp::Div => a / b,
                        BinOp::Mod | BinOp::EucMod => a % b,
                        _ => return None,
                    };
                    Some(Float(v))
                }
                (Int(a), Float(_)) => const_binop(op, &Float(*a as f64), r),
                (Float(_), Int(b)) => const_binop(op, l, &Float(*b as f64)),
                _ => None,
            }
        }
        BinOp::Eq => Some(Bool(const_eq(l, r))),
        BinOp::Ne => Some(Bool(!const_eq(l, r))),
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            let lt = const_lt(l, r)?;
            let eq = const_eq(l, r);
            let v = match op {
                BinOp::Lt => lt,
                BinOp::Le => lt || eq,
                BinOp::Gt => !lt && !eq,
                BinOp::Ge => !lt || eq,
                _ => unreachable!(),
            };
            Some(Bool(v))
        }
        BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => match (l, r) {
            (Int(a), Int(b)) => {
                let v = match op {
                    BinOp::BitAnd => a & b,
                    BinOp::BitOr => a | b,
                    BinOp::BitXor => a ^ b,
                    BinOp::Shl => {
                        if *a >= 0 && *a <= u64::MAX as i128 && *b < 64 {
                            ((*a as u64).wrapping_shl(*b as u32)) as i128
                        } else {
                            a << b
                        }
                    }
                    BinOp::Shr => {
                        if *a >= 0 && *a <= u64::MAX as i128 && *b < 64 {
                            ((*a as u64).wrapping_shr(*b as u32)) as i128
                        } else {
                            a >> b
                        }
                    }
                    _ => return None,
                };
                Some(Int(v))
            }
            _ => None,
        },
        _ => None,
    }
}

/// D5：常量相等比较（Int/Float 互比提升、Bool、Str、Null、Void）。
fn const_eq(l: &IrConst, r: &IrConst) -> bool {
    use IrConst::{Bool, Float, Int, Null, Str, Void};
    match (l, r) {
        (Int(a), Int(b)) => a == b,
        (Float(a), Float(b)) => a == b,
        (Int(a), Float(b)) => *a as f64 == *b,
        (Float(a), Int(b)) => *a == *b as f64,
        (Bool(a), Bool(b)) => a == b,
        (Str(a), Str(b)) => a == b,
        (Null, Null) => true,
        (Void, Void) => true,
        _ => l == r,
    }
}

/// D5：常量小于比较（Int/Float 互比提升；其他不支持 → None）。
fn const_lt(l: &IrConst, r: &IrConst) -> Option<bool> {
    use IrConst::{Float, Int};
    match (l, r) {
        (Int(a), Int(b)) => Some(a < b),
        (Float(a), Float(b)) => Some(a < b),
        (Int(a), Float(b)) => Some((*a as f64) < *b),
        (Float(a), Int(b)) => Some(*a < *b as f64),
        _ => None,
    }
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
        // main(args: o Vec(String))——单参数 = 命令行参数（0 号 = 程序名）；或零参版本。
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
            // 指针/装箱实参解引用后匹配
            let a = deref_value(ctx, a);
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

/// 值是否为连续类（[`IrModule::continuous`] 运行时门；`DeepCopy` 指令判定）。
/// 非 Class 值（标量/数组/切片/枚举/指针等）恒 false——恒等 = 引用别名。
fn is_continuous_class(ctx: &Ctx, module: &IrModule, v: &IrValue) -> bool {
    match v {
        IrValue::Class(c) => module.continuous.contains(&class_name(ctx, *c)),
        _ => false,
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
        // 装箱胖指针：data 深拷贝（新 cell），vtbl/alloc 原样携带
        IrValue::Boxed(c) => {
            let (data, vtbl, alloc) = match &ctx.cells[c] {
                Cell::Boxed { data, vtbl, alloc } => (*data, vtbl.clone(), alloc.clone()),
                _ => return IrValue::Boxed(c),
            };
            let cv = ctx.cell_value(data).clone();
            let copied = deep_copy(ctx, cv);
            let new_data = ctx.alloc(Cell::Value(copied));
            IrValue::Boxed(ctx.alloc(Cell::Boxed {
                data: new_data,
                vtbl,
                alloc,
            }))
        }
        // 集合（G4）：Vec items 深拷贝（新 Elems），alloc 原样携带；Map 字段深拷贝
        IrValue::Vec(c) => {
            let (arr, alloc) = match &ctx.cells[c] {
                Cell::Vec { arr, alloc } => (arr.clone(), alloc.clone()),
                _ => return IrValue::Vec(c),
            };
            let copied = deep_copy(ctx, arr);
            IrValue::Vec(ctx.alloc(Cell::Vec { arr: copied, alloc }))
        }
        IrValue::Map(c) => {
            let (fields, alloc) = match &ctx.cells[c] {
                Cell::Map { fields, alloc } => (fields.clone(), alloc.clone()),
                _ => return IrValue::Map(c),
            };
            let new_fields: HashMap<String, usize> = fields
                .iter()
                .map(|(k, vc)| {
                    let cv = ctx.cell_value(*vc).clone();
                    let copied = deep_copy(ctx, cv);
                    (k.clone(), ctx.alloc(Cell::Value(copied)))
                })
                .collect();
            IrValue::Map(ctx.alloc(Cell::Map {
                fields: new_fields,
                alloc,
            }))
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
        IrValue::Vec(_) => "array".into(),
        IrValue::Map(_) => "Map".into(),
        IrValue::Slice { .. } => "slice".into(),
        IrValue::Class(c) => class_name(ctx, *c),
        IrValue::Arena(_) => "Arena".into(),
        IrValue::Enum { name, .. } => name.clone(),
        IrValue::Opt(_) => "optional".into(),
        IrValue::Err { .. } => "error".into(),
        IrValue::Ptr(_) => "pointer".into(),
        IrValue::Boxed(_) => "pointer".into(),
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
fn exec_func(
    ctx: &mut Ctx,
    module: &IrModule,
    idx: usize,
    args: &[IrValue],
    depth: usize,
) -> R<IrValue> {
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
fn exec_body(
    ctx: &mut Ctx,
    module: &IrModule,
    func: &IrFunc,
    frame: Frame,
    depth: usize,
) -> R<IrValue> {
    ctx.cur_depth = depth;
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
                ctx.set(
                    &frame,
                    *temp,
                    match val {
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
                    },
                );
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
                ctx.set(
                    &frame,
                    *temp,
                    match op {
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
                    },
                );
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
                let callee_idx = pick_func(ctx, module, name, &arg_vals)
                    .ok_or_else(|| IrError::msg("NoFunction", format!("no function `{name}`")))?;
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
                // 解引用：Ptr/Boxed → pointee；非 Ptr → 恒等（对齐 tree-walking `deref_value`）
                let v = match ctx.get(&frame, *a) {
                    IrValue::Ptr(cell) => ctx.cell_value(*cell).clone(),
                    IrValue::Boxed(cell) => match &ctx.cells[*cell] {
                        Cell::Boxed { data, .. } => ctx.cell_value(*data).clone(),
                        _ => IrValue::Void,
                    },
                    other => other.clone(),
                };
                ctx.set(&frame, *temp, v);
            }
            IrInst::StorePtr { target, value } => {
                let t = ctx.get(&frame, *target).clone();
                let v = ctx.get(&frame, *value).clone();
                match t {
                    IrValue::Ptr(cell) => ctx.set_cell(cell, v),
                    // 装箱胖指针：写穿 data cell
                    IrValue::Boxed(cell) => {
                        let data = match &ctx.cells[cell] {
                            Cell::Boxed { data, .. } => Some(*data),
                            _ => None,
                        };
                        match data {
                            Some(d) => ctx.set_cell(d, v),
                            None => return Err(IrError::msg("BadAssign", "store to non-pointer")),
                        }
                    }
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
                // K1 union 写路径：字段写后字节重解释同步其余字段（对齐 interp assign_class_field）
                if let IrValue::Class(c) = bv {
                    if let Cell::Class { fields, .. } = &ctx.cells[c] {
                        if fields.contains_key("@union") {
                            union_sync_ir(ctx, module, c, field)?;
                        }
                    }
                }
            }
            IrInst::UnionSync { class, written } => {
                let c = match ctx.get(&frame, *class) {
                    IrValue::Class(c) => *c,
                    _ => return Err(IrError::msg("TypeError", "union sync on non-class")),
                };
                union_sync_ir(ctx, module, c, written)?;
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
            IrInst::StoreSlice {
                base,
                lo,
                hi,
                value,
            } => {
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
            IrInst::MakeEnum {
                temp,
                name,
                variant,
                payload,
            } => {
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
                        _ => {
                            return Err(IrError::msg("TupleArity", "expected tuple in destructure"))
                        }
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
            IrInst::DeepCopy { temp, a } => {
                let v = ctx.get(&frame, *a).clone();
                // 运行时门：仅连续类深拷贝（标量/数组/非连续类恒等 = 引用别名）
                let v = if is_continuous_class(ctx, module, &v) {
                    deep_copy(ctx, v)
                } else {
                    v
                };
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
            IrInst::MatchTest {
                temp,
                subject,
                pattern,
            } => {
                let sv = deref_value(ctx, ctx.get(&frame, *subject)).clone();
                ctx.set(&frame, *temp, IrValue::Bool(match_pattern(&sv, pattern)));
            }
            IrInst::MakeRange { temp, lo, hi } => {
                let lo_v = deref_value(ctx, ctx.get(&frame, *lo)).clone();
                let hi_v = deref_value(ctx, ctx.get(&frame, *hi)).clone();
                let (lo_i, hi_i) = match (lo_v, hi_v) {
                    (IrValue::Int(a), IrValue::Int(b)) => (a, b),
                    _ => return Err(IrError::msg("TypeError", "range bounds must be integers")),
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
            IrInst::IterNext {
                has,
                iter,
                slot,
                read_only,
            } => {
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
                            // Read 捕获：槽 cell 置为该项值副本（与容器无别名；
                            // 非 Value cell——如 Map 的 KV Class 条目——用 read_cell 还原句柄）
                            let v = ctx.read_cell(it.cell);
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
                let v = if name == "alloc" && ctx.current_alloc.is_some() {
                    // Q8：线程子任务期间 `alloc` 解析到每线程 arena（对齐 oracle `lookup`）
                    ctx.current_alloc.clone().unwrap()
                } else {
                    let cell = ctx.globals.get(name).copied().ok_or_else(|| {
                        IrError::msg("NoGlobal", format!("undefined global `{name}`"))
                    })?;
                    ctx.cell_value(cell).clone()
                };
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
                    } => {
                        call_closure_ir(ctx, module, func, &captures, &arg_vals, is_mut, depth + 1)?
                    }
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
                let raw = ctx.get(&frame, *base).clone();
                // G3：装箱胖指针 .alloc() → 携带的分配器引用（三字宽胖指针的 alloc 字）
                if let IrValue::Boxed(bc) = &raw {
                    if method == "alloc" {
                        if let Cell::Boxed { alloc, .. } = &ctx.cells[*bc] {
                            ctx.set(&frame, *temp, alloc.clone());
                            pc += 1;
                            continue;
                        }
                    }
                }
                // G4：集合 .alloc() → 构造 `init(alloc)` 时携带的分配器引用
                if let IrValue::Vec(vc) = &raw {
                    if method == "alloc" {
                        if let Cell::Vec { alloc, .. } = &ctx.cells[*vc] {
                            ctx.set(&frame, *temp, alloc.clone());
                            pc += 1;
                            continue;
                        }
                    }
                }
                if let IrValue::Map(mc) = &raw {
                    if method == "alloc" {
                        if let Cell::Map { alloc, .. } = &ctx.cells[*mc] {
                            ctx.set(&frame, *temp, alloc.clone());
                            pc += 1;
                            continue;
                        }
                    }
                }
                let self_v = deref_value(ctx, &raw).clone();
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

/// 限定名根的隐式环境/虚拟根分派（io.*、alloc.*、json.parse、csv.parse、String.from、math.*、
/// Arena.init、serialize.*）
fn is_dotted_implicit_root(root: &str) -> bool {
    IMPLICIT_ENV.contains(&root)
        || matches!(
            root,
            "json" | "csv" | "String" | "math" | "Arena" | "serialize"
        )
}

/// 错误值（码 = 编译期错误码表；内建产生的错误与 `error.X` 字面量同码）
fn err_val(module: &IrModule, name: &str) -> IrValue {
    let code = module.error_codes.get(name).copied().unwrap_or(0);
    IrValue::Err {
        name: name.to_string(),
        code,
    }
}

/// 分配 n 字节零初始化内存；n ≤ 0 → 空；n 超出可表示容量 / 分配失败 → None
/// （调用方转 `error.OutOfMemory`——与 interp `alloc_zeroed_bytes` 对齐；
/// `vec![0u8; n]` 对超大 n 会直接中止进程，分配失败应为可 catch 的错误值）
fn alloc_zeroed_bytes_ir(n: i128) -> Option<Vec<u8>> {
    if n <= 0 {
        return Some(Vec::new());
    }
    if n as u128 > usize::MAX as u128 {
        return None;
    }
    let mut v = Vec::new();
    v.try_reserve_exact(n as usize).ok()?;
    v.resize(n as usize, 0u8);
    Some(v)
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

/// 集合 Vec（G4）：items 存于共享 Elems cell；`alloc` = 构造时携带的分配器引用
fn make_vec_with(ctx: &mut Ctx, items: Vec<IrValue>, alloc: IrValue) -> IrValue {
    let arr = make_arr(ctx, items);
    let inner = match arr {
        IrValue::Arr(c) => c,
        _ => unreachable!("make_arr 恒返回 Arr"),
    };
    IrValue::Vec(ctx.alloc(Cell::Vec {
        arr: IrValue::Arr(inner),
        alloc,
    }))
}

/// 集合 Map（G4）：键 → 字段 cell；`alloc` = 构造时携带的分配器引用
fn make_map_with(ctx: &mut Ctx, fields: HashMap<String, usize>, alloc: IrValue) -> IrValue {
    IrValue::Map(ctx.alloc(Cell::Map { fields, alloc }))
}

/// M5.4 Io 实例（含 fs/time/net 子模块 + G1-G5 扩展——对齐 oracle `io_value_with_runtime`
/// interp.rs:2016-2043）。net 含 `udp` 子命名空间；stdout/stderr 独立字节流；
/// ipc/storage/archive/text/rng 各命名空间类名供 call_builtin_method 分派。
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
    // G1（E3.1）：`io.net.udp` 子命名空间（bind/send_to/recv_from）——UdpSocket 实例
    // 方法由类名分派，命名空间形式委托同实现（对齐 oracle net_fields 含 udp）。
    let udp = ctx.alloc(Cell::Class {
        name: "Udp".into(),
        fields: HashMap::new(),
    });
    let udp_cell = ctx.alloc(Cell::Value(IrValue::Class(udp)));
    let mut net_fields = HashMap::new();
    net_fields.insert("udp".into(), udp_cell);
    let net = ctx.alloc(Cell::Class {
        name: "Net".into(),
        fields: net_fields,
    });
    let net_cell = ctx.alloc(Cell::Value(IrValue::Class(net)));
    let mut fields = HashMap::new();
    fields.insert("fs".into(), fs_cell);
    fields.insert("time".into(), time_cell);
    fields.insert("net".into(), net_cell);
    fields.insert("runtime".into(), ctx.alloc(Cell::Value(str_val("threaded"))));
    // G2（io 差异项）：io.stdout/io.stderr 独立字节流（write_all 写真实句柄；
    // 类名 Stdout/Stderr 供分派，无 fd 注册表）
    let stdout = ctx.alloc(Cell::Class {
        name: "Stdout".into(),
        fields: HashMap::new(),
    });
    fields.insert("stdout".into(), ctx.alloc(Cell::Value(IrValue::Class(stdout))));
    let stderr = ctx.alloc(Cell::Class {
        name: "Stderr".into(),
        fields: HashMap::new(),
    });
    fields.insert("stderr".into(), ctx.alloc(Cell::Value(IrValue::Class(stderr))));
    // G3（E3.2 ipc）：io.ipc.pipe() / io.ipc.shm(name, size)——进程内 IPC 原语
    let ipc = ctx.alloc(Cell::Class {
        name: "Ipc".into(),
        fields: HashMap::new(),
    });
    fields.insert("ipc".into(), ctx.alloc(Cell::Value(IrValue::Class(ipc))));
    // G4（E3.3 storage/archive）：io.storage.open(path) / io.archive.compress/decompress
    let storage = ctx.alloc(Cell::Class {
        name: "Storage".into(),
        fields: HashMap::new(),
    });
    fields.insert("storage".into(), ctx.alloc(Cell::Value(IrValue::Class(storage))));
    let archive = ctx.alloc(Cell::Class {
        name: "Archive".into(),
        fields: HashMap::new(),
    });
    fields.insert("archive".into(), ctx.alloc(Cell::Value(IrValue::Class(archive))));
    // G5（E3.3 text/rng）：io.text.* 正则；io.rng.* 伪随机数（类名 RngNs 避开示例
    // 84-rng 的用户类 Rng——内建方法先于用户方法分派）
    let text = ctx.alloc(Cell::Class {
        name: "Text".into(),
        fields: HashMap::new(),
    });
    fields.insert("text".into(), ctx.alloc(Cell::Value(IrValue::Class(text))));
    let rng = ctx.alloc(Cell::Class {
        name: "RngNs".into(),
        fields: HashMap::new(),
    });
    fields.insert("rng".into(), ctx.alloc(Cell::Value(IrValue::Class(rng))));
    IrValue::Class(ctx.alloc(Cell::Class {
        name: "Io".into(),
        fields,
    }))
}

/// 隐式环境值（对齐 oracle 隐式环境注入：alloc→Alloc、io/test_io/stdout/stderr→Io、
/// pi→Float(PI)、Vec/Deque/Table→空 Arr、Map→空 Map）
fn implicit_env_value(ctx: &mut Ctx, name: &str) -> IrValue {
    match name {
        "alloc" => {
            // Q8：每线程 alloc 覆盖（线程 fn 运行期间）优先；否则全局 Class("Alloc") 哨兵
            if let Some(a) = &ctx.current_alloc {
                a.clone()
            } else {
                IrValue::Class(ctx.alloc(Cell::Class {
                    name: "Alloc".into(),
                    fields: HashMap::new(),
                }))
            }
        }
        // Arena 类型构造根（G1）：`Arena.init(alloc)` → 真实 arena 句柄
        "Arena" => IrValue::Arena(ctx.alloc(Cell::Arena(ArenaStateIr::new()))),
        "io" | "test_io" | "stdout" | "stderr" => io_value_ir(ctx),
        "pi" => IrValue::Float(std::f64::consts::PI),
        // G4：集合隐式根 = 空容器，持全局 alloc（`Vec(i32)` 类型表达式 / `Vec.init(alloc)` 基）
        "Vec" | "Deque" | "Table" => {
            let alloc = implicit_env_value(ctx, "alloc");
            make_vec_with(ctx, Vec::new(), alloc)
        }
        "Map" => {
            let alloc = implicit_env_value(ctx, "alloc");
            make_map_with(ctx, HashMap::new(), alloc)
        }
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
        // 集合（G4）：Vec 句柄（Ptr(Vec) 一层 deref 后为 Vec）——共享 Elems 元素
        IrValue::Vec(c) => match &ctx.cells[c] {
            Cell::Vec {
                arr: IrValue::Arr(ac),
                ..
            } => match &ctx.cells[*ac] {
                Cell::Elems(e) => Ok(e.iter().map(|ec| ctx.cell_value(*ec).clone()).collect()),
                _ => Err(IrError::msg("TypeError", "bad vec")),
            },
            _ => Err(IrError::msg("TypeError", "bad vec")),
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
        // 集合（G4）：Map 句柄 → KV 条目（key/value 字段）
        IrValue::Map(c) => {
            let fields = match &ctx.cells[c] {
                Cell::Map { fields, .. } => fields.clone(),
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
        IrValue::Boxed(c) => match &ctx.cells[*c] {
            Cell::Boxed { data, .. } => value_to_bytes_ir(ctx, ctx.cell_value(*data)),
            _ => vec![],
        },
        // 集合（G4）：Vec 委托 Arr 字节化
        IrValue::Vec(c) => match &ctx.cells[*c] {
            Cell::Vec { arr, .. } => value_to_bytes_ir(ctx, arr),
            _ => vec![],
        },
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
        // 集合（G4）：Vec 委托 Arr JSON 化；Map 序列化为对象
        IrValue::Vec(c) => match &ctx.cells[*c] {
            Cell::Vec { arr, .. } => value_to_json_ir(ctx, arr),
            _ => "null".into(),
        },
        IrValue::Map(c) => match &ctx.cells[*c] {
            Cell::Map { fields, .. } => {
                let items: Vec<String> = fields
                    .iter()
                    .map(|(k, vc)| {
                        format!("\"{k}\":{}", value_to_json_ir(ctx, ctx.cell_value(*vc)))
                    })
                    .collect();
                format!("{{{}}}", items.join(","))
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
        IrValue::Boxed(c) => match &ctx.cells[*c] {
            Cell::Boxed { data, .. } => value_to_json_ir(ctx, ctx.cell_value(*data)),
            _ => "null".into(),
        },
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

// ---------- K1 无标签 union（ADR-0014，2026-08-18）----------
// 运行时形态 = `Cell::Class` + `@union` 标记；写字段 → 字节重解释同步其余字段
// （C 风格内存双关）。helper 对齐 interp `union_write_scalar`/`union_read_scalar`/
// `union_sync_fields`。

/// 类型名（union 字段须为标量 → `Type::Named(n, _)`）
fn union_ty_name(t: &Type) -> Option<String> {
    match t.strip() {
        Type::Named(n, _) => Some(n.clone()),
        _ => None,
    }
}

/// 标量值 → 小端字节（i128/u128 全 16 字节，对齐 interp union_write_scalar）
fn write_scalar_ir(out: &mut [u8], n: &str, v: &IrValue) {
    match (n, v) {
        ("i8" | "u8", IrValue::Int(i)) => out[0] = *i as u8,
        ("i16" | "u16", IrValue::Int(i)) => {
            out[..2].copy_from_slice(&(*i as i16).to_le_bytes())
        }
        ("i32" | "u32", IrValue::Int(i)) => {
            out[..4].copy_from_slice(&(*i as i32).to_le_bytes())
        }
        ("i64" | "u64" | "isize" | "usize", IrValue::Int(i)) => {
            out[..8].copy_from_slice(&(*i as i64).to_le_bytes())
        }
        ("i128" | "u128", IrValue::Int(i)) => {
            out[..16].copy_from_slice(&i.to_le_bytes())
        }
        ("f32", IrValue::Float(f)) => out[..4].copy_from_slice(&(*f as f32).to_le_bytes()),
        ("f64" | "f16" | "f128", IrValue::Float(f)) => {
            out[..8].copy_from_slice(&f.to_le_bytes())
        }
        ("bool", IrValue::Bool(b)) => out[0] = if *b { 1 } else { 0 },
        _ => {}
    }
}

/// 小端字节 → 标量值（对齐 interp union_read_scalar）
fn read_scalar_ir(bytes: &[u8], n: &str) -> R<IrValue> {
    let trunc = |msg: &str| IrError::msg("InvalidBytes", msg);
    match n {
        "i8" | "u8" => Ok(IrValue::Int(bytes.first().copied().unwrap_or(0) as i128)),
        "i16" | "u16" => {
            let b = bytes.get(..2).ok_or_else(|| trunc("truncated union bytes"))?;
            Ok(IrValue::Int(i16::from_le_bytes(b.try_into().unwrap()) as i128))
        }
        "i32" | "u32" => {
            let b = bytes.get(..4).ok_or_else(|| trunc("truncated union bytes"))?;
            Ok(IrValue::Int(i32::from_le_bytes(b.try_into().unwrap()) as i128))
        }
        "i64" | "u64" | "isize" | "usize" => {
            let b = bytes.get(..8).ok_or_else(|| trunc("truncated union bytes"))?;
            Ok(IrValue::Int(i64::from_le_bytes(b.try_into().unwrap()) as i128))
        }
        "i128" | "u128" => {
            let b = bytes.get(..16).ok_or_else(|| trunc("truncated union bytes"))?;
            Ok(IrValue::Int(i128::from_le_bytes(b.try_into().unwrap())))
        }
        "f32" => {
            let b = bytes.get(..4).ok_or_else(|| trunc("truncated union bytes"))?;
            Ok(IrValue::Float(f32::from_le_bytes(b.try_into().unwrap()) as f64))
        }
        "f64" | "f16" | "f128" => {
            let b = bytes.get(..8).ok_or_else(|| trunc("truncated union bytes"))?;
            Ok(IrValue::Float(f64::from_le_bytes(b.try_into().unwrap())))
        }
        "bool" => Ok(IrValue::Bool(bytes.first().copied().unwrap_or(0) != 0)),
        _ => Ok(IrValue::Void),
    }
}

/// K1 union 写字段同步（IR 运行时）：写 `written` 字段后，把该字段字节重解释为
/// 其余每个字段的类型（C 风格 union 语义，字段全标量）。`c` = `Cell::Class` 索引。
fn union_sync_ir(ctx: &mut Ctx, module: &IrModule, c: usize, written: &str) -> R<()> {
    let (cname, fields) = match &ctx.cells[c] {
        Cell::Class { name, fields } => (name.clone(), fields.clone()),
        _ => return Err(IrError::msg("TypeError", "union sync on non-class")),
    };
    let decls = module.unions.get(&cname).cloned().ok_or_else(|| {
        IrError::msg(
            "TypeError",
            format!("`{cname}` 不是 union 类型"),
        )
    })?;
    let wcell = fields.get(written).copied().ok_or_else(|| {
        IrError::msg("NoField", format!("union `{cname}` has no field `{written}`"))
    })?;
    let wv = ctx.cell_value(wcell).clone();
    let wty = decls
        .iter()
        .find(|(n, _)| n == written)
        .map(|(_, t)| t.clone())
        .ok_or_else(|| {
            IrError::msg(
                "NoField",
                format!("union `{cname}` has no field `{written}`"),
            )
        })?;
    let wname = union_ty_name(&wty).ok_or_else(|| {
        IrError::msg("TypeError", "union 字段必须为标量类型")
    })?;
    let width = scalar_size_ir(&wname)
        .ok_or_else(|| IrError::msg("TypeError", format!("字段 `{wname}` 无标量宽度")))?;
    let mut buf = vec![0u8; width];
    write_scalar_ir(&mut buf, &wname, &wv);
    for (fdname, fdty) in &decls {
        if fdname == written {
            continue;
        }
        let Some(fname) = union_ty_name(fdty) else { continue };
        let dv = read_scalar_ir(&buf, &fname)?;
        let nc = ctx.alloc(Cell::Value(dv));
        if let Cell::Class { fields: fs, .. } = &mut ctx.cells[c] {
            fs.insert(fdname.clone(), nc);
        }
    }
    Ok(())
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
        if fmt[i] == b'{' {
            if let Some(close) = fmt[i + 1..].iter().position(|&c| c == b'}') {
                if argi < args.len() {
                    let v = deref_value(ctx, &args[argi]);
                    let s = format_spec_value_ir(ctx, v, &fmt[i + 1..i + 1 + close])?;
                    out.extend_from_slice(s.as_bytes());
                    argi += 1;
                }
                i += close + 2;
                continue;
            }
        }
        out.push(fmt[i]);
        i += 1;
    }
    ctx.out.extend_from_slice(&out);
    Ok(())
}

/// 格式说明符（B1/B3，镜像 interp `format_spec_value`）：`{}` 默认 / `{d}` / `{x}` /
/// `{X}` / `{b}` / `{e}` / `{s}` + 宽度/对齐/精度（`{:8}`、`{:<6}`、`{:.2}`）。
/// 未知类型字符 → `FormatError`（B2：不再按字面量静默输出）。
fn format_spec_value_ir(ctx: &Ctx, v: &IrValue, inner: &[u8]) -> R<String> {
    let mut p = if inner.first() == Some(&b':') { 1 } else { 0 };
    let align = match inner.get(p) {
        Some(b'<') | Some(b'>') | Some(b'^') => {
            let a = inner[p];
            p += 1;
            a
        }
        _ => b'>',
    };
    let mut width: Option<usize> = None;
    let mut ws = String::new();
    while p < inner.len() && inner[p].is_ascii_digit() {
        ws.push(inner[p] as char);
        p += 1;
    }
    if !ws.is_empty() {
        width = ws.parse().ok();
    }
    let mut precision: Option<usize> = None;
    if p < inner.len() && inner[p] == b'.' {
        p += 1;
        let mut ps = String::new();
        while p < inner.len() && inner[p].is_ascii_digit() {
            ps.push(inner[p] as char);
            p += 1;
        }
        precision = ps.parse().ok();
    }
    let ty = inner.get(p).copied();
    if p + usize::from(ty.is_some()) < inner.len() {
        return Err(IrError::msg("FormatError", "unknown format specifier"));
    }
    let display = v.display(ctx);
    let mut s = match ty {
        Some(b'd') => match v {
            IrValue::Int(n) => n.to_string(),
            IrValue::Float(f) => f.to_string(),
            _ => display,
        },
        Some(b'x') => match v {
            IrValue::Int(n) => format!("{n:x}"),
            _ => display,
        },
        Some(b'X') => match v {
            IrValue::Int(n) => format!("{n:X}"),
            _ => display,
        },
        Some(b'b') => match v {
            IrValue::Int(n) => format!("{n:b}"),
            _ => display,
        },
        Some(b'e') => match v {
            IrValue::Float(f) => format!("{f:e}"),
            _ => display,
        },
        Some(b's') => display,
        Some(_) => return Err(IrError::msg("FormatError", "unknown format specifier")),
        None => display,
    };
    if let Some(pr) = precision {
        if let IrValue::Float(f) = v {
            s = format!("{f:.pr$}");
        }
    }
    if let Some(w) = width {
        if s.len() < w {
            let pad = w - s.len();
            match align {
                b'<' => s = format!("{s}{}", " ".repeat(pad)),
                b'^' => {
                    let l = pad / 2;
                    s = format!("{}{s}{}", " ".repeat(l), " ".repeat(pad - l));
                }
                _ => s = format!("{}{s}", " ".repeat(pad)),
            }
        }
    }
    Ok(s)
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
                "add" => {
                    Some(IrValue::Int(a.checked_add(*b).ok_or_else(|| {
                        IrError::msg("Overflow", "integer overflow")
                    })?))
                }
                "sub" => {
                    Some(IrValue::Int(a.checked_sub(*b).ok_or_else(|| {
                        IrError::msg("Overflow", "integer overflow")
                    })?))
                }
                "mul" => {
                    Some(IrValue::Int(a.checked_mul(*b).ok_or_else(|| {
                        IrError::msg("Overflow", "integer overflow")
                    })?))
                }
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

fn parser_pos(_ctx: &Ctx, args: &[IrValue], ix: usize) -> R<usize> {
    let v = args
        .get(ix)
        .ok_or_else(|| IrError::msg("ArityMismatch", "missing argument"))?;
    // 不 deref：位置参数是 `&pos` 指针（AddrSlot → IrValue::Ptr(cell)），
    // deref_value 会追到 pointee（Int）导致 Ptr 匹配失败（对齐 oracle interp get_pos）
    match v {
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
            let n: i128 = String::from_utf8_lossy(&data[start..i])
                .parse()
                .unwrap_or(0);
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

// ---- G1-G5 注册表句柄解析（Dir/Pipe/Shm/KvStore）----

/// Dir 值 → 注册表 fd（`_fd` 字段；先 deref_value 剥 Ptr）
fn dir_fd_ir(ctx: &Ctx, v: &IrValue) -> R<i64> {
    match deref_value(ctx, v) {
        IrValue::Class(c) => match &ctx.cells[*c] {
            Cell::Class { fields, .. } => match fields.get("_fd") {
                Some(fc) => match ctx.cell_value(*fc) {
                    IrValue::Int(fd) => Ok(*fd as i64),
                    _ => Err(IrError::msg("BadFd", "bad dir descriptor")),
                },
                _ => Err(IrError::msg("BadFd", "bad dir descriptor")),
            },
            _ => Err(IrError::msg("BadFd", "bad dir descriptor")),
        },
        _ => Err(IrError::msg("TypeError", "expected Dir")),
    }
}

/// Pipe 值 → 管道 id（`pipe` 字段；先 deref_value 剥 Ptr）
fn pipe_id_ir(ctx: &Ctx, v: &IrValue) -> R<i64> {
    match deref_value(ctx, v) {
        IrValue::Class(c) => match &ctx.cells[*c] {
            Cell::Class { fields, .. } => match fields.get("pipe") {
                Some(fc) => match ctx.cell_value(*fc) {
                    IrValue::Int(id) => Ok(*id as i64),
                    _ => Err(IrError::msg("BadFd", "bad pipe descriptor")),
                },
                _ => Err(IrError::msg("BadFd", "bad pipe descriptor")),
            },
            _ => Err(IrError::msg("BadFd", "bad pipe descriptor")),
        },
        _ => Err(IrError::msg("TypeError", "expected pipe")),
    }
}

/// Shm 值 → 共享内存 id（`shm` 字段）
fn shm_id_ir(ctx: &Ctx, v: &IrValue) -> R<i64> {
    match deref_value(ctx, v) {
        IrValue::Class(c) => match &ctx.cells[*c] {
            Cell::Class { fields, .. } => match fields.get("shm") {
                Some(fc) => match ctx.cell_value(*fc) {
                    IrValue::Int(id) => Ok(*id as i64),
                    _ => Err(IrError::msg("BadFd", "bad shm descriptor")),
                },
                _ => Err(IrError::msg("BadFd", "bad shm descriptor")),
            },
            _ => Err(IrError::msg("BadFd", "bad shm descriptor")),
        },
        _ => Err(IrError::msg("TypeError", "expected Shm")),
    }
}

/// KvStore 值 → 注册表 id（`store` 字段；先 deref_value 剥 Ptr）
fn store_id_ir(ctx: &Ctx, v: &IrValue) -> R<i64> {
    match deref_value(ctx, v) {
        IrValue::Class(c) => match &ctx.cells[*c] {
            Cell::Class { fields, .. } => match fields.get("store") {
                Some(fc) => match ctx.cell_value(*fc) {
                    IrValue::Int(id) => Ok(*id as i64),
                    _ => Err(IrError::msg("BadFd", "bad store descriptor")),
                },
                _ => Err(IrError::msg("BadFd", "bad store descriptor")),
            },
            _ => Err(IrError::msg("BadFd", "bad store descriptor")),
        },
        _ => Err(IrError::msg("TypeError", "expected KvStore")),
    }
}

// ---- G1-G5 网络/文件系统共享实现（对齐 oracle interp.rs 对应函数）----

/// 解析 UDP 对端地址串 "host:port" → (host, port)。
fn parse_udp_addr_ir(s: &str) -> std::result::Result<(String, u16), &'static str> {
    match s.rsplit_once(':') {
        Some((host, port)) => match port.parse::<u16>() {
            Ok(p) => Ok((host.to_string(), p)),
            Err(_) => Err("InvalidAddress"),
        },
        None => Err("InvalidAddress"),
    }
}

/// UDP 绑定共享实现：`udp_bind(host, port)` → UdpSocket 值（fd 注册表）；
/// 读超时 200ms（recv_from 空队列 → error.TimedOut，不阻塞挂起测试）。
fn udp_bind_ir(ctx: &mut Ctx, module: &IrModule, host: &str, port: u16) -> R<IrValue> {
    let addr = format!("{host}:{port}");
    match std::net::UdpSocket::bind(&addr) {
        Ok(sock) => {
            let _ = sock.set_read_timeout(Some(std::time::Duration::from_millis(200)));
            let fd = ctx.next_net_fd;
            ctx.next_net_fd += 1;
            ctx.udp_sockets.insert(fd, sock);
            let mut fields = HashMap::new();
            fields.insert(
                "fd".into(),
                ctx.alloc(Cell::Value(IrValue::Int(fd as i128))),
            );
            Ok(IrValue::Class(ctx.alloc(Cell::Class {
                name: "UdpSocket".into(),
                fields,
            })))
        }
        Err(e) => Ok(err_val(module, &io_error_name_ir(&e))),
    }
}

/// G1（E3.1）：HTTP GET 客户端——`http://host[:port][/path]` → TCP connect →
/// `GET {path} HTTP/1.1` + Host 头 → 读响应 → 按 Content-Length 提取体。
fn http_get_ir(url: &str) -> std::result::Result<Vec<u8>, String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| "InvalidUrl".to_string())?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (
            h.to_string(),
            p.parse::<u16>().map_err(|_| "InvalidUrl".to_string())?,
        ),
        None => (authority.to_string(), 80u16),
    };
    let mut stream = std::net::TcpStream::connect((host.as_str(), port))
        .map_err(|e| io_error_name_ir(&e))?;
    let req = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(req.as_bytes())
        .map_err(|e| io_error_name_ir(&e))?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| io_error_name_ir(&e))?;
    // 状态行 + 头段由第一个空行分隔；体按 Content-Length 取（无则取空行后全部）
    let head_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .ok_or_else(|| "BadResponse".to_string())?;
    let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
    let body = &raw[head_end..];
    if !head.starts_with("HTTP/1.1 200") && !head.starts_with("HTTP/1.0 200") {
        // 非 200：体返回给调用方诊断（错误名 = Http{code}）
        let code = head
            .split_whitespace()
            .nth(1)
            .unwrap_or("000")
            .to_string();
        return Err(format!("Http{code}"));
    }
    let mut len: Option<usize> = None;
    for line in head.lines() {
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            if let Ok(n) = v.trim().parse::<usize>() {
                len = Some(n);
            }
        }
    }
    Ok(match len {
        Some(n) => body[..n.min(body.len())].to_vec(),
        None => body.to_vec(),
    })
}

/// G2（io 差异项）：枚举目录路径为 Vec(DirEntry)——每条 = {name: 文件名, is_dir: 是否目录}。
/// 供 io.fs.list_dir（路径/句柄双形态）与 dir.list_dir(alloc) 共用。
fn list_dir_entries_ir(ctx: &mut Ctx, module: &IrModule, path: &str) -> R<IrValue> {
    match std::fs::read_dir(path) {
        Ok(rd) => {
            let entries: Vec<IrValue> = rd
                .flatten()
                .map(|e| {
                    let mut fields = HashMap::new();
                    fields.insert(
                        "name".into(),
                        ctx.alloc(Cell::Value(str_val(&e.file_name().to_string_lossy()))),
                    );
                    fields.insert(
                        "is_dir".into(),
                        ctx.alloc(Cell::Value(IrValue::Bool(
                            e.file_type().map(|t| t.is_dir()).unwrap_or(false),
                        ))),
                    );
                    IrValue::Class(ctx.alloc(Cell::Class {
                        name: "DirEntry".into(),
                        fields,
                    }))
                })
                .collect();
            Ok(make_arr(ctx, entries))
        }
        Err(e) => Ok(err_val(module, &io_error_name_ir(&e))),
    }
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
            // G2（io 差异项）：双形态——第一参为 Str（路径）或 Dir 值（句柄）；
            // 返回 Vec(DirEntry)，每条 {name, is_dir}（对齐 oracle call_fs_method）。
            let v0 = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "list_dir"))?;
            let v0d = deref_value(ctx, v0);
            match v0d {
                IrValue::Class(c) if class_name(ctx, *c) == "Dir" => {
                    let fd = dir_fd_ir(ctx, v0d)?;
                    let path = ctx
                        .dirs
                        .get(&fd)
                        .ok_or_else(|| IrError::msg("BadFd", "bad dir"))?
                        .clone();
                    let entries = list_dir_entries_ir(ctx, module, &path)?;
                    Ok(Some(entries))
                }
                IrValue::Str(s) => {
                    let path = String::from_utf8_lossy(s).into_owned();
                    let entries = list_dir_entries_ir(ctx, module, &path)?;
                    Ok(Some(entries))
                }
                _ => Err(IrError::msg("TypeError", "list_dir expects path or Dir")),
            }
        }
        // G2（io 差异项）：io.fs.open_dir(path) !Dir——目录句柄。
        // 读校验成功则注册 fd→path（供 dir.list_dir / dir.close），返回 Dir 值。
        "open_dir" => {
            let path = path_arg_ir(ctx, args, 0)?;
            match std::fs::read_dir(&path) {
                Ok(_) => {
                    let fd = ctx.next_dir_fd;
                    ctx.next_dir_fd += 1;
                    ctx.dirs.insert(fd, path);
                    let mut fields = HashMap::new();
                    fields.insert(
                        "_fd".into(),
                        ctx.alloc(Cell::Value(IrValue::Int(fd as i128))),
                    );
                    Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                        name: "Dir".into(),
                        fields,
                    }))))
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

fn call_time_method_ir(ctx: &mut Ctx, field: &str, args: &[IrValue]) -> R<Option<IrValue>> {
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
        // G5（E3.3 time 完整）：单调测量——tick()（纳秒计数，epoch 基准）/ elapsed(tick)
        //（自 tick 起毫秒数）。时区完整留 1.x（需 tz 库）。
        "tick" => {
            let ns = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as i128;
            Ok(Some(IrValue::Int(ns)))
        }
        "elapsed" => {
            let tick = int_arg_ir(ctx, args, 0)?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as i128;
            Ok(Some(IrValue::Int((now - tick).max(0) / 1_000_000)))
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
        // G1（E3.1）：io.net.get(url) !&[u8]——HTTP GET 客户端，返回响应体字节
        "get" => {
            let url = str_arg_ir(ctx, args, 0)?;
            let url = String::from_utf8_lossy(&url).to_string();
            match http_get_ir(&url) {
                Ok(body) => Ok(Some(str_bytes_val(body))),
                Err(name) => Ok(Some(err_val(module, &name))),
            }
        }
        // Q20 双语：命名空间形式 io.net.read_all(&conn, alloc) ≡ conn.read_all(alloc)
        //（write/shutdown/close/local_port 同构；第一个实参解引用剥 Ptr → 实例方法）
        "read_all" | "write" | "shutdown" | "close" | "local_port" => {
            let conn = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", field))?;
            let conn = deref_value(ctx, conn).clone();
            call_conn_method_ir(ctx, module, &conn, field, &args[1..])
        }
        // io.net.accept(&server) !Conn ≡ server.accept()
        "accept" => {
            let srv = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "accept"))?;
            let srv = deref_value(ctx, srv).clone();
            call_listener_method_ir(ctx, module, &srv, "accept", &args[1..])
        }
        _ => Ok(None),
    }
}

// ---- G1（E3.1）UDP：io.net.udp 命名空间 + UdpSocket 实例 ----

/// io.net.udp 命名空间分派：`bind(port)` / `bind(host, port) !UdpSocket`；
/// send_to/recv_from/close 命名空间形式（第一实参为 socket）委托实例方法。
/// bind 首参为整型 → port 首参（bind(0, alloc) 亦归此，alloc 忽略）。
fn call_udp_ns_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "bind" => {
            let is_port_first = args
                .first()
                .map(|a| matches!(deref_value(ctx, a), IrValue::Int(_)))
                .unwrap_or(false);
            let (host, port_i) = if is_port_first {
                ("127.0.0.1".to_string(), 0)
            } else if args.len() >= 2 {
                let h = str_arg_ir(ctx, args, 0)?;
                (String::from_utf8_lossy(&h).to_string(), 1)
            } else {
                ("127.0.0.1".to_string(), 0)
            };
            let port = int_arg_ir(ctx, args, port_i)? as u16;
            Ok(Some(udp_bind_ir(ctx, module, &host, port)?))
        }
        "send_to" | "recv_from" | "close" => {
            let sock = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", field))?;
            let sock = deref_value(ctx, sock).clone();
            call_udp_socket_method_ir(ctx, module, &sock, field, &args[1..])
        }
        _ => Ok(None),
    }
}

/// UdpSocket 实例方法：send_to(addr, data) !void / recv_from(alloc) ![addr, data] /
/// local_port() !u16 / close() !void。recv_from 空队列（200ms 读超时）→ error.TimedOut。
fn call_udp_socket_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    self_v: &IrValue,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let fd = net_fd_ir(ctx, self_v)?;
    match field {
        "send_to" => {
            let addr = str_arg_ir(ctx, args, 0)?;
            let addr = String::from_utf8_lossy(&addr).to_string();
            let (host, port) = parse_udp_addr_ir(&addr).map_err(|e| IrError::msg(e, "udp addr"))?;
            let data = str_arg_ir(ctx, args, 1)?;
            let sock = ctx
                .udp_sockets
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad udp socket"))?;
            match sock.send_to(&data, (host.as_str(), port)) {
                Ok(_) => Ok(Some(IrValue::Void)),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "recv_from" => {
            let sock = ctx
                .udp_sockets
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad udp socket"))?;
            let mut buf = vec![0u8; 65536];
            match sock.recv_from(&mut buf) {
                Ok((n, peer)) => {
                    buf.truncate(n);
                    let addr = peer.to_string();
                    Ok(Some(make_arr(
                        ctx,
                        vec![str_val(&addr), str_bytes_val(buf)],
                    )))
                }
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "local_port" => {
            let sock = ctx
                .udp_sockets
                .get(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad udp socket"))?;
            match sock.local_addr() {
                Ok(a) => Ok(Some(IrValue::Int(a.port() as i128))),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "close" => {
            ctx.udp_sockets.remove(&fd);
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

// ---- G2（io 差异项）Dir：open_dir 返回的目录句柄 ----

/// Dir 类方法分派：`dir.list_dir(alloc) !Vec(DirEntry)`（重开枚举）/
/// `dir.close()`（注销句柄）。
fn call_dir_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    self_v: &IrValue,
    field: &str,
    _args: &[IrValue],
) -> R<Option<IrValue>> {
    let fd = dir_fd_ir(ctx, self_v)?;
    match field {
        "list_dir" => {
            let path = ctx
                .dirs
                .get(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad dir"))?
                .clone();
            let entries = list_dir_entries_ir(ctx, module, &path)?;
            Ok(Some(entries))
        }
        "close" => {
            ctx.dirs.remove(&fd);
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

// ---- G3（E3.2 ipc）：管道 + 共享内存 ----

/// io.ipc 命名空间分派：`pipe()`（匿名管道 → `[reader, writer]`）/ `shm(name, size) !Shm`
///（命名共享内存）。进程内 IPC 原语——注册表 + 类名分派承载（Q20 双语），协作式模型下
/// 读写均不阻塞。
fn call_ipc_method_ir(
    ctx: &mut Ctx,
    _module: &IrModule,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "pipe" => {
            let pid = ctx.next_pipe_fd;
            ctx.next_pipe_fd += 1;
            ctx.pipes.insert(
                pid,
                PipeIr {
                    buf: Vec::new(),
                    writer_open: true,
                },
            );
            let reader = {
                let mut fld = HashMap::new();
                fld.insert("pipe".into(), ctx.alloc(Cell::Value(IrValue::Int(pid as i128))));
                IrValue::Class(ctx.alloc(Cell::Class {
                    name: "PipeReader".into(),
                    fields: fld,
                }))
            };
            let writer = {
                let mut fld = HashMap::new();
                fld.insert("pipe".into(), ctx.alloc(Cell::Value(IrValue::Int(pid as i128))));
                IrValue::Class(ctx.alloc(Cell::Class {
                    name: "PipeWriter".into(),
                    fields: fld,
                }))
            };
            Ok(Some(make_arr(ctx, vec![reader, writer])))
        }
        "shm" => {
            // name 参数当前仅用于形态约束（命名共享内存的标识语义），区域本体按 id 注册
            let _name = path_arg_ir(ctx, args, 0)?;
            let size = int_arg_ir(ctx, args, 1)?.max(0) as usize;
            let id = ctx.next_shm_fd;
            ctx.next_shm_fd += 1;
            ctx.shms.insert(id, vec![0u8; size]);
            let mut fld = HashMap::new();
            fld.insert("shm".into(), ctx.alloc(Cell::Value(IrValue::Int(id as i128))));
            Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                name: "Shm".into(),
                fields: fld,
            }))))
        }
        _ => Ok(None),
    }
}

/// Pipe 类方法分派（is_reader 区分读写端）：写端 `write(data) !void` / `close() !void`；
/// 读端 `read(alloc) !&[u8]`（排空可读字节；空且写端开 → 空切片，不阻塞）/
/// `read_all(alloc) !&[u8]` / `is_closed() bool` / `close() !void`。
/// `close` 幂等：读端 close 注销注册表（管道随之拆除）、写端 close 仅置 writer_open=false；
/// 管道已注销后再 close 为 no-op（不报 BadFd）。
fn call_pipe_method_ir(
    ctx: &mut Ctx,
    _module: &IrModule,
    is_reader: bool,
    self_v: &IrValue,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let pid = pipe_id_ir(ctx, self_v)?;
    match field {
        "close" => {
            if is_reader {
                ctx.pipes.remove(&pid);
            } else if let Some(pipe) = ctx.pipes.get_mut(&pid) {
                pipe.writer_open = false;
            }
            Ok(Some(IrValue::Void))
        }
        "write" if !is_reader => {
            let data = str_arg_ir(ctx, args, 0)?;
            let pipe = ctx
                .pipes
                .get_mut(&pid)
                .ok_or_else(|| IrError::msg("BadFd", "bad pipe"))?;
            pipe.buf.extend_from_slice(&data);
            Ok(Some(IrValue::Void))
        }
        "read" | "read_all" if is_reader => {
            let pipe = ctx
                .pipes
                .get_mut(&pid)
                .ok_or_else(|| IrError::msg("BadFd", "bad pipe"))?;
            let out = std::mem::take(&mut pipe.buf);
            Ok(Some(str_bytes_val(out)))
        }
        "is_closed" if is_reader => {
            let pipe = ctx
                .pipes
                .get(&pid)
                .ok_or_else(|| IrError::msg("BadFd", "bad pipe"))?;
            Ok(Some(IrValue::Bool(!pipe.writer_open)))
        }
        _ => Ok(None),
    }
}

/// Shm 类方法分派：`write(data) !void`（覆盖内容，截断到 size）/ `read(alloc) !&[u8]`
/// / `close() !void`（注销句柄）。
fn call_shm_method_ir(
    ctx: &mut Ctx,
    _module: &IrModule,
    self_v: &IrValue,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let id = shm_id_ir(ctx, self_v)?;
    match field {
        "write" => {
            let data = str_arg_ir(ctx, args, 0)?;
            let shm = ctx
                .shms
                .get_mut(&id)
                .ok_or_else(|| IrError::msg("BadFd", "bad shm"))?;
            let cap = shm.capacity();
            let take = data.len().min(cap);
            shm.clear();
            shm.extend_from_slice(&data[..take]);
            Ok(Some(IrValue::Void))
        }
        "read" => {
            let shm = ctx
                .shms
                .get(&id)
                .ok_or_else(|| IrError::msg("BadFd", "bad shm"))?;
            Ok(Some(str_bytes_val(shm.clone())))
        }
        "close" => {
            ctx.shms.remove(&id);
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

// ---- G4（E3.3 storage）文件持久化键值存储 ----

/// io.storage.open(path) !KvStore——打开/创建文件持久化的键值存储。
/// 文件存在则装载既有条目（二进制格式：u32 键长 + 键 + u32 值长 + 值，小端）；
/// 缺文件视为空库（close 时创建）。KvStore 值持 `store` id → 注册表。
fn call_storage_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "open" => {
            let path = path_arg_ir(ctx, args, 0)?;
            let mut entries = HashMap::new();
            if let Ok(bytes) = std::fs::read(&path) {
                let mut i = 0usize;
                while i < bytes.len() {
                    // 格式：u32 键长 + 键 + u32 值长 + 值（vlen 不紧跟 klen——键在中间）
                    if i + 4 > bytes.len() {
                        return Ok(Some(err_val(module, "InvalidFormat")));
                    }
                    let klen = u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
                    i += 4;
                    if i + klen + 4 > bytes.len() {
                        return Ok(Some(err_val(module, "InvalidFormat")));
                    }
                    let key = bytes[i..i + klen].to_vec();
                    i += klen;
                    let vlen = u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
                    i += 4;
                    if i + vlen > bytes.len() {
                        return Ok(Some(err_val(module, "InvalidFormat")));
                    }
                    let val = bytes[i..i + vlen].to_vec();
                    entries.insert(key, val);
                    i += vlen;
                }
            }
            let id = ctx.next_store_fd;
            ctx.next_store_fd += 1;
            ctx.stores.insert(id, (path, entries));
            let mut fld = HashMap::new();
            fld.insert("store".into(), ctx.alloc(Cell::Value(IrValue::Int(id as i128))));
            Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                name: "KvStore".into(),
                fields: fld,
            }))))
        }
        _ => Ok(None),
    }
}

/// KvStore 实例方法：`put(key, value) !void` / `get(key) !?&[u8]`（缺失 → null）/
/// `contains(key) bool` / `remove(key) !void`（幂等）/ `len() usize` /
/// `close() !void`（落盘 + 注销注册表；已关闭再 close 为 no-op）。
fn call_store_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    self_v: &IrValue,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let id = store_id_ir(ctx, self_v)?;
    match field {
        "put" => {
            let key = str_arg_ir(ctx, args, 0)?;
            let value = str_arg_ir(ctx, args, 1)?;
            let store = ctx
                .stores
                .get_mut(&id)
                .ok_or_else(|| IrError::msg("BadFd", "bad store"))?;
            store.1.insert(key, value);
            Ok(Some(IrValue::Void))
        }
        "get" => {
            let key = str_arg_ir(ctx, args, 0)?;
            let store = ctx
                .stores
                .get(&id)
                .ok_or_else(|| IrError::msg("BadFd", "bad store"))?;
            match store.1.get(&key) {
                Some(val) => Ok(Some(opt_val(Some(str_bytes_val(val.clone()))))),
                None => Ok(Some(IrValue::Opt(None))),
            }
        }
        "contains" => {
            let key = str_arg_ir(ctx, args, 0)?;
            let store = ctx
                .stores
                .get(&id)
                .ok_or_else(|| IrError::msg("BadFd", "bad store"))?;
            Ok(Some(IrValue::Bool(store.1.contains_key(&key))))
        }
        "remove" => {
            let key = str_arg_ir(ctx, args, 0)?;
            let store = ctx
                .stores
                .get_mut(&id)
                .ok_or_else(|| IrError::msg("BadFd", "bad store"))?;
            store.1.remove(&key);
            Ok(Some(IrValue::Void))
        }
        "len" => {
            let store = ctx
                .stores
                .get(&id)
                .ok_or_else(|| IrError::msg("BadFd", "bad store"))?;
            Ok(Some(IrValue::Int(store.1.len() as i128)))
        }
        "close" => {
            if let Some((path, entries)) = ctx.stores.remove(&id) {
                // 落盘：二进制格式（u32 键长 + 键 + u32 值长 + 值，小端）写回 path
                let mut out = Vec::new();
                for (k, v) in &entries {
                    out.extend_from_slice(&(k.len() as u32).to_le_bytes());
                    out.extend_from_slice(k);
                    out.extend_from_slice(&(v.len() as u32).to_le_bytes());
                    out.extend_from_slice(v);
                }
                if let Err(e) = std::fs::write(&path, out) {
                    return Ok(Some(err_val(module, &io_error_name_ir(&e))));
                }
            }
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

// ---- G4（E3.3 archive）RLE 压缩 ----

/// io.archive.compress(data) !&[u8] / io.archive.decompress(data) !&[u8]——
/// RLE 压缩（encode_rle/decode_rle 共享层）。非法压缩数据 → error.InvalidFormat。
fn call_archive_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "compress" => {
            let data = str_arg_ir(ctx, args, 0)?;
            Ok(Some(str_bytes_val(encode_rle(&data))))
        }
        "decompress" => {
            let data = str_arg_ir(ctx, args, 0)?;
            match decode_rle(&data) {
                Ok(out) => Ok(Some(str_bytes_val(out))),
                Err(_) => Ok(Some(err_val(module, "InvalidFormat"))),
            }
        }
        _ => Ok(None),
    }
}

// ---- G5（E3.3 text）正则文本处理 ----

/// io.text.* —— `matches(pattern, text) bool`（是否含匹配；`^`/`$` 锚定控制
/// 全串）/ `find(pattern, text) ?int`（首个匹配起点；无 → null）/
/// `replace(pattern, text, repl) &[u8]`（替换全部非重叠匹配，每处取最长）/
/// `split(pattern, text) Vec(&[u8])`（按匹配分割，含空段）。非法模式 →
/// error.InvalidFormat。
fn call_text_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "matches" => {
            let pat = str_arg_ir(ctx, args, 0)?;
            let text = str_arg_ir(ctx, args, 1)?;
            let ast = match parse_regex(&pat) {
                Some(a) => a,
                None => return Ok(Some(err_val(module, "InvalidFormat"))),
            };
            let mut m = RegexMatcher::new(&ast, &text);
            Ok(Some(IrValue::Bool(m.find_at(0).is_some())))
        }
        "find" => {
            let pat = str_arg_ir(ctx, args, 0)?;
            let text = str_arg_ir(ctx, args, 1)?;
            let ast = match parse_regex(&pat) {
                Some(a) => a,
                None => return Ok(Some(err_val(module, "InvalidFormat"))),
            };
            let mut m = RegexMatcher::new(&ast, &text);
            match m.find_at(0) {
                Some((s, _e)) => Ok(Some(opt_val(Some(IrValue::Int(s as i128))))),
                None => Ok(Some(IrValue::Opt(None))),
            }
        }
        "replace" => {
            let pat = str_arg_ir(ctx, args, 0)?;
            let text = str_arg_ir(ctx, args, 1)?;
            let repl = str_arg_ir(ctx, args, 2)?;
            let ast = match parse_regex(&pat) {
                Some(a) => a,
                None => return Ok(Some(err_val(module, "InvalidFormat"))),
            };
            let mut m = RegexMatcher::new(&ast, &text);
            let mut out: Vec<u8> = Vec::new();
            let mut last = 0usize;
            let mut cur = 0usize;
            loop {
                let mf = m.find_at(cur);
                match mf {
                    Some((s, e)) => {
                        out.extend_from_slice(&text[last.min(text.len())..s]);
                        out.extend_from_slice(&repl);
                        if e > s {
                            last = e;
                            cur = e;
                            if e == text.len() {
                                break;
                            }
                        } else {
                            // 空匹配：复制该位置字节后前进，避免死循环
                            last = s + 1;
                            cur = s + 1;
                            if s < text.len() {
                                out.push(text[s]);
                            }
                            if cur > text.len() {
                                break;
                            }
                        }
                    }
                    None => break,
                }
            }
            out.extend_from_slice(&text[last.min(text.len())..]);
            Ok(Some(str_bytes_val(out)))
        }
        "split" => {
            let pat = str_arg_ir(ctx, args, 0)?;
            let text = str_arg_ir(ctx, args, 1)?;
            let ast = match parse_regex(&pat) {
                Some(a) => a,
                None => return Ok(Some(err_val(module, "InvalidFormat"))),
            };
            let mut m = RegexMatcher::new(&ast, &text);
            let mut parts: Vec<IrValue> = Vec::new();
            let mut start = 0usize;
            let mut cur = 0usize;
            loop {
                match m.find_at(cur) {
                    Some((s, e)) => {
                        parts.push(str_bytes_val(text[start..s].to_vec()));
                        if e > s {
                            start = e;
                            cur = e;
                            if e == text.len() {
                                break; // 匹配到末尾：尾空段由最后 push 补
                            }
                        } else {
                            // 空匹配：不消耗字符（该位置字节归下一段），仅前进搜索游标
                            start = s;
                            cur = s + 1;
                            if cur > text.len() {
                                break;
                            }
                        }
                    }
                    None => break,
                }
            }
            parts.push(str_bytes_val(text[start.min(text.len())..].to_vec()));
            let alloc = implicit_env_value(ctx, "alloc");
            Ok(Some(make_vec_with(ctx, parts, alloc)))
        }
        _ => Ok(None),
    }
}

// ---- G5（E3.3 rng）伪随机数 ----

/// io.rng.* —— `seed(v)`（重置状态；0 → 回退默认）/ `next() int`（下个原始
/// 64 位）/ `int(n) int`（[0, n) 均匀，拒绝采样免模偏差）/ `float() f64`
/// （[0, 1)，高 53 位均匀）。全局态在 Ctx（协作式单线程执行下安全）；
/// 命名空间类名 RngNs 避开示例 84-rng 的用户类 Rng。
fn call_rng_method_ir(
    ctx: &mut Ctx,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "seed" => {
            let v = int_arg_ir(ctx, args, 0)?;
            ctx.rng_state = if v == 0 { 0x9e37_79b9_7f4a_7c15 } else { v as u64 };
            Ok(Some(IrValue::Void))
        }
        "next" => {
            let n = xorshift64(&mut ctx.rng_state);
            Ok(Some(IrValue::Int(n as i128)))
        }
        "int" => {
            let bound = int_arg_ir(ctx, args, 0)?;
            if bound <= 0 {
                return Ok(Some(IrValue::Int(0)));
            }
            let b = bound as u64;
            let threshold = b.wrapping_neg() % b;
            let mut v = xorshift64(&mut ctx.rng_state);
            while v < threshold {
                v = xorshift64(&mut ctx.rng_state);
            }
            Ok(Some(IrValue::Int((v % b) as i128)))
        }
        "float" => {
            let v = xorshift64(&mut ctx.rng_state) >> 11;
            let f = (v as f64) / ((1u64 << 53) as f64);
            Ok(Some(IrValue::Float(f)))
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
        IrValue::Map(c) => *c,
        _ => return Ok(None),
    };
    match method {
        // G4：.alloc() → 构造 `init(alloc)` 时携带的分配器引用（Class("Map") 无 alloc → 全局）
        "alloc" => {
            let alloc = match self_v {
                IrValue::Map(mc) => match &ctx.cells[*mc] {
                    Cell::Map { alloc, .. } => Some(alloc.clone()),
                    _ => None,
                },
                _ => None,
            };
            Ok(Some(match alloc {
                Some(a) => a,
                None => implicit_env_value(ctx, "alloc"),
            }))
        }
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
                Cell::Class { fields, .. } | Cell::Map { fields, .. } => {
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
                Cell::Class { fields, .. } | Cell::Map { fields, .. } => {
                    fields.get(&key).map(|fc| ctx.cell_value(*fc).clone())
                }
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
                Cell::Class { fields, .. } | Cell::Map { fields, .. } => fields.contains_key(&key),
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
                Cell::Class { fields, .. } | Cell::Map { fields, .. } => {
                    fields.remove(&key);
                    Ok(Some(IrValue::Void))
                }
                _ => Err(IrError::msg("TypeError", "remove expects Map")),
            }
        }
        "len" => {
            let n = match &ctx.cells[c] {
                Cell::Class { fields, .. } | Cell::Map { fields, .. } => fields.len(),
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
            match self_v {
                // G4：Map 句柄 → 新 Map（携带自身 alloc）；Class("Map") → 旧形态 Class
                IrValue::Map(mc) => {
                    let alloc = match &ctx.cells[*mc] {
                        Cell::Map { alloc, .. } => alloc.clone(),
                        _ => implicit_env_value(ctx, "alloc"),
                    };
                    Ok(Some(make_map_with(ctx, fields, alloc)))
                }
                _ => Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                    name: "Map".into(),
                    fields,
                })))),
            }
        }
        _ => Ok(None),
    }
}

fn call_alloc_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
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
                _ => Err(IrError::msg(
                    "TypeError",
                    "alloc.init expects type name or literal",
                )),
            }
        }
        "alloc" => {
            let n = int_arg_ir(ctx, args, 0)?;
            match alloc_zeroed_bytes_ir(n) {
                Some(b) => {
                    // G5/§8.3 Debug 泄漏检测：登记分配（IR 无行号 → line 0；无引用计数不注销）
                    ctx.alloc_tracker.push((b.len(), 0));
                    Ok(Some(str_bytes_val(b)))
                }
                None => Ok(Some(err_val(module, "OutOfMemory"))),
            }
        }
        // G5/§8.3 Debug 泄漏检测：本 run 内已分配数
        "leaks" => Ok(Some(IrValue::Int(ctx.alloc_tracker.len() as i128))),
        // G5/§8.3 Debug 泄漏检测：分配清单文本（`leak: line L: N bytes` 每行）
        "leak_report" => {
            let mut out = Vec::new();
            for (size, line) in &ctx.alloc_tracker {
                out.extend_from_slice(&format!("leak: line {line}: {size} bytes\n").into_bytes());
            }
            Ok(Some(str_bytes_val(out)))
        }
        "deinit" => Ok(Some(IrValue::Void)),
        _ => Ok(None),
    }
}

fn call_arena_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    arena_cell: usize,
    method: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match method {
        "alloc" => {
            if args.len() != 1 {
                return Err(IrError::msg("ArityMismatch", "arena.alloc expects 1 arg"));
            }
            if let IrValue::Int(_) = deref_value(ctx, &args[0]) {
                let n = int_arg_ir(ctx, args, 0)?;
                if n < 0 {
                    return Err(IrError::msg("TypeError", "arena.alloc size must be >= 0"));
                }
                if n as u128 > usize::MAX as u128 {
                    return Ok(Some(err_val(module, "OutOfMemory")));
                }
                let n = n as usize;
                let bump_res = match &mut ctx.cells[arena_cell] {
                    Cell::Arena(st) => st.bump(n),
                    _ => unreachable!("cell {arena_cell} is not an arena"),
                };
                match bump_res {
                    Ok((bidx, off)) => {
                        let region = match &ctx.cells[arena_cell] {
                            Cell::Arena(st) => st.blocks[bidx][off..off + n].to_vec(),
                            _ => unreachable!(),
                        };
                        Ok(Some(str_bytes_val(region)))
                    }
                    Err(ArenaAllocErrIr::Deinit) => Err(IrError::msg(
                        "ArenaDeinitialized",
                        "arena.alloc after deinit",
                    )),
                    Err(ArenaAllocErrIr::Oom) => Ok(Some(err_val(module, "OutOfMemory"))),
                }
            } else {
                // 非整数实参：类型字面量构造（arena.alloc(Node{...}) 兼容形态）
                Ok(args.first().cloned())
            }
        }
        // arena.init(T) / arena.init(T{...})（E2：typed 构造，对齐 oracle call_arena_method
        // interp.rs "init" 双形态；bump 记账——堆上 class = 指针宽 8，连续 class IR 无布局
        // 表也按 8，与 alloc.init IR 同源简化）。
        "init" => {
            if args.len() != 1 {
                return Err(IrError::msg("ArityMismatch", "arena.init expects 1 arg"));
            }
            let v = deref_value(ctx, &args[0]).clone();
            let inst = match v {
                // 类型名参数（未知/枚举类型回退 Const Str）→ 空 class 实例
                IrValue::Str(s) => IrValue::Class(ctx.alloc(Cell::Class {
                    name: String::from_utf8_lossy(&s).to_string(),
                    fields: HashMap::new(),
                })),
                // 字面量 / 已知 class 默认字段构造（lower_alloc_init_defaults）→ 原样返回
                IrValue::Class(c) => IrValue::Class(c),
                _ => {
                    return Err(IrError::msg(
                        "TypeError",
                        "arena.init expects type name or literal",
                    ))
                }
            };
            let bump_res = match &mut ctx.cells[arena_cell] {
                Cell::Arena(st) => st.bump(8),
                _ => unreachable!("cell {arena_cell} is not an arena"),
            };
            match bump_res {
                Ok(_) => Ok(Some(inst)),
                Err(ArenaAllocErrIr::Deinit) => Err(IrError::msg(
                    "ArenaDeinitialized",
                    "arena.init after deinit",
                )),
                Err(ArenaAllocErrIr::Oom) => Ok(Some(err_val(module, "OutOfMemory"))),
            }
        }
        "deinit" => {
            if !args.is_empty() {
                return Err(IrError::msg("ArityMismatch", "arena.deinit expects 0 args"));
            }
            match &mut ctx.cells[arena_cell] {
                Cell::Arena(st) => st.deinit(),
                _ => unreachable!("cell {arena_cell} is not an arena"),
            }
            Ok(Some(IrValue::Void))
        }
        "bytes" => {
            if !args.is_empty() {
                return Err(IrError::msg("ArityMismatch", "arena.bytes expects 0 args"));
            }
            let total = match &ctx.cells[arena_cell] {
                Cell::Arena(st) => st.total,
                _ => unreachable!(),
            };
            Ok(Some(IrValue::Int(total as i128)))
        }
        "blocks" => {
            if !args.is_empty() {
                return Err(IrError::msg("ArityMismatch", "arena.blocks expects 0 args"));
            }
            let blocks = match &ctx.cells[arena_cell] {
                Cell::Arena(st) => st.blocks.len(),
                _ => unreachable!(),
            };
            Ok(Some(IrValue::Int(blocks as i128)))
        }
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
        // io.exit(ExitType, code)：正常退出信号（execute_ir 读 ctx.exit_code 映射退出码，
        // F2——与 oracle Interp.exit_code 对齐）
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
            ctx.exit_code = Some(code);
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

// ---- 组 G 线程生命周期（协作式延迟执行；对齐 oracle call_thread_method interp.rs:4610）----

/// Thread 类方法分派：`join() !T` / `cancel() !void` / `is_done() bool` / `detach()`。
fn call_thread_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    self_v: &IrValue,
    method: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    if !args.is_empty() {
        return Err(IrError::msg(
            "ArityMismatch",
            format!("Thread.{method} expects 0 args"),
        ));
    }
    let thread = match self_v {
        IrValue::Class(c) => *c,
        _ => return Err(IrError::msg("TypeError", "Thread method on non-Thread")),
    };
    match method {
        // 运行到完成并返回结果（错误 union 以 `IrValue::Err` 透传；done 后读缓存）
        "join" => Ok(Some(thread_run_ir(ctx, module, thread)?)),
        // detach：运行到完成并丢弃结果（副作用发生）、置 detached 标记
        "detach" => {
            let _ = thread_run_ir(ctx, module, thread)?;
            thread_set_field_ir(ctx, thread, "detached", IrValue::Bool(true));
            Ok(Some(IrValue::Void))
        }
        "is_done" => Ok(Some(IrValue::Bool(thread_field_bool_ir(
            ctx,
            thread,
            "done",
        )))),
        "cancel" => {
            // 协作式：置标志；运行点（join/detach）检查后跳过执行 → error.Cancelled
            thread_set_field_ir(ctx, thread, "cancel", IrValue::Bool(true));
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

/// 运行线程到完成（对齐 oracle `thread_run` interp.rs:4647）：
/// - 已运行（done）→ 返回缓存 result；
/// - 已取消（cancel 且未运行）→ 置 done、缓存 `error.Cancelled`、返回 Cancelled；
/// - 否则在线程每线程 alloc（Q8）覆盖下调用 fn，缓存 result 并置 done。
/// 硬错误（StackOverflow/AssertFailed/ExitRequested 等）不缓存、直接传播。
fn thread_run_ir(ctx: &mut Ctx, module: &IrModule, thread: usize) -> R<IrValue> {
    let (callee, arg_vals, alloc_v, cancelled, done) = {
        let fields = match &ctx.cells[thread] {
            Cell::Class { fields, .. } => fields.clone(),
            _ => return Err(IrError::msg("TypeError", "bad Thread")),
        };
        let getf = |k: &str| -> IrValue {
            match fields.get(k) {
                Some(c) => ctx.cell_value(*c).clone(),
                None => IrValue::Void,
            }
        };
        let callee = getf("fn");
        // args 字段 = `make_arr` 产物（Arr → Cell::Elems）
        let arg_vals = match getf("args") {
            IrValue::Arr(c) => match &ctx.cells[c] {
                Cell::Elems(e) => e.iter().map(|ec| ctx.cell_value(*ec).clone()).collect(),
                _ => vec![],
            },
            _ => vec![],
        };
        let alloc_v = getf("alloc");
        let cancelled = matches!(getf("cancel"), IrValue::Bool(true));
        let done = matches!(getf("done"), IrValue::Bool(true));
        (callee, arg_vals, alloc_v, cancelled, done)
    };
    if done {
        return thread_result_ir(ctx, thread);
    }
    if cancelled {
        let err_v = err_val(module, "Cancelled");
        thread_set_field_ir(ctx, thread, "done", IrValue::Bool(true));
        thread_set_field_ir(ctx, thread, "result", err_v.clone());
        return Ok(err_v);
    }
    // Q8：子任务执行期间绑定每线程 alloc（对齐 oracle `push_scope` + `bind("alloc", …)`；
    // 嵌套线程 save/restore 自然恢复外层 alloc）
    let saved = ctx.current_alloc.take();
    ctx.current_alloc = Some(alloc_v);
    let r = match callee {
        IrValue::Fn(fname) => {
            let idx = pick_func(ctx, module, &fname, &arg_vals).ok_or_else(|| {
                IrError::msg("NoFunction", format!("no function `{fname}`"))
            })?;
            exec_func(ctx, module, idx, &arg_vals, ctx.cur_depth + 1)
        }
        IrValue::Closure {
            func,
            captures,
            is_mut,
            ..
        } => call_closure_ir(ctx, module, func, &captures, &arg_vals, is_mut, ctx.cur_depth + 1),
        _ => Err(IrError::msg("NotCallable", "Thread fn is not callable")),
    };
    ctx.current_alloc = saved;
    let result = r?;
    thread_set_field_ir(ctx, thread, "done", IrValue::Bool(true));
    thread_set_field_ir(ctx, thread, "result", result.clone());
    Ok(result)
}

/// 读取线程缓存结果（done 或已取消时有效）
fn thread_result_ir(ctx: &Ctx, thread: usize) -> R<IrValue> {
    match &ctx.cells[thread] {
        Cell::Class { fields, .. } => match fields.get("result") {
            Some(c) => Ok(ctx.cell_value(*c).clone()),
            None => Err(IrError::msg("TypeError", "Thread has no result")),
        },
        _ => Err(IrError::msg("TypeError", "bad Thread")),
    }
}

/// 写线程字段（Thread 类字段 cell 索引定位）
fn thread_set_field_ir(ctx: &mut Ctx, thread: usize, key: &str, v: IrValue) {
    let fc = match &ctx.cells[thread] {
        Cell::Class { fields, .. } => fields.get(key).copied(),
        _ => None,
    };
    if let Some(c) = fc {
        ctx.cells[c] = Cell::Value(v);
    }
}

/// 读线程布尔字段（缺省 false）
fn thread_field_bool_ir(ctx: &Ctx, thread: usize, key: &str) -> bool {
    match &ctx.cells[thread] {
        Cell::Class { fields, .. } => match fields.get(key) {
            Some(c) => matches!(ctx.cell_value(*c), IrValue::Bool(true)),
            None => false,
        },
        _ => false,
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
            fields.insert(
                String::from_utf8_lossy(&ks).to_string(),
                ctx.alloc(Cell::Value(val)),
            );
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
    while i < b.len() && (b[i].is_ascii_digit() || matches!(b[i], b'-' | b'+' | b'.' | b'e' | b'E'))
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
                args.get(0)
                    .ok_or_else(|| IrError::msg("ArityMismatch", "split"))?,
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
                args.get(0)
                    .ok_or_else(|| IrError::msg("ArityMismatch", "find"))?,
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
        // G2（io 差异项）：to_upper/to_lower——ASCII 大小写转换（非 ASCII 字节不变）
        (IrValue::Str(s), "to_upper") | (IrValue::Str(s), "to_lower") => {
            let upper = method == "to_upper";
            let out: Vec<u8> = s
                .iter()
                .map(|&b| {
                    if upper {
                        b.to_ascii_uppercase()
                    } else {
                        b.to_ascii_lowercase()
                    }
                })
                .collect();
            Ok(Some(str_bytes_val(out)))
        }
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
        // G4：`Vec(T).init(alloc)`——集合空容器，捕获分配器引用（缺省回退全局 alloc）
        (IrValue::Arr(_), "init") => {
            let alloc_v = args
                .first()
                .cloned()
                .unwrap_or_else(|| implicit_env_value(ctx, "alloc"));
            Ok(Some(make_vec_with(ctx, Vec::new(), alloc_v)))
        }
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
            let alloc = implicit_env_value(ctx, "alloc");
            Ok(Some(make_vec_with(ctx, items, alloc)))
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
        // G4：`Map(K,V).init(alloc)`——集合空容器，捕获分配器引用（缺省回退全局 alloc）
        (IrValue::Map(_), "init") => {
            let alloc_v = args
                .first()
                .cloned()
                .unwrap_or_else(|| implicit_env_value(ctx, "alloc"));
            Ok(Some(make_map_with(ctx, HashMap::new(), alloc_v)))
        }
        // 集合（G4）：Map 句柄方法与 Class("Map") 共用实现
        (IrValue::Map(_), m) => call_map_method_ir(ctx, &self_v, m, args),
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Map" => {
            call_map_method_ir(ctx, &self_v, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Alloc" => {
            call_alloc_method_ir(ctx, module, m, args)
        }
        (IrValue::Arena(c), m) => call_arena_method_ir(ctx, module, *c, m, args),
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Io" => {
            call_io_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Thread" => {
            call_thread_method_ir(ctx, module, &self_v, m, args)
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
        // ---- G1-G5 模块分派（Q20 双语：interp/IR 同一套类名）----
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Udp" => {
            call_udp_ns_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "UdpSocket" => {
            call_udp_socket_method_ir(ctx, module, &self_v, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Dir" => {
            call_dir_method_ir(ctx, module, &self_v, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Ipc" => {
            call_ipc_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "PipeReader" => {
            call_pipe_method_ir(ctx, module, true, &self_v, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "PipeWriter" => {
            call_pipe_method_ir(ctx, module, false, &self_v, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Shm" => {
            call_shm_method_ir(ctx, module, &self_v, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Storage" => {
            call_storage_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "KvStore" => {
            call_store_method_ir(ctx, module, &self_v, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Archive" => {
            call_archive_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Text" => {
            call_text_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "RngNs" => {
            call_rng_method_ir(ctx, m, args)
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
    // serialize 命名空间（M5.3）：解析辅助组——serialize.parse_int 等对齐自由内建，
    // serialize.json.parse/csv.parse 对齐虚拟根（与 interp call_serialize_builtin 对齐）
    if let Some(rest) = name.strip_prefix("serialize.") {
        return call_serialize_builtin_ir(ctx, module, rest, args);
    }
    match name {
        // Arena.init(alloc) 内建：真实 arena 句柄（对齐 oracle interp.rs:2559-2562
        // 特判——返回新建 arena，而非 Void）
        "Arena.init" => {
            return Ok(IrValue::Arena(ctx.alloc(Cell::Arena(ArenaStateIr::new()))));
        }
        // Table(T).init(alloc, rows, cols, init)（M8；G4：外层 Vec 持分配器引用）
        "Table.init" => {
            if args.len() < 4 {
                return Err(IrError::msg("ArityMismatch", "Table.init expects 4 args"));
            }
            let alloc_v = args[0].clone();
            let rows = match deref_value(ctx, &args[1]) {
                IrValue::Int(i) => (*i).max(0) as usize,
                _ => return Err(IrError::msg("TypeError", "Table.init rows must be int")),
            };
            let cols = match deref_value(ctx, &args[2]) {
                IrValue::Int(i) => (*i).max(0) as usize,
                _ => return Err(IrError::msg("TypeError", "Table.init cols must be int")),
            };
            let init_v = args[3].clone();
            let mut grid = Vec::new();
            for _ in 0..rows {
                let mut row = Vec::new();
                for _ in 0..cols {
                    row.push(init_v.clone());
                }
                grid.push(make_arr(ctx, row));
            }
            return Ok(make_vec_with(ctx, grid, alloc_v));
        }
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

/// serialize 命名空间（M5.3）：解析辅助组组织为库命名空间。
/// `rest` 为去掉 `serialize.` 前缀的辅助名；json/csv 虚拟根对齐 call_dotted_implicit 对应
/// 分支，其余对齐自由内建 call_builtin（parse_int/parse_float/parse_number/skip_space/
/// peek/advance/is_digit/expect）。
fn call_serialize_builtin_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    rest: &str,
    args: &[IrValue],
) -> R<IrValue> {
    match rest {
        "json.parse" => {
            let data = str_arg_ir(ctx, args, 0)?;
            let obj = parse_json_obj_ir(ctx, &String::from_utf8_lossy(&data))?;
            let mut fields = HashMap::new();
            for (k, v) in obj {
                fields.insert(k, ctx.alloc(Cell::Value(v)));
            }
            Ok(IrValue::Class(ctx.alloc(Cell::Class {
                name: "Map".into(),
                fields,
            })))
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
            Ok(make_arr(ctx, rows))
        }
        _ => call_builtin(ctx, module, rest, args, &mut None),
    }
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
            if args.is_empty() || args.len() > 2 {
                return Err(IrError::msg("ArityMismatch", "box expects 1-2 args"));
            }
            // G3：分配器引用——显式传入或回退全局 alloc（`box(v)` 单参形态）
            let alloc_v = if args.len() > 1 {
                args[1].clone()
            } else {
                implicit_env_value(ctx, "alloc")
            };
            let data = ctx.alloc(Cell::Value(args[0].clone()));
            let vtbl = ir_type_name(ctx, &args[0]);
            Ok(IrValue::Boxed(ctx.alloc(Cell::Boxed {
                data,
                vtbl,
                alloc: alloc_v,
            })))
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
        // ---------- 组 G 线程（E2.2，协作式延迟执行） ----------
        // spawn(f, args...) o Thread(T)：立即返回句柄但不并发运行——join/detach 时才
        // 执行到完成（真并行留第三块 E2）。构造 `Class("Thread")`，字段经 cell 承载。
        "spawn" => {
            if args.is_empty() {
                return Err(IrError::msg(
                    "ArityMismatch",
                    "spawn expects at least callee",
                ));
            }
            let callee = deref_value(ctx, &args[0]).clone();
            match &callee {
                IrValue::Fn(_) | IrValue::Closure { .. } => {}
                _ => return Err(IrError::msg("NotCallable", "spawn callee is not callable")),
            }
            // Q8：每线程独立 Arena 实例（子任务执行期间绑定为 alloc）
            let alloc_v = IrValue::Arena(ctx.alloc(Cell::Arena(ArenaStateIr::new())));
            let args_arr = make_arr(ctx, args[1..].to_vec());
            let mut fields = HashMap::new();
            fields.insert("fn".to_string(), ctx.alloc(Cell::Value(callee)));
            fields.insert("args".to_string(), ctx.alloc(Cell::Value(args_arr)));
            fields.insert("alloc".to_string(), ctx.alloc(Cell::Value(alloc_v)));
            fields.insert(
                "cancel".to_string(),
                ctx.alloc(Cell::Value(IrValue::Bool(false))),
            );
            fields.insert(
                "done".to_string(),
                ctx.alloc(Cell::Value(IrValue::Bool(false))),
            );
            fields.insert(
                "detached".to_string(),
                ctx.alloc(Cell::Value(IrValue::Bool(false))),
            );
            fields.insert("result".to_string(), ctx.alloc(Cell::Value(IrValue::Void)));
            Ok(IrValue::Class(ctx.alloc(Cell::Class {
                name: "Thread".into(),
                fields,
            })))
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
                    None => Err(IrError::msg(
                        "IndexOutOfBounds",
                        "@enumFromInt: index out of bounds",
                    )),
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
        "@volatileLoad" => {
            // K2：@volatileLoad(ptr)——读穿指针。IR 参考解释器无优化器，volatile
            // 透明 = deref_value（对齐 interp deref_checked）；原生 LLVM volatile 指令层体现。
            if args.len() != 1 {
                return Err(IrError::msg("ArityMismatch", "@volatileLoad"));
            }
            Ok(deref_value(ctx, &args[0]).clone())
        }
        "@volatileStore" => {
            // K2：@volatileStore(ptr, v)——写穿指针（对齐 StorePtr 写穿语义）
            if args.len() != 2 {
                return Err(IrError::msg("ArityMismatch", "@volatileStore"));
            }
            let t = args[0].clone();
            let v = args[1].clone();
            match t {
                IrValue::Ptr(cell) => ctx.set_cell(cell, v),
                IrValue::Boxed(cell) => {
                    let data = match &ctx.cells[cell] {
                        Cell::Boxed { data, .. } => Some(*data),
                        _ => None,
                    };
                    match data {
                        Some(d) => ctx.set_cell(d, v),
                        None => {
                            return Err(IrError::msg(
                                "BadAssign",
                                "@volatileStore to non-pointer",
                            ))
                        }
                    }
                }
                _ => return Err(IrError::msg("BadAssign", "@volatileStore to non-pointer")),
            }
            Ok(IrValue::Void)
        }
        "@ptrFromInt" => {
            // K4：@ptrFromInt(addr)——整数地址 → 虚拟指针。登记过（@intFromPtr）→ 重建
            // 原指针（round-trip 保真，含 Ptr/Boxed 变体）；未登记 → 合成匿名槽（同地址
            // 幂等，对齐 interp 语义）。IR 指针 = cell 索引（永不回收，地址稳定）。
            if args.len() != 1 {
                return Err(IrError::msg("ArityMismatch", "@ptrFromInt"));
            }
            match deref_value(ctx, &args[0]).clone() {
                IrValue::Int(i) => {
                    if let Some(v) = ctx.addr_registry.get(&i) {
                        return Ok(v.clone());
                    }
                    let cell = ctx.alloc(Cell::Value(IrValue::Void));
                    ctx.addr_registry.insert(i, IrValue::Ptr(cell));
                    Ok(IrValue::Ptr(cell))
                }
                _ => Err(IrError::msg("TypeError", "@ptrFromInt expects an integer")),
            }
        }
        "@intFromPtr" => {
            // K4：@intFromPtr(p)——指针 → 整数地址。cell 索引即地址（对齐 interp Rc 堆地址
            // 的角色；登记原值进 addr_registry 供 @ptrFromInt 重建）。
            if args.len() != 1 {
                return Err(IrError::msg("ArityMismatch", "@intFromPtr"));
            }
            match &args[0] {
                IrValue::Ptr(cell) | IrValue::Boxed(cell) => {
                    let addr = *cell as i128;
                    ctx.addr_registry.insert(addr, args[0].clone());
                    Ok(IrValue::Int(addr))
                }
                _ => Err(IrError::msg("TypeError", "@intFromPtr expects a pointer")),
            }
        }
        "@compileError" => {
            let msg = if args.is_empty() {
                "compileError".to_string()
            } else {
                deref_value(ctx, &args[0]).display(ctx)
            };
            Err(IrError::msg(
                "CompileError",
                format!("@compileError: {msg}"),
            ))
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
            Ok(make_arr(ctx, vec![IrValue::Int(r), IrValue::Bool(false)]))
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
        // ---------- 格式辅助（M5.3 serialize）：fmt_int/fmt_float → String ----------
        "fmt_int" => {
            let v = deref_value(ctx, &args[0]);
            match v {
                IrValue::Int(i) => Ok(str_val(&i.to_string())),
                _ => Err(IrError::msg("TypeError", "fmt_int expects integer")),
            }
        }
        "fmt_float" => {
            let v = deref_value(ctx, &args[0]);
            match v {
                IrValue::Float(f) => Ok(str_val(&IrValue::Float(*f).display(ctx))),
                IrValue::Int(i) => Ok(str_val(&IrValue::Float(*i as f64).display(ctx))),
                _ => Err(IrError::msg("TypeError", "fmt_float expects float")),
            }
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
                            let r =
                                call_closure_value_ir(ctx, module, f, &[x.1.clone(), y.1.clone()]);
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
                _ => {
                    return Err(IrError::msg(
                        "TypeError",
                        "binary_search expects array or slice",
                    ))
                }
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
                    *fail = Some(format!(
                        "expect_error failed: expected error.{w}, got error.{g}"
                    ));
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
