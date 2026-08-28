use super::alloc_block::AllocBlock;
use super::alloc_err::AllocErr;

/// 自定义分配器接口（Rust 侧实现，供 Custom 变体使用）
pub trait AllocatorTrait {
    fn alloc(&mut self, n: usize) -> Result<AllocBlock, AllocErr>;
    fn free(&mut self, block: &AllocBlock);
    fn realloc(&mut self, block: &AllocBlock, n: usize) -> Result<AllocBlock, AllocErr> {
        // 默认实现：alloc + copy + free（对不支持 realloc 的后端兜底）
        let new_block = self.alloc(n)?;
        let copy_len = block.len.min(n);
        copy_alloc_block(block, &new_block, copy_len);
        self.free(block);
        Ok(new_block)
    }
    fn deinit(&mut self) {}
    /// 克隆自身（用于 AllocatorImpl::Clone）
    fn clone_box(&self) -> Box<dyn AllocatorTrait>;
}

/// 复制源数据到目标 AllocBlock（realloc 辅助，避免同时借用两个 RefCell）
pub(super) fn copy_alloc_block(src: &AllocBlock, dst: &AllocBlock, len: usize) {
    let src_data = {
        let s = src.data.borrow();
        s[src.offset..src.offset + len].to_vec()
    };
    let mut d = dst.data.borrow_mut();
    d[dst.offset..dst.offset + len].copy_from_slice(&src_data);
}
