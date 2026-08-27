//! 字节码解码：HBC2 二进制 → IrModule 反序列化与执行
//!
//! 定义：结构体：Reader

use super::opcode::*;
use super::*;

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
    // 全局表（Phase 5）：声明序名字列表
    let n_globals = r.u32()? as usize;
    let mut globals = Vec::with_capacity(n_globals);
    for _ in 0..n_globals {
        globals.push(r.str()?);
    }
    // 错误码表（Phase 7）：名 → 码
    let n_ec = r.u32()? as usize;
    let mut error_codes = HashMap::with_capacity(n_ec);
    for _ in 0..n_ec {
        let name = r.str()?;
        let code = r.u32()?;
        error_codes.insert(name, code);
    }
    // 枚举变体表（Phase 7）：枚举名 → 变体名序
    let n_ev = r.u32()? as usize;
    let mut enum_variants = HashMap::with_capacity(n_ev);
    for _ in 0..n_ev {
        let name = r.str()?;
        let n_v = r.u32()? as usize;
        let mut variants = Vec::with_capacity(n_v);
        for _ in 0..n_v {
            variants.push(r.str()?);
        }
        enum_variants.insert(name, variants);
    }
    // [continuous] 类名表（P11d）：DeepCopy 指令运行时门
    let n_cont = r.u32()? as usize;
    let mut continuous = HashSet::with_capacity(n_cont);
    for _ in 0..n_cont {
        continuous.insert(r.str()?);
    }
    // K1 无标签 union 表（H1/ADR-0014）：union 名 → 字段（名 + 类型，声明序）
    let n_unions = r.u32()? as usize;
    let mut unions = HashMap::with_capacity(n_unions);
    for _ in 0..n_unions {
        let name = r.str()?;
        let n_f = r.u32()? as usize;
        let mut fields = Vec::with_capacity(n_f);
        for _ in 0..n_f {
            let fname = r.str()?;
            let fty = decode_type(&mut r)?;
            fields.push((fname, fty));
        }
        unions.insert(name, fields);
    }
    Ok(IrModule {
        funcs,
        closures,
        func_index,
        globals,
        error_codes,
        enum_variants,
        continuous,
        unions,
        type_implements: HashMap::new(),
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
    let exported = r.u8()? != 0;
    let n_insts = r.u32()? as usize;
    let mut body = Vec::with_capacity(n_insts);
    for _ in 0..n_insts {
        body.push(decode_inst(r)?);
    }
    Ok(IrFunc {
        name,
        params,
        param_ty,
        ret_ty: Type::Named("void".to_string(), vec![]),
        param_defaults,
        defaults,
        n_slots,
        body,
        is_test,
        exported,
        is_extern: false,
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
        1 => Type::Ptr(Box::new(decode_type(r)?), r.u8()? != 0),
        2 => Type::Slice(Box::new(decode_type(r)?), r.u8()? != 0),
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
        9 => Type::ComptimeInt(r.u32()? as usize),
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
        48 => IrInst::UnionSync {
            class: r.u32()? as usize,
            written: r.str()?,
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
            is_mut: r.u8()? != 0,
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
        // Phase 5 global/const：opcode 41-42
        41 => IrInst::LoadGlobal {
            temp: r.u32()? as usize,
            name: r.str()?,
        },
        42 => IrInst::StoreGlobal {
            name: r.str()?,
            value: r.u32()? as usize,
        },
        43 => IrInst::GlobalAddr {
            temp: r.u32()? as usize,
            name: r.str()?,
        },
        44 => IrInst::PushDefer {
            id: r.u32()? as usize,
        },
        45 => IrInst::JumpIfNotDefer {
            id: r.u32()? as usize,
            label: r.u32()? as usize,
        },
        46 => IrInst::PopDefer {
            id: r.u32()? as usize,
        },
        47 => IrInst::DeepCopy {
            temp: r.u32()? as usize,
            a: r.u32()? as usize,
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
            return Err("字节码意外截断".into());
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.bytes(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, String> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn i128(&mut self) -> Result<i128, String> {
        let b = self.bytes(16)?;
        let mut arr = [0u8; 16];
        arr.copy_from_slice(b);
        Ok(i128::from_le_bytes(arr))
    }

    fn f64(&mut self) -> Result<f64, String> {
        let b = self.bytes(8)?;
        let mut arr = [0u8; 8];
        arr.copy_from_slice(b);
        Ok(f64::from_le_bytes(arr))
    }

    fn str(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        let s = self.bytes(n)?;
        String::from_utf8(s.to_vec()).map_err(|_| "非法 UTF-8 字符串".to_string())
    }
}
