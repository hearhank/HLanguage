use std::cell::RefCell;
use std::rc::Weak;

/// 全局分配器 Debug 泄漏登记（§8.3：分配记录表；`weak` 持分配数据的弱引用——
/// 值被销毁（作用域退出自动销毁）后升级失败，即视为已释放。退出时仍可升级者 = 泄漏）。
#[derive(Debug)]
pub struct LeakRecord {
    /// 分配大小（字节）
    pub size: usize,
    /// 分配点行号（调用 `alloc.alloc(n)` 处；IR 侧无行号 → 0）
    pub line: u32,
    /// 分配数据弱引用（存活判定）
    pub weak: Weak<RefCell<Vec<u8>>>,
}
