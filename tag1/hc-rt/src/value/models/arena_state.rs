use std::cell::RefCell;
use std::rc::Rc;

use super::arena_alloc_err::ArenaAllocErr;
use super::leak_record::LeakRecord;

/// Arena 默认块大小（首块及新块下限；单块申请大于此值时按实际大小开块）
pub const ARENA_BLOCK_SIZE: usize = 1024;

/// 分配器对齐下限（§2.3：H 值为 i128/f64 承载，对齐 ≥ 16 字节，与 tag1 `%Value` 盒一致）。
/// bump 游标按此圆整，返回区域起始相对块起点恒为 16 的倍数。
pub const ALLOC_ALIGN: usize = 16;

/// 对齐到 `ALLOC_ALIGN` 倍数（向上圆整；`align_up(x)` = `(x + A - 1) & !(A - 1)`）
fn align_up(x: usize, a: usize) -> usize {
    (x + a - 1) & !(a - 1)
}

/// Arena 分配器状态（G1：真实 bump + 块链表；`deinit` 批量归还 backing）
#[derive(Debug, Clone)]
pub struct ArenaState {
    /// 已提交块（真实 backing 内存；每块独立 Vec，bump 从当前块切，不足时申请新块）
    pub blocks: Vec<Rc<RefCell<Vec<u8>>>>,
    /// 当前块内游标（下一分配起点）
    pub cursor: usize,
    /// 累计分配字节（统计；`arena.bytes()`）
    pub total: usize,
    /// 可用标志（`deinit` 后 false → `alloc` 抛 `ArenaDeinitialized`）
    pub live: bool,
    /// G5/§8.3 Debug 泄漏检测：Arena 块分配登记表（`Arena.init(alloc)` 时从 Interp 共享；
    /// deinit 清 block 后弱引用失效自动视为释放；退出时仍存活者 = 泄漏）
    pub alloc_tracker: Option<Rc<RefCell<Vec<LeakRecord>>>>,
}

unsafe impl Send for ArenaState {}

impl ArenaState {
    pub fn new() -> Self {
        Self {
            blocks: vec![],
            cursor: 0,
            total: 0,
            live: true,
            alloc_tracker: None,
        }
    }

    /// bump 分配 `n` 字节零初始化内存：当前块剩余空间足够则切出，
    /// 不足则向 backing 申请新块（大小 = `max(ARENA_BLOCK_SIZE, n)`）。
    /// 返回（块引用, 块内偏移）——调用方按 `[off..off+n]` 读出区域。
    ///
    /// **对齐（G5/§2.3）**：切出前把游标圆整到 `ALLOC_ALIGN`（16）的倍数，保证
    /// 返回区域起始相对块起点 16 对齐；对齐填充计入 `total`（真实 bump 语义——
    /// 分配器消耗对齐后的空间）。新块游标从 0 起，起点天然对齐。
    pub fn bump(&mut self, n: usize) -> Result<(Rc<RefCell<Vec<u8>>>, usize), ArenaAllocErr> {
        if !self.live {
            return Err(ArenaAllocErr::Deinit);
        }
        let aligned = align_up(self.cursor, ALLOC_ALIGN);
        let need_new =
            self.blocks.is_empty() || aligned + n > self.blocks.last().unwrap().borrow().len();
        if need_new {
            let size = n.max(ARENA_BLOCK_SIZE);
            let mut block = Vec::new();
            // 优雅失败（`vec![0u8; size]` 对超大 size 会中止进程）
            block
                .try_reserve_exact(size)
                .map_err(|_| ArenaAllocErr::Oom)?;
            block.resize(size, 0u8);
            let block_rc = Rc::new(RefCell::new(block));
            // G5/§8.3 Debug 泄漏检测：登记 Arena 块分配（弱引用随 deinit 失效）
            if let Some(ref tracker) = self.alloc_tracker {
                tracker.borrow_mut().push(LeakRecord {
                    size: block_rc.borrow().len(),
                    line: 0,
                    weak: Rc::downgrade(&block_rc),
                });
            }
            self.blocks.push(block_rc);
            self.cursor = 0;
        }
        let block = self.blocks.last().unwrap().clone();
        let off = align_up(self.cursor, ALLOC_ALIGN);
        self.total += off + n - self.cursor;
        self.cursor = off + n;
        Ok((block, off))
    }

    /// deinit：清空全部块（归还 backing）、重置统计、标记不可用
    pub fn deinit(&mut self) {
        self.blocks.clear();
        self.cursor = 0;
        self.total = 0;
        self.live = false;
    }
}
