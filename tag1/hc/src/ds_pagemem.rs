//! PageMem 页内存池（标准库数据结构缺口 A6）——纯函数共享层（ADR-0004 语义唯一源）
//!
//! 固定容量页内存池，内部以 `Vec<usize>` 空闲链表管理。
//! H 语言 API：`io.pagemem.init(num_pages)` → PageMem；`.alloc() ?usize` /
//! `.free(idx)` / `.available() usize` / `.total() usize`。

/// 页内存池内部状态。
#[derive(Clone, Debug)]
pub struct PageMem {
    /// 空闲页索引栈（LIFO 分配）
    free: Vec<usize>,
    /// 总页数
    total: usize,
}

/// 创建 `num_pages` 页的 PageMem。
pub fn pagemem_new(num_pages: usize) -> PageMem {
    let free: Vec<usize> = (0..num_pages).rev().collect();
    PageMem {
        free,
        total: num_pages,
    }
}

/// 分配一页。无空闲页时返回 None。
pub fn pagemem_alloc(pm: &mut PageMem) -> Option<usize> {
    pm.free.pop()
}

/// 释放一页。若 idx 越界或已释放则忽略（安全）。
pub fn pagemem_free(pm: &mut PageMem, idx: usize) {
    if idx < pm.total && !pm.free.contains(&idx) {
        pm.free.push(idx);
    }
}

/// 空闲页数。
pub fn pagemem_available(pm: &PageMem) -> usize {
    pm.free.len()
}

/// 总页数。
pub fn pagemem_total(pm: &PageMem) -> usize {
    pm.total
}

/// 是否已分配指定索引。
pub fn pagemem_is_allocated(pm: &PageMem, idx: usize) -> bool {
    idx < pm.total && !pm.free.contains(&idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_zero() {
        let mut pm = pagemem_new(0);
        assert_eq!(pagemem_total(&pm), 0);
        assert_eq!(pagemem_available(&pm), 0);
        assert_eq!(pagemem_alloc(&mut pm), None);
    }

    #[test]
    fn new_with_pages() {
        let pm = pagemem_new(10);
        assert_eq!(pagemem_total(&pm), 10);
        assert_eq!(pagemem_available(&pm), 10);
    }

    #[test]
    fn alloc_all() {
        let mut pm = pagemem_new(5);
        for i in 0..5 {
            let idx = pagemem_alloc(&mut pm);
            assert_eq!(idx, Some(i)); // LIFO: pop from [4,3,2,1,0] → 0, then 1, ...
        }
        assert_eq!(pagemem_available(&pm), 0);
        assert_eq!(pagemem_alloc(&mut pm), None);
    }

    #[test]
    fn alloc_free_reuse() {
        let mut pm = pagemem_new(3);
        let a = pagemem_alloc(&mut pm).unwrap();
        let _b = pagemem_alloc(&mut pm).unwrap();
        assert_eq!(pagemem_available(&pm), 1);
        pagemem_free(&mut pm, a);
        assert_eq!(pagemem_available(&pm), 2);
        // 释放后重新分配应得到刚释放的索引（LIFO）
        let c = pagemem_alloc(&mut pm).unwrap();
        assert_eq!(c, a);
    }

    #[test]
    fn free_invalid_ignored() {
        let mut pm = pagemem_new(5);
        pagemem_free(&mut pm, 100); // 越界，忽略
        assert_eq!(pagemem_available(&pm), 5);
        pagemem_free(&mut pm, 0); // 已空闲，忽略（double-free 安全）
        assert_eq!(pagemem_available(&pm), 5);
    }

    #[test]
    fn alloc_free_cycle() {
        let mut pm = pagemem_new(2);
        let a = pagemem_alloc(&mut pm).unwrap();
        let b = pagemem_alloc(&mut pm).unwrap();
        assert_eq!(pagemem_alloc(&mut pm), None);
        pagemem_free(&mut pm, a);
        assert_eq!(pagemem_available(&pm), 1);
        let c = pagemem_alloc(&mut pm).unwrap();
        assert_eq!(c, a);
        pagemem_free(&mut pm, b);
        assert_eq!(pagemem_available(&pm), 1);
    }

    #[test]
    fn is_allocated() {
        let mut pm = pagemem_new(5);
        assert!(!pagemem_is_allocated(&pm, 0));
        assert!(!pagemem_is_allocated(&pm, 100)); // 越界
        let idx = pagemem_alloc(&mut pm).unwrap();
        assert!(pagemem_is_allocated(&pm, idx));
        pagemem_free(&mut pm, idx);
        assert!(!pagemem_is_allocated(&pm, idx));
    }
}
