use super::value::Value;

/// 惰性迭代器操作类型（filter/map 按链式调用顺序存储）
#[derive(Debug, Clone)]
pub enum LazyOp {
    /// 筛选闭包：返回 false 则跳过该元素
    Filter(Value),
    /// 变换闭包：变换元素值
    Map(Value),
}

unsafe impl Send for LazyOp {}
