//! OS 线程控制块（ADR-0028：自 ir/mod.rs 拆分；IR 版本）

use super::*;

/// OS 线程控制块（IR 版本）
#[derive(Debug)]
pub(crate) struct ThreadStateIr {
    pub(in crate::ir) join_handle: Option<thread::JoinHandle<()>>,
    pub(in crate::ir) result: Arc<Mutex<Option<ThreadResultIr>>>,
    pub(in crate::ir) cancel: Arc<AtomicBool>,
    pub(in crate::ir) done: Arc<AtomicBool>,
}
