//! 协程状态（ADR-0028：自 ir/mod.rs 拆分；IR 版本）

/// 协程状态（IR 版本）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GStateIr {
    Runnable,
    Running,
    Waiting,
    Done,
}
