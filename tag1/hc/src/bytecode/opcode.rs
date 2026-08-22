//! 字节码标签：类型标签（T_*）、binop/unop 编号映射。

use crate::ir::{IrBinOp, IrUnOp};

pub(super) const T_INT: u8 = 0;
pub(super) const T_FLOAT: u8 = 1;
pub(super) const T_BOOL: u8 = 2;
pub(super) const T_STR: u8 = 3;
pub(super) const T_VOID: u8 = 4;
pub(super) const T_NULL: u8 = 5;
pub(super) const T_ERR: u8 = 6;
pub(super) const T_END: u8 = 7;

pub(super) fn binop_tag(op: IrBinOp) -> u8 {
    use IrBinOp::*;
    match op {
        Add => 0,
        Sub => 1,
        Mul => 2,
        Div => 3,
        Mod => 4,
        EucMod => 5,
        BitAnd => 6,
        BitOr => 7,
        BitXor => 8,
        Shl => 9,
        Shr => 10,
        Eq => 11,
        Ne => 12,
        Lt => 13,
        Le => 14,
        Gt => 15,
        Ge => 16,
    }
}

pub(super) fn binop_from(tag: u8) -> Result<IrBinOp, String> {
    use IrBinOp::*;
    Ok(match tag {
        0 => Add,
        1 => Sub,
        2 => Mul,
        3 => Div,
        4 => Mod,
        5 => EucMod,
        6 => BitAnd,
        7 => BitOr,
        8 => BitXor,
        9 => Shl,
        10 => Shr,
        11 => Eq,
        12 => Ne,
        13 => Lt,
        14 => Le,
        15 => Gt,
        16 => Ge,
        _ => return Err(format!("未知 binop 标签 {tag}")),
    })
}

pub(super) fn unop_tag(op: IrUnOp) -> u8 {
    match op {
        IrUnOp::Neg => 0,
        IrUnOp::Not => 1,
        IrUnOp::BitNot => 2,
    }
}

pub(super) fn unop_from(tag: u8) -> Result<IrUnOp, String> {
    Ok(match tag {
        0 => IrUnOp::Neg,
        1 => IrUnOp::Not,
        2 => IrUnOp::BitNot,
        _ => return Err(format!("未知 unop 标签 {tag}")),
    })
}
