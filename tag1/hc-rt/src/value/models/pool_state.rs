use super::alloc_block::AllocBlock;
use super::allocator_impl::AllocatorImpl;

/// 固定大小对象池状态（空闲链表 + 后备分配器）
pub struct PoolState {
    pub item_size: usize,
    pub free_list: Vec<AllocBlock>,
    pub backing: Box<AllocatorImpl>,
}

unsafe impl Send for PoolState {}

impl PoolState {
    pub fn new(backing: AllocatorImpl, item_size: usize) -> Self {
        Self {
            item_size,
            free_list: Vec::new(),
            backing: Box::new(backing),
        }
    }
}
