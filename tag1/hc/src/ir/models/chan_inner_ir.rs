//! 通道内部状态（ADR-0028：自 ir/mod.rs 拆分；chan<T> 队列与关闭标志）

use super::*;

#[derive(Debug)]
pub struct ChanInnerIr {
    pub queue: VecDeque<IrValue>,
    pub closed: bool,
}
