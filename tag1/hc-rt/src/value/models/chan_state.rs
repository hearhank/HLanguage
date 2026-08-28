use std::sync::{Condvar, Mutex as StdMutex};

use super::chan_inner::ChanInner;

/// 通道状态（M:N 协程通信）
#[derive(Debug)]
pub struct ChanState {
    pub inner: StdMutex<ChanInner>,
    pub send_cond: Condvar,
    pub recv_cond: Condvar,
    pub capacity: usize,
}
