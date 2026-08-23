//! RingBuf 环形缓冲（标准库数据结构缺口 A6）——纯函数共享层（ADR-0004 语义唯一源）
//!
//! 固定容量 FIFO 环形缓冲，内部以 `Vec<i128>` 存储。
//! H 语言 API：`io.ringbuf.init(cap)` → RingBuf；`.push(v)` / `.pop() ?T` /
//! `.len() usize` / `.capacity() usize` / `.is_full() bool` / `.is_empty() bool` /
//! `.clear()` / `.peek(idx) ?T`。

/// 环形缓冲内部状态。
#[derive(Clone, Debug)]
pub struct RingBuf {
    buf: Vec<i128>,
    head: usize,
    len: usize,
    cap: usize,
}

/// 创建容量为 `cap` 的 RingBuf。
pub fn ringbuf_new(cap: usize) -> RingBuf {
    // 空容量：零分配
    let buf = if cap == 0 {
        Vec::new()
    } else {
        Vec::with_capacity(cap)
    };
    RingBuf {
        buf,
        head: 0,
        len: 0,
        cap,
    }
}

/// 元素个数。
pub fn ringbuf_len(rb: &RingBuf) -> usize {
    rb.len
}

/// 容量。
pub fn ringbuf_capacity(rb: &RingBuf) -> usize {
    rb.cap
}

/// 是否为空。
pub fn ringbuf_is_empty(rb: &RingBuf) -> bool {
    rb.len == 0
}

/// 是否已满。
pub fn ringbuf_is_full(rb: &RingBuf) -> bool {
    rb.len == rb.cap
}

/// 尾部推入。已满时返回 false（不覆盖）。
pub fn ringbuf_push(rb: &mut RingBuf, v: i128) -> bool {
    if rb.len == rb.cap {
        return false;
    }
    let idx = (rb.head + rb.len) % rb.cap;
    if idx < rb.buf.len() {
        rb.buf[idx] = v;
    } else {
        rb.buf.push(v);
    }
    rb.len += 1;
    true
}

/// 头部弹出。空时返回 None。
pub fn ringbuf_pop(rb: &mut RingBuf) -> Option<i128> {
    if rb.len == 0 {
        return None;
    }
    let v = rb.buf[rb.head];
    rb.head = (rb.head + 1) % rb.cap;
    rb.len -= 1;
    Some(v)
}

/// 读取指定偏移位置的元素（不弹出）。越界返回 None。
pub fn ringbuf_peek(rb: &RingBuf, idx: usize) -> Option<i128> {
    if idx >= rb.len {
        return None;
    }
    let pos = (rb.head + idx) % rb.cap;
    Some(rb.buf[pos])
}

/// 清空缓冲。
pub fn ringbuf_clear(rb: &mut RingBuf) {
    rb.head = 0;
    rb.len = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_empty() {
        let rb = ringbuf_new(0);
        assert_eq!(ringbuf_len(&rb), 0);
        assert!(ringbuf_is_empty(&rb));
        assert!(ringbuf_is_full(&rb));
    }

    #[test]
    fn new_with_capacity() {
        let rb = ringbuf_new(64);
        assert_eq!(ringbuf_capacity(&rb), 64);
        assert_eq!(ringbuf_len(&rb), 0);
        assert!(ringbuf_is_empty(&rb));
        assert!(!ringbuf_is_full(&rb));
    }

    #[test]
    fn push_pop_basic() {
        let mut rb = ringbuf_new(4);
        assert!(ringbuf_push(&mut rb, 42));
        assert!(ringbuf_push(&mut rb, 99));
        assert!(ringbuf_push(&mut rb, -1));
        assert_eq!(ringbuf_len(&rb), 3);
        assert!(!ringbuf_is_full(&rb));
        assert_eq!(ringbuf_pop(&mut rb), Some(42));
        assert_eq!(ringbuf_pop(&mut rb), Some(99));
        assert_eq!(ringbuf_pop(&mut rb), Some(-1));
        assert!(ringbuf_is_empty(&rb));
        assert_eq!(ringbuf_pop(&mut rb), None);
    }

    #[test]
    fn push_when_full_returns_false() {
        let mut rb = ringbuf_new(2);
        assert!(ringbuf_push(&mut rb, 1));
        assert!(ringbuf_push(&mut rb, 2));
        assert!(!ringbuf_push(&mut rb, 3)); // 已满
        assert_eq!(ringbuf_len(&rb), 2);
    }

    #[test]
    fn wrap_around() {
        let mut rb = ringbuf_new(3);
        assert!(ringbuf_push(&mut rb, 10));
        assert!(ringbuf_push(&mut rb, 20));
        assert!(ringbuf_push(&mut rb, 30));
        assert_eq!(ringbuf_pop(&mut rb), Some(10)); // head → 1
        assert!(ringbuf_push(&mut rb, 40)); // 回绕到位置 0
        assert_eq!(ringbuf_pop(&mut rb), Some(20));
        assert_eq!(ringbuf_pop(&mut rb), Some(30));
        assert_eq!(ringbuf_pop(&mut rb), Some(40));
        assert!(ringbuf_is_empty(&rb));
    }

    #[test]
    fn peek() {
        let mut rb = ringbuf_new(5);
        assert!(ringbuf_push(&mut rb, 100));
        assert!(ringbuf_push(&mut rb, 200));
        assert!(ringbuf_push(&mut rb, 300));
        assert_eq!(ringbuf_peek(&rb, 0), Some(100));
        assert_eq!(ringbuf_peek(&rb, 1), Some(200));
        assert_eq!(ringbuf_peek(&rb, 2), Some(300));
        assert_eq!(ringbuf_peek(&rb, 3), None);
    }

    #[test]
    fn clear() {
        let mut rb = ringbuf_new(10);
        assert!(ringbuf_push(&mut rb, 1));
        assert!(ringbuf_push(&mut rb, 2));
        assert_eq!(ringbuf_len(&rb), 2);
        ringbuf_clear(&mut rb);
        assert!(ringbuf_is_empty(&rb));
        assert_eq!(ringbuf_pop(&mut rb), None);
    }

    #[test]
    fn pop_empty_returns_none() {
        let mut rb = ringbuf_new(10);
        assert_eq!(ringbuf_pop(&mut rb), None);
        assert!(ringbuf_push(&mut rb, 7));
        assert_eq!(ringbuf_pop(&mut rb), Some(7));
        assert_eq!(ringbuf_pop(&mut rb), None);
    }

    #[test]
    fn fifo_order() {
        let mut rb = ringbuf_new(100);
        let vals: Vec<i128> = (0..50).collect();
        for &v in &vals {
            assert!(ringbuf_push(&mut rb, v));
        }
        for &v in &vals {
            assert_eq!(ringbuf_pop(&mut rb), Some(v));
        }
        assert!(ringbuf_is_empty(&rb));
    }

    #[test]
    fn large_values() {
        let mut rb = ringbuf_new(10);
        let big: i128 = 1 << 100;
        assert!(ringbuf_push(&mut rb, big));
        assert_eq!(ringbuf_pop(&mut rb), Some(big));
    }

    #[test]
    fn negative_values() {
        let mut rb = ringbuf_new(10);
        assert!(ringbuf_push(&mut rb, -42));
        assert!(ringbuf_push(&mut rb, -1));
        assert_eq!(ringbuf_pop(&mut rb), Some(-42));
        assert_eq!(ringbuf_pop(&mut rb), Some(-1));
    }
}
