//! K1 无标签 union 元数据（ADR-0028：自 ir/mod.rs 拆分）

use super::*;

/// K1 无标签 union（ADR-0014）：字段（名 + 标量类型，声明序）——字段内存重叠，
/// size = 最大字段宽度；`@union` 标记 + 写字段字节重解释同步其余字段。
#[derive(Debug, Default, Clone)]
pub struct UnionInfo {
    pub fields: Vec<(String, Type)>,
}
