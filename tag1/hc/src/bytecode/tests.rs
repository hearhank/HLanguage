//! 字节码编解码单元测试。

use super::encode::{push_str, push_u32};
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
        exported: false,
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
            IrInst::StorePtr {
                target: 7,
                value: 1,
            },
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
            // K1 union：UnionSync（opcode 48）紧随 MakeClass 构造
            IrInst::UnionSync {
                class: 7,
                written: "i".to_string(),
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
                is_mut: true,
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
            // Phase 6 defer/errdefer
            IrInst::PushDefer { id: 0 },
            IrInst::JumpIfNotDefer { id: 0, label: 3 },
            IrInst::PopDefer { id: 0 },
            // P11d [continuous] 值语义
            IrInst::DeepCopy { temp: 7, a: 0 },
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
        exported: true,
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
        exported: false,
        body: vec![IrInst::Return { temp: 1 }],
    };
    let mut error_codes = HashMap::new();
    error_codes.insert("error.FileNotFound".to_string(), 1u32);
    error_codes.insert("error.OutOfMemory".to_string(), 2u32);
    let mut enum_variants = HashMap::new();
    enum_variants.insert(
        "Kind".to_string(),
        vec!["A".to_string(), "B".to_string(), "C".to_string()],
    );
    let mut unions = HashMap::new();
    unions.insert(
        "Num".to_string(),
        vec![
            ("i".to_string(), Type::Named("i32".into(), vec![])),
            ("f".to_string(), Type::Named("f32".into(), vec![])),
        ],
    );
    IrModule {
        funcs: vec![f0, f1],
        closures: vec![c0],
        func_index,
        globals: vec!["g_counter".to_string(), "g_name".to_string()],
        error_codes,
        enum_variants,
        continuous: HashSet::from(["Point".to_string()]),
        unions,
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
    // K5：exported 标志往返（仅影响原生符号层，运行时透明）
    assert!(!d.funcs[0].exported);
    assert!(d.funcs[1].exported);
    assert_eq!(d.funcs[0].body.len(), m.funcs[0].body.len());
    assert_eq!(d.funcs[1].params, Vec::<usize>::new());
    assert_eq!(d.closures[0].name, "<closure>");
    // [continuous] 类名表（P11d）往返
    assert_eq!(d.continuous, m.continuous);
    assert!(d.continuous.contains("Point"));
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
    bytes.push(0); // exported
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
    bytes.push(0); // exported
    push_u32(&mut bytes, 1); // n_insts
    bytes.push(3); // Bin
    bytes.push(0xFF); // 非法 binop 标签
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0); // n_closures
    assert!(decode(&bytes).unwrap_err().contains("binop"));
}
