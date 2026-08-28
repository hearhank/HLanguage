/// 分配失败错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocErr {
    OutOfMemory,
    InvalidSize,
}
