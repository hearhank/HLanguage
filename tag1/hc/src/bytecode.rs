//! M3.2 字节码 VM：`IrModule` 的紧凑序列化（HBC2）+ 装载执行。
//!
//! 字节码是共享 IR（ADR-0004 唯一语义源）的**序列化**——执行复用
//! [`crate::ir::run_ir`]，不另写 dispatch 循环，禁止各后端私语义。
//! 覆盖范围 = M3.1 切片（标量 / 控制流 / 函数调用 / 错误值通道）
//! + Phase 1 指针（取址 / 解引用 / 写穿）+ Phase 2 聚合（字段/索引/字面量/
//! 解构/move/unwrap）+ Phase 3 switch + range + for（opcode 31–36 + 模式描述符）。
//! 子集外特性同 `ir::lower` 以 `error.Unsupported` 硬错误拒绝。
//!
//! 格式（全小端）：
//! ```text
//! magic "HBC2" · u32 version · u32 n_funcs
//! 函数索引表（还原 IrModule.func_index）: u32 n_entries · { name · u32 n_idx · {u32 idx}* }*
//! 函数 × n_funcs: name · u32 n_params · {u32 param}* · Type×n_param_ty
//!   · u8×n_param_defaults · (present? Const)* × n_defaults · u32 n_slots · u8 is_test
//!   · u32 n_insts · { opcode u8 · 操作数 }*
//! 闭包表（Phase 4）: u32 n_closures · 函数 × n_closures（同上格式）
//! ```
//! 常量载荷保留全精度：`i128` 16 字节、`f64` 8 字节、字符串长度前缀。

use crate::ast::Type;
use crate::ir::{run_ir, IrBinOp, IrConst, IrError, IrFunc, IrInst, IrModule, IrPattern, IrUnOp, IrValue};
use std::collections::HashMap;

pub const MAGIC: [u8; 4] = *b"HBC2";
pub const VERSION: u32 = 2;

// ---------- 常量 / 运算符 / 指令 标签 ----------

const T_INT: u8 = 0;
const T_FLOAT: u8 = 1;
const T_BOOL: u8 = 2;
const T_STR: u8 = 3;
const T_VOID: u8 = 4;
const T_NULL: u8 = 5;
const T_ERR: u8 = 6;
const T_END: u8 = 7;

fn binop_tag(op: IrBinOp) -> u8 {
    use IrBinOp::*;
    match op {
        Add => 0,
        Sub => 1,
        Mul => 2,
        Div => 3,
        Mod => 4,
        EucMod => 5,
        BitAnd => 6,
        BitOr => 7,
        BitXor => 8,
        Shl => 9,
        Shr => 10,
        Eq => 11,
        Ne => 12,
        Lt => 13,
        Le => 14,
        Gt => 15,
        Ge => 16,
    }
}

fn binop_from(tag: u8) -> Result<IrBinOp, String> {
    use IrBinOp::*;
    Ok(match tag {
        0 => Add,
        1 => Sub,
        2 => Mul,
        3 => Div,
        4 => Mod,
        5 => EucMod,
        6 => BitAnd,
        7 => BitOr,
        8 => BitXor,
        9 => Shl,
        10 => Shr,
        11 => Eq,
        12 => Ne,
        13 => Lt,
        14 => Le,
        15 => Gt,
        16 => Ge,
        _ => return Err(format!("未知 binop 标签 {tag}")),
    })
}

fn unop_tag(op: IrUnOp) -> u8 {
    match op {
        IrUnOp::Neg => 0,
        IrUnOp::Not => 1,
        IrUnOp::BitNot => 2,
    }
}

fn unop_from(tag: u8) -> Result<IrUnOp, String> {
    Ok(match tag {
        0 => IrUnOp::Neg,
        1 => IrUnOp::Not,
        2 => IrUnOp::BitNot,
        _ => return Err(format!("未知 unop 标签 {tag}")),
    })
}

// ---------- 编码 ----------

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
        IrInst::StoreSlice { base, lo, hi, value } => {
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
        IrInst::MakeEnum { temp, name, variant, payload } => {
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
            out.push(*c);
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

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_str(out: &mut Vec<u8>, s: &str) {
    push_u32(out, s.len() as u32);
    out.extend_from_slice(s.as_bytes());
}

// ---------- 解码 ----------

/// 反序列化 HBC2 字节码为 [`IrModule`]；格式损坏返回描述性错误。
pub fn decode(bytes: &[u8]) -> Result<IrModule, String> {
    let mut r = Reader::new(bytes);
    if r.bytes(4)? != MAGIC.as_slice() {
        return Err("不是 HBC2 字节码（魔数不匹配）".into());
    }
    let version = r.u32()?;
    if version != VERSION {
        return Err(format!("不支持的字节码版本 {version}（当前 {VERSION}）"));
    }
    let n_funcs = r.u32()? as usize;
    let n_entries = r.u32()? as usize;
    let mut func_index = HashMap::with_capacity(n_entries);
    for _ in 0..n_entries {
        let name = r.str()?;
        let n_idxs = r.u32()? as usize;
        let mut idxs = Vec::with_capacity(n_idxs);
        for _ in 0..n_idxs {
            idxs.push(r.u32()? as usize);
        }
        func_index.insert(name, idxs);
    }
    let mut funcs = Vec::with_capacity(n_funcs);
    for _ in 0..n_funcs {
        funcs.push(decode_func(&mut r)?);
    }
    let n_closures = r.u32()? as usize;
    let mut closures = Vec::with_capacity(n_closures);
    for _ in 0..n_closures {
        closures.push(decode_func(&mut r)?);
    }
    Ok(IrModule {
        funcs,
        closures,
        func_index,
    })
}

fn decode_func(r: &mut Reader) -> Result<IrFunc, String> {
    let name = r.str()?;
    let n_params = r.u32()? as usize;
    let mut params = Vec::with_capacity(n_params);
    for _ in 0..n_params {
        params.push(r.u32()? as usize);
    }
    let n_pty = r.u32()? as usize;
    let mut param_ty = Vec::with_capacity(n_pty);
    for _ in 0..n_pty {
        param_ty.push(decode_type(r)?);
    }
    let n_pdef = r.u32()? as usize;
    let mut param_defaults = Vec::with_capacity(n_pdef);
    for _ in 0..n_pdef {
        param_defaults.push(r.u8()? != 0);
    }
    let n_defs = r.u32()? as usize;
    let mut defaults = Vec::with_capacity(n_defs);
    for _ in 0..n_defs {
        if r.u8()? != 0 {
            defaults.push(Some(decode_const(r)?));
        } else {
            defaults.push(None);
        }
    }
    let n_slots = r.u32()? as usize;
    let is_test = r.u8()? != 0;
    let n_insts = r.u32()? as usize;
    let mut body = Vec::with_capacity(n_insts);
    for _ in 0..n_insts {
        body.push(decode_inst(r)?);
    }
    Ok(IrFunc {
        name,
        params,
        param_ty,
        param_defaults,
        defaults,
        n_slots,
        body,
        is_test,
    })
}

/// Type 解码（对齐 `encode_type`；tag 0-8）
fn decode_type(r: &mut Reader) -> Result<Type, String> {
    Ok(match r.u8()? {
        0 => Type::Named(r.str()?, {
            let n = r.u32()? as usize;
            let mut args = Vec::with_capacity(n);
            for _ in 0..n {
                args.push(decode_type(r)?);
            }
            args
        }),
        1 => Type::Ptr(
            Box::new(decode_type(r)?),
            r.u8()? != 0,
        ),
        2 => Type::Slice(
            Box::new(decode_type(r)?),
            r.u8()? != 0,
        ),
        3 => Type::Optional(Box::new(decode_type(r)?)),
        4 => {
            let e = if r.u8()? != 0 {
                Some(Box::new(decode_type(r)?))
            } else {
                None
            };
            Type::ErrorUnion(e, Box::new(decode_type(r)?))
        }
        5 => {
            let n = r.u32()? as usize;
            let mut items = Vec::with_capacity(n);
            for _ in 0..n {
                items.push(decode_type(r)?);
            }
            Type::Tuple(items)
        }
        6 => Type::Array(r.u32()? as usize, Box::new(decode_type(r)?)),
        7 => Type::Infer,
        8 => Type::Owned(Box::new(decode_type(r)?)),
        other => return Err(format!("未知 Type 标签 {other}")),
    })
}

fn decode_inst(r: &mut Reader) -> Result<IrInst, String> {
    let op = r.u8()?;
    Ok(match op {
        0 => IrInst::Const {
            temp: r.u32()? as usize,
            val: decode_const(r)?,
        },
        1 => IrInst::Load {
            temp: r.u32()? as usize,
            slot: r.u32()? as usize,
        },
        2 => IrInst::Store {
            slot: r.u32()? as usize,
            temp: r.u32()? as usize,
        },
        3 => IrInst::Bin {
            op: binop_from(r.u8()?)?,
            temp: r.u32()? as usize,
            a: r.u32()? as usize,
            b: r.u32()? as usize,
        },
        4 => IrInst::Un {
            op: unop_from(r.u8()?)?,
            temp: r.u32()? as usize,
            a: r.u32()? as usize,
        },
        5 => IrInst::Jump {
            label: r.u32()? as usize,
        },
        6 => IrInst::JumpIf {
            temp: r.u32()? as usize,
            label: r.u32()? as usize,
        },
        7 => IrInst::JumpIfNot {
            temp: r.u32()? as usize,
            label: r.u32()? as usize,
        },
        8 => IrInst::JumpIfNull {
            temp: r.u32()? as usize,
            label: r.u32()? as usize,
        },
        9 => IrInst::Label {
            id: r.u32()? as usize,
        },
        10 => IrInst::Call {
            name: r.str()?,
            args: read_usize_vec(r)?,
            temp: r.u32()? as usize,
        },
        11 => IrInst::CallBuiltin {
            name: r.str()?,
            args: read_usize_vec(r)?,
            temp: r.u32()? as usize,
        },
        12 => IrInst::JumpIfErr {
            temp: r.u32()? as usize,
            label: r.u32()? as usize,
        },
        13 => IrInst::Return {
            temp: r.u32()? as usize,
        },
        14 => IrInst::ReturnVoid,
        // Phase 1 指针：opcode 15-18
        15 => IrInst::AddrSlot {
            temp: r.u32()? as usize,
            slot: r.u32()? as usize,
        },
        16 => IrInst::AddrValue {
            temp: r.u32()? as usize,
            value: r.u32()? as usize,
        },
        17 => IrInst::Deref {
            temp: r.u32()? as usize,
            a: r.u32()? as usize,
        },
        18 => IrInst::StorePtr {
            target: r.u32()? as usize,
            value: r.u32()? as usize,
        },
        // Phase 2 聚合：opcode 19-30
        19 => IrInst::Field {
            temp: r.u32()? as usize,
            base: r.u32()? as usize,
            field: r.str()?,
        },
        20 => IrInst::StoreField {
            base: r.u32()? as usize,
            field: r.str()?,
            value: r.u32()? as usize,
        },
        21 => IrInst::Index {
            temp: r.u32()? as usize,
            base: r.u32()? as usize,
            index: r.u32()? as usize,
        },
        22 => IrInst::StoreIndex {
            base: r.u32()? as usize,
            index: r.u32()? as usize,
            value: r.u32()? as usize,
        },
        23 => IrInst::SliceOf {
            temp: r.u32()? as usize,
            base: r.u32()? as usize,
            lo: r.u32()? as usize,
            hi: r.u32()? as usize,
        },
        24 => IrInst::StoreSlice {
            base: r.u32()? as usize,
            lo: r.u32()? as usize,
            hi: r.u32()? as usize,
            value: r.u32()? as usize,
        },
        25 => IrInst::MakeArr {
            temp: r.u32()? as usize,
            items: read_usize_vec(r)?,
        },
        26 => IrInst::MakeClass {
            temp: r.u32()? as usize,
            ty: r.str()?,
            fields: {
                let n = r.u32()? as usize;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    let k = r.str()?;
                    let val = r.u32()? as usize;
                    v.push((k, val));
                }
                v
            },
        },
        27 => IrInst::MakeEnum {
            temp: r.u32()? as usize,
            name: r.str()?,
            variant: r.str()?,
            payload: if r.u8()? != 0 {
                Some(r.u32()? as usize)
            } else {
                None
            },
        },
        28 => IrInst::Destructure {
            value: r.u32()? as usize,
            slots: {
                let n = r.u32()? as usize;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    if r.u8()? != 0 {
                        v.push(Some(r.u32()? as usize));
                    } else {
                        v.push(None);
                    }
                }
                v
            },
        },
        29 => IrInst::Move {
            temp: r.u32()? as usize,
            a: r.u32()? as usize,
        },
        30 => IrInst::Unwrap {
            temp: r.u32()? as usize,
            a: r.u32()? as usize,
        },
        // Phase 3 switch / 区间 / for：opcode 31-36
        31 => IrInst::MatchTest {
            temp: r.u32()? as usize,
            subject: r.u32()? as usize,
            pattern: decode_pattern(r)?,
        },
        32 => IrInst::MakeRange {
            temp: r.u32()? as usize,
            lo: r.u32()? as usize,
            hi: r.u32()? as usize,
        },
        33 => IrInst::EnumPayload {
            temp: r.u32()? as usize,
            a: r.u32()? as usize,
        },
        34 => IrInst::IterMake {
            temp: r.u32()? as usize,
            base: r.u32()? as usize,
        },
        35 => IrInst::IterNext {
            has: r.u32()? as usize,
            iter: r.u32()? as usize,
            slot: r.u32()? as usize,
            read_only: r.u8()? != 0,
        },
        36 => IrInst::IterWriteBack {
            iter: r.u32()? as usize,
            slot: r.u32()? as usize,
        },
        37 => IrInst::MakeClosure {
            temp: r.u32()? as usize,
            func: r.u32()? as usize,
            captures: {
                let n = r.u32()? as usize;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push((r.str()?, r.u32()? as usize));
                }
                v
            },
            is_move: r.u8()? != 0,
        },
        38 => IrInst::FnRef {
            temp: r.u32()? as usize,
            name: r.str()?,
        },
        39 => IrInst::CallIndirect {
            temp: r.u32()? as usize,
            callee: r.u32()? as usize,
            args: {
                let n = r.u32()? as usize;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(r.u32()? as usize);
                }
                v
            },
        },
        40 => IrInst::CallMethod {
            temp: r.u32()? as usize,
            base: r.u32()? as usize,
            method: r.str()?,
            args: {
                let n = r.u32()? as usize;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(r.u32()? as usize);
                }
                v
            },
        },
        _ => return Err(format!("未知指令 opcode {op}")),
    })
}

/// switch 模式解码（对齐 `encode_pattern`；tag 0-5）
fn decode_pattern(r: &mut Reader) -> Result<IrPattern, String> {
    let tag = r.u8()?;
    Ok(match tag {
        0 => IrPattern::Error(r.str()?),
        1 => IrPattern::Ident(r.str()?),
        2 => IrPattern::Int(r.i128()?),
        3 => IrPattern::Float(r.f64()?),
        4 => IrPattern::Str(r.str()?),
        5 => IrPattern::Char(r.u8()?),
        _ => return Err(format!("未知 switch 模式 tag {tag}")),
    })
}

fn decode_const(r: &mut Reader) -> Result<IrConst, String> {
    let tag = r.u8()?;
    Ok(match tag {
        T_INT => IrConst::Int(r.i128()?),
        T_FLOAT => IrConst::Float(r.f64()?),
        T_BOOL => IrConst::Bool(r.u8()? != 0),
        T_STR => IrConst::Str(r.str()?),
        T_VOID => IrConst::Void,
        T_NULL => IrConst::Null,
        T_ERR => IrConst::Err {
            name: r.str()?,
            code: r.u32()?,
        },
        T_END => IrConst::End,
        _ => return Err(format!("未知常量标签 {tag}")),
    })
}

fn read_usize_vec(r: &mut Reader) -> Result<Vec<usize>, String> {
    let n = r.u32()? as usize;
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(r.u32()? as usize);
    }
    Ok(v)
}

/// 字节码装载执行：`decode` + 复用 [`run_ir`]（唯一语义源）。
pub fn run_bytecode(bytes: &[u8], entry: &str, args: &[IrValue]) -> Result<IrValue, IrError> {
    let module = decode(bytes).map_err(|msg| IrError::msg("BadBytecode", msg))?;
    run_ir(&module, entry, args)
}

// ---------- 字节读取器 ----------

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.pos + n > self.data.len() {
            return Err("字节码截断".into());
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.bytes(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, String> {
        let mut arr = [0u8; 4];
        arr.copy_from_slice(self.bytes(4)?);
        Ok(u32::from_le_bytes(arr))
    }

    fn i128(&mut self) -> Result<i128, String> {
        let mut arr = [0u8; 16];
        arr.copy_from_slice(self.bytes(16)?);
        Ok(i128::from_le_bytes(arr))
    }

    fn f64(&mut self) -> Result<f64, String> {
        let mut arr = [0u8; 8];
        arr.copy_from_slice(self.bytes(8)?);
        Ok(f64::from_le_bytes(arr))
    }

    fn str(&mut self) -> Result<String, String> {
        let len = self.u32()? as usize;
        let b = self.bytes(len)?;
        String::from_utf8(b.to_vec()).map_err(|_| "非法 UTF-8 字符串".to_string())
    }
}

// ---------- 单元测试 ----------

#[cfg(test)]
mod tests {
    use super::*;

    /// 手工构造覆盖全部指令 + 全部常量标签 + 全部 binop/unop 标签 + 闭包表的模块
    fn exhaustive_module() -> IrModule {
        let mut func_index = HashMap::new();
        func_index.insert("main".to_string(), vec![0]);
        func_index.insert("m.f".to_string(), vec![1]);
        func_index.insert("m.g".to_string(), vec![0, 1]); // 重载双候选
        let f0 = IrFunc {
            name: "main".to_string(),
            params: vec![0, 1],
            param_ty: vec![Type::Named("i32".into(), vec![]), Type::Infer],
            param_defaults: vec![false, true],
            defaults: vec![None, Some(IrConst::Int(42))],
            n_slots: 8,
            is_test: false,
            body: vec![
                IrInst::Const {
                    temp: 0,
                    val: IrConst::Int(-9_223_372_036_854_775_807i128),
                },
                IrInst::Const {
                    temp: 1,
                    val: IrConst::Float(3.141592653589793),
                },
                IrInst::Const {
                    temp: 2,
                    val: IrConst::Bool(true),
                },
                IrInst::Const {
                    temp: 3,
                    val: IrConst::Str("héllo→世界".to_string()),
                },
                IrInst::Const {
                    temp: 4,
                    val: IrConst::Void,
                },
                IrInst::Const {
                    temp: 5,
                    val: IrConst::Null,
                },
                IrInst::Const {
                    temp: 6,
                    val: IrConst::Err {
                        name: "NotFound".to_string(),
                        code: 0,
                    },
                },
                IrInst::Const {
                    temp: 7,
                    val: IrConst::End,
                },
                IrInst::Load { temp: 7, slot: 0 },
                IrInst::Store { slot: 0, temp: 7 },
                // Phase 1 指针：AddrSlot/AddrValue/Deref/StorePtr
                IrInst::AddrSlot { temp: 7, slot: 1 },
                IrInst::AddrValue { temp: 7, value: 1 },
                IrInst::Deref { temp: 7, a: 1 },
                IrInst::StorePtr { target: 7, value: 1 },
                IrInst::Bin {
                    op: IrBinOp::Add,
                    temp: 7,
                    a: 0,
                    b: 1,
                },
                IrInst::Bin {
                    op: IrBinOp::Ge,
                    temp: 7,
                    a: 0,
                    b: 1,
                },
                IrInst::Un {
                    op: IrUnOp::BitNot,
                    temp: 7,
                    a: 0,
                },
                IrInst::Jump { label: 1 },
                IrInst::JumpIf { temp: 2, label: 1 },
                IrInst::JumpIfNot { temp: 2, label: 1 },
                IrInst::JumpIfNull { temp: 5, label: 1 },
                IrInst::JumpIfErr { temp: 6, label: 1 },
                IrInst::Label { id: 1 },
                IrInst::Call {
                    name: "m.f".to_string(),
                    args: vec![0, 1],
                    temp: 7,
                },
                IrInst::CallBuiltin {
                    name: "expect_eq".to_string(),
                    args: vec![0, 1],
                    temp: 7,
                },
                // Phase 2 聚合：Field/StoreField/Index/StoreIndex/SliceOf/StoreSlice/
                // MakeArr/MakeClass/MakeEnum/Destructure/Move/Unwrap
                IrInst::Field {
                    temp: 7,
                    base: 0,
                    field: "len".to_string(),
                },
                IrInst::StoreField {
                    base: 0,
                    field: "x".to_string(),
                    value: 7,
                },
                IrInst::Index {
                    temp: 7,
                    base: 0,
                    index: 1,
                },
                IrInst::StoreIndex {
                    base: 0,
                    index: 1,
                    value: 7,
                },
                IrInst::SliceOf {
                    temp: 7,
                    base: 0,
                    lo: 0,
                    hi: 7,
                },
                IrInst::StoreSlice {
                    base: 0,
                    lo: 0,
                    hi: 2,
                    value: 7,
                },
                IrInst::MakeArr {
                    temp: 7,
                    items: vec![0, 1],
                },
                IrInst::MakeClass {
                    temp: 7,
                    ty: "Rect".to_string(),
                    fields: vec![("w".to_string(), 0), ("h".to_string(), 1)],
                },
                IrInst::MakeEnum {
                    temp: 7,
                    name: "Color".to_string(),
                    variant: "Red".to_string(),
                    payload: Some(0),
                },
                IrInst::Destructure {
                    value: 7,
                    slots: vec![Some(0), None, Some(1)],
                },
                IrInst::Move { temp: 7, a: 0 },
                IrInst::Unwrap { temp: 7, a: 0 },
                IrInst::Return { temp: 7 },
                IrInst::ReturnVoid,
                // Phase 4 闭包 / 函数引用 / 方法 / 动态调用
                IrInst::MakeClosure {
                    temp: 7,
                    func: 0,
                    captures: vec![("x".to_string(), 0), ("y".to_string(), 1)],
                    is_move: true,
                },
                IrInst::FnRef {
                    temp: 7,
                    name: "m.g".to_string(),
                },
                IrInst::CallIndirect {
                    temp: 7,
                    callee: 7,
                    args: vec![0, 1],
                },
                IrInst::CallMethod {
                    temp: 7,
                    base: 0,
                    method: "area".to_string(),
                    args: vec![1],
                },
            ],
        };
        let f1 = IrFunc {
            name: "m.f".to_string(),
            params: vec![],
            param_ty: vec![],
            param_defaults: vec![],
            defaults: vec![],
            n_slots: 1,
            is_test: true,
            body: vec![IrInst::ReturnVoid],
        };
        let c0 = IrFunc {
            name: "<closure>".to_string(),
            params: vec![0, 1],
            param_ty: vec![Type::Infer, Type::Named("i32".into(), vec![])],
            param_defaults: vec![false, false],
            defaults: vec![None, None],
            n_slots: 2,
            is_test: false,
            body: vec![IrInst::Return { temp: 1 }],
        };
        IrModule {
            funcs: vec![f0, f1],
            closures: vec![c0],
            func_index,
        }
    }

    #[test]
    fn round_trip_is_identity() {
        let m = exhaustive_module();
        let bytes = encode(&m);
        // encode → decode → encode 字节级等价（覆盖全部字段/标签）
        assert_eq!(encode(&decode(&bytes).expect("decode")), bytes);
        // 编码确定性：两次 encode 输出一致
        assert_eq!(encode(&m), bytes);
    }

    #[test]
    fn decode_round_trip_reconstructs_structure() {
        let m = exhaustive_module();
        let d = decode(&encode(&m)).expect("decode");
        assert_eq!(d.funcs.len(), 2);
        assert_eq!(d.closures.len(), 1);
        assert_eq!(d.func_index, m.func_index);
        assert_eq!(d.funcs[0].name, "main");
        assert_eq!(d.funcs[0].params, vec![0, 1]);
        assert_eq!(d.funcs[0].param_ty[0], Type::Named("i32".into(), vec![]));
        assert_eq!(d.funcs[0].param_defaults, vec![false, true]);
        assert_eq!(d.funcs[0].defaults[1], Some(IrConst::Int(42)));
        assert_eq!(d.funcs[0].n_slots, 8);
        assert!(!d.funcs[0].is_test);
        assert!(d.funcs[1].is_test);
        assert_eq!(d.funcs[0].body.len(), m.funcs[0].body.len());
        assert_eq!(d.funcs[1].params, Vec::<usize>::new());
        assert_eq!(d.closures[0].name, "<closure>");
    }

    #[test]
    fn decode_rejects_bad_magic() {
        let mut bytes = encode(&exhaustive_module());
        bytes[0] = b'X';
        assert!(decode(&bytes).unwrap_err().contains("魔数"));
    }

    #[test]
    fn decode_rejects_bad_version() {
        let mut bytes = encode(&exhaustive_module());
        // version 位于偏移 4..8
        bytes[4] = 0xFF;
        bytes[5] = 0xFF;
        assert!(decode(&bytes).unwrap_err().contains("版本"));
    }

    #[test]
    fn decode_rejects_truncation() {
        let bytes = encode(&exhaustive_module());
        for cut in [0, 1, 4, 8, bytes.len() / 2] {
            assert!(decode(&bytes[..cut]).is_err(), "截断 {cut} 字节应报错");
        }
    }

    #[test]
    fn decode_rejects_unknown_opcode() {
        // 手工：单函数、单条指令，opcode 非法（闭包表在函数之后——整体覆盖最后
        // 一字节不再命中 opcode，改为精确构造）
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC);
        push_u32(&mut bytes, VERSION);
        push_u32(&mut bytes, 1); // n_funcs
        push_u32(&mut bytes, 1); // n_entries
        push_str(&mut bytes, "main");
        push_u32(&mut bytes, 1); // n_idx
        push_u32(&mut bytes, 0);
        push_str(&mut bytes, "main");
        push_u32(&mut bytes, 0); // n_params
        push_u32(&mut bytes, 0); // n_param_ty
        push_u32(&mut bytes, 0); // n_param_defaults
        push_u32(&mut bytes, 0); // n_defaults
        push_u32(&mut bytes, 1); // n_slots
        bytes.push(0); // is_test
        push_u32(&mut bytes, 1); // n_insts
        bytes.push(0xFF); // 非法 opcode
        push_u32(&mut bytes, 0); // n_closures
        assert!(decode(&bytes).unwrap_err().contains("opcode"));
    }

    #[test]
    fn decode_rejects_unknown_binop_tag() {
        // 手工：单函数、单条 Bin 指令，binop 标签非法
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC);
        push_u32(&mut bytes, VERSION);
        push_u32(&mut bytes, 1); // n_funcs
        push_u32(&mut bytes, 1); // n_entries
        push_str(&mut bytes, "main");
        push_u32(&mut bytes, 1); // n_idx
        push_u32(&mut bytes, 0);
        push_str(&mut bytes, "main");
        push_u32(&mut bytes, 0); // n_params
        push_u32(&mut bytes, 0); // n_param_ty
        push_u32(&mut bytes, 0); // n_param_defaults
        push_u32(&mut bytes, 0); // n_defaults
        push_u32(&mut bytes, 2); // n_slots
        bytes.push(0); // is_test
        push_u32(&mut bytes, 1); // n_insts
        bytes.push(3); // Bin
        bytes.push(0xFF); // 非法 binop 标签
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0); // n_closures
        assert!(decode(&bytes).unwrap_err().contains("binop"));
    }
}
