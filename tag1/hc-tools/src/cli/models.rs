//! CLI 数据模型：一个类型一个文件（ADR-0028）

mod dangle_mode;
mod test_mode;

pub(crate) use dangle_mode::DangleMode;
pub(crate) use test_mode::TestMode;
