//! 堆单元模型（ADR-0028：自 ir/mod.rs 拆分；Phase 1：别名与 tree-walking `Rc<RefCell<Value>>` 对齐）

use super::*;

/// 堆单元（cell）：槽持有的共享可变数据。槽 → cell 索引（[`Frame`]），
/// 指针 = `IrValue::Ptr(cell)`——多槽/多指针可共享同一 cell，写穿即别名。
#[derive(Debug, Clone)]
pub enum Cell {
    /// 普通值单元
    Value(IrValue),
    /// 数组底层（Phase 2）：元素 cell 索引（共享——切片/写索引/别名共用底层）
    Elems(Vec<usize>),
    /// 类实例（Phase 2）：类型名 + 字段 → 字段 cell 索引（字段为普通值，无别名）
    Class {
        name: String,
        fields: HashMap<String, usize>,
    },
    /// 迭代器（Phase 3）：`iter_items` 展开结果 + 前进游标。
    /// `items[i].cell` 为第 i 项的共享源 cell（Arr/Slice）或新 cell（Map/Str/用户迭代）；
    /// `is_ref` 表示是否与源容器共享（Mut/Move 捕获可写穿）。
    Iter { items: Vec<IterItem>, next: usize },
    /// Arena 分配器状态（G1：真实 bump + 块链表；deinit 批量归还 backing）
    Arena(ArenaStateIr),
    /// 装箱/接口胖指针（G3：data + vtbl + alloc 三字宽；对齐 tree-walking `BoxedData`）。
    /// data = pointee 的 cell 索引（`Cell::Value`）；vtbl = 具体类型名（tag1 静态标注）；
    /// alloc = 分配器引用（全局 alloc 或 Arena 句柄）。
    Boxed {
        data: usize,
        vtbl: String,
        alloc: IrValue,
    },
    /// 集合 Vec（G4：`arr` 恒为 `IrValue::Arr(items_cell)`——deref peel 共享底层
    /// `Cell::Elems`；`alloc` = 构造 `init(alloc)` 时携带的分配器引用）
    Vec { arr: IrValue, alloc: IrValue },
    /// 集合 Map（G4：键 → 字段 cell 索引；`alloc` = 构造时携带的分配器引用）
    Map {
        fields: HashMap<String, usize>,
        alloc: IrValue,
    },
}
