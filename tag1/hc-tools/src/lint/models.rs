//! lint 数据模型：一个类型一个文件（ADR-0028）

mod lint_diag;
mod lint_rule;

pub use lint_diag::LintDiag;
pub use lint_rule::LintRule;
