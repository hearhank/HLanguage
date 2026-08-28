//! 协程结果（ADR-0028：自 ir/mod.rs 拆分；IR 版本）

use super::*;

/// 协程结果（IR 版本）
#[derive(Debug, Clone)]
pub(crate) enum GResultIr {
    Ok(IrValue),
    Err(IrError),
}
