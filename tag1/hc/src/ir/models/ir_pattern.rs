//! switch 模式（ADR-0028：自 ir/mod.rs 拆分；对齐 AST `crate::ast::SwitchPattern`）

/// switch 模式（对齐 AST [`crate::ast::SwitchPattern`]；`Else` 不发射 MatchTest——
/// 在 lower 阶段识别为兜底臂，其余模式全部失败后落入）。
#[derive(Debug, Clone)]
pub enum IrPattern {
    /// error.NotFound → 主题为 `Err{name}` 且 name 相等
    Error(String),
    /// 标识符 / 枚举变体 / true/false / null
    Ident(String),
    Int(i128),
    Float(f64),
    Str(String),
    Char(u32),
}
