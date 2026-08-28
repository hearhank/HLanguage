//! IR 层统一 Result 别名（ADR-0028：自 ir/mod.rs 拆分）

use super::*;

pub(crate) type R<T> = std::result::Result<T, IrError>;
