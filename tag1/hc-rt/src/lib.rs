//! H 语言运行时（hc-rt）
//!
//! tag1 垂直切片：值模型 + tree-walking 解释器（脚本模式 `hc run` / `hc test`，全语言）。
//! 字节码 VM 与 LLVM 原生后端在 `hc` crate（复用 `hc::ir` 唯一语义源）；本 crate 只提供
//! 解释执行路径。第三块（元编程/并发/异步/自举等）归后续里程碑。

pub mod interp;
pub mod value;

pub use interp::{parse_int_text, RtError, Interp};
pub use value::Value;
