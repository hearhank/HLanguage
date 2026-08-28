//! IR 深比较（ADR-0028：自 ir/mod.rs 拆分；数组/Map/Class 按值相等）

use super::*;

/// 数组深比较（元素按值比较，递归）
pub(crate) fn arr_eq(ctx: &Ctx, a: usize, b: usize) -> bool {
    let (ae, be) = match (&ctx.cells[a], &ctx.cells[b]) {
        (Cell::Elems(x), Cell::Elems(y)) => (x.clone(), y.clone()),
        _ => return false,
    };
    ae.len() == be.len()
        && ae
            .iter()
            .zip(be.iter())
            .all(|(x, y)| ctx.cell_value(*x).value_eq(ctx, ctx.cell_value(*y)))
}

/// 集合 Map 深比较（G4）：键值表按值相等（键集相同 + 每键字段值相等）
pub(crate) fn map_eq(ctx: &Ctx, a: usize, b: usize) -> bool {
    let (af, bf) = match (&ctx.cells[a], &ctx.cells[b]) {
        (Cell::Map { fields: x, .. }, Cell::Map { fields: y, .. }) => (x.clone(), y.clone()),
        _ => return false,
    };
    if af.len() != bf.len() {
        return false;
    }
    af.iter().all(|(k, fc)| {
        bf.get(k).map_or(false, |bc| {
            ctx.cell_value(*fc).value_eq(ctx, ctx.cell_value(*bc))
        })
    })
}

/// 集合 Map 与 Class("Map") 深比较（G4）：Map 字段表 vs class 字段表
pub(crate) fn map_class_eq(ctx: &Ctx, m: usize, c: usize) -> bool {
    let (fm, fc) = match (&ctx.cells[m], &ctx.cells[c]) {
        (Cell::Map { fields: x, .. }, Cell::Class { fields: y, .. }) => (x.clone(), y.clone()),
        _ => return false,
    };
    if fm.len() != fc.len() {
        return false;
    }
    fm.iter().all(|(k, mc)| {
        fc.get(k).map_or(false, |cc| {
            ctx.cell_value(*mc).value_eq(ctx, ctx.cell_value(*cc))
        })
    })
}

/// 类深比较：类型名相同 + 字段数相同 + 每字段按值相等
pub(crate) fn class_eq(ctx: &Ctx, a: usize, b: usize) -> bool {
    let (an, af) = match &ctx.cells[a] {
        Cell::Class { name, fields } => (name.clone(), fields.clone()),
        _ => return false,
    };
    let (bn, bf) = match &ctx.cells[b] {
        Cell::Class { name, fields } => (name.clone(), fields.clone()),
        _ => return false,
    };
    if an != bn || af.len() != bf.len() {
        return false;
    }
    af.iter().all(|(k, fc)| {
        bf.get(k).map_or(false, |bc| {
            ctx.cell_value(*fc).value_eq(ctx, ctx.cell_value(*bc))
        })
    })
}
