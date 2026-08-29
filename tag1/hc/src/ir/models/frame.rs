//! IR 调用帧（ADR-0028：自 ir/mod.rs 拆分；槽 → cell 索引映射）

use super::*;

/// 帧：槽内联值 + 惰性装箱（K5 S8——每调用零 cell 分配；`&x`/捕获/迭代重定向
/// 时才为该槽分配 cell，此后读写写穿 cell）。
/// `defers`：本调用内待运行 defer 的多重集（PushDefer 增 / PopDefer 减；守卫判成员）。
/// 运行时 LIFO 顺序由编译期发射顺序保证，故此处仅需「是否待运行」判定，无需栈序。
#[derive(Debug, Clone)]
pub struct Frame {
    /// 槽内联值（cell_of[slot] < 0 时读写直达此处）
    pub values: Vec<IrValue>,
    /// 槽 → cell（-1 = 未装箱；`&x`/闭包捕获/迭代 Mut 捕获时置位——此后读写写穿 cell）
    pub cell_of: Vec<i64>,
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
