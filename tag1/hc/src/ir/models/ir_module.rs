//! IR 模块结构（ADR-0028：自 ir/mod.rs 拆分）

use super::*;

#[derive(Debug, Clone, Default)]
pub struct IrModule {
    pub funcs: Vec<IrFunc>,
    /// 闭包函数表（Phase 4）：`IrValue::Closure.func` / `MakeClosure.func` 索引。
    /// 与 funcs 同构（body/params/n_slots），但绝不参与 `func_index` 按名分派。
    pub closures: Vec<IrFunc>,
    /// 函数名（扁平 + 限定）→ 索引表（声明序，支持重载/可选参数多候选；
    /// 对齐 oracle `pick_fn` interp.rs:2665-2796 的候选池）
    pub func_index: HashMap<String, Vec<usize>>,
    /// 全局/常量名（声明序，扁平——namespace 内 global 扁平化对齐 oracle
    /// `exec_decl_top` 无前缀登记；错误集别名除外）。运行时 `IrRuntime::init`
    /// 预分配 cell 后执行全部 `@__init__` 函数（多文件合并 = 多个 init 依次运行）。
    pub globals: Vec<String>,
    /// 错误名 → 码（M2.6 编译期错误码表；运行时内建产生的错误值携带与
    /// `error.X` 字面量一致的码——`value_eq` 按码比较，须同一张表）。
    pub error_codes: HashMap<String, u32>,
    /// 枚举名（扁平 + 全限定）→ 变体名（声明序；Phase 7 `@intFromEnum`/`@enumFromInt`
    /// 运行时分派按序求索引，对齐 oracle `TypeDef::Enum`）。
    pub enum_variants: HashMap<String, Vec<String>>,
    /// [continuous] 类名（扁平 + 全限定）：`DeepCopy` 指令运行时门——仅连续类值
    /// 语义深拷贝，标量/数组/非连续类恒等（引用别名）。由 `TypeTable.classes` 汇集。
    pub continuous: HashSet<String>,
    /// K1 无标签 union（扁平 + 全限定）→ 字段声明（名 + 类型，声明序）：`UnionSync`
    /// 与 `store_field` 写路径字节重解释同步用（ADR-0014）。
    pub unions: HashMap<String, Vec<(String, Type)>>,
    /// ADR-0027：类型名 → 实现的接口名列表（编译期接口分派用）
    pub type_implements: HashMap<String, Vec<String>>,
}
