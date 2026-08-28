//! Arena 分配器状态（ADR-0028：自 ir/mod.rs 拆分；G1：真实 bump + 块链表）

use super::*;

/// Arena 默认块大小（IR 侧；对齐 tree-walking `value::ARENA_BLOCK_SIZE`）
const ARENA_BLOCK_SIZE_IR: usize = 1024;

/// 分配器对齐下限（G5/§2.3：H 值为 i128/f64 承载，对齐 ≥ 16；对齐 tree-walking `ALLOC_ALIGN`）
const ALLOC_ALIGN_IR: usize = 16;

/// 对齐到 `a` 倍数（向上圆整）
fn align_up_ir(x: usize, a: usize) -> usize {
    (x + a - 1) & !(a - 1)
}

/// Arena 分配器状态（G1：真实 bump + 块链表）
#[derive(Debug, Clone, Default)]
pub struct ArenaStateIr {
    /// 已提交块（真实 backing 内存；bump 从当前块切，不足时申请新块）
    pub blocks: Vec<Vec<u8>>,
    /// 当前块内游标（下一分配起点）
    pub cursor: usize,
    /// 累计分配字节（统计；`arena.bytes()`）
    pub total: usize,
    /// 可用标志（`deinit` 后 false → `alloc` 抛 `ArenaDeinitialized`）
    pub live: bool,
    /// G5/§8.3 Debug 泄漏检测：Arena 块分配登记表（bump 时登记，deinit 时清空）
    pub alloc_tracker: Vec<(usize, u32)>,
}

impl ArenaStateIr {
    pub fn new() -> Self {
        Self {
            blocks: vec![],
            cursor: 0,
            total: 0,
            live: true,
            alloc_tracker: vec![],
        }
    }

    /// bump 分配 `n` 字节零初始化内存；不足时申请新块（大小 = `max(ARENA_BLOCK_SIZE_IR, n)`）。
    /// 返回（块索引, 块内偏移）；失败（deinit / OOM）返回 Err。
    ///
    /// **对齐（G5/§2.3）**：切出前把游标圆整到 `ALLOC_ALIGN_IR`（16）的倍数，保证
    /// 返回区域起始相对块起点 16 对齐；对齐填充计入 `total`（对齐 tree-walking `bump`）。
    pub(in crate::ir) fn bump(&mut self, n: usize) -> Result<(usize, usize), ArenaAllocErrIr> {
        if !self.live {
            return Err(ArenaAllocErrIr::Deinit);
        }
        let aligned = align_up_ir(self.cursor, ALLOC_ALIGN_IR);
        let need_new = self.blocks.is_empty() || aligned + n > self.blocks.last().unwrap().len();
        if need_new {
            let size = n.max(ARENA_BLOCK_SIZE_IR);
            let mut block = Vec::new();
            // 优雅失败（`vec![0u8; size]` 对超大 size 会中止进程）
            block
                .try_reserve_exact(size)
                .map_err(|_| ArenaAllocErrIr::Oom)?;
            block.resize(size, 0u8);
            // G5/§8.3 Debug 泄漏检测：登记 Arena 块分配
            self.alloc_tracker.push((block.len(), 0));
            self.blocks.push(block);
            self.cursor = 0;
        }
        let idx = self.blocks.len() - 1;
        let off = align_up_ir(self.cursor, ALLOC_ALIGN_IR);
        self.total += off + n - self.cursor;
        self.cursor = off + n;
        Ok((idx, off))
    }

    /// deinit：清空全部块（归还 backing）、重置统计、标记不可用
    pub(in crate::ir) fn deinit(&mut self) {
        self.blocks.clear();
        self.alloc_tracker.clear();
        self.cursor = 0;
        self.total = 0;
        self.live = false;
    }
}
