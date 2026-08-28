//! 协程调度器内部状态（ADR-0028：自 ir/mod.rs 拆分；IR 版本）

use std::collections::{HashMap, VecDeque};

use super::g_id_ir::GIdIr;
use super::goroutine_ir::GoroutineIr;

/// 调度器内部状态（IR 版本，共享，Arc<Mutex<>> 保护）
pub(in crate::ir) struct SchedulerInnerIr {
    pub(in crate::ir) global_queue: VecDeque<GIdIr>,
    pub(in crate::ir) goroutines: HashMap<GIdIr, GoroutineIr>,
    pub(in crate::ir) next_gid: GIdIr,
    pub(in crate::ir) num_workers: usize,
}
