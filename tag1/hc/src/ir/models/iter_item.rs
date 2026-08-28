//! 迭代项（ADR-0028：自 ir/mod.rs 拆分）

/// 迭代项：共享源 cell（或新 cell）+ 是否源容器引用（对齐 oracle `iter_items` 的 `(cell, is_ref)`）。
#[derive(Debug, Clone)]
pub struct IterItem {
    pub cell: usize,
    pub is_ref: bool,
}
