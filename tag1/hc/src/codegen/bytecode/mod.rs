//! 字节码 VM（HBC2）：序列化格式、编码解码与执行入口

mod decode;
mod encode;
pub mod opcode;
#[cfg(test)]
mod tests;

use crate::ast::Type;
use crate::ir::{run_ir, IrConst, IrError, IrFunc, IrInst, IrModule, IrPattern, IrValue};
use std::collections::{HashMap, HashSet};

pub const MAGIC: [u8; 4] = *b"HBC2";
/// v7：H1 增 K1 union 表（UnionSync 写路径字节重解释）+ opcode 48。
/// v6：P11d 增 [continuous] 类名表（DeepCopy 指令运行时门）+ opcode 47。
pub const VERSION: u32 = 7;

pub use self::decode::decode;
pub use self::decode::run_bytecode;
pub use self::encode::encode;
