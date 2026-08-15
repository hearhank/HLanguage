//! H 语言运行时（hc-rt）
//!
//! tag1 垂直切片：值模型 + tree-walking 解释器（脚本模式 `hc run` / `hc test`）。
//! 字节码 VM、LLVM 原生后端、完整所有权运行时归后续里程碑（07 计划 M3/M4）。

pub mod interp;
pub mod value;

pub use interp::{parse_int_text, RtError, Interp};
pub use value::Value;
