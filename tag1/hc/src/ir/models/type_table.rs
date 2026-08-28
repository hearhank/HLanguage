//! 类型元数据表（ADR-0028：自 ir/mod.rs 拆分；Phase 2：class/enum/namespace 判型）

use super::*;

#[derive(Debug, Default, Clone)]
pub struct TypeTable {
    /// class 名（扁平 + 全限定）→ 元数据
    pub classes: HashMap<String, ClassInfo>,
    /// enum 名（扁平 + 全限定）→ 变体集
    pub enums: HashMap<String, EnumInfo>,
    /// K1 无标签 union 名（扁平 + 全限定）→ 字段声明（ADR-0014）
    pub unions: HashMap<String, UnionInfo>,
    /// namespace 名（扁平 + 全限定）
    pub namespaces: std::collections::HashSet<String>,
}
