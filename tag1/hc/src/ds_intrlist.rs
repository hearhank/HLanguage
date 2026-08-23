//! IntrList 侵入式链表（标准库数据结构缺口 A6）——纯函数共享层（ADR-0004 语义唯一源）
//!
//! 双向链表，内部以 `Vec<Option<Node>>` 索引管理。
//! H 语言 API：`io.intrlist.init()` → IntrList；`.push_front(v) usize` /
//! `.pop_front() ?T` / `.push_back(v) usize` / `.pop_back() ?T` /
//! `.remove(idx) ?T` / `.len() usize` / `.is_empty() bool` / `.clear()`。

/// 链表节点。
#[derive(Clone, Debug)]
pub struct Node {
    pub prev: Option<usize>,
    pub next: Option<usize>,
    pub value: i128,
}

/// 侵入式链表内部状态。
#[derive(Clone, Debug)]
pub struct IntrList {
    nodes: Vec<Option<Node>>,
    head: Option<usize>,
    tail: Option<usize>,
    len: usize,
    /// 空闲节点索引栈（LIFO 重用）
    free: Vec<usize>,
}

/// 创建空链表。
pub fn intrlist_new() -> IntrList {
    IntrList {
        nodes: Vec::new(),
        head: None,
        tail: None,
        len: 0,
        free: Vec::new(),
    }
}

/// 分配一个节点索引（从空闲栈或新分配）。
fn alloc_node(list: &mut IntrList) -> usize {
    if let Some(idx) = list.free.pop() {
        idx
    } else {
        let idx = list.nodes.len();
        list.nodes.push(None);
        idx
    }
}

/// 头部推入。返回节点索引。
pub fn intrlist_push_front(list: &mut IntrList, v: i128) -> usize {
    let idx = alloc_node(list);
    list.nodes[idx] = Some(Node {
        prev: None,
        next: list.head,
        value: v,
    });
    if let Some(old_head) = list.head {
        if let Some(Some(ref mut node)) = list.nodes.get_mut(old_head) {
            node.prev = Some(idx);
        }
    } else {
        list.tail = Some(idx);
    }
    list.head = Some(idx);
    list.len += 1;
    idx
}

/// 尾部推入。返回节点索引。
pub fn intrlist_push_back(list: &mut IntrList, v: i128) -> usize {
    let idx = alloc_node(list);
    list.nodes[idx] = Some(Node {
        prev: list.tail,
        next: None,
        value: v,
    });
    if let Some(old_tail) = list.tail {
        if let Some(Some(ref mut node)) = list.nodes.get_mut(old_tail) {
            node.next = Some(idx);
        }
    } else {
        list.head = Some(idx);
    }
    list.tail = Some(idx);
    list.len += 1;
    idx
}

/// 头部弹出。空时返回 None。
pub fn intrlist_pop_front(list: &mut IntrList) -> Option<i128> {
    let head = list.head?;
    let node = list.nodes[head].take()?;
    list.head = node.next;
    if let Some(next) = node.next {
        if let Some(Some(ref mut n)) = list.nodes.get_mut(next) {
            n.prev = None;
        }
    } else {
        list.tail = None;
    }
    list.free.push(head);
    list.len -= 1;
    Some(node.value)
}

/// 尾部弹出。空时返回 None。
pub fn intrlist_pop_back(list: &mut IntrList) -> Option<i128> {
    let tail = list.tail?;
    let node = list.nodes[tail].take()?;
    list.tail = node.prev;
    if let Some(prev) = node.prev {
        if let Some(Some(ref mut n)) = list.nodes.get_mut(prev) {
            n.next = None;
        }
    } else {
        list.head = None;
    }
    list.free.push(tail);
    list.len -= 1;
    Some(node.value)
}

/// 移除指定索引的节点。索引无效或已移除时返回 None。
pub fn intrlist_remove(list: &mut IntrList, idx: usize) -> Option<i128> {
    let node = list.nodes.get_mut(idx)?;
    let node = node.take()?;
    // 更新前后节点的链接
    if let Some(prev) = node.prev {
        if let Some(Some(ref mut n)) = list.nodes.get_mut(prev) {
            n.next = node.next;
        }
    } else {
        list.head = node.next;
    }
    if let Some(next) = node.next {
        if let Some(Some(ref mut n)) = list.nodes.get_mut(next) {
            n.prev = node.prev;
        }
    } else {
        list.tail = node.prev;
    }
    list.free.push(idx);
    list.len -= 1;
    Some(node.value)
}

/// 元素个数。
pub fn intrlist_len(list: &IntrList) -> usize {
    list.len
}

/// 是否为空。
pub fn intrlist_is_empty(list: &IntrList) -> bool {
    list.len == 0
}

/// 清空链表。
pub fn intrlist_clear(list: &mut IntrList) {
    list.nodes.clear();
    list.head = None;
    list.tail = None;
    list.len = 0;
    list.free.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_empty() {
        let list = intrlist_new();
        assert_eq!(intrlist_len(&list), 0);
        assert!(intrlist_is_empty(&list));
        assert_eq!(intrlist_pop_front(&mut intrlist_new()), None);
        assert_eq!(intrlist_pop_back(&mut intrlist_new()), None);
    }

    #[test]
    fn push_front_pop_front() {
        let mut list = intrlist_new();
        intrlist_push_front(&mut list, 10);
        intrlist_push_front(&mut list, 20);
        intrlist_push_front(&mut list, 30);
        assert_eq!(intrlist_len(&list), 3);
        assert_eq!(intrlist_pop_front(&mut list), Some(30));
        assert_eq!(intrlist_pop_front(&mut list), Some(20));
        assert_eq!(intrlist_pop_front(&mut list), Some(10));
        assert!(intrlist_is_empty(&list));
    }

    #[test]
    fn push_back_pop_back() {
        let mut list = intrlist_new();
        intrlist_push_back(&mut list, 10);
        intrlist_push_back(&mut list, 20);
        intrlist_push_back(&mut list, 30);
        assert_eq!(intrlist_len(&list), 3);
        assert_eq!(intrlist_pop_back(&mut list), Some(30));
        assert_eq!(intrlist_pop_back(&mut list), Some(20));
        assert_eq!(intrlist_pop_back(&mut list), Some(10));
        assert!(intrlist_is_empty(&list));
    }

    #[test]
    fn push_front_pop_back() {
        let mut list = intrlist_new();
        intrlist_push_front(&mut list, 10);
        intrlist_push_front(&mut list, 20);
        intrlist_push_front(&mut list, 30);
        // list: 30 ↔ 20 ↔ 10
        assert_eq!(intrlist_pop_back(&mut list), Some(10));
        assert_eq!(intrlist_pop_back(&mut list), Some(20));
        assert_eq!(intrlist_pop_back(&mut list), Some(30));
        assert!(intrlist_is_empty(&list));
    }

    #[test]
    fn push_back_pop_front() {
        let mut list = intrlist_new();
        intrlist_push_back(&mut list, 10);
        intrlist_push_back(&mut list, 20);
        intrlist_push_back(&mut list, 30);
        // list: 10 ↔ 20 ↔ 30
        assert_eq!(intrlist_pop_front(&mut list), Some(10));
        assert_eq!(intrlist_pop_front(&mut list), Some(20));
        assert_eq!(intrlist_pop_front(&mut list), Some(30));
        assert!(intrlist_is_empty(&list));
    }

    #[test]
    fn remove_middle() {
        let mut list = intrlist_new();
        let a = intrlist_push_back(&mut list, 10);
        let b = intrlist_push_back(&mut list, 20);
        let c = intrlist_push_back(&mut list, 30);
        assert_eq!(intrlist_remove(&mut list, b), Some(20));
        assert_eq!(intrlist_len(&list), 2);
        // list: 10 ↔ 30
        assert_eq!(intrlist_pop_front(&mut list), Some(10));
        assert_eq!(intrlist_pop_front(&mut list), Some(30));
        assert!(intrlist_is_empty(&list));
    }

    #[test]
    fn remove_head() {
        let mut list = intrlist_new();
        let a = intrlist_push_back(&mut list, 10);
        let _b = intrlist_push_back(&mut list, 20);
        assert_eq!(intrlist_remove(&mut list, a), Some(10));
        assert_eq!(intrlist_len(&list), 1);
        assert_eq!(intrlist_pop_front(&mut list), Some(20));
    }

    #[test]
    fn remove_tail() {
        let mut list = intrlist_new();
        let _a = intrlist_push_back(&mut list, 10);
        let b = intrlist_push_back(&mut list, 20);
        assert_eq!(intrlist_remove(&mut list, b), Some(20));
        assert_eq!(intrlist_len(&list), 1);
        assert_eq!(intrlist_pop_front(&mut list), Some(10));
    }

    #[test]
    fn remove_invalid() {
        let mut list = intrlist_new();
        assert_eq!(intrlist_remove(&mut list, 0), None);
        assert_eq!(intrlist_remove(&mut list, 100), None);
    }

    #[test]
    fn clear() {
        let mut list = intrlist_new();
        intrlist_push_back(&mut list, 1);
        intrlist_push_back(&mut list, 2);
        intrlist_push_back(&mut list, 3);
        assert_eq!(intrlist_len(&list), 3);
        intrlist_clear(&mut list);
        assert!(intrlist_is_empty(&list));
        assert_eq!(intrlist_pop_front(&mut list), None);
    }

    #[test]
    fn node_reuse() {
        let mut list = intrlist_new();
        let a = intrlist_push_back(&mut list, 10);
        let b = intrlist_push_back(&mut list, 20);
        assert_eq!(intrlist_len(&list), 2);
        let _ = intrlist_remove(&mut list, a);
        let _ = intrlist_remove(&mut list, b);
        assert!(intrlist_is_empty(&list));
        // 节点应被重用
        let c = intrlist_push_back(&mut list, 30);
        assert_eq!(c, b); // 最后释放的节点被重用（LIFO）
        let d = intrlist_push_back(&mut list, 40);
        assert_eq!(d, a);
    }
}
