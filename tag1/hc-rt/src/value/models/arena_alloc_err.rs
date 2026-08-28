/// bump 分配失败原因（调用方映射为 H 可处理错误）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArenaAllocErr {
    /// arena 已 deinit，不可再分配
    Deinit,
    /// backing 分配失败 / 超出可表示容量
    Oom,
}
