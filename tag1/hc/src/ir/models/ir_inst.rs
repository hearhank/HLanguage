//! IR 指令集（ADR-0028：自 ir/mod.rs 拆分）

use super::*;

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
