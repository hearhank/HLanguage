//! IR 一元运算符（ADR-0028：自 ir/mod.rs 拆分）

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrUnOp {
    Neg,
    Not,
    BitNot,
}
