//! 协程调度器（ADR-0028：自 ir/mod.rs 拆分；M:N 模型，IR 版本，Tasks 2+3+4）

/// 协程（Goroutine）ID（IR 版本）
pub(crate) type GIdIr = u64;
