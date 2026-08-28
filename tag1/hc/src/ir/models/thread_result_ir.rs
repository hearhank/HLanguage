//! 线程运行结果（ADR-0028：自 ir/mod.rs 拆分；跨线程传递，IR 版本）

use super::*;

/// 线程运行结果（跨线程传递，IR 版本）
#[derive(Debug)]
pub(crate) enum ThreadResultIr {
    Ok(IrValue),
    Err(IrError),
}
