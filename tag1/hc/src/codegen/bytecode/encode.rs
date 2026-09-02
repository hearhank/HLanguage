//! 字节码编码：IrModule → HBC2 二进制序列化

use super::opcode::*;
use super::*;

/// 序列化 [`IrModule`] 为 HBC2 字节码。
pub fn encode(module: &IrModule) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    push_u32(&mut out, VERSION);
    push_u32(&mut out, module.funcs.len() as u32);
    // 函数索引表：按名排序，保证编码确定性（HashMap 迭代顺序随机）。
    // 一名多候选（重载/可选参数）→ 索引数组。
    let mut entries: Vec<(&String, &Vec<usize>)> = module.func_index.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    push_u32(&mut out, entries.len() as u32);
    for (name, idxs) in entries {
        push_str(&mut out, name);
        push_u32(&mut out, idxs.len() as u32);
        for idx in idxs {
            push_u32(&mut out, *idx as u32);
        }
    }
    for f in &module.funcs {
        encode_func(&mut out, f);
    }
    // 闭包函数表（Phase 4）：与 funcs 同构，绝不进入 func_index
    push_u32(&mut out, module.closures.len() as u32);
    for f in &module.closures {
        encode_func(&mut out, f);
    }
    // 全局表（Phase 5）：声明序名字列表（cell 由运行时预分配）
    push_u32(&mut out, module.globals.len() as u32);
    for name in &module.globals {
        push_str(&mut out, name);
    }
    // 错误码表（Phase 7）：名 → 码（内建错误值与 error.X 字面量同码）
    let mut ec: Vec<(&String, &u32)> = module.error_codes.iter().collect();
    ec.sort_by(|a, b| a.0.cmp(b.0));
    push_u32(&mut out, ec.len() as u32);
    for (name, code) in ec {
        push_str(&mut out, name);
        push_u32(&mut out, *code);
    }
    // 枚举变体表（Phase 7）：枚举名 → 变体名序（@intFromEnum/@enumFromInt 运行时分派）
    let mut ev: Vec<(&String, &Vec<String>)> = module.enum_variants.iter().collect();
    ev.sort_by(|a, b| a.0.cmp(b.0));
    push_u32(&mut out, ev.len() as u32);
    for (name, variants) in ev {
        push_str(&mut out, name);
        push_u32(&mut out, variants.len() as u32);
        for v in variants {
            push_str(&mut out, v);
        }
    }
    // [continuous] 类名表（P11d，还原 IrModule.continuous——DeepCopy 运行时门）；
    // 排序保证编码确定性（HashSet 迭代顺序随机）。
    let mut cont: Vec<&String> = module.continuous.iter().collect();
    cont.sort();
    push_u32(&mut out, cont.len() as u32);
    for name in cont {
        push_str(&mut out, name);
    }
    // K1 无标签 union 表（H1/ADR-0014）：union 名 → 字段（名 + 类型，声明序）；
    // `UnionSync`/`store_field` 写路径字节重解释同步用。排序保证编码确定性。
    let mut uf: Vec<(&String, &Vec<(String, Type)>)> = module.unions.iter().collect();
    uf.sort_by(|a, b| a.0.cmp(b.0));
    push_u32(&mut out, uf.len() as u32);
    for (name, fields) in uf {
        push_str(&mut out, name);
        push_u32(&mut out, fields.len() as u32);
        for (fname, fty) in fields {
            push_str(&mut out, fname);
            encode_type(&mut out, fty);
        }
    }
    out
}

fn encode_func(out: &mut Vec<u8>, f: &IrFunc) {
    push_str(out, &f.name);
    push_u32(out, f.params.len() as u32);
    for p in &f.params {
        push_u32(out, *p as u32);
    }
    push_u32(out, f.param_ty.len() as u32);
    for t in &f.param_ty {
        encode_type(out, t);
    }
    push_u32(out, f.param_defaults.len() as u32);
    for d in &f.param_defaults {
        out.push(*d as u8);
    }
    push_u32(out, f.defaults.len() as u32);
    for d in &f.defaults {
        match d {
            Some(c) => {
                out.push(1);
                encode_const(out, c);
            }
            None => out.push(0),
        }
    }
    push_u32(out, f.n_slots as u32);
    out.push(f.is_test as u8);
    out.push(f.exported as u8);
    push_u32(out, f.body.len() as u32);
    for inst in &f.body {
        encode_inst(out, inst);
    }
}

/// Type 编码（对齐 `decode_type`；tag 0-8）
fn encode_type(out: &mut Vec<u8>, t: &Type) {
    match t {
        Type::Named(n, args) => {
            out.push(0);
            push_str(out, n);
            push_u32(out, args.len() as u32);
            for a in args {
                encode_type(out, a);
            }
        }
        Type::Ptr(inner, mut_) => {
            out.push(1);
            encode_type(out, inner);
            out.push(*mut_ as u8);
        }
        Type::Slice(inner, mut_) => {
            out.push(2);
            encode_type(out, inner);
            out.push(*mut_ as u8);
        }
        Type::Optional(inner) => {
            out.push(3);
            encode_type(out, inner);
        }
        Type::ErrorUnion(e, inner) => {
            out.push(4);
            match e {
                Some(e) => {
                    out.push(1);
                    encode_type(out, e);
                }
                None => out.push(0),
            }
            encode_type(out, inner);
        }
        Type::Tuple(items) => {
            out.push(5);
            push_u32(out, items.len() as u32);
            for i in items {
                encode_type(out, i);
            }
        }
        Type::Array(n, inner) => {
            out.push(6);
            push_u32(out, *n as u32);
            encode_type(out, inner);
        }
        Type::Infer => out.push(7),
        Type::Owned(inner) => {
            out.push(8);
            encode_type(out, inner);
        }
        // K1/ADR-0036：mut T 可写值形态——权限标注非类型身份，按 inner 编码
        Type::MutValue(inner) => encode_type(out, inner),
        // comptime_int 字面量（组 D：类型实参位置；不落 IR，防御性编码/解码对称）
        Type::ComptimeInt(v) => {
            out.push(9);
            push_u32(out, *v as u32);
        }
    }
}

fn encode_inst(out: &mut Vec<u8>, inst: &IrInst) {
    match inst {
        IrInst::Const { temp, val } => {
            out.push(0);
            push_u32(out, *temp as u32);
            encode_const(out, val);
        }
        IrInst::Load { temp, slot } => {
            out.push(1);
            push_u32(out, *temp as u32);
            push_u32(out, *slot as u32);
        }
        IrInst::Store { slot, temp } => {
            out.push(2);
            push_u32(out, *slot as u32);
            push_u32(out, *temp as u32);
        }
        IrInst::Bin { op, temp, a, b } => {
            out.push(3);
            out.push(binop_tag(*op));
            push_u32(out, *temp as u32);
            push_u32(out, *a as u32);
            push_u32(out, *b as u32);
        }
        IrInst::Un { op, temp, a } => {
            out.push(4);
            out.push(unop_tag(*op));
            push_u32(out, *temp as u32);
            push_u32(out, *a as u32);
        }
        IrInst::Jump { label } => {
            out.push(5);
            push_u32(out, *label as u32);
        }
        IrInst::JumpIf { temp, label } => {
            out.push(6);
            push_u32(out, *temp as u32);
            push_u32(out, *label as u32);
        }
        IrInst::JumpIfNot { temp, label } => {
            out.push(7);
            push_u32(out, *temp as u32);
            push_u32(out, *label as u32);
        }
        IrInst::JumpIfNull { temp, label } => {
            out.push(8);
            push_u32(out, *temp as u32);
            push_u32(out, *label as u32);
        }
        IrInst::Label { id } => {
            out.push(9);
            push_u32(out, *id as u32);
        }
        IrInst::Call { name, args, temp } => {
            out.push(10);
            push_str(out, name);
            push_u32(out, args.len() as u32);
            for a in args {
                push_u32(out, *a as u32);
            }
            push_u32(out, *temp as u32);
        }
        IrInst::CallBuiltin { name, args, temp } => {
            out.push(11);
            push_str(out, name);
            push_u32(out, args.len() as u32);
            for a in args {
                push_u32(out, *a as u32);
            }
            push_u32(out, *temp as u32);
        }
        IrInst::JumpIfErr { temp, label } => {
            out.push(12);
            push_u32(out, *temp as u32);
            push_u32(out, *label as u32);
        }
        IrInst::Return { temp } => {
            out.push(13);
            push_u32(out, *temp as u32);
        }
        IrInst::ReturnVoid => {
            out.push(14);
        }
        // Phase 1 指针：opcode 15-18
        IrInst::AddrSlot { temp, slot } => {
            out.push(15);
            push_u32(out, *temp as u32);
            push_u32(out, *slot as u32);
        }
        IrInst::AddrValue { temp, value } => {
            out.push(16);
            push_u32(out, *temp as u32);
            push_u32(out, *value as u32);
        }
        IrInst::Deref { temp, a } => {
            out.push(17);
            push_u32(out, *temp as u32);
            push_u32(out, *a as u32);
        }
        IrInst::StorePtr { target, value } => {
            out.push(18);
            push_u32(out, *target as u32);
            push_u32(out, *value as u32);
        }
        // Phase 2 聚合：opcode 19-30
        IrInst::Field { temp, base, field } => {
            out.push(19);
            push_u32(out, *temp as u32);
            push_u32(out, *base as u32);
            push_str(out, field);
        }
        IrInst::StoreField { base, field, value } => {
            out.push(20);
            push_u32(out, *base as u32);
            push_str(out, field);
            push_u32(out, *value as u32);
        }
        IrInst::Index { temp, base, index } => {
            out.push(21);
            push_u32(out, *temp as u32);
            push_u32(out, *base as u32);
            push_u32(out, *index as u32);
        }
        IrInst::StoreIndex { base, index, value } => {
            out.push(22);
            push_u32(out, *base as u32);
            push_u32(out, *index as u32);
            push_u32(out, *value as u32);
        }
        IrInst::SliceOf { temp, base, lo, hi } => {
            out.push(23);
            push_u32(out, *temp as u32);
            push_u32(out, *base as u32);
            push_u32(out, *lo as u32);
            push_u32(out, *hi as u32);
        }
        IrInst::StoreSlice {
            base,
            lo,
            hi,
            value,
        } => {
            out.push(24);
            push_u32(out, *base as u32);
            push_u32(out, *lo as u32);
            push_u32(out, *hi as u32);
            push_u32(out, *value as u32);
        }
        IrInst::MakeArr { temp, items } => {
            out.push(25);
            push_u32(out, *temp as u32);
            push_u32(out, items.len() as u32);
            for it in items {
                push_u32(out, *it as u32);
            }
        }
        IrInst::MakeClass { temp, ty, fields } => {
            out.push(26);
            push_u32(out, *temp as u32);
            push_str(out, ty);
            push_u32(out, fields.len() as u32);
            for (k, v) in fields {
                push_str(out, k);
                push_u32(out, *v as u32);
            }
        }
        IrInst::UnionSync { class, written } => {
            out.push(48);
            push_u32(out, *class as u32);
            push_str(out, written);
        }
        IrInst::MakeEnum {
            temp,
            name,
            variant,
            payload,
        } => {
            out.push(27);
            push_u32(out, *temp as u32);
            push_str(out, name);
            push_str(out, variant);
            match payload {
                Some(p) => {
                    out.push(1);
                    push_u32(out, *p as u32);
                }
                None => out.push(0),
            }
        }
        IrInst::Destructure { value, slots } => {
            out.push(28);
            push_u32(out, *value as u32);
            push_u32(out, slots.len() as u32);
            for s in slots {
                match s {
                    Some(s) => {
                        out.push(1);
                        push_u32(out, *s as u32);
                    }
                    None => out.push(0),
                }
            }
        }
        IrInst::Move { temp, a } => {
            out.push(29);
            push_u32(out, *temp as u32);
            push_u32(out, *a as u32);
        }
        IrInst::Unwrap { temp, a } => {
            out.push(30);
            push_u32(out, *temp as u32);
            push_u32(out, *a as u32);
        }
        // Phase 3 switch / 区间 / for：opcode 31-36
        IrInst::MatchTest {
            temp,
            subject,
            pattern,
        } => {
            out.push(31);
            push_u32(out, *temp as u32);
            push_u32(out, *subject as u32);
            encode_pattern(out, pattern);
        }
        IrInst::MakeRange { temp, lo, hi } => {
            out.push(32);
            push_u32(out, *temp as u32);
            push_u32(out, *lo as u32);
            push_u32(out, *hi as u32);
        }
        IrInst::EnumPayload { temp, a } => {
            out.push(33);
            push_u32(out, *temp as u32);
            push_u32(out, *a as u32);
        }
        IrInst::IterMake { temp, base } => {
            out.push(34);
            push_u32(out, *temp as u32);
            push_u32(out, *base as u32);
        }
        IrInst::IterNext {
            has,
            iter,
            slot,
            read_only,
        } => {
            out.push(35);
            push_u32(out, *has as u32);
            push_u32(out, *iter as u32);
            push_u32(out, *slot as u32);
            out.push(*read_only as u8);
        }
        IrInst::IterWriteBack { iter, slot } => {
            out.push(36);
            push_u32(out, *iter as u32);
            push_u32(out, *slot as u32);
        }
        // Phase 4 闭包 / 函数引用 / 方法 / 动态调用：opcode 37-40
        IrInst::MakeClosure {
            temp,
            func,
            captures,
            is_move,
            is_mut,
        } => {
            out.push(37);
            push_u32(out, *temp as u32);
            push_u32(out, *func as u32);
            push_u32(out, captures.len() as u32);
            for (name, slot) in captures {
                push_str(out, name);
                push_u32(out, *slot as u32);
            }
            out.push(*is_move as u8);
            out.push(*is_mut as u8);
        }
        IrInst::FnRef { temp, name } => {
            out.push(38);
            push_u32(out, *temp as u32);
            push_str(out, name);
        }
        IrInst::CallIndirect { temp, callee, args } => {
            out.push(39);
            push_u32(out, *temp as u32);
            push_u32(out, *callee as u32);
            push_u32(out, args.len() as u32);
            for a in args {
                push_u32(out, *a as u32);
            }
        }
        IrInst::CallMethod {
            temp,
            base,
            method,
            args,
        } => {
            out.push(40);
            push_u32(out, *temp as u32);
            push_u32(out, *base as u32);
            push_str(out, method);
            push_u32(out, args.len() as u32);
            for a in args {
                push_u32(out, *a as u32);
            }
        }
        // Phase 5 global/const：opcode 41-42
        IrInst::LoadGlobal { temp, name } => {
            out.push(41);
            push_u32(out, *temp as u32);
            push_str(out, name);
        }
        IrInst::StoreGlobal { name, value } => {
            out.push(42);
            push_str(out, name);
            push_u32(out, *value as u32);
        }
        IrInst::GlobalAddr { temp, name } => {
            out.push(43);
            push_u32(out, *temp as u32);
            push_str(out, name);
        }
        // Phase 6 defer/errdefer：opcode 44-46
        IrInst::PushDefer { id } => {
            out.push(44);
            push_u32(out, *id as u32);
        }
        IrInst::JumpIfNotDefer { id, label } => {
            out.push(45);
            push_u32(out, *id as u32);
            push_u32(out, *label as u32);
        }
        IrInst::PopDefer { id } => {
            out.push(46);
            push_u32(out, *id as u32);
        }
        // P11d [continuous] 值语义：opcode 47
        IrInst::DeepCopy { temp, a } => {
            out.push(47);
            push_u32(out, *temp as u32);
            push_u32(out, *a as u32);
        }
    }
}

/// switch 模式编码（对齐 `decode_pattern`；tag 0-5）
fn encode_pattern(out: &mut Vec<u8>, pat: &IrPattern) {
    match pat {
        IrPattern::Error(s) => {
            out.push(0);
            push_str(out, s);
        }
        IrPattern::Ident(s) => {
            out.push(1);
            push_str(out, s);
        }
        IrPattern::Int(i) => {
            out.push(2);
            out.extend_from_slice(&i.to_le_bytes());
        }
        IrPattern::Float(f) => {
            out.push(3);
            out.extend_from_slice(&f.to_le_bytes());
        }
        IrPattern::Str(s) => {
            out.push(4);
            push_str(out, s);
        }
        IrPattern::Char(c) => {
            out.push(5);
            // D11（ADR-0037）：char 码点 u32（LE 4 字节，格式升级与 decode 对称）
            out.extend_from_slice(&c.to_le_bytes());
        }
    }
}

fn encode_const(out: &mut Vec<u8>, val: &IrConst) {
    match val {
        IrConst::Int(i) => {
            out.push(T_INT);
            out.extend_from_slice(&i.to_le_bytes());
        }
        IrConst::Float(f) => {
            out.push(T_FLOAT);
            out.extend_from_slice(&f.to_le_bytes());
        }
        IrConst::Bool(b) => {
            out.push(T_BOOL);
            out.push(*b as u8);
        }
        IrConst::Str(s) => {
            out.push(T_STR);
            push_str(out, s);
        }
        IrConst::Void => out.push(T_VOID),
        IrConst::Null => out.push(T_NULL),
        IrConst::Err { name, code } => {
            out.push(T_ERR);
            push_str(out, name);
            push_u32(out, *code);
        }
        IrConst::End => out.push(T_END),
    }
}

pub(super) fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub(super) fn push_str(out: &mut Vec<u8>, s: &str) {
    push_u32(out, s.len() as u32);
    out.extend_from_slice(s.as_bytes());
}
