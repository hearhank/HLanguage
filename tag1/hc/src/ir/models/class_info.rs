//! class 元数据（ADR-0028：自 ir/mod.rs 拆分）

use super::*;

#[derive(Debug, Default, Clone)]
pub struct ClassInfo {
    /// 字段（名 + 类型，声明序）——`alloc.init(T)` 默认字段构造用（对齐 oracle
    /// `default_value`：无参构造 = 类型空实例，字段逐默认值）。
    pub fields: Vec<(String, Type)>,
    pub methods: Vec<String>,
    /// [continuous] 连续内存值类型（H1 特性标注）：赋值即复制（值语义），非别名。
    /// 驱动 `Stmt::VarDecl` 降级发射 `DeepCopy`（对齐 oracle `type_is_continuous`）。
    pub continuous: bool,
}
