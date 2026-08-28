//! IR 值底层操作（ADR-0028：自 ir/mod.rs 拆分；解引用/索引/类型描述等共用工具）

use super::*;

/// 解引用：Ptr/Boxed → pointee（对齐 tree-walking `deref_value`）；pointee 为 Vec 时
/// 一并剥为共享 Arr；非 Ptr → 恒等。tree-walking 递归解引用，此处引用返回无法递归，
/// 故在 Ptr/Boxed/Vec 三处显式 peel（一层 `Ptr(Vec)`/`Boxed(Vec)` 即达 Arr）。
pub(crate) fn deref_value<'a>(ctx: &'a Ctx, v: &'a IrValue) -> &'a IrValue {
    match v {
        IrValue::Ptr(c) => match &ctx.cells[*c] {
            Cell::Value(v) => peel_vec(ctx, v),
            _ => v,
        },
        IrValue::Boxed(c) => match &ctx.cells[*c] {
            Cell::Boxed { data, .. } => match &ctx.cells[*data] {
                Cell::Value(v) => peel_vec(ctx, v),
                _ => v,
            },
            _ => v,
        },
        // G4：Vec peel → 共享底层的 Arr（对齐 tree-walking `Value::Vec => Value::Arr`）
        IrValue::Vec(c) => match &ctx.cells[*c] {
            Cell::Vec { arr, .. } => arr,
            _ => v,
        },
        other => other,
    }
}

/// G4：`IrValue::Vec` 剥为其底层 Arr 的引用；非 Vec 恒等（peel 辅助）
pub(crate) fn peel_vec<'a>(ctx: &'a Ctx, v: &'a IrValue) -> &'a IrValue {
    match v {
        IrValue::Vec(c) => match &ctx.cells[*c] {
            Cell::Vec { arr, .. } => arr,
            _ => v,
        },
        other => other,
    }
}

/// 索引值 → usize（负/非整 → BadIndex，对齐 tree-walking `as_index`）
pub(crate) fn as_index(ctx: &Ctx, v: &IrValue) -> R<usize> {
    match deref_value(ctx, v) {
        IrValue::Int(i) if *i >= 0 => Ok(*i as usize),
        _ => Err(IrError::msg("BadIndex", "bad index")),
    }
}

/// 值形态描述（`NotIterable` 错误消息；对齐 tree-walking `type_name` 的通俗面）
pub(crate) fn type_descr(v: &IrValue) -> String {
    match v {
        IrValue::Int(_) => "i32".into(),
        IrValue::Float(_) => "f64".into(),
        IrValue::Bool(_) => "bool".into(),
        IrValue::String(_) => "String".into(),
        IrValue::Opt(_) => "?T".into(),
        IrValue::Err { name, .. } => format!("error.{name}"),
        IrValue::Ptr(_) => "*T".into(),
        IrValue::Boxed(_) => "*T".into(),
        IrValue::Arr(_) => "[]T".into(),
        IrValue::Vec(_) => "[]T".into(),
        IrValue::Map(_) => "Map".into(),
        IrValue::Slice { .. } => "[]T".into(),
        IrValue::Class(_) => "class".into(),
        IrValue::Arena(_) => "Arena".into(),
        IrValue::Enum { name, .. } => name.clone(),
        IrValue::End => "end".into(),
        IrValue::Iter(_) => "<iter>".into(),
        IrValue::Fn(_) => "fn".into(),
        IrValue::Closure { .. } => "closure".into(),
        IrValue::Mutex(_) => "Mutex".into(),
        IrValue::Chan(_) => "Chan".into(),
        IrValue::Void => "void".into(),
    }
}
