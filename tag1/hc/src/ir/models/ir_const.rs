//! IR 常量（ADR-0028：自 ir/mod.rs 拆分）

#[derive(Debug, Clone, PartialEq)]
pub enum IrConst {
    Int(i128),
    Float(f64),
    Bool(bool),
    Str(String),
    Void,
    Null,
    /// error.Name（错误值 = 普通值，走值通道；code = M2.6 编译期错误码）
    Err {
        name: String,
        code: u32,
    },
    /// 开区间切片 `arr[a..]` 的上界哨兵（Phase 2）
    End,
}
