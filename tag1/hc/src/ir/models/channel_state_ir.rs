//! 通道状态（ADR-0028：自 ir/mod.rs 拆分；E4：四模式容器真并行）

use super::*;

/// 通道状态（IR 版本，E4：四模式容器真并行）
#[derive(Debug)]
pub(crate) enum ChannelStateIr {
    Pipe {
        sender: std::sync::mpsc::Sender<IrValue>,
        receiver: std::sync::mpsc::Receiver<IrValue>,
    },
    /// 三通（1 写 N 读）：Mutex+Condvar 队列
    Tee {
        queue: Arc<Mutex<VecDeque<IrValue>>>,
        condvar: Arc<Condvar>,
    },
    /// 漏斗（N 写 1 读）：Mutex+Condvar 队列
    Funnel {
        queue: Arc<Mutex<VecDeque<IrValue>>>,
        condvar: Arc<Condvar>,
    },
    /// 集线器（N 写 N 读）：Mutex+Condvar 队列
    Hub {
        queue: Arc<Mutex<VecDeque<IrValue>>>,
        condvar: Arc<Condvar>,
    },
}
