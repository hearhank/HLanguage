//! IR 函数结构（ADR-0028：自 ir/mod.rs 拆分）

use super::*;

#[derive(Debug, Clone)]
pub struct IrFunc {
    pub name: String,
    /// 参数槽号（声明序）
    pub params: Vec<usize>,
    /// 参数类型（声明序，重载按实参值类型分派用；与 params 等长）
    pub param_ty: Vec<Type>,
    /// 返回类型（None → void，即 Type::Named("void", vec![])）
    pub ret_ty: Type,
    /// 参数是否有默认值（声明序；可选参数 = 尾部默认，对齐 ADR-0009）
    pub param_defaults: Vec<bool>,
    /// 参数默认常量值（编译期常量默认值；缺失尾参时调用点补齐）
    pub defaults: Vec<Option<IrConst>>,
    /// 槽总数（参数 + 局部变量 + 临时）
    pub n_slots: usize,
    pub body: Vec<IrInst>,
    pub is_test: bool,
    /// K5（ADR-0014）：`export fn`——原生符号级导出（LLVM 外部 thunk 生成依据）
    pub exported: bool,
    /// A1（ADR-0020）：`extern fn`——纯声明（无 body，链接期解析外部 C 符号）。
    /// LLVM 后端生成 `declare` 而非 `define`；解释器拒绝调用。
    pub is_extern: bool,
}
