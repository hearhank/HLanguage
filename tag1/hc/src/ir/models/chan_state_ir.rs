//! 通道状态（ADR-0028：自 ir/mod.rs 拆分；chan<T>：M:N 协程通信）

use super::*;

/// 通道状态（IR 版本，chan<T>：M:N 协程通信）
#[derive(Debug)]
pub struct ChanStateIr {
    pub inner: Mutex<ChanInnerIr>,
    pub send_cond: Condvar,
    pub recv_cond: Condvar,
    pub capacity: usize,
}
