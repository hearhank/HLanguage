//! Bitmap 位图（标准库数据结构缺口 A6）——纯函数共享层（ADR-0004 语义唯一源）
//!
//! 紧凑位数组，内部以 `Vec<u64>` 存储。
//! H 语言 API：`Bitmap.init(nbits)` → Bitmap；`.set(idx)` / `.get(idx)` / `.clear(idx)` /
//! `.count()`（置位计数）/ `.len()`（总位数）。

/// 创建 Bitmap 内部存储（`Vec<u64>`，nbits 向上取整到 64 的倍数）。
pub fn bitmap_new(nbits: usize) -> Vec<u64> {
    let words = nbits.div_ceil(64);
    vec![0u64; words]
}

/// 置位 `idx`（0-based）。若 idx 越界则忽略（安全）。
pub fn bitmap_set(words: &mut [u64], idx: usize) {
    if let Some(w) = words.get_mut(idx >> 6) {
        *w |= 1u64 << (idx & 63);
    }
}

/// 获取 `idx` 位的值。越界返回 false。
pub fn bitmap_get(words: &[u64], idx: usize) -> bool {
    words
        .get(idx >> 6)
        .map_or(false, |w| (w >> (idx & 63)) & 1 == 1)
}

/// 清除 `idx` 位。越界忽略。
pub fn bitmap_clear(words: &mut [u64], idx: usize) {
    if let Some(w) = words.get_mut(idx >> 6) {
        *w &= !(1u64 << (idx & 63));
    }
}

/// 置位计数（popcount）。
pub fn bitmap_count(words: &[u64]) -> usize {
    words.iter().map(|w| w.count_ones() as usize).sum()
}

/// 总位数。
pub fn bitmap_len(words: &[u64]) -> usize {
    words.len() * 64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_zero() {
        let w = bitmap_new(0);
        assert_eq!(w.len(), 0);
    }

    #[test]
    fn new_small() {
        let w = bitmap_new(10);
        assert_eq!(w.len(), 1);
        assert_eq!(bitmap_len(&w), 64);
        assert_eq!(bitmap_count(&w), 0);
    }

    #[test]
    fn new_64() {
        let w = bitmap_new(64);
        assert_eq!(w.len(), 1);
        assert_eq!(bitmap_len(&w), 64);
    }

    #[test]
    fn new_65() {
        let w = bitmap_new(65);
        assert_eq!(w.len(), 2);
        assert_eq!(bitmap_len(&w), 128);
    }

    #[test]
    fn set_get_single() {
        let mut w = bitmap_new(100);
        bitmap_set(&mut w, 42);
        assert!(bitmap_get(&w, 42));
        assert!(!bitmap_get(&w, 41));
        assert!(!bitmap_get(&w, 43));
    }

    #[test]
    fn set_clear_cycle() {
        let mut w = bitmap_new(64);
        bitmap_set(&mut w, 0);
        bitmap_set(&mut w, 63);
        assert!(bitmap_get(&w, 0));
        assert!(bitmap_get(&w, 63));
        bitmap_clear(&mut w, 0);
        assert!(!bitmap_get(&w, 0));
        assert!(bitmap_get(&w, 63));
    }

    #[test]
    fn count_ones() {
        let mut w = bitmap_new(200);
        assert_eq!(bitmap_count(&w), 0);
        bitmap_set(&mut w, 0);
        bitmap_set(&mut w, 1);
        bitmap_set(&mut w, 100);
        assert_eq!(bitmap_count(&w), 3);
        bitmap_clear(&mut w, 1);
        assert_eq!(bitmap_count(&w), 2);
    }

    #[test]
    fn out_of_bounds_safe() {
        let mut w = bitmap_new(10);
        bitmap_set(&mut w, 100); // 越界，忽略
        assert!(!bitmap_get(&w, 100));
        bitmap_clear(&mut w, 100); // 越界，忽略
    }

    #[test]
    fn all_bits() {
        let mut w = bitmap_new(256);
        for i in 0..256 {
            bitmap_set(&mut w, i);
        }
        assert_eq!(bitmap_count(&w), 256);
        for i in 0..256 {
            assert!(bitmap_get(&w, i));
        }
        for i in 0..256 {
            bitmap_clear(&mut w, i);
        }
        assert_eq!(bitmap_count(&w), 0);
    }
}
