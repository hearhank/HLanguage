//! M3.2 字节码 VM：`IrModule` 的紧凑序列化（HBC2）+ 装载执行。
//!
//! 字节码是共享 IR（ADR-0004 唯一语义源）的**序列化**——执行复用
//! [`crate::ir::run_ir`]，不另写 dispatch 循环，禁止各后端私语义。
//! 覆盖范围 = M3.1 切片（标量 / 控制流 / 函数调用 / 错误值通道）
//! + Phase 1 指针（取址 / 解引用 / 写穿）+ Phase 2 聚合（字段/索引/字面量/
//! 解构/move/unwrap）+ Phase 3 switch + range + for（opcode 31–36 + 模式描述符）
//! + Phase 4 闭包/函数引用/方法/重载（opcode 37–40 + 闭包表）+ Phase 5 global/const
//! （opcode 41–43 + 全局表，43 = `&global` 取址）。
//! 子集外特性同 `ir::lower` 以 `error.Unsupported` 硬错误拒绝。
//!
//! 格式（全小端）：
//! ```text
//! magic "HBC2" · u32 version · u32 n_funcs
//! 函数索引表（还原 IrModule.func_index）: u32 n_entries · { name · u32 n_idx · {u32 idx}* }*
//! 函数 × n_funcs: name · u32 n_params · {u32 param}* · Type×n_param_ty
//!   · u8×n_param_defaults · (present? Const)* × n_defaults · u32 n_slots · u8 is_test · u8 exported
//!   · u32 n_insts · { opcode u8 · 操作数 }*
//! 闭包表（Phase 4）: u32 n_closures · 函数 × n_closures（同上格式）
//! 全局表（Phase 5，还原 IrModule.globals）: u32 n_globals · { name } × n_globals
//! 枚举变体表（Phase 7）: u32 n_enums · { name · u32 n_var · {str}* }*
//! [continuous] 类名表（P11d）: u32 n_cont · { name }*（DeepCopy 指令运行时门）
//! K1 union 表（H1/ADR-0014）: u32 n_unions · { name · u32 n_f · { fname · Type }* }*
//! ```
//! 常量载荷保留全精度：`i128` 16 字节、`f64` 8 字节、字符串长度前缀。

mod decode;
mod encode;
pub mod opcode;
#[cfg(test)]
mod tests;

use crate::ast::Type;
use crate::ir::{
    run_ir, IrBinOp, IrConst, IrError, IrFunc, IrInst, IrModule, IrPattern, IrUnOp, IrValue,
};
use std::collections::{HashMap, HashSet};

pub const MAGIC: [u8; 4] = *b"HBC2";
/// v7：H1 增 K1 union 表（UnionSync 写路径字节重解释）+ opcode 48。
/// v6：P11d 增 [continuous] 类名表（DeepCopy 指令运行时门）+ opcode 47。
pub const VERSION: u32 = 7;

pub use self::decode::decode;
pub use self::decode::run_bytecode;
pub use self::encode::encode;
