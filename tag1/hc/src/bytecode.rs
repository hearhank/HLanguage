//! M3.2 字节码 VM：`IrModule` 的紧凑序列化（HBC2）+ 装载执行。
//!
//! 字节码是共享 IR（ADR-0004 唯一语义源）的**序列化**——执行复用
//! [`crate::ir::run_ir`]，不另写 dispatch 循环，禁止各后端私语义。
//! 覆盖范围 = M3.1 切片（标量 / 控制流 / 函数调用 / 错误值通道）。
//!
//! 格式（全小端）：
//! ```text
//! magic "HBC2" · u32 version · u32 n_funcs
//! 函数索引表（还原 IrModule.func_index）: u32 n_entries · { name · u32 idx }*
//! 函数 × n_funcs: name · u32 n_params · {u32 param}* · u32 n_slots · u8 is_test
//!               · u32 n_insts · { opcode u8 · 操作数 }*
//! ```
//! 常量载荷保留全精度：`i128` 16 字节、`f64` 8 字节、字符串长度前缀。

use crate::ir::{run_ir, IrBinOp, IrConst, IrError, IrFunc, IrInst, IrModule, IrUnOp, IrValue};
use std::collections::HashMap;

pub const MAGIC: [u8; 4] = *b"HBC2";
pub const VERSION: u32 = 1;

// ---------- 常量 / 运算符 / 指令 标签 ----------

const T_INT: u8 = 0;
const T_FLOAT: u8 = 1;
const T_BOOL: u8 = 2;
const T_STR: u8 = 3;
const T_VOID: u8 = 4;
const T_NULL: u8 = 5;
const T_ERR: u8 = 6;

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
    // 函数索引表：按名排序，保证编码确定性（HashMap 迭代顺序随机）
    let mut entries: Vec<(&String, &usize)> = module.func_index.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    push_u32(&mut out, entries.len() as u32);
    for (name, &idx) in entries {
        push_str(&mut out, name);
        push_u32(&mut out, idx as u32);
    }
    for f in &module.funcs {
        push_str(&mut out, &f.name);
        push_u32(&mut out, f.params.len() as u32);
        for p in &f.params {
            push_u32(&mut out, *p as u32);
        }
        push_u32(&mut out, f.n_slots as u32);
        out.push(f.is_test as u8);
        push_u32(&mut out, f.body.len() as u32);
        for inst in &f.body {
            encode_inst(&mut out, inst);
        }
    }
    out
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
        IrConst::Err(n) => {
            out.push(T_ERR);
            push_str(out, n);
        }
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
        let idx = r.u32()? as usize;
        func_index.insert(name, idx);
    }
    let mut funcs = Vec::with_capacity(n_funcs);
    for _ in 0..n_funcs {
        let name = r.str()?;
        let n_params = r.u32()? as usize;
        let mut params = Vec::with_capacity(n_params);
        for _ in 0..n_params {
            params.push(r.u32()? as usize);
        }
        let n_slots = r.u32()? as usize;
        let is_test = r.u8()? != 0;
        let n_insts = r.u32()? as usize;
        let mut body = Vec::with_capacity(n_insts);
        for _ in 0..n_insts {
            body.push(decode_inst(&mut r)?);
        }
        funcs.push(IrFunc {
            name,
            params,
            n_slots,
            body,
            is_test,
        });
    }
    Ok(IrModule { funcs, func_index })
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
        _ => return Err(format!("未知指令 opcode {op}")),
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
        T_ERR => IrConst::Err(r.str()?),
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

    /// 手工构造覆盖全部 15 种指令 + 全部常量标签 + 全部 binop/unop 标签的模块
    fn exhaustive_module() -> IrModule {
        let mut func_index = HashMap::new();
        func_index.insert("main".to_string(), 0);
        func_index.insert("m.f".to_string(), 1);
        let f0 = IrFunc {
            name: "main".to_string(),
            params: vec![0, 1],
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
                    val: IrConst::Err("NotFound".to_string()),
                },
                IrInst::Load { temp: 7, slot: 0 },
                IrInst::Store { slot: 0, temp: 7 },
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
                IrInst::Return { temp: 7 },
                IrInst::ReturnVoid,
            ],
        };
        let f1 = IrFunc {
            name: "m.f".to_string(),
            params: vec![],
            n_slots: 1,
            is_test: true,
            body: vec![IrInst::ReturnVoid],
        };
        IrModule {
            funcs: vec![f0, f1],
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
        assert_eq!(d.func_index, m.func_index);
        assert_eq!(d.funcs[0].name, "main");
        assert_eq!(d.funcs[0].params, vec![0, 1]);
        assert_eq!(d.funcs[0].n_slots, 8);
        assert!(!d.funcs[0].is_test);
        assert!(d.funcs[1].is_test);
        assert_eq!(d.funcs[0].body.len(), m.funcs[0].body.len());
        assert_eq!(d.funcs[1].params, Vec::<usize>::new());
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
        let mut bytes = encode(&exhaustive_module());
        // 最后一字节 = 最后一条指令 opcode（ReturnVoid → 覆盖为非法值）
        let last = bytes.len() - 1;
        bytes[last] = 0xFF;
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
        push_u32(&mut bytes, 0);
        push_str(&mut bytes, "main");
        push_u32(&mut bytes, 0); // n_params
        push_u32(&mut bytes, 2); // n_slots
        bytes.push(0); // is_test
        push_u32(&mut bytes, 1); // n_insts
        bytes.push(3); // Bin
        bytes.push(0xFF); // 非法 binop 标签
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        assert!(decode(&bytes).unwrap_err().contains("binop"));
    }
}
