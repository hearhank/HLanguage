//! 协程执行单元（ADR-0028：自 ir/mod.rs 拆分；IR 版本）

use super::g_id_ir::GIdIr;
use super::g_result_ir::GResultIr;
use super::g_state_ir::GStateIr;

/// 协程（Goroutine）——轻量执行单元（IR 版本）
pub(crate) struct GoroutineIr {
    pub id: GIdIr,
    pub state: GStateIr,
    pub name: String,
    pub task: Option<Box<dyn FnOnce() + Send>>,
    pub result: Option<GResultIr>,
}
