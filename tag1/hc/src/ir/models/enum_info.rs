//! enum 元数据（ADR-0028：自 ir/mod.rs 拆分）

#[derive(Debug, Default, Clone)]
pub struct EnumInfo {
    /// 变体名（声明序——`@intFromEnum`/`@enumFromInt` 运行时分派按序求索引）
    pub variants: Vec<String>,
}
