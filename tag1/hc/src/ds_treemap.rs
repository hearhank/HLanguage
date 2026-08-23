//! TreeMap 有序映射（标准库数据结构缺口 A6）——纯函数共享层（ADR-0004 语义唯一源）
//!
//! 基于二叉搜索树（BST）的有序键值存储，内部以 `Vec<Option<TreeNode>>` 管理节点。
//! H 语言 API：`io.treemap.init()` → TreeMap；`.insert(key, value)` /
//! `.get(key) ?T` / `.contains(key) bool` / `.remove(key) ?T` /
//! `.len() usize` / `.is_empty() bool` / `.clear()`。

/// 树节点。
#[derive(Clone, Debug)]
pub struct TreeNode {
    pub key: i128,
    pub value: i128,
    pub left: Option<usize>,
    pub right: Option<usize>,
}

/// TreeMap 内部状态。
#[derive(Clone, Debug)]
pub struct TreeMap {
    nodes: Vec<Option<TreeNode>>,
    root: Option<usize>,
    len: usize,
    /// 空闲节点索引栈（LIFO 重用）
    free: Vec<usize>,
}

/// 创建空 TreeMap。
pub fn treemap_new() -> TreeMap {
    TreeMap {
        nodes: Vec::new(),
        root: None,
        len: 0,
        free: Vec::new(),
    }
}

/// 分配一个节点索引（从空闲栈或新分配）。
fn alloc_node(tm: &mut TreeMap) -> usize {
    if let Some(idx) = tm.free.pop() {
        idx
    } else {
        let idx = tm.nodes.len();
        tm.nodes.push(None);
        idx
    }
}

/// 插入键值对。如果键已存在，更新值。
pub fn treemap_insert(tm: &mut TreeMap, key: i128, value: i128) {
    // 如果 root 为空，直接创建根节点
    if tm.root.is_none() {
        let idx = alloc_node(tm);
        tm.nodes[idx] = Some(TreeNode {
            key,
            value,
            left: None,
            right: None,
        });
        tm.root = Some(idx);
        tm.len += 1;
        return;
    }

    // 非递归遍历查找插入位置
    let mut cur = tm.root.unwrap();
    loop {
        let key_match = match &tm.nodes[cur] {
            Some(n) => n.key == key,
            None => unreachable!(),
        };
        if key_match {
            // 键已存在，更新值
            if let Some(n) = &mut tm.nodes[cur] {
                n.value = value;
            }
            return;
        }
        let go_left = match &tm.nodes[cur] {
            Some(n) => key < n.key,
            None => unreachable!(),
        };
        if go_left {
            let has_left = match &tm.nodes[cur] {
                Some(n) => n.left.is_some(),
                None => unreachable!(),
            };
            if has_left {
                cur = tm.nodes[cur].as_ref().unwrap().left.unwrap();
            } else {
                let idx = alloc_node(tm);
                tm.nodes[idx] = Some(TreeNode {
                    key,
                    value,
                    left: None,
                    right: None,
                });
                if let Some(n) = &mut tm.nodes[cur] {
                    n.left = Some(idx);
                }
                tm.len += 1;
                return;
            }
        } else {
            let has_right = match &tm.nodes[cur] {
                Some(n) => n.right.is_some(),
                None => unreachable!(),
            };
            if has_right {
                cur = tm.nodes[cur].as_ref().unwrap().right.unwrap();
            } else {
                let idx = alloc_node(tm);
                tm.nodes[idx] = Some(TreeNode {
                    key,
                    value,
                    left: None,
                    right: None,
                });
                if let Some(n) = &mut tm.nodes[cur] {
                    n.right = Some(idx);
                }
                tm.len += 1;
                return;
            }
        }
    }
}

/// 获取键对应的值。键不存在时返回 None。
pub fn treemap_get(tm: &TreeMap, key: i128) -> Option<i128> {
    let mut cur = tm.root?;
    loop {
        let node = tm.nodes[cur].as_ref()?;
        if key == node.key {
            return Some(node.value);
        }
        if key < node.key {
            cur = node.left?;
        } else {
            cur = node.right?;
        }
    }
}

/// 检查是否包含指定键。
pub fn treemap_contains(tm: &TreeMap, key: i128) -> bool {
    treemap_get(tm, key).is_some()
}

/// 移除键对应的节点。键不存在时返回 None。
pub fn treemap_remove(tm: &mut TreeMap, key: i128) -> Option<i128> {
    // 找到要删除的节点及其父节点
    let root = tm.root?;
    let mut parent: Option<usize> = None;
    let mut cur = root;
    let mut side: isize = 0; // -1 = left, 0 = root, 1 = right

    loop {
        let node = match &tm.nodes[cur] {
            Some(n) => n.clone(),
            None => return None,
        };
        if key == node.key {
            break;
        }
        parent = Some(cur);
        if key < node.key {
            match node.left {
                Some(left) => {
                    cur = left;
                    side = -1;
                }
                None => return None,
            }
        } else {
            match node.right {
                Some(right) => {
                    cur = right;
                    side = 1;
                }
                None => return None,
            }
        }
    }

    // 获取要删除的节点
    let node = tm.nodes[cur].take()?;
    let value = node.value;

    // 情况 1：无子节点
    if node.left.is_none() && node.right.is_none() {
        if let Some(p) = parent {
            if side == -1 {
                tm.nodes[p].as_mut().unwrap().left = None;
            } else {
                tm.nodes[p].as_mut().unwrap().right = None;
            }
        } else {
            tm.root = None;
        }
        tm.free.push(cur);
    }
    // 情况 2：只有右子节点
    else if node.left.is_none() {
        if let Some(p) = parent {
            if side == -1 {
                tm.nodes[p].as_mut().unwrap().left = node.right;
            } else {
                tm.nodes[p].as_mut().unwrap().right = node.right;
            }
        } else {
            tm.root = node.right;
        }
        tm.free.push(cur);
    }
    // 情况 3：只有左子节点
    else if node.right.is_none() {
        if let Some(p) = parent {
            if side == -1 {
                tm.nodes[p].as_mut().unwrap().left = node.left;
            } else {
                tm.nodes[p].as_mut().unwrap().right = node.left;
            }
        } else {
            tm.root = node.left;
        }
        tm.free.push(cur);
    }
    // 情况 4：有两个子节点
    // 找到中序后继（右子树的最小节点），用后继替换当前节点，然后删除后继
    else {
        let right = node.right.unwrap();
        // 如果右子节点没有左子节点，则右子节点本身就是后继
        let has_left = tm.nodes[right].as_ref().unwrap().left.is_some();
        if !has_left {
            // 后继就是 right
            let succ_node = tm.nodes[right].take().unwrap();
            // right 的右子节点成为新的 right
            let new_right = succ_node.right;
            // 重建当前节点（用后继的数据）
            tm.nodes[cur] = Some(TreeNode {
                key: succ_node.key,
                value: succ_node.value,
                left: node.left,
                right: new_right,
            });
            // 更新父节点（如果 cur 是 root，则不变）
            if let Some(p) = parent {
                if side == -1 {
                    tm.nodes[p].as_mut().unwrap().left = Some(cur);
                } else {
                    tm.nodes[p].as_mut().unwrap().right = Some(cur);
                }
            }
            // 释放 right 索引
            tm.free.push(right);
        } else {
            // 找到右子树的最小节点
            let mut succ_parent = right;
            let mut succ = tm.nodes[right].as_ref().unwrap().left.unwrap();
            loop {
                let sn = tm.nodes[succ].as_ref().unwrap().clone();
                match sn.left {
                    Some(left) => {
                        succ_parent = succ;
                        succ = left;
                    }
                    None => break,
                }
            }
            // succ 是最小节点，它没有左子节点
            let succ_node = tm.nodes[succ].take().unwrap();
            // succ 的右子节点成为 succ_parent 的左子节点
            tm.nodes[succ_parent].as_mut().unwrap().left = succ_node.right;
            // 重建当前节点（用后继的数据）
            tm.nodes[cur] = Some(TreeNode {
                key: succ_node.key,
                value: succ_node.value,
                left: node.left,
                right: node.right,
            });
            // 更新父节点
            if let Some(p) = parent {
                if side == -1 {
                    tm.nodes[p].as_mut().unwrap().left = Some(cur);
                } else {
                    tm.nodes[p].as_mut().unwrap().right = Some(cur);
                }
            }
            // 释放 succ 索引
            tm.free.push(succ);
        }
    }

    tm.len -= 1;
    Some(value)
}

/// 元素个数。
pub fn treemap_len(tm: &TreeMap) -> usize {
    tm.len
}

/// 是否为空。
pub fn treemap_is_empty(tm: &TreeMap) -> bool {
    tm.len == 0
}

/// 清空 TreeMap。
pub fn treemap_clear(tm: &mut TreeMap) {
    tm.nodes.clear();
    tm.root = None;
    tm.len = 0;
    tm.free.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_empty() {
        let tm = treemap_new();
        assert_eq!(treemap_len(&tm), 0);
        assert!(treemap_is_empty(&tm));
        assert_eq!(treemap_get(&tm, 42), None);
    }

    #[test]
    fn insert_and_get() {
        let mut tm = treemap_new();
        treemap_insert(&mut tm, 10, 100);
        treemap_insert(&mut tm, 20, 200);
        treemap_insert(&mut tm, 5, 50);
        assert_eq!(treemap_len(&tm), 3);
        assert_eq!(treemap_get(&tm, 10), Some(100));
        assert_eq!(treemap_get(&tm, 20), Some(200));
        assert_eq!(treemap_get(&tm, 5), Some(50));
        assert_eq!(treemap_get(&tm, 99), None);
    }

    #[test]
    fn update_existing_key() {
        let mut tm = treemap_new();
        treemap_insert(&mut tm, 10, 100);
        treemap_insert(&mut tm, 10, 999);
        assert_eq!(treemap_len(&tm), 1);
        assert_eq!(treemap_get(&tm, 10), Some(999));
    }

    #[test]
    fn contains() {
        let mut tm = treemap_new();
        treemap_insert(&mut tm, 42, 1);
        assert!(treemap_contains(&tm, 42));
        assert!(!treemap_contains(&tm, 43));
    }

    #[test]
    fn remove_leaf() {
        let mut tm = treemap_new();
        treemap_insert(&mut tm, 10, 100);
        treemap_insert(&mut tm, 20, 200);
        assert_eq!(treemap_remove(&mut tm, 20), Some(200));
        assert_eq!(treemap_len(&tm), 1);
        assert_eq!(treemap_get(&tm, 10), Some(100));
        assert_eq!(treemap_get(&tm, 20), None);
    }

    #[test]
    fn remove_root_with_one_child() {
        let mut tm = treemap_new();
        treemap_insert(&mut tm, 10, 100);
        treemap_insert(&mut tm, 20, 200);
        assert_eq!(treemap_remove(&mut tm, 10), Some(100));
        assert_eq!(treemap_len(&tm), 1);
        assert_eq!(treemap_get(&tm, 20), Some(200));
    }

    #[test]
    fn remove_root_with_two_children() {
        let mut tm = treemap_new();
        treemap_insert(&mut tm, 10, 100);
        treemap_insert(&mut tm, 5, 50);
        treemap_insert(&mut tm, 20, 200);
        assert_eq!(treemap_remove(&mut tm, 10), Some(100));
        assert_eq!(treemap_len(&tm), 2);
        assert!(treemap_get(&tm, 5) == Some(50));
        assert!(treemap_get(&tm, 20) == Some(200));
    }

    #[test]
    fn remove_middle_with_two_children() {
        let mut tm = treemap_new();
        treemap_insert(&mut tm, 30, 300);
        treemap_insert(&mut tm, 10, 100);
        treemap_insert(&mut tm, 50, 500);
        treemap_insert(&mut tm, 20, 200);
        treemap_insert(&mut tm, 40, 400);
        // 删除 30（根，有两个子节点，后继是 40）
        assert_eq!(treemap_remove(&mut tm, 30), Some(300));
        assert_eq!(treemap_len(&tm), 4);
        // 验证所有键仍然可访问
        assert_eq!(treemap_get(&tm, 10), Some(100));
        assert_eq!(treemap_get(&tm, 20), Some(200));
        assert_eq!(treemap_get(&tm, 40), Some(400));
        assert_eq!(treemap_get(&tm, 50), Some(500));
    }

    #[test]
    fn remove_nonexistent() {
        let mut tm = treemap_new();
        treemap_insert(&mut tm, 10, 100);
        assert_eq!(treemap_remove(&mut tm, 99), None);
        assert_eq!(treemap_len(&tm), 1);
    }

    #[test]
    fn clear() {
        let mut tm = treemap_new();
        treemap_insert(&mut tm, 1, 10);
        treemap_insert(&mut tm, 2, 20);
        treemap_insert(&mut tm, 3, 30);
        assert_eq!(treemap_len(&tm), 3);
        treemap_clear(&mut tm);
        assert!(treemap_is_empty(&tm));
        assert_eq!(treemap_get(&tm, 1), None);
        assert_eq!(treemap_get(&tm, 2), None);
        assert_eq!(treemap_get(&tm, 3), None);
    }

    #[test]
    fn insert_many_ascending() {
        let mut tm = treemap_new();
        for i in 0..100 {
            treemap_insert(&mut tm, i, i * 10);
        }
        assert_eq!(treemap_len(&tm), 100);
        for i in 0..100 {
            assert_eq!(treemap_get(&tm, i), Some(i * 10));
        }
    }

    #[test]
    fn insert_many_descending() {
        let mut tm = treemap_new();
        for i in (0..100).rev() {
            treemap_insert(&mut tm, i, i * 10);
        }
        assert_eq!(treemap_len(&tm), 100);
        for i in 0..100 {
            assert_eq!(treemap_get(&tm, i), Some(i * 10));
        }
    }

    #[test]
    fn remove_all() {
        let mut tm = treemap_new();
        for i in 0..50 {
            treemap_insert(&mut tm, i, i);
        }
        for i in 0..50 {
            assert_eq!(treemap_remove(&mut tm, i), Some(i));
        }
        assert!(treemap_is_empty(&tm));
    }

    #[test]
    fn negative_keys() {
        let mut tm = treemap_new();
        treemap_insert(&mut tm, -10, 100);
        treemap_insert(&mut tm, -20, 200);
        treemap_insert(&mut tm, 0, 0);
        assert_eq!(treemap_get(&tm, -10), Some(100));
        assert_eq!(treemap_get(&tm, -20), Some(200));
        assert_eq!(treemap_get(&tm, 0), Some(0));
    }

    #[test]
    fn remove_with_successor_as_right_child() {
        // 测试后继是右子节点的情况（右子节点没有左子节点）
        let mut tm = treemap_new();
        treemap_insert(&mut tm, 10, 100);
        treemap_insert(&mut tm, 5, 50);
        treemap_insert(&mut tm, 15, 150); // 15 是 10 的后继，且没有左子节点
        assert_eq!(treemap_remove(&mut tm, 10), Some(100));
        assert_eq!(treemap_len(&tm), 2);
        assert_eq!(treemap_get(&tm, 5), Some(50));
        assert_eq!(treemap_get(&tm, 15), Some(150));
    }

    #[test]
    fn remove_with_deep_successor() {
        // 测试后继在右子树深处的情况
        let mut tm = treemap_new();
        treemap_insert(&mut tm, 20, 200);
        treemap_insert(&mut tm, 10, 100);
        treemap_insert(&mut tm, 30, 300);
        treemap_insert(&mut tm, 25, 250);
        treemap_insert(&mut tm, 35, 350);
        treemap_insert(&mut tm, 22, 220); // 22 是 20 的后继
        assert_eq!(treemap_remove(&mut tm, 20), Some(200));
        assert_eq!(treemap_len(&tm), 5);
        assert_eq!(treemap_get(&tm, 10), Some(100));
        assert_eq!(treemap_get(&tm, 22), Some(220));
        assert_eq!(treemap_get(&tm, 25), Some(250));
        assert_eq!(treemap_get(&tm, 30), Some(300));
        assert_eq!(treemap_get(&tm, 35), Some(350));
    }
}
