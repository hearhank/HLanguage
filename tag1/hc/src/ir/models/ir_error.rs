//! IR 错误值（ADR-0028：自 ir/mod.rs 拆分）

/// IR 层错误（name 对齐 oracle 错误类别，message 为人类可读信息）
#[derive(Debug, Clone)]
pub struct IrError {
    pub name: String,
    pub message: String,
}

impl IrError {
    pub fn msg(name: &str, message: impl Into<String>) -> Self {
        IrError {
            name: name.to_string(),
            message: message.into(),
        }
    }
}
