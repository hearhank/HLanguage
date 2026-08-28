//! IR 迭代器构建（ADR-0028：自 ir/mod.rs 拆分；for 循环的迭代项展开，对齐 oracle）

use super::*;

/// 展开可迭代对象为迭代项列表（对齐 oracle `iter_items`，`interp.rs:1162-1217`）：
/// - Arr/Slice：共享元素 cell，`is_ref=true`（写穿别名）
/// - Class "Map"：KV 条目新 cell（`key` 为新建 Str cell、`value` 共享源字段 cell），`is_ref=false`
/// - 其它 Class：用户 IIterable——循环调用 `{Type}.next(self)` 至 `Opt(None)`/`Void`
/// - Str：字节 Int 新 cell，`is_ref=false`
/// - 其余 → NotIterable
pub(crate) fn make_iter(
    ctx: &mut Ctx,
    module: &IrModule,
    v: &IrValue,
    depth: usize,
) -> R<Vec<IterItem>> {
    let v = deref_value(ctx, v).clone();
    match v {
        IrValue::Arr(c) => match &ctx.cells[c] {
            Cell::Elems(e) => Ok(e
                .iter()
                .map(|ec| IterItem {
                    cell: *ec,
                    is_ref: true,
                })
                .collect()),
            _ => Err(IrError::msg(
                "NotIterable",
                "array cell is not an element list",
            )),
        },
        // 集合（G4）：Vec 句柄遍历（Ptr(Vec) 一层 deref 后为 Vec——共享 Elems）
        IrValue::Vec(c) => match &ctx.cells[c] {
            Cell::Vec {
                arr: IrValue::Arr(ac),
                ..
            } => match &ctx.cells[*ac] {
                Cell::Elems(e) => Ok(e
                    .iter()
                    .map(|ec| IterItem {
                        cell: *ec,
                        is_ref: true,
                    })
                    .collect()),
                _ => Err(IrError::msg(
                    "NotIterable",
                    "vec cell is not an element list",
                )),
            },
            _ => Err(IrError::msg("NotIterable", "vec cell is corrupt")),
        },
        IrValue::Slice { data, start, len } => match &ctx.cells[data] {
            Cell::Elems(e) => Ok(e[start..start + len]
                .iter()
                .map(|ec| IterItem {
                    cell: *ec,
                    is_ref: true,
                })
                .collect()),
            _ => Err(IrError::msg(
                "NotIterable",
                "slice data is not an element list",
            )),
        },
        IrValue::Class(c) => {
            // 先克隆字段表，释放 `ctx.cells` 借用（Map 分支内需可变借用 ctx.alloc）
            let (name, fields) = match &ctx.cells[c] {
                Cell::Class { name, fields } => (name.clone(), fields.clone()),
                _ => return Err(IrError::msg("NotIterable", "class cell is corrupt")),
            };
            if name == "Map" {
                // Map 遍历：KV 条目（key/value 字段，value 共享源字段 cell——与 for |kv| 一致）
                let items: Vec<IterItem> = fields
                    .iter()
                    .map(|(k, vc)| {
                        let mut fs = HashMap::new();
                        fs.insert(
                            "key".into(),
                            ctx.alloc(Cell::Value(IrValue::String(StringDataIr::from_bytes(
                                k.clone().into_bytes(),
                            )))),
                        );
                        fs.insert("value".into(), *vc);
                        let kv = ctx.alloc(Cell::Class {
                            name: "KV".into(),
                            fields: fs,
                        });
                        IterItem {
                            cell: kv,
                            is_ref: false,
                        }
                    })
                    .collect();
                Ok(items)
            } else {
                // 用户类 IIterable：next() 返回 Opt(None)/Void——tag1 的 next 返回 ?T
                let fname = format!("{name}.next");
                let idx = pick_func(ctx, module, &fname, &[v.clone()]).ok_or_else(|| {
                    IrError::msg(
                        "NotIterable",
                        format!("type `{name}` has no `next` method (IIterable)"),
                    )
                })?;
                let self_v = v.clone();
                let mut items = Vec::new();
                loop {
                    let nv = exec_func(ctx, module, idx, &[self_v.clone()], depth + 1)?;
                    match nv {
                        IrValue::Opt(Some(inner)) => items.push(IterItem {
                            cell: ctx.alloc(Cell::Value(*inner)),
                            is_ref: false,
                        }),
                        IrValue::Opt(None) | IrValue::Void => break,
                        other => items.push(IterItem {
                            cell: ctx.alloc(Cell::Value(other)),
                            is_ref: false,
                        }),
                    }
                }
                Ok(items)
            }
        }
        // 集合（G4）：Map 句柄 → KV 条目（key/value 字段，value 原样共享 cell）
        IrValue::Map(c) => {
            let fields = match &ctx.cells[c] {
                Cell::Map { fields, .. } => fields.clone(),
                _ => return Err(IrError::msg("NotIterable", "map cell is corrupt")),
            };
            let items: Vec<IterItem> = fields
                .iter()
                .map(|(k, vc)| {
                    let mut fs = HashMap::new();
                    fs.insert(
                        "key".into(),
                        ctx.alloc(Cell::Value(IrValue::String(StringDataIr::from_bytes(
                            k.clone().into_bytes(),
                        )))),
                    );
                    fs.insert("value".into(), *vc);
                    let kv = ctx.alloc(Cell::Class {
                        name: "KV".into(),
                        fields: fs,
                    });
                    IterItem {
                        cell: kv,
                        is_ref: false,
                    }
                })
                .collect();
            Ok(items)
        }
        IrValue::String(s) => Ok(s
            .as_slice()
            .iter()
            .map(|b| IterItem {
                cell: ctx.alloc(Cell::Value(IrValue::Int(*b as i128))),
                is_ref: false,
            })
            .collect()),
        other => Err(IrError::msg(
            "NotIterable",
            format!("value of type `{}` is not iterable", type_descr(&other)),
        )),
    }
}
