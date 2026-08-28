//! bump 分配失败原因（ADR-0028：自 ir/mod.rs 拆分）

/// bump 分配失败原因（调用方映射为 IR 错误 / `error.OutOfMemory`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ir) enum ArenaAllocErrIr {
    Deinit,
    Oom,
}
