//! IR switch 模式匹配（ADR-0028：自 ir/mod.rs 拆分；对齐 oracle 模式匹配语义）

use super::*;

/// 模式匹配（对齐 oracle `match_pattern`，`interp.rs:1342-1361`）：
/// subject 已 deref 一次；`Else` 不在此处理（lower 阶段识别为兜底臂）。
pub(crate) fn match_pattern(subject: &IrValue, pat: &IrPattern) -> bool {
    match (subject, pat) {
        (IrValue::Enum { variant, .. }, IrPattern::Ident(s)) => variant == s,
        (IrValue::Int(i), IrPattern::Int(s)) => *i == *s,
        (IrValue::Float(f), IrPattern::Float(s)) => *f == *s,
        (IrValue::String(st), IrPattern::Str(s)) => st.as_slice() == s.as_bytes(),
        (IrValue::Int(c), IrPattern::Char(pc)) => *c == *pc as i128,
        (IrValue::Err { name, .. }, IrPattern::Error(pe)) => name == pe,
        (IrValue::Bool(b), IrPattern::Ident(s)) => (*b && s == "true") || (!*b && s == "false"),
        (IrValue::Opt(None), IrPattern::Ident(s)) => s == "null",
        _ => false,
    }
}

/// 枚举负载捕获：subject 为 `Enum{payload:Some(p)}` → p；否则 → subject 本身
/// （对齐 oracle `exec_switch_arm` 的负载捕获分支，`interp.rs:1318-1323`）。
pub(crate) fn enum_payload(ctx: &Ctx, v: &IrValue) -> R<IrValue> {
    let v = deref_value(ctx, v).clone();
    match v {
        IrValue::Enum {
            payload: Some(p), ..
        } => Ok(*p),
        other => Ok(other),
    }
}
