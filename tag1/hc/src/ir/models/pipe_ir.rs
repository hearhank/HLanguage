//! io.ipc 管道共享态（ADR-0028：自 ir/mod.rs 拆分）

/// io.ipc 管道共享态（协作式：读写均不阻塞；writer_open=false 且空缓冲 = 读端空切片）
#[derive(Debug, Default)]
pub struct PipeIr {
    pub buf: Vec<u8>,
    pub writer_open: bool,
}
