use super::lazy_op::LazyOp;
use super::value::Value;

/// 惰性迭代器数据（A7：`next()` 按需求值，filter/map 链式延迟计算）
/// 操作按链式调用顺序存储在 `ops` 中，`lazy_iter_next` 按序应用。
/// 例如 `arr.map(g).filter(f)` → ops = [Map(g), Filter(f)]，
/// 对每个源元素：先 Map(g) 变换，再 Filter(f) 筛选。
#[derive(Debug, Clone)]
pub struct LazyIterData {
    /// 源数据（原始可迭代值：Arr/Slice/Str/Map/Vec/Class）
    pub source: Value,
    /// 当前位置（源的迭代索引）
    pub index: usize,
    /// 源类型名（"arr"/"slice"/"str"/"map"/"vec"/"class"）
    pub source_type: String,
    /// 操作列表（按链式调用顺序存储：filter/map 交错，按序应用）
    pub ops: Vec<LazyOp>,
    /// Map 遍历键缓存（非 Map 源时为空；构造时固定顺序保证确定性遍历）
    pub keys_cache: Vec<String>,
}

unsafe impl Send for LazyIterData {}
