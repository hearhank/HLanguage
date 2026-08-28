use std::cell::RefCell;
use std::rc::Rc;

use super::alloc_block::AllocBlock;
use super::alloc_err::AllocErr;
use super::allocator_trait::{copy_alloc_block, AllocatorTrait};
use super::arena_alloc_err::ArenaAllocErr;
use super::arena_state::ArenaState;
use super::pool_state::PoolState;

/// 分配器实现枚举（Phase 1：统一分配器接口，四变体）
pub enum AllocatorImpl {
    /// 无状态全局分配器（每 alloc 创建独立 Vec）
    Page,
    /// Arena bump 分配器（复用现有 ArenaState）
    Arena(Rc<RefCell<ArenaState>>),
    /// 固定大小对象池（空闲链表复用 + 后备分配器）
    Pool(Rc<RefCell<PoolState>>),
    /// 自定义分配器（Rust 侧实现，后续开放 H 侧）
    Custom(Box<dyn AllocatorTrait>),
}

unsafe impl Send for AllocatorImpl {}

impl std::fmt::Debug for AllocatorImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Page => write!(f, "PageAllocator"),
            Self::Arena(a) => {
                let d = a.borrow();
                write!(
                    f,
                    "ArenaAllocator(bytes={}, blocks={})",
                    d.total,
                    d.blocks.len()
                )
            }
            Self::Pool(p) => {
                let d = p.borrow();
                write!(
                    f,
                    "PoolAllocator(item_size={}, free={})",
                    d.item_size,
                    d.free_list.len()
                )
            }
            Self::Custom(_) => write!(f, "CustomAllocator(...)"),
        }
    }
}

impl Clone for AllocatorImpl {
    fn clone(&self) -> Self {
        match self {
            Self::Page => Self::Page,
            Self::Arena(a) => Self::Arena(a.clone()),
            Self::Pool(p) => {
                let d = p.borrow();
                Self::Pool(Rc::new(RefCell::new(PoolState {
                    item_size: d.item_size,
                    free_list: d.free_list.clone(),
                    backing: d.backing.clone(),
                })))
            }
            Self::Custom(c) => Self::Custom(c.clone_box()),
        }
    }
}

impl AllocatorImpl {
    /// 分配 n 字节零初始化内存
    pub fn alloc(&mut self, n: usize) -> Result<AllocBlock, AllocErr> {
        match self {
            Self::Page => {
                if n == 0 {
                    return Ok(AllocBlock {
                        data: Rc::new(RefCell::new(Vec::new())),
                        offset: 0,
                        len: 0,
                    });
                }
                let mut v = Vec::new();
                v.try_reserve_exact(n).map_err(|_| AllocErr::OutOfMemory)?;
                v.resize(n, 0u8);
                Ok(AllocBlock {
                    data: Rc::new(RefCell::new(v)),
                    offset: 0,
                    len: n,
                })
            }
            Self::Arena(arena) => {
                let mut a = arena.borrow_mut();
                let (data, offset) = a.bump(n).map_err(|e| match e {
                    ArenaAllocErr::Deinit => AllocErr::InvalidSize,
                    ArenaAllocErr::Oom => AllocErr::OutOfMemory,
                })?;
                Ok(AllocBlock {
                    data,
                    offset,
                    len: n,
                })
            }
            Self::Pool(pool) => {
                let mut p = pool.borrow_mut();
                if n > p.item_size {
                    return Err(AllocErr::InvalidSize);
                }
                // 先从空闲链表取，没有再分配
                if let Some(block) = p.free_list.pop() {
                    Ok(block)
                } else {
                    let item_size = p.item_size;
                    p.backing.alloc(item_size)
                }
            }
            Self::Custom(impl_) => impl_.alloc(n),
        }
    }

    /// 释放内存块
    pub fn free(&mut self, block: &AllocBlock) {
        match self {
            Self::Page => {
                // Page 分配器：Rc 引用归零时自动释放，此处空操作
            }
            Self::Arena(_) => {
                // Arena：不逐对象 free，deinit 统一释放
            }
            Self::Pool(pool) => {
                let mut p = pool.borrow_mut();
                let item_size = p.item_size;
                // 重置偏移并放回空闲链表复用
                p.free_list.push(AllocBlock {
                    data: block.data.clone(),
                    offset: 0,
                    len: item_size,
                });
            }
            Self::Custom(impl_) => impl_.free(block),
        }
    }

    /// 调整内存块大小
    pub fn realloc(&mut self, block: &AllocBlock, n: usize) -> Result<AllocBlock, AllocErr> {
        match self {
            Self::Page => {
                if n == 0 {
                    return Ok(AllocBlock {
                        data: Rc::new(RefCell::new(Vec::new())),
                        offset: 0,
                        len: 0,
                    });
                }
                let mut v = block.data.borrow_mut();
                let old_len = v.len();
                if n <= old_len {
                    v.truncate(n);
                    return Ok(AllocBlock {
                        data: block.data.clone(),
                        offset: 0,
                        len: n,
                    });
                }
                v.try_reserve_exact(n - old_len)
                    .map_err(|_| AllocErr::OutOfMemory)?;
                v.resize(n, 0u8);
                Ok(AllocBlock {
                    data: block.data.clone(),
                    offset: 0,
                    len: n,
                })
            }
            Self::Arena(_) => {
                // Arena 不支持 realloc，走 alloc + copy + free
                let new_block = self.alloc(n)?;
                let copy_len = block.len.min(n);
                copy_alloc_block(block, &new_block, copy_len);
                self.free(block);
                Ok(new_block)
            }
            Self::Pool(pool) => {
                let p = pool.borrow_mut();
                if n > p.item_size {
                    return Err(AllocErr::InvalidSize);
                }
                let item_size = p.item_size;
                // 相同大小直接返回原块
                Ok(AllocBlock {
                    data: block.data.clone(),
                    offset: 0,
                    len: item_size,
                })
            }
            Self::Custom(impl_) => impl_.realloc(block, n),
        }
    }

    /// 释放分配器持有的资源
    pub fn deinit(&mut self) {
        match self {
            Self::Page => {}
            Self::Arena(arena) => {
                arena.borrow_mut().deinit();
            }
            Self::Pool(pool) => {
                let mut p = pool.borrow_mut();
                // 归还空闲链表所有块到后备分配器
                let blocks: Vec<AllocBlock> = p.free_list.drain(..).collect();
                for block in &blocks {
                    p.backing.free(block);
                }
                p.backing.deinit();
            }
            Self::Custom(impl_) => impl_.deinit(),
        }
    }
}
