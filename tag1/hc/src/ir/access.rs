//! IR 字段/索引/切片访问（ADR-0028：自 ir/mod.rs 拆分；对齐 tree-walking eval_field/eval_index）

use super::*;

/// 字段读取：Class 字段 / Str/Arr/Slice/Map `.len` 内建字段；无字段 → NoField
pub(crate) fn field_value(ctx: &Ctx, b: &IrValue, field: &str) -> R<IrValue> {
    match b {
        IrValue::Class(c) => match &ctx.cells[*c] {
            Cell::Class { name, fields } => {
                // Map 内建字段：len
                if name == "Map" && field == "len" {
                    return Ok(IrValue::Int(fields.len() as i128));
                }
                match fields.get(field) {
                    Some(fc) => Ok(ctx.cell_value(*fc).clone()),
                    None => Err(IrError::msg("NoField", format!("no field `{field}`"))),
                }
            }
            _ => Err(IrError::msg("NoField", format!("no field `{field}`"))),
        },
        IrValue::String(s) => {
            if field == "len" {
                Ok(IrValue::Int(s.len() as i128))
            } else {
                Err(IrError::msg("NoField", format!("no field `{field}`")))
            }
        }
        IrValue::Arr(c) => match &ctx.cells[*c] {
            Cell::Elems(e) => {
                if field == "len" {
                    Ok(IrValue::Int(e.len() as i128))
                } else {
                    Err(IrError::msg("NoField", format!("no field `{field}`")))
                }
            }
            _ => Err(IrError::msg("NoField", format!("no field `{field}`"))),
        },
        // 集合（G4）：Vec 委托 Arr 字段读；Map 字段读（.len）
        IrValue::Vec(c) => match &ctx.cells[*c] {
            Cell::Vec { arr, .. } => field_value(ctx, arr, field),
            _ => Err(IrError::msg("NoField", format!("no field `{field}`"))),
        },
        IrValue::Map(c) => match &ctx.cells[*c] {
            Cell::Map { fields, .. } => {
                if field == "len" {
                    Ok(IrValue::Int(fields.len() as i128))
                } else {
                    Err(IrError::msg("NoField", format!("no field `{field}`")))
                }
            }
            _ => Err(IrError::msg("NoField", format!("no field `{field}`"))),
        },
        IrValue::Slice { len, .. } => {
            if field == "len" {
                Ok(IrValue::Int(*len as i128))
            } else {
                Err(IrError::msg("NoField", format!("no field `{field}`")))
            }
        }
        _ => Err(IrError::msg("NoField", format!("no field `{field}`"))),
    }
}

/// 字段写入：仅 Class 目标（非 Class → TypeError）；字段为普通值——写入即替换
pub(crate) fn store_field(ctx: &mut Ctx, b: &IrValue, field: &str, v: IrValue) -> R<()> {
    let c = match b {
        IrValue::Class(c) => *c,
        _ => return Err(IrError::msg("TypeError", "store to non-class")),
    };
    // 先分配新字段 cell，避免在 cells 的可变借用内再次借用 ctx
    let nc = ctx.alloc(Cell::Value(v));
    match &mut ctx.cells[c] {
        Cell::Class { fields, .. } => {
            fields.insert(field.to_string(), nc);
            Ok(())
        }
        _ => Err(IrError::msg("TypeError", "store to non-class")),
    }
}

/// 索引读取：Arr/Slice 元素（克隆值）、Str 字节；越界 → IndexOutOfBounds；非可索引 → NotIndexable
pub(crate) fn index_value(ctx: &Ctx, b: &IrValue, i: usize) -> R<IrValue> {
    match b {
        IrValue::Arr(c) => match &ctx.cells[*c] {
            Cell::Elems(e) => {
                if i >= e.len() {
                    return Err(IrError::msg("IndexOutOfBounds", "index out of bounds"));
                }
                Ok(ctx.cell_value(e[i]).clone())
            }
            _ => Err(IrError::msg("NotIndexable", "not indexable")),
        },
        IrValue::String(s) => {
            let bytes = s.as_slice();
            if i >= bytes.len() {
                return Err(IrError::msg("IndexOutOfBounds", "index out of bounds"));
            }
            Ok(IrValue::Int(bytes[i] as i128))
        }
        IrValue::Slice { data, start, len } => {
            if i >= *len {
                return Err(IrError::msg("IndexOutOfBounds", "index out of bounds"));
            }
            match &ctx.cells[*data] {
                Cell::Elems(e) => Ok(ctx.cell_value(e[*start + i]).clone()),
                _ => Err(IrError::msg("NotIndexable", "not indexable")),
            }
        }
        _ => Err(IrError::msg("NotIndexable", "not indexable")),
    }
}

/// 索引写入：仅 Arr 目标（非 Arr → TypeError）；写穿元素 cell（切片/视图共享）
pub(crate) fn store_index(ctx: &mut Ctx, b: &IrValue, i: usize, v: IrValue) -> R<()> {
    let ec = match b {
        IrValue::Arr(c) => match &ctx.cells[*c] {
            Cell::Elems(e) => {
                if i >= e.len() {
                    return Err(IrError::msg("IndexOutOfBounds", "index out of bounds"));
                }
                e[i]
            }
            _ => return Err(IrError::msg("TypeError", "store to non-array")),
        },
        _ => return Err(IrError::msg("TypeError", "store to non-array")),
    };
    ctx.set_cell(ec, v);
    Ok(())
}

/// 切片：Arr → 共享视图；Str → 字节拷贝；Slice → 重切片；越界 → IndexOutOfBounds
pub(crate) fn slice_of(ctx: &Ctx, b: &IrValue, lo: usize, hi: usize, open_end: bool) -> R<IrValue> {
    match b {
        IrValue::Arr(c) => match &ctx.cells[*c] {
            Cell::Elems(e) => {
                let total = e.len();
                let h = if open_end { total } else { hi };
                if h > total || lo > total {
                    return Err(IrError::msg("IndexOutOfBounds", "slice out of bounds"));
                }
                Ok(IrValue::Slice {
                    data: *c,
                    start: lo,
                    len: h.saturating_sub(lo),
                })
            }
            _ => Err(IrError::msg("NotIndexable", "not indexable")),
        },
        IrValue::String(s) => {
            let bytes = s.as_slice();
            let h = if open_end { bytes.len() } else { hi };
            if h > bytes.len() || lo > bytes.len() {
                return Err(IrError::msg("IndexOutOfBounds", "slice out of bounds"));
            }
            Ok(IrValue::String(StringDataIr::from_bytes(
                bytes[lo..h].to_vec(),
            )))
        }
        IrValue::Slice { data, start, len } => {
            let total = *len;
            let h = if open_end { total } else { hi };
            if h > total || lo > total {
                return Err(IrError::msg("IndexOutOfBounds", "slice out of bounds"));
            }
            Ok(IrValue::Slice {
                data: *data,
                start: *start + lo,
                len: h.saturating_sub(lo),
            })
        }
        _ => Err(IrError::msg("NotIndexable", "not indexable")),
    }
}

/// 切片写回：仅 Arr 目标且仅 Set（其余 → TypeError/BadAssign，由调用方判定）；
/// 源元素从 lo 起写入目标，受目标长度约束（非 Arr 源静默无操作——对齐 oracle）。
pub(crate) fn store_slice(ctx: &mut Ctx, b: &IrValue, lo: usize, hi: usize, v: &IrValue) -> R<()> {
    let c = match b {
        IrValue::Arr(c) => *c,
        _ => return Err(IrError::msg("TypeError", "store to non-array")),
    };
    let total = ctx.elems_len(c);
    if hi > total || lo > total {
        return Err(IrError::msg("IndexOutOfBounds", "slice out of bounds"));
    }
    // 源元素值快照（先克隆，避免可变借用冲突）
    let src_vals: Vec<IrValue> = match v {
        IrValue::Arr(sc) => match &ctx.cells[*sc] {
            Cell::Elems(e) => e.iter().map(|ec| ctx.cell_value(*ec).clone()).collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    let target_cells: Vec<usize> = match &ctx.cells[c] {
        Cell::Elems(e) => e.clone(),
        _ => return Err(IrError::msg("TypeError", "store to non-array")),
    };
    for (k, sv) in src_vals.iter().enumerate() {
        if lo + k < total {
            ctx.set_cell(target_cells[lo + k], sv.clone());
        }
    }
    Ok(())
}
