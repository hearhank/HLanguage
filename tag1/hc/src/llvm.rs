//! M3.3 LLVM 原生后端（emit-.ll 文本 + 外部 `zig cc` 驱动）
//!
//! 与 M3.1 IR 参考解释器（`ir::run_ir`）共用 `IrModule`（ADR-0004 唯一语义源），
//! 逐条对齐 `exec_func` 的动态语义。IR 槽是无类型的 `IrValue`，首轮用**统一带标签
//! 值表示** `%Value = { i32 tag, i128 data }`（正确性优先），动态运算集中到导言 helper，
//! 避免每个 `Bin` 内联 tag-dispatch。i128 载荷修复 i64 截断；浮点位模式存低 64 位。
//!
//! 覆盖（tag1 切片）：标量 / 控制流 / 函数调用 / 错误值通道 / 断言内建
//! + Phase 1 指针（`&`/`&mut` 取址 → `%Value` tag 7 载荷 = 槽地址；`p.*` 解引用；
//! 写穿经 `hc_store_ptr`）。指针比较：同指针身份相等（`hc_eq`），
//! Ptr 与普通值比较时先解引用（对齐 `IrValue::value_eq`）；`<` 仅 Ptr/Ptr 按地址序。
//! 已知简化见 07-bootstrap-plan.md：NUL 结尾字符串字面量、
//! 无优化 pass、硬错误消息依赖 libc `puts`/`exit`。

use crate::errorcodes::ErrorCodeTable;
use crate::ir::{IrBinOp, IrConst, IrFunc, IrInst, IrModule, IrUnOp};
use std::collections::HashMap;
use std::fmt::Write as _;

// ---------- 值 tag 常量（与 IrValue 动态分派对应） ----------

const T_VOID: i32 = 0;
const T_NULL: i32 = 1;
const T_INT: i32 = 2;
const T_FLOAT: i32 = 3;
const T_BOOL: i32 = 4;
const T_STR: i32 = 5;
const T_ERR: i32 = 6;
const T_PTR: i32 = 7;
// ---- Phase 2 聚合 ----
const T_ARR: i32 = 8;
const T_SLICE: i32 = 9;
const T_CLASS: i32 = 10;
const T_ENUM: i32 = 11;
const T_END: i32 = 12;

/// 生成完整 `.ll` 模块文本（导言 + 每个 `IrFunc` + `main` 包装）。
pub fn codegen(module: &IrModule, errors: &ErrorCodeTable) -> String {
    let strings = collect_strings(module);
    let mut canon: HashMap<String, String> = HashMap::new();
    for (name, &idx) in &module.func_index {
        canon.insert(name.clone(), format!("hc_fn{idx}"));
    }
    let mut out = String::new();
    emit_preamble(&mut out, &strings);
    for (idx, f) in module.funcs.iter().enumerate() {
        emit_func(&mut out, f, idx, &strings, errors, &canon);
    }
    emit_main_wrapper(&mut out, module);
    out
}

/// 生成「测试驱动」`.ll` 模块文本（导言 + 每个 `IrFunc` + `test fn` 跑器 main，Q-T5）。
/// 与 [`codegen`] 同导言与函数发射，仅入口包装从 `main` 换成 [`emit_test_runner`]。
pub fn codegen_tests(module: &IrModule, errors: &ErrorCodeTable) -> String {
    let strings = collect_strings(module);
    let mut canon: HashMap<String, String> = HashMap::new();
    for (name, &idx) in &module.func_index {
        canon.insert(name.clone(), format!("hc_fn{idx}"));
    }
    let mut out = String::new();
    emit_preamble(&mut out, &strings);
    for (idx, f) in module.funcs.iter().enumerate() {
        emit_func(&mut out, f, idx, &strings, errors, &canon);
    }
    emit_test_runner(&mut out, module);
    out
}

/// 收集全部字符串常量（去重、保序）。除 `Str` 常量外，还收集 Phase 2 指令携带的
/// 字面量字段名 / 类型名 / 枚举名变体名——它们需要以模块级全局字符串形式供 helper 取地址。
fn collect_strings(module: &IrModule) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<String> = Vec::new();
    for f in &module.funcs {
        for inst in &f.body {
            match inst {
                IrInst::Const { val: IrConst::Str(s), .. } => push_str(s, &mut seen, &mut out),
                IrInst::Field { field, .. } | IrInst::StoreField { field, .. } => {
                    push_str(field, &mut seen, &mut out)
                }
                IrInst::MakeClass { ty, fields, .. } => {
                    push_str(ty, &mut seen, &mut out);
                    for (n, _) in fields {
                        push_str(n, &mut seen, &mut out);
                    }
                }
                IrInst::MakeEnum { name, variant, .. } => {
                    push_str(name, &mut seen, &mut out);
                    push_str(variant, &mut seen, &mut out);
                }
                _ => {}
            }
        }
    }
    out
}

/// 去重入表。
fn push_str(s: &str, seen: &mut HashMap<String, usize>, out: &mut Vec<String>) {
    if !seen.contains_key(s) {
        seen.insert(s.to_string(), out.len());
        out.push(s.to_string());
    }
}

/// 字符串全局下标与长度（GEP 指令发射用；LLVM 18+ 无常量表达式 GEP）。
fn str_idx(strings: &[String], s: &str) -> (usize, usize) {
    let idx = strings.iter().position(|x| x == s).unwrap_or(0);
    (idx, s.len() + 1)
}

/// 模板替换（避免 `format!` 对 LLVM `{`/`}` 的转义噪音）。
fn tpl(template: &str, subs: &[(&str, &str)]) -> String {
    let mut s = template.to_string();
    for (k, v) in subs {
        s = s.replace(k, v);
    }
    s
}

/// LLVM `c"..."` 字符串转义：可打印 ASCII 原样，其余 `\XX` 十六进制。
fn llvm_escape(bytes: &[u8]) -> String {
    let mut s = String::new();
    for &b in bytes {
        if (0x20..0x7e).contains(&b) && b != b'"' && b != b'\\' {
            s.push(b as char);
        } else {
            s.push_str(&format!("\\{:02X}", b));
        }
    }
    s
}

struct Msg {
    key: &'static str,
    text: &'static str,
}

const MSGS: &[Msg] = &[
    Msg { key: "overflow", text: "error.Overflow: integer overflow" },
    Msg { key: "divzero", text: "error.DivisionByZero" },
    Msg { key: "assert", text: "error.AssertFailed" },
    Msg { key: "nofunc", text: "error.NoFunction" },
    Msg { key: "typeerr", text: "error.TypeError" },
    Msg { key: "badassign", text: "error.BadAssign" },
    Msg { key: "unhandled", text: "error: unhandled error value reached entry point" },
    // Phase 2 聚合运行时硬错误（对齐 tree-walking RtError 名称）
    Msg { key: "oom", text: "error.OutOfMemory" },
    Msg { key: "indexoob", text: "error.IndexOutOfBounds" },
    Msg { key: "badindex", text: "error.BadIndex" },
    Msg { key: "notindexable", text: "error.NotIndexable" },
    Msg { key: "nullunwrap", text: "error.NullUnwrap" },
    Msg { key: "nofield", text: "error.NoField" },
    Msg { key: "tuplearity", text: "error.TupleArity" },
];

// ---------- 导言 ----------

fn emit_preamble(out: &mut String, strings: &[String]) {
    out.push_str("; H M3.3 LLVM 原生后端（自动生成；`zig cc file.ll -o file.exe`）\n\n");
    out.push_str("%Value = type { i32, i128 }\n");
    // Phase 2 聚合堆对象（聚合 `%Value` 的 data = 堆对象指针）
    out.push_str("%ArrObj = type { i64, %Value* }\n");
    out.push_str("%SliceObj = type { %Value*, i64, i64 }\n");
    out.push_str("%Field = type { i8*, %Value }\n");
    out.push_str("%ClassObj = type { i8*, i64, %Field* }\n");
    out.push_str("%EnumObj = type { i8*, i8*, %Value* }\n");
    out.push_str("%SeqInfo = type { %Value*, i64, i64 }\n");
    out.push_str("%FindRes = type { i1, %Value }\n\n");

    // 外部符号（libc + 溢出内建）
    out.push_str("declare i32 @strcmp(i8*, i8*)\n");
    out.push_str("declare i32 @puts(i8*)\n");
    out.push_str("declare void @exit(i32) noreturn\n");
    out.push_str("declare i64 @strlen(i8*)\n");
    out.push_str("declare noalias i8* @malloc(i64)\n");
    out.push_str("declare noalias i8* @realloc(i8*, i64)\n");
    out.push_str("declare void @llvm.memcpy.p0i8.p0i8.i64(i8*, i8*, i64, i1)\n");
    out.push_str("declare { i128, i1 } @llvm.sadd.with.overflow.i128(i128, i128)\n");
    out.push_str("declare { i128, i1 } @llvm.ssub.with.overflow.i128(i128, i128)\n");
    out.push_str("declare { i128, i1 } @llvm.smul.with.overflow.i128(i128, i128)\n\n");

    // 断言失败标志（全局；单线程顺序执行）
    out.push_str("@hc_fail_msg = global i8* null\n");
    out.push_str("@.void_value = private unnamed_addr constant %Value { i32 0, i128 0 }\n");
    out.push_str("@.empty_str_s = private unnamed_addr constant [1 x i8] c\"\\00\"\n");
    // `.len` 内建字段名字符串（`hc_field` 对 Str/Arr/Slice 判定用）
    out.push_str("@.hc_len = private unnamed_addr constant [4 x i8] c\"len\\00\"\n\n");

    // 硬错误消息全局
    for m in MSGS {
        let n = m.text.len() + 1;
        let esc = llvm_escape(m.text.as_bytes());
        let _ = writeln!(out, "@.msg_{} = private unnamed_addr constant [{n} x i8] c\"{esc}\\00\"", m.key);
    }
    out.push('\n');

    // 字符串常量全局（去重后）
    for (i, s) in strings.iter().enumerate() {
        let n = s.len() + 1;
        let esc = llvm_escape(s.as_bytes());
        let _ = writeln!(out, "@.str.{i} = private unnamed_addr constant [{n} x i8] c\"{esc}\\00\"");
    }
    out.push('\n');

    // 中止 + 各硬错误无参包装
    out.push_str("define void @hc_abort(i8* %msg) {\n  call i32 @puts(i8* %msg)\n  call void @exit(i32 1)\n  unreachable\n}\n\n");
    for m in MSGS {
        let n = m.text.len() + 1;
        let _ = writeln!(out, "define void @hc_abort_{}() {{", m.key);
        let _ = writeln!(out, "  %p = getelementptr inbounds [{n} x i8], ptr @.msg_{}, i64 0, i64 0", m.key);
        let _ = writeln!(out, "  call void @hc_abort(i8* %p)");
        out.push_str("  unreachable\n}\n\n");
    }
    // 切片外函数调用（运行时 NoFunction 硬错误）
    out.push_str("define %Value @hc_no_function() {\n  call void @hc_abort_nofunc()\n  unreachable\n}\n\n");

    emit_arith_helpers(out);
    emit_bit_helpers(out);
    emit_cmp_helpers(out);
    emit_unary_helpers(out);
    emit_assert_helpers(out);
    emit_pointer_helpers(out);
    emit_aggregate_helpers(out);
}

// ---------- 算术 helper（加/减/乘/除/模/欧几里得模） ----------

const TPL_OVERFLOW: &str = r#"define %Value @FNAME@(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %ai = icmp eq i32 %ta, 2
  %bi = icmp eq i32 %tb, 2
  %both = and i1 %ai, %bi
  br i1 %both, label %int_op, label %chk_float
chk_float:
  %af = icmp eq i32 %ta, 3
  %bf = icmp eq i32 %tb, 3
  %any = or i1 %af, %bf
  br i1 %any, label %float_op, label %other
int_op:
  %res = call { i128, i1 } @INTRINSIC@(i128 %da, i128 %db)
  %rv = extractvalue { i128, i1 } %res, 0
  %ov = extractvalue { i128, i1 } %res, 1
  br i1 %ov, label %ovf, label %int_ok
int_ok:
  %i0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %i1 = insertvalue %Value %i0, i128 %rv, 1
  ret %Value %i1
ovf:
  call void @hc_abort_overflow()
  unreachable
float_op:
  %da_t = trunc i128 %da to i64
  %fa_raw = bitcast i64 %da_t to double
  %fa_int = sitofp i128 %da to double
  %fa = select i1 %af, double %fa_raw, double %fa_int
  %db_t = trunc i128 %db to i64
  %fb_raw = bitcast i64 %db_t to double
  %fb_int = sitofp i128 %db to double
  %fb = select i1 %bf, double %fb_raw, double %fb_int
  %fr = @FOP@ double %fa, %fb
  %fr_bits64 = bitcast double %fr to i64
  %fr_bits = zext i64 %fr_bits64 to i128
  %f0 = insertvalue %Value { i32 0, i128 0 }, i32 3, 0
  %f1 = insertvalue %Value %f0, i128 %fr_bits, 1
  ret %Value %f1
other:
  ret %Value { i32 2, i128 0 }
}
"#;

const TPL_DIVMOD: &str = r#"define %Value @FNAME@(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %ai = icmp eq i32 %ta, 2
  %bi = icmp eq i32 %tb, 2
  %both = and i1 %ai, %bi
  br i1 %both, label %int_op, label %chk_float
chk_float:
  %af = icmp eq i32 %ta, 3
  %bf = icmp eq i32 %tb, 3
  %any = or i1 %af, %bf
  br i1 %any, label %float_op, label %other
int_op:
  %bz = icmp eq i128 %db, 0
  br i1 %bz, label %divzero, label %int_ok
int_ok:
  %rv = @IOP@ i128 %da, %db
  %i0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %i1 = insertvalue %Value %i0, i128 %rv, 1
  ret %Value %i1
divzero:
  call void @hc_abort_divzero()
  unreachable
float_op:
  %da_t = trunc i128 %da to i64
  %fa_raw = bitcast i64 %da_t to double
  %fa_int = sitofp i128 %da to double
  %fa = select i1 %af, double %fa_raw, double %fa_int
  %db_t = trunc i128 %db to i64
  %fb_raw = bitcast i64 %db_t to double
  %fb_int = sitofp i128 %db to double
  %fb = select i1 %bf, double %fb_raw, double %fb_int
  %fr = @FOP@ double %fa, %fb
  %fr_bits64 = bitcast double %fr to i64
  %fr_bits = zext i64 %fr_bits64 to i128
  %f0 = insertvalue %Value { i32 0, i128 0 }, i32 3, 0
  %f1 = insertvalue %Value %f0, i128 %fr_bits, 1
  ret %Value %f1
other:
  ret %Value { i32 2, i128 0 }
}
"#;

const TPL_EUCMOD: &str = r#"define %Value @hc_eucmod(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %ai = icmp eq i32 %ta, 2
  %bi = icmp eq i32 %tb, 2
  %both = and i1 %ai, %bi
  br i1 %both, label %int_op, label %chk_float
chk_float:
  %af = icmp eq i32 %ta, 3
  %bf = icmp eq i32 %tb, 3
  %any = or i1 %af, %bf
  br i1 %any, label %float_op, label %other
int_op:
  %bz = icmp eq i128 %db, 0
  br i1 %bz, label %divzero, label %int_ok
int_ok:
  %rm = srem i128 %da, %db
  %rneg = icmp slt i128 %rm, 0
  %dbneg = icmp slt i128 %db, 0
  %dbnegv = sub i128 0, %db
  %mabs = select i1 %dbneg, i128 %dbnegv, i128 %db
  %rm2 = add i128 %rm, %mabs
  %rv = select i1 %rneg, i128 %rm2, i128 %rm
  %i0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %i1 = insertvalue %Value %i0, i128 %rv, 1
  ret %Value %i1
divzero:
  call void @hc_abort_divzero()
  unreachable
float_op:
  %da_t = trunc i128 %da to i64
  %fa_raw = bitcast i64 %da_t to double
  %fa_int = sitofp i128 %da to double
  %fa = select i1 %af, double %fa_raw, double %fa_int
  %db_t = trunc i128 %db to i64
  %fb_raw = bitcast i64 %db_t to double
  %fb_int = sitofp i128 %db to double
  %fb = select i1 %bf, double %fb_raw, double %fb_int
  %fr = frem double %fa, %fb
  %fr_bits64 = bitcast double %fr to i64
  %fr_bits = zext i64 %fr_bits64 to i128
  %f0 = insertvalue %Value { i32 0, i128 0 }, i32 3, 0
  %f1 = insertvalue %Value %f0, i128 %fr_bits, 1
  ret %Value %f1
other:
  ret %Value { i32 2, i128 0 }
}
"#;

fn emit_arith_helpers(out: &mut String) {
    for (fname, intr, fop) in [
        ("hc_add", "llvm.sadd.with.overflow.i128", "fadd"),
        ("hc_sub", "llvm.ssub.with.overflow.i128", "fsub"),
        ("hc_mul", "llvm.smul.with.overflow.i128", "fmul"),
    ] {
        let fname = format!("@{fname}");
        let intr = format!("@{intr}");
        out.push_str(&tpl(TPL_OVERFLOW, &[("@FNAME@", &fname), ("@INTRINSIC@", &intr), ("@FOP@", fop)]));
        out.push('\n');
    }
    for (fname, iop, fop) in [
        ("hc_div", "sdiv", "fdiv"),
        ("hc_mod", "srem", "frem"),
    ] {
        let fname = format!("@{fname}");
        out.push_str(&tpl(TPL_DIVMOD, &[("@FNAME@", &fname), ("@IOP@", iop), ("@FOP@", fop)]));
        out.push('\n');
    }
    out.push_str(TPL_EUCMOD);
    out.push_str("\n\n");
}

// ---------- 位运算 helper（and/or/xor/shift） ----------

const TPL_BITOP: &str = r#"define %Value @FNAME@(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %ai = icmp eq i32 %ta, 2
  %bi = icmp eq i32 %tb, 2
  %both = and i1 %ai, %bi
  br i1 %both, label %do, label %other
do:
  %r = @BOP@ i128 %da, %db
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %r, 1
  ret %Value %v1
other:
  ret %Value { i32 2, i128 0 }
}
"#;

const TPL_SHIFT: &str = r#"define %Value @FNAME@(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %ai = icmp eq i32 %ta, 2
  %bi = icmp eq i32 %tb, 2
  %both = and i1 %ai, %bi
  br i1 %both, label %do, label %other
do:
  %sh = and i128 %db, 127
  %r = @SHOP@ i128 %da, %sh
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %r, 1
  ret %Value %v1
other:
  ret %Value { i32 2, i128 0 }
}
"#;

fn emit_bit_helpers(out: &mut String) {
    for (fname, bop) in [
        ("hc_bitand", "and"),
        ("hc_bitor", "or"),
        ("hc_bitxor", "xor"),
    ] {
        let fname = format!("@{fname}");
        out.push_str(&tpl(TPL_BITOP, &[("@FNAME@", &fname), ("@BOP@", bop)]));
        out.push('\n');
    }
    for (fname, shop) in [("hc_shl", "shl"), ("hc_shr", "ashr")] {
        let fname = format!("@{fname}");
        out.push_str(&tpl(TPL_SHIFT, &[("@FNAME@", &fname), ("@SHOP@", shop)]));
        out.push('\n');
    }
    out.push('\n');
}

// ---------- 比较 / 真值 helper ----------

/// 纯值相等（非指针操作数）；指针由 [`HC_EQ_DISPATCH`] 先归一化/分流后进入。
const HC_EQ_PLAIN: &str = r#"define i1 @hc_eq_plain(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %a_int = icmp eq i32 %ta, 2
  %b_int = icmp eq i32 %tb, 2
  %ii_case = and i1 %a_int, %b_int
  %ii_eq = icmp eq i128 %da, %db
  %ii_res = and i1 %ii_case, %ii_eq
  %b_float = icmp eq i32 %tb, 3
  %if_case = and i1 %a_int, %b_float
  %if_fa = sitofp i128 %da to double
  %db_t = trunc i128 %db to i64
  %if_fb = bitcast i64 %db_t to double
  %if_eq = fcmp oeq double %if_fa, %if_fb
  %if_res = and i1 %if_case, %if_eq
  %a_float = icmp eq i32 %ta, 3
  %fi_case = and i1 %a_float, %b_int
  %da_t = trunc i128 %da to i64
  %fi_fa = bitcast i64 %da_t to double
  %fi_fb = sitofp i128 %db to double
  %fi_eq = fcmp oeq double %fi_fa, %fi_fb
  %fi_res = and i1 %fi_case, %fi_eq
  %ff_case = and i1 %a_float, %b_float
  %ff_eq = fcmp oeq double %fi_fa, %if_fb
  %ff_res = and i1 %ff_case, %ff_eq
  %a_bool = icmp eq i32 %ta, 4
  %b_bool = icmp eq i32 %tb, 4
  %bb_case = and i1 %a_bool, %b_bool
  %bb_eq = icmp eq i128 %da, %db
  %bb_res = and i1 %bb_case, %bb_eq
  %a_str = icmp eq i32 %ta, 5
  %b_str = icmp eq i32 %tb, 5
  %ss_case = and i1 %a_str, %b_str
  %ss_pa = inttoptr i128 %da to i8*
  %ss_pb = inttoptr i128 %db to i8*
  %ss_es = getelementptr inbounds [1 x i8], ptr @.empty_str_s, i64 0, i64 0
  %ss_pa_s = select i1 %ss_case, i8* %ss_pa, i8* %ss_es
  %ss_pb_s = select i1 %ss_case, i8* %ss_pb, i8* %ss_es
  %ss_cmp = call i32 @strcmp(i8* %ss_pa_s, i8* %ss_pb_s)
  %ss_eq = icmp eq i32 %ss_cmp, 0
  %ss_res = and i1 %ss_case, %ss_eq
  %a_null = icmp eq i32 %ta, 1
  %b_null = icmp eq i32 %tb, 1
  %nn_res = and i1 %a_null, %b_null
  %a_void = icmp eq i32 %ta, 0
  %b_void = icmp eq i32 %tb, 0
  %vv_res = and i1 %a_void, %b_void
  %a_err = icmp eq i32 %ta, 6
  %b_err = icmp eq i32 %tb, 6
  %ee_case = and i1 %a_err, %b_err
  %ee_eq = icmp eq i128 %da, %db
  %ee_res = and i1 %ee_case, %ee_eq
  %r1 = or i1 %ii_res, %if_res
  %r2 = or i1 %r1, %fi_res
  %r3 = or i1 %r2, %ff_res
  %r4 = or i1 %r3, %bb_res
  %r5 = or i1 %r4, %ss_res
  %r6 = or i1 %r5, %nn_res
  %r7 = or i1 %r6, %vv_res
  %r8 = or i1 %r7, %ee_res
  ; Phase 2 聚合：两值均为聚合 tag（>=8）→ 深比较（递归 helper）
  %a_agg = icmp uge i32 %ta, 8
  %b_agg = icmp uge i32 %tb, 8
  %agg_case = and i1 %a_agg, %b_agg
  %agg_call = call i1 @hc_eq_agg(%Value %a, %Value %b)
  %agg_res = and i1 %agg_case, %agg_call
  %r9 = or i1 %r8, %agg_res
  ret i1 %r9
}
"#;

const HC_LT: &str = r#"define i1 @hc_lt(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %a_int = icmp eq i32 %ta, 2
  %b_int = icmp eq i32 %tb, 2
  %ii_case = and i1 %a_int, %b_int
  %ii_lt = icmp slt i128 %da, %db
  %ii_res = and i1 %ii_case, %ii_lt
  %b_float = icmp eq i32 %tb, 3
  %if_case = and i1 %a_int, %b_float
  %if_fa = sitofp i128 %da to double
  %db_t = trunc i128 %db to i64
  %if_fb = bitcast i64 %db_t to double
  %if_lt = fcmp olt double %if_fa, %if_fb
  %if_res = and i1 %if_case, %if_lt
  %a_float = icmp eq i32 %ta, 3
  %fi_case = and i1 %a_float, %b_int
  %da_t = trunc i128 %da to i64
  %fi_fa = bitcast i64 %da_t to double
  %fi_fb = sitofp i128 %db to double
  %fi_lt = fcmp olt double %fi_fa, %fi_fb
  %fi_res = and i1 %fi_case, %fi_lt
  %ff_case = and i1 %a_float, %b_float
  %ff_lt = fcmp olt double %fi_fa, %if_fb
  %ff_res = and i1 %ff_case, %ff_lt
  %a_bool = icmp eq i32 %ta, 4
  %b_bool = icmp eq i32 %tb, 4
  %bb_case = and i1 %a_bool, %b_bool
  %bb_lt = icmp slt i128 %da, %db
  %bb_res = and i1 %bb_case, %bb_lt
  %a_str = icmp eq i32 %ta, 5
  %b_str = icmp eq i32 %tb, 5
  %ss_case = and i1 %a_str, %b_str
  %ss_pa = inttoptr i128 %da to i8*
  %ss_pb = inttoptr i128 %db to i8*
  %ss_es = getelementptr inbounds [1 x i8], ptr @.empty_str_s, i64 0, i64 0
  %ss_pa_s = select i1 %ss_case, i8* %ss_pa, i8* %ss_es
  %ss_pb_s = select i1 %ss_case, i8* %ss_pb, i8* %ss_es
  %ss_cmp = call i32 @strcmp(i8* %ss_pa_s, i8* %ss_pb_s)
  %ss_lt = icmp slt i32 %ss_cmp, 0
  %ss_res = and i1 %ss_case, %ss_lt
  %a_ptr = icmp eq i32 %ta, 7
  %b_ptr = icmp eq i32 %tb, 7
  %pp_case = and i1 %a_ptr, %b_ptr
  %pp_lt = icmp slt i128 %da, %db
  %pp_res = and i1 %pp_case, %pp_lt
  %r1 = or i1 %ii_res, %if_res
  %r2 = or i1 %r1, %fi_res
  %r3 = or i1 %r2, %ff_res
  %r4 = or i1 %r3, %bb_res
  %r5 = or i1 %r4, %ss_res
  %r6 = or i1 %r5, %pp_res
  ret i1 %r6
}
"#;

const HC_TRUTHY: &str = r#"define i1 @hc_truthy(%Value %v) {
entry:
  %t = extractvalue %Value %v, 0
  %d = extractvalue %Value %v, 1
  %is_bool = icmp eq i32 %t, 4
  br i1 %is_bool, label %bool_, label %chk_int
bool_:
  %b = icmp ne i128 %d, 0
  ret i1 %b
chk_int:
  %is_int = icmp eq i32 %t, 2
  br i1 %is_int, label %int_, label %chk_float
int_:
  %i = icmp ne i128 %d, 0
  ret i1 %i
chk_float:
  %is_float = icmp eq i32 %t, 3
  br i1 %is_float, label %float_, label %chk_str
float_:
  %d_t = trunc i128 %d to i64
  %f = bitcast i64 %d_t to double
  %fn = fcmp une double %f, 0.000000e+00
  ret i1 %fn
chk_str:
  %is_str = icmp eq i32 %t, 5
  br i1 %is_str, label %str_, label %chk_null
str_:
  %p = inttoptr i128 %d to i8*
  %c = load i8, i8* %p
  %ne = icmp ne i8 %c, 0
  ret i1 %ne
chk_null:
  %is_null = icmp eq i32 %t, 1
  br i1 %is_null, label %null_, label %other
null_:
  ret i1 false
other:
  ret i1 true
}
"#;

const HC_IS_ERR: &str = r#"define i1 @hc_is_err(%Value %v) {
  %t = extractvalue %Value %v, 0
  %r = icmp eq i32 %t, 6
  ret i1 %r
}
"#;

const HC_IS_NULL: &str = r#"define i1 @hc_is_null(%Value %v) {
  %t = extractvalue %Value %v, 0
  %r = icmp eq i32 %t, 1
  ret i1 %r
}
"#;

const HC_BOOL: &str = r#"define %Value @hc_bool(i1 zeroext %b) {
  %d = zext i1 %b to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 4, 0
  %v1 = insertvalue %Value %v0, i128 %d, 1
  ret %Value %v1
}
"#;

/// 相等分派（Phase 1 指针）：两指针 → 载荷地址身份；否则解引用归一化后走纯值比较
/// （对齐 `IrValue::value_eq`：`(Ptr,Ptr)` 身份、`(Ptr,b)` 解引用）。
const HC_EQ_DISPATCH: &str = r#"define i1 @hc_eq(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %a_ptr = icmp eq i32 %ta, 7
  %b_ptr = icmp eq i32 %tb, 7
  %both_ptr = and i1 %a_ptr, %b_ptr
  br i1 %both_ptr, label %ptr_id, label %mixed
ptr_id:
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %id = icmp eq i128 %da, %db
  ret i1 %id
mixed:
  %an = call %Value @hc_deref(%Value %a)
  %bn = call %Value @hc_deref(%Value %b)
  %eq = call i1 @hc_eq_plain(%Value %an, %Value %bn)
  ret i1 %eq
}
"#;

fn emit_cmp_helpers(out: &mut String) {
    out.push_str(HC_EQ_PLAIN);
    out.push('\n');
    out.push_str(HC_EQ_DISPATCH);
    out.push('\n');
    out.push_str(HC_LT);
    out.push('\n');
    out.push_str(HC_TRUTHY);
    out.push('\n');
    out.push_str(HC_IS_ERR);
    out.push('\n');
    out.push_str(HC_IS_NULL);
    out.push('\n');
    out.push_str(HC_BOOL);
    out.push('\n');
}

// ---------- 一元运算 helper ----------

const HC_NEG: &str = r#"define %Value @hc_neg(%Value %v) {
entry:
  %t = extractvalue %Value %v, 0
  %d = extractvalue %Value %v, 1
  %is_int = icmp eq i32 %t, 2
  br i1 %is_int, label %int_, label %chk_float
int_:
  %n = sub i128 0, %d
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %n, 1
  ret %Value %v1
chk_float:
  %is_float = icmp eq i32 %t, 3
  br i1 %is_float, label %float_, label %err
float_:
  %d_t = trunc i128 %d to i64
  %f = bitcast i64 %d_t to double
  %nf = fneg double %f
  %bits64 = bitcast double %nf to i64
  %bits = zext i64 %bits64 to i128
  %f0 = insertvalue %Value { i32 0, i128 0 }, i32 3, 0
  %f1 = insertvalue %Value %f0, i128 %bits, 1
  ret %Value %f1
err:
  call void @hc_abort_typeerr()
  unreachable
}
"#;

const HC_BITNOT: &str = r#"define %Value @hc_bitnot(%Value %v) {
entry:
  %t = extractvalue %Value %v, 0
  %d = extractvalue %Value %v, 1
  %is_int = icmp eq i32 %t, 2
  br i1 %is_int, label %int_, label %err
int_:
  %n = xor i128 %d, -1
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %n, 1
  ret %Value %v1
err:
  call void @hc_abort_typeerr()
  unreachable
}
"#;

const HC_NOT: &str = r#"define %Value @hc_not(%Value %v) {
  %b = call i1 @hc_truthy(%Value %v)
  %n = xor i1 %b, true
  %d = zext i1 %n to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 4, 0
  %v1 = insertvalue %Value %v0, i128 %d, 1
  ret %Value %v1
}
"#;

fn emit_unary_helpers(out: &mut String) {
    out.push_str(HC_NEG);
    out.push('\n');
    out.push_str(HC_BITNOT);
    out.push('\n');
    out.push_str(HC_NOT);
    out.push('\n');
}

// ---------- 指针 helper（Phase 1：取址 / 解引用 / 写穿） ----------

/// 解引用：tag 7 指针 → 载荷 `inttoptr` 后 load；非指针恒等（对齐
/// tree-walking `deref_value` 与 `IrValue::Deref` 的 identity 分支）。
const HC_DEREF: &str = r#"define %Value @hc_deref(%Value %v) {
entry:
  %t = extractvalue %Value %v, 0
  %is_ptr = icmp eq i32 %t, 7
  br i1 %is_ptr, label %deref, label %identity
deref:
  %d = extractvalue %Value %v, 1
  %pp = inttoptr i128 %d to %Value*
  %pv = load %Value, %Value* %pp
  ret %Value %pv
identity:
  ret %Value %v
}
"#;

/// 写穿：tag 7 指针 → `inttoptr` 后 store；非指针 → BadAssign（对齐
/// `StorePtr` 对非指针的硬错误）。
const HC_STORE_PTR: &str = r#"define void @hc_store_ptr(%Value %p, %Value %v) {
entry:
  %t = extractvalue %Value %p, 0
  %is_ptr = icmp eq i32 %t, 7
  br i1 %is_ptr, label %sp, label %err
sp:
  %d = extractvalue %Value %p, 1
  %pp = inttoptr i128 %d to %Value*
  store %Value %v, %Value* %pp
  ret void
err:
  call void @hc_abort_badassign()
  unreachable
}
"#;

/// 比较前归一化：指针解引用、普通值恒等。与 [`HC_EQ_DISPATCH`] 配合，
/// 让 `hc_eq` 在指针与非指针混合时对齐 `IrValue::value_eq`。
fn emit_pointer_helpers(out: &mut String) {
    out.push_str(HC_DEREF);
    out.push('\n');
    out.push_str(HC_STORE_PTR);
    out.push('\n');
}

// ---------- Phase 2 聚合 helper（堆对象分配 / 字段 / 索引 / 切片 / 字面量 / 深比较） ----------

const HC_ALLOC: &str = r#"define i8* @hc_alloc(i64 %size) {
entry:
  %z = icmp eq i64 %size, 0
  %sz = select i1 %z, i64 1, i64 %size
  %p = call i8* @malloc(i64 %sz)
  %ok = icmp ne i8* %p, null
  br i1 %ok, label %okb, label %oom
oom:
  call void @hc_abort_oom()
  unreachable
okb:
  ret i8* %p
}
"#;

const HC_MAKE_ARR: &str = r#"define %Value @hc_make_arr(i64 %n) {
entry:
  %arrsz = ptrtoint %ArrObj* getelementptr (%ArrObj, %ArrObj* null, i32 1) to i64
  %raw = call i8* @hc_alloc(i64 %arrsz)
  %op = bitcast i8* %raw to %ArrObj*
  %vsz = ptrtoint %Value* getelementptr (%Value, %Value* null, i32 1) to i64
  %isz = mul i64 %vsz, %n
  %iraw = call i8* @hc_alloc(i64 %isz)
  %items = bitcast i8* %iraw to %Value*
  %o0 = insertvalue %ArrObj undef, i64 %n, 0
  %o1 = insertvalue %ArrObj %o0, %Value* %items, 1
  store %ArrObj %o1, %ArrObj* %op
  %dp = ptrtoint %ArrObj* %op to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 8, 0
  %v1 = insertvalue %Value %v0, i128 %dp, 1
  ret %Value %v1
}
"#;

const HC_ARR_SET: &str = r#"define void @hc_arr_set(%Value %arr, i64 %i, %Value %v) {
entry:
  %d = extractvalue %Value %arr, 1
  %op = inttoptr i128 %d to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %op
  %items = extractvalue %ArrObj %ao, 1
  %p = getelementptr %Value, %Value* %items, i64 %i
  store %Value %v, %Value* %p
  ret void
}
"#;

const HC_MAKE_CLASS: &str = r#"define %Value @hc_make_class(i8* %ty, i64 %n) {
entry:
  %os = ptrtoint %ClassObj* getelementptr (%ClassObj, %ClassObj* null, i32 1) to i64
  %raw = call i8* @hc_alloc(i64 %os)
  %op = bitcast i8* %raw to %ClassObj*
  %fsz = ptrtoint %Field* getelementptr (%Field, %Field* null, i32 1) to i64
  %fs = mul i64 %fsz, %n
  %fraw = call i8* @hc_alloc(i64 %fs)
  %fields = bitcast i8* %fraw to %Field*
  %o0 = insertvalue %ClassObj undef, i8* %ty, 0
  %o1 = insertvalue %ClassObj %o0, i64 %n, 1
  %o2 = insertvalue %ClassObj %o1, %Field* %fields, 2
  store %ClassObj %o2, %ClassObj* %op
  %dp = ptrtoint %ClassObj* %op to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 10, 0
  %v1 = insertvalue %Value %v0, i128 %dp, 1
  ret %Value %v1
}
"#;

const HC_CLASS_SET: &str = r#"define void @hc_class_set(%Value %obj, i64 %i, i8* %fname, %Value %v) {
entry:
  %d = extractvalue %Value %obj, 1
  %op = inttoptr i128 %d to %ClassObj*
  %ao = load %ClassObj, %ClassObj* %op
  %fields = extractvalue %ClassObj %ao, 2
  %p = getelementptr %Field, %Field* %fields, i64 %i
  %f0 = insertvalue %Field undef, i8* %fname, 0
  %f1 = insertvalue %Field %f0, %Value %v, 1
  store %Field %f1, %Field* %p
  ret void
}
"#;

const HC_MAKE_ENUM: &str = r#"define %Value @hc_make_enum(i8* %name, i8* %variant, %Value* %payload) {
entry:
  %os = ptrtoint %EnumObj* getelementptr (%EnumObj, %EnumObj* null, i32 1) to i64
  %raw = call i8* @hc_alloc(i64 %os)
  %op = bitcast i8* %raw to %EnumObj*
  %o0 = insertvalue %EnumObj undef, i8* %name, 0
  %o1 = insertvalue %EnumObj %o0, i8* %variant, 1
  %o2 = insertvalue %EnumObj %o1, %Value* %payload, 2
  store %EnumObj %o2, %EnumObj* %op
  %dp = ptrtoint %EnumObj* %op to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 11, 0
  %v1 = insertvalue %Value %v0, i128 %dp, 1
  ret %Value %v1
}
"#;

const HC_UNWRAP: &str = r#"define %Value @hc_unwrap(%Value %v) {
entry:
  %t = extractvalue %Value %v, 0
  %is_opt = icmp eq i32 %t, 1
  br i1 %is_opt, label %opt, label %id
id:
  ret %Value %v
opt:
  %d = extractvalue %Value %v, 1
  %is_none = icmp eq i128 %d, 0
  br i1 %is_none, label %nullb, label %some
nullb:
  call void @hc_abort_nullunwrap()
  unreachable
some:
  %p = inttoptr i128 %d to %Value*
  %pv = load %Value, %Value* %p
  ret %Value %pv
}
"#;

const HC_INDEX: &str = r#"define %Value @hc_index(%Value %base, %Value %idx) {
entry:
  %b = call %Value @hc_deref(%Value %base)
  %it = extractvalue %Value %idx, 0
  %is_int = icmp eq i32 %it, 2
  br i1 %is_int, label %have, label %badidx
badidx:
  call void @hc_abort_badindex()
  unreachable
have:
  %id = extractvalue %Value %idx, 1
  %ix = trunc i128 %id to i64
  %neg = icmp slt i64 %ix, 0
  br i1 %neg, label %badidx, label %bt
bt:
  %t = extractvalue %Value %b, 0
  %is_arr = icmp eq i32 %t, 8
  br i1 %is_arr, label %arr, label %chk_slc
arr:
  %d = extractvalue %Value %b, 1
  %op = inttoptr i128 %d to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %op
  %len = extractvalue %ArrObj %ao, 0
  %items = extractvalue %ArrObj %ao, 1
  %oob = icmp uge i64 %ix, %len
  br i1 %oob, label %oob_err, label %ok_arr
ok_arr:
  %p = getelementptr %Value, %Value* %items, i64 %ix
  %v = load %Value, %Value* %p
  ret %Value %v
oob_err:
  call void @hc_abort_indexoob()
  unreachable
chk_slc:
  %is_slc = icmp eq i32 %t, 9
  br i1 %is_slc, label %slc, label %chk_str
slc:
  %d2 = extractvalue %Value %b, 1
  %op2 = inttoptr i128 %d2 to %SliceObj*
  %so = load %SliceObj, %SliceObj* %op2
  %sitems = extractvalue %SliceObj %so, 0
  %sstart = extractvalue %SliceObj %so, 1
  %slen = extractvalue %SliceObj %so, 2
  %oob2 = icmp uge i64 %ix, %slen
  br i1 %oob2, label %oob_err, label %ok_slc
ok_slc:
  %ii = add i64 %sstart, %ix
  %p2 = getelementptr %Value, %Value* %sitems, i64 %ii
  %v2 = load %Value, %Value* %p2
  ret %Value %v2
chk_str:
  %is_str = icmp eq i32 %t, 5
  br i1 %is_str, label %strb, label %notidx
strb:
  %d3 = extractvalue %Value %b, 1
  %sp = inttoptr i128 %d3 to i8*
  %blen = call i64 @strlen(i8* %sp)
  %oob3 = icmp uge i64 %ix, %blen
  br i1 %oob3, label %oob_err, label %ok_str
ok_str:
  %sp2 = getelementptr i8, i8* %sp, i64 %ix
  %bval = load i8, i8* %sp2
  %bint = zext i8 %bval to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %bint, 1
  ret %Value %v1
notidx:
  call void @hc_abort_notindexable()
  unreachable
}
"#;

const HC_STORE_INDEX: &str = r#"define void @hc_store_index(%Value %base, %Value %idx, %Value %v) {
entry:
  %b = call %Value @hc_deref(%Value %base)
  %it = extractvalue %Value %idx, 0
  %is_int = icmp eq i32 %it, 2
  br i1 %is_int, label %have, label %badidx
badidx:
  call void @hc_abort_badindex()
  unreachable
have:
  %id = extractvalue %Value %idx, 1
  %ix = trunc i128 %id to i64
  %neg = icmp slt i64 %ix, 0
  br i1 %neg, label %badidx, label %bt
bt:
  %t = extractvalue %Value %b, 0
  %is_arr = icmp eq i32 %t, 8
  br i1 %is_arr, label %arr, label %err
err:
  call void @hc_abort_typeerr()
  unreachable
arr:
  %d = extractvalue %Value %b, 1
  %op = inttoptr i128 %d to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %op
  %len = extractvalue %ArrObj %ao, 0
  %items = extractvalue %ArrObj %ao, 1
  %oob = icmp uge i64 %ix, %len
  br i1 %oob, label %oob_err, label %ok
ok:
  %p = getelementptr %Value, %Value* %items, i64 %ix
  store %Value %v, %Value* %p
  ret void
oob_err:
  call void @hc_abort_indexoob()
  unreachable
}
"#;

const HC_SLICE: &str = r#"define %Value @hc_slice(%Value %base, %Value %lo, %Value %hi) {
entry:
  %b = call %Value @hc_deref(%Value %base)
  %lt = extractvalue %Value %lo, 0
  %li = icmp eq i32 %lt, 2
  br i1 %li, label %have_lo, label %badidx
badidx:
  call void @hc_abort_badindex()
  unreachable
have_lo:
  %ld = extractvalue %Value %lo, 1
  %lox = trunc i128 %ld to i64
  %lneg = icmp slt i64 %lox, 0
  br i1 %lneg, label %badidx, label %have_hi
have_hi:
  %het = extractvalue %Value %hi, 0
  %is_end = icmp eq i32 %het, 12
  br i1 %is_end, label %set_open, label %chk_hint
chk_hint:
  %hi_int = icmp eq i32 %het, 2
  br i1 %hi_int, label %set_closed, label %badidx
set_open:
  br label %bt
set_closed:
  %hdx = extractvalue %Value %hi, 1
  %hixc = trunc i128 %hdx to i64
  %hneg = icmp slt i64 %hixc, 0
  br i1 %hneg, label %badidx, label %bt
bt:
  %open = phi i1 [ true, %set_open ], [ false, %set_closed ]
  %hix = phi i64 [ 0, %set_open ], [ %hixc, %set_closed ]
  %t = extractvalue %Value %b, 0
  %is_arr = icmp eq i32 %t, 8
  br i1 %is_arr, label %arr, label %chk_slc
arr:
  %d = extractvalue %Value %b, 1
  %op = inttoptr i128 %d to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %op
  %len = extractvalue %ArrObj %ao, 0
  %items = extractvalue %ArrObj %ao, 1
  %hsel = select i1 %open, i64 %len, i64 %hix
  %hbad = icmp ugt i64 %hsel, %len
  %lbad = icmp ugt i64 %lox, %len
  %bad = or i1 %hbad, %lbad
  br i1 %bad, label %oob_err, label %mk_slc
mk_slc:
  %nl = sub i64 %hsel, %lox
  %os2 = ptrtoint %SliceObj* getelementptr (%SliceObj, %SliceObj* null, i32 1) to i64
  %raw = call i8* @hc_alloc(i64 %os2)
  %op2 = bitcast i8* %raw to %SliceObj*
  %s0 = insertvalue %SliceObj undef, %Value* %items, 0
  %s1 = insertvalue %SliceObj %s0, i64 %lox, 1
  %s2 = insertvalue %SliceObj %s1, i64 %nl, 2
  store %SliceObj %s2, %SliceObj* %op2
  %dp = ptrtoint %SliceObj* %op2 to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 9, 0
  %v1 = insertvalue %Value %v0, i128 %dp, 1
  ret %Value %v1
oob_err:
  call void @hc_abort_indexoob()
  unreachable
chk_slc:
  %is_slc = icmp eq i32 %t, 9
  br i1 %is_slc, label %slc, label %chk_str
slc:
  %d2 = extractvalue %Value %b, 1
  %op3 = inttoptr i128 %d2 to %SliceObj*
  %so = load %SliceObj, %SliceObj* %op3
  %sitems = extractvalue %SliceObj %so, 0
  %sstart = extractvalue %SliceObj %so, 1
  %slen = extractvalue %SliceObj %so, 2
  %h2 = select i1 %open, i64 %slen, i64 %hix
  %hbad2 = icmp ugt i64 %h2, %slen
  %lbad2 = icmp ugt i64 %lox, %slen
  %bad2 = or i1 %hbad2, %lbad2
  br i1 %bad2, label %oob_err, label %mk_slc2
mk_slc2:
  %nl2 = sub i64 %h2, %lox
  %nstart = add i64 %sstart, %lox
  %os3 = ptrtoint %SliceObj* getelementptr (%SliceObj, %SliceObj* null, i32 1) to i64
  %raw2 = call i8* @hc_alloc(i64 %os3)
  %op4 = bitcast i8* %raw2 to %SliceObj*
  %t0 = insertvalue %SliceObj undef, %Value* %sitems, 0
  %t1 = insertvalue %SliceObj %t0, i64 %nstart, 1
  %t2 = insertvalue %SliceObj %t1, i64 %nl2, 2
  store %SliceObj %t2, %SliceObj* %op4
  %dp2 = ptrtoint %SliceObj* %op4 to i128
  %w0 = insertvalue %Value { i32 0, i128 0 }, i32 9, 0
  %w1 = insertvalue %Value %w0, i128 %dp2, 1
  ret %Value %w1
chk_str:
  %is_str = icmp eq i32 %t, 5
  br i1 %is_str, label %strb, label %notidx
strb:
  %d3 = extractvalue %Value %b, 1
  %sp = inttoptr i128 %d3 to i8*
  %blen = call i64 @strlen(i8* %sp)
  %h3 = select i1 %open, i64 %blen, i64 %hix
  %hbad3 = icmp ugt i64 %h3, %blen
  %lbad3 = icmp ugt i64 %lox, %blen
  %bad3 = or i1 %hbad3, %lbad3
  br i1 %bad3, label %oob_err, label %mk_str
mk_str:
  %nl3 = sub i64 %h3, %lox
  %nbytes = add i64 %nl3, 1
  %buf = call i8* @hc_alloc(i64 %nbytes)
  %srcp = getelementptr i8, i8* %sp, i64 %lox
  call void @llvm.memcpy.p0i8.p0i8.i64(i8* %buf, i8* %srcp, i64 %nl3, i1 false)
  %nulp = getelementptr i8, i8* %buf, i64 %nl3
  store i8 0, i8* %nulp
  %pi = ptrtoint i8* %buf to i128
  %x0 = insertvalue %Value { i32 0, i128 0 }, i32 5, 0
  %x1 = insertvalue %Value %x0, i128 %pi, 1
  ret %Value %x1
notidx:
  call void @hc_abort_notindexable()
  unreachable
}
"#;

const HC_STORE_SLICE: &str = r#"define void @hc_store_slice(%Value %base, %Value %lo, %Value %hi, %Value %v) {
entry:
  %b = call %Value @hc_deref(%Value %base)
  %t = extractvalue %Value %b, 0
  %is_arr = icmp eq i32 %t, 8
  br i1 %is_arr, label %arr, label %err
err:
  call void @hc_abort_typeerr()
  unreachable
arr:
  %d = extractvalue %Value %b, 1
  %op = inttoptr i128 %d to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %op
  %len = extractvalue %ArrObj %ao, 0
  %items = extractvalue %ArrObj %ao, 1
  %lt = extractvalue %Value %lo, 0
  %li = icmp eq i32 %lt, 2
  br i1 %li, label %hl, label %badidx
badidx:
  call void @hc_abort_badindex()
  unreachable
hl:
  %ld = extractvalue %Value %lo, 1
  %lox = trunc i128 %ld to i64
  %lneg = icmp slt i64 %lox, 0
  br i1 %lneg, label %badidx, label %hh
hh:
  %het = extractvalue %Value %hi, 0
  %is_end = icmp eq i32 %het, 12
  br i1 %is_end, label %badidx, label %chk_hint
chk_hint:
  %hi_int = icmp eq i32 %het, 2
  br i1 %hi_int, label %hh2, label %badidx
hh2:
  %hd = extractvalue %Value %hi, 1
  %hix = trunc i128 %hd to i64
  %hneg = icmp slt i64 %hix, 0
  br i1 %hneg, label %badidx, label %bounds
bounds:
  %hbad = icmp ugt i64 %hix, %len
  %lbad = icmp ugt i64 %lox, %len
  %bad = or i1 %hbad, %lbad
  br i1 %bad, label %oob, label %src
oob:
  call void @hc_abort_indexoob()
  unreachable
src:
  %sv = call %Value @hc_deref(%Value %v)
  %st = extractvalue %Value %sv, 0
  %is_src = icmp eq i32 %st, 8
  br i1 %is_src, label %snap, label %done
done:
  ret void
snap:
  %sd = extractvalue %Value %sv, 1
  %sop = inttoptr i128 %sd to %ArrObj*
  %sao = load %ArrObj, %ArrObj* %sop
  %slen = extractvalue %ArrObj %sao, 0
  %sitems = extractvalue %ArrObj %sao, 1
  %vsz = ptrtoint %Value* getelementptr (%Value, %Value* null, i32 1) to i64
  %bsz = mul i64 %vsz, %slen
  %braw = call i8* @hc_alloc(i64 %bsz)
  %buf = bitcast i8* %braw to %Value*
  %bytes = mul i64 %slen, %vsz
  %sp_ = bitcast %Value* %sitems to i8*
  %bp_ = bitcast %Value* %buf to i8*
  call void @llvm.memcpy.p0i8.p0i8.i64(i8* %bp_, i8* %sp_, i64 %bytes, i1 false)
  br label %loop
loop:
  %k = phi i64 [ 0, %snap ], [ %k2, %next ]
  %kdone = icmp uge i64 %k, %slen
  br i1 %kdone, label %done, label %body
body:
  %dst_i = add i64 %lox, %k
  %inb = icmp ult i64 %dst_i, %len
  br i1 %inb, label %write, label %next
write:
  %bp = getelementptr %Value, %Value* %buf, i64 %k
  %bv = load %Value, %Value* %bp
  %dp_ = getelementptr %Value, %Value* %items, i64 %dst_i
  store %Value %bv, %Value* %dp_
  br label %next
next:
  %k2 = add i64 %k, 1
  br label %loop
}
"#;

const HC_CLASS_FIND: &str = r#"define %FindRes @hc_class_find(%Value %base, i8* %field) {
entry:
  %t = extractvalue %Value %base, 0
  %is = icmp eq i32 %t, 10
  br i1 %is, label %cls, label %nf
cls:
  %d = extractvalue %Value %base, 1
  %op = inttoptr i128 %d to %ClassObj*
  %co = load %ClassObj, %ClassObj* %op
  %cnt = extractvalue %ClassObj %co, 1
  %fs = extractvalue %ClassObj %co, 2
  br label %loop
loop:
  %i = phi i64 [ 0, %cls ], [ %i2, %next ]
  %done = icmp uge i64 %i, %cnt
  br i1 %done, label %nf, label %body
body:
  %fp = getelementptr %Field, %Field* %fs, i64 %i
  %f = load %Field, %Field* %fp
  %fn = extractvalue %Field %f, 0
  %c = call i32 @strcmp(i8* %fn, i8* %field)
  %eq = icmp eq i32 %c, 0
  br i1 %eq, label %found, label %next
next:
  %i2 = add i64 %i, 1
  br label %loop
found:
  %fv = extractvalue %Field %f, 1
  %r0 = insertvalue %FindRes undef, i1 true, 0
  %r1 = insertvalue %FindRes %r0, %Value %fv, 1
  ret %FindRes %r1
nf:
  ret %FindRes { i1 false, %Value undef }
}
"#;

const HC_FIELD: &str = r#"define %Value @hc_field(%Value %base, i8* %field) {
entry:
  %b = call %Value @hc_deref(%Value %base)
  %t = extractvalue %Value %b, 0
  %is_class = icmp eq i32 %t, 10
  br i1 %is_class, label %cls, label %chk_str
cls:
  %fr = call %FindRes @hc_class_find(%Value %b, i8* %field)
  %ok = extractvalue %FindRes %fr, 0
  br i1 %ok, label %found, label %nf
found:
  %v = extractvalue %FindRes %fr, 1
  ret %Value %v
nf:
  call void @hc_abort_nofield()
  unreachable
chk_str:
  %is_str = icmp eq i32 %t, 5
  br i1 %is_str, label %strb, label %chk_arr
strb:
  %lenp = getelementptr inbounds [4 x i8], ptr @.hc_len, i64 0, i64 0
  %c = call i32 @strcmp(i8* %field, i8* %lenp)
  %iseq = icmp eq i32 %c, 0
  br i1 %iseq, label %len_ok, label %nf
len_ok:
  %d = extractvalue %Value %b, 1
  %sp = inttoptr i128 %d to i8*
  %bl = call i64 @strlen(i8* %sp)
  %bi = zext i64 %bl to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %bi, 1
  ret %Value %v1
chk_arr:
  %is_arr = icmp eq i32 %t, 8
  br i1 %is_arr, label %arrb, label %chk_slc
arrb:
  %lenp2 = getelementptr inbounds [4 x i8], ptr @.hc_len, i64 0, i64 0
  %c2 = call i32 @strcmp(i8* %field, i8* %lenp2)
  %iseq2 = icmp eq i32 %c2, 0
  br i1 %iseq2, label %arr_len, label %nf
arr_len:
  %d2 = extractvalue %Value %b, 1
  %op = inttoptr i128 %d2 to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %op
  %al = extractvalue %ArrObj %ao, 0
  %ai = zext i64 %al to i128
  %w0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %w1 = insertvalue %Value %w0, i128 %ai, 1
  ret %Value %w1
chk_slc:
  %is_slc = icmp eq i32 %t, 9
  br i1 %is_slc, label %slcb, label %nf
slcb:
  %lenp3 = getelementptr inbounds [4 x i8], ptr @.hc_len, i64 0, i64 0
  %c3 = call i32 @strcmp(i8* %field, i8* %lenp3)
  %iseq3 = icmp eq i32 %c3, 0
  br i1 %iseq3, label %slc_len, label %nf
slc_len:
  %d3 = extractvalue %Value %b, 1
  %op2 = inttoptr i128 %d3 to %SliceObj*
  %so = load %SliceObj, %SliceObj* %op2
  %sl = extractvalue %SliceObj %so, 2
  %si = zext i64 %sl to i128
  %u0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %u1 = insertvalue %Value %u0, i128 %si, 1
  ret %Value %u1
}
"#;

const HC_STORE_FIELD: &str = r#"define void @hc_store_field(%Value %base, i8* %field, %Value %v) {
entry:
  %b = call %Value @hc_deref(%Value %base)
  %t = extractvalue %Value %b, 0
  %is = icmp eq i32 %t, 10
  br i1 %is, label %cls, label %err
err:
  call void @hc_abort_typeerr()
  unreachable
cls:
  %d = extractvalue %Value %b, 1
  %op = inttoptr i128 %d to %ClassObj*
  %co = load %ClassObj, %ClassObj* %op
  %cnt = extractvalue %ClassObj %co, 1
  %fs = extractvalue %ClassObj %co, 2
  br label %loop
loop:
  %i = phi i64 [ 0, %cls ], [ %i2, %next ]
  %done = icmp uge i64 %i, %cnt
  br i1 %done, label %append, label %body
body:
  %fp = getelementptr %Field, %Field* %fs, i64 %i
  %f = load %Field, %Field* %fp
  %fn = extractvalue %Field %f, 0
  %c = call i32 @strcmp(i8* %fn, i8* %field)
  %eq = icmp eq i32 %c, 0
  br i1 %eq, label %found, label %next
next:
  %i2 = add i64 %i, 1
  br label %loop
found:
  %nf0 = insertvalue %Field undef, i8* %field, 0
  %nf1 = insertvalue %Field %nf0, %Value %v, 1
  store %Field %nf1, %Field* %fp
  ret void
append:
  %newcnt = add i64 %cnt, 1
  %fsz = ptrtoint %Field* getelementptr (%Field, %Field* null, i32 1) to i64
  %newsz = mul i64 %fsz, %newcnt
  %fsp = bitcast %Field* %fs to i8*
  %newp = call i8* @realloc(i8* %fsp, i64 %newsz)
  %newf = bitcast i8* %newp to %Field*
  %fp2 = getelementptr %Field, %Field* %newf, i64 %cnt
  %n2 = insertvalue %Field undef, i8* %field, 0
  %n3 = insertvalue %Field %n2, %Value %v, 1
  store %Field %n3, %Field* %fp2
  %c0 = insertvalue %ClassObj %co, i64 %newcnt, 1
  %c1 = insertvalue %ClassObj %c0, %Field* %newf, 2
  store %ClassObj %c1, %ClassObj* %op
  ret void
}
"#;

const HC_SEQ_INFO: &str = r#"define %SeqInfo @hc_seq_info(%Value %v) {
entry:
  %t = extractvalue %Value %v, 0
  %d = extractvalue %Value %v, 1
  %is_arr = icmp eq i32 %t, 8
  br i1 %is_arr, label %arr, label %slc
arr:
  %ap = inttoptr i128 %d to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %ap
  %alen = extractvalue %ArrObj %ao, 0
  %aitems = extractvalue %ArrObj %ao, 1
  %r0 = insertvalue %SeqInfo undef, %Value* %aitems, 0
  %r1 = insertvalue %SeqInfo %r0, i64 0, 1
  %r2 = insertvalue %SeqInfo %r1, i64 %alen, 2
  ret %SeqInfo %r2
slc:
  %sp = inttoptr i128 %d to %SliceObj*
  %so = load %SliceObj, %SliceObj* %sp
  %sitems = extractvalue %SliceObj %so, 0
  %sstart = extractvalue %SliceObj %so, 1
  %slen = extractvalue %SliceObj %so, 2
  %q0 = insertvalue %SeqInfo undef, %Value* %sitems, 0
  %q1 = insertvalue %SeqInfo %q0, i64 %sstart, 1
  %q2 = insertvalue %SeqInfo %q1, i64 %slen, 2
  ret %SeqInfo %q2
}
"#;

const HC_EQ_AGG: &str = r#"define i1 @hc_eq_agg(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %a_end = icmp eq i32 %ta, 12
  %b_end = icmp eq i32 %tb, 12
  %ee = and i1 %a_end, %b_end
  br i1 %ee, label %ret_true, label %chk_seq
chk_seq:
  %a_arr = icmp eq i32 %ta, 8
  %a_slc = icmp eq i32 %ta, 9
  %a_seq = or i1 %a_arr, %a_slc
  %b_arr = icmp eq i32 %tb, 8
  %b_slc = icmp eq i32 %tb, 9
  %b_seq = or i1 %b_arr, %b_slc
  %both_seq = and i1 %a_seq, %b_seq
  br i1 %both_seq, label %seq, label %chk_cls
seq:
  %ai = call %SeqInfo @hc_seq_info(%Value %a)
  %bi = call %SeqInfo @hc_seq_info(%Value %b)
  %abase = extractvalue %SeqInfo %ai, 0
  %astart = extractvalue %SeqInfo %ai, 1
  %alen = extractvalue %SeqInfo %ai, 2
  %bbase = extractvalue %SeqInfo %bi, 0
  %bstart = extractvalue %SeqInfo %bi, 1
  %blen = extractvalue %SeqInfo %bi, 2
  %le = icmp eq i64 %alen, %blen
  br i1 %le, label %loop, label %ret_false
loop:
  %k = phi i64 [ 0, %seq ], [ %k2, %lnext ]
  %kdone = icmp uge i64 %k, %alen
  br i1 %kdone, label %ret_true, label %lbody
lbody:
  %ai2 = add i64 %astart, %k
  %bi2 = add i64 %bstart, %k
  %ap = getelementptr %Value, %Value* %abase, i64 %ai2
  %bp = getelementptr %Value, %Value* %bbase, i64 %bi2
  %av = load %Value, %Value* %ap
  %bv = load %Value, %Value* %bp
  %eq = call i1 @hc_eq(%Value %av, %Value %bv)
  br i1 %eq, label %lnext, label %ret_false
lnext:
  %k2 = add i64 %k, 1
  br label %loop
chk_cls:
  %a_cls = icmp eq i32 %ta, 10
  %b_cls = icmp eq i32 %tb, 10
  %both_cls = and i1 %a_cls, %b_cls
  br i1 %both_cls, label %cls, label %chk_enum
cls:
  %ad = extractvalue %Value %a, 1
  %bd = extractvalue %Value %b, 1
  %aop = inttoptr i128 %ad to %ClassObj*
  %bop = inttoptr i128 %bd to %ClassObj*
  %ao = load %ClassObj, %ClassObj* %aop
  %bo = load %ClassObj, %ClassObj* %bop
  %an = extractvalue %ClassObj %ao, 0
  %bn = extractvalue %ClassObj %bo, 0
  %acnt = extractvalue %ClassObj %ao, 1
  %bcnt = extractvalue %ClassObj %bo, 1
  %afs = extractvalue %ClassObj %ao, 2
  %ncmp = call i32 @strcmp(i8* %an, i8* %bn)
  %neq = icmp eq i32 %ncmp, 0
  %ceq = icmp eq i64 %acnt, %bcnt
  %pre = and i1 %neq, %ceq
  br i1 %pre, label %cloop, label %ret_false
cloop:
  %i = phi i64 [ 0, %cls ], [ %i2, %cnext ]
  %idone = icmp uge i64 %i, %acnt
  br i1 %idone, label %ret_true, label %cbody
cbody:
  %fp = getelementptr %Field, %Field* %afs, i64 %i
  %f = load %Field, %Field* %fp
  %fname = extractvalue %Field %f, 0
  %fval = extractvalue %Field %f, 1
  %bres = call %FindRes @hc_class_find(%Value %b, i8* %fname)
  %bfound = extractvalue %FindRes %bres, 0
  br i1 %bfound, label %cmatch, label %ret_false
cmatch:
  %bval = extractvalue %FindRes %bres, 1
  %fveq = call i1 @hc_eq(%Value %fval, %Value %bval)
  br i1 %fveq, label %cnext, label %ret_false
cnext:
  %i2 = add i64 %i, 1
  br label %cloop
chk_enum:
  %a_enum = icmp eq i32 %ta, 11
  %b_enum = icmp eq i32 %tb, 11
  %both_enum = and i1 %a_enum, %b_enum
  br i1 %both_enum, label %enm, label %ret_false
enm:
  %ed = extractvalue %Value %a, 1
  %fd = extractvalue %Value %b, 1
  %eop = inttoptr i128 %ed to %EnumObj*
  %fop = inttoptr i128 %fd to %EnumObj*
  %eo = load %EnumObj, %EnumObj* %eop
  %fo = load %EnumObj, %EnumObj* %fop
  %en = extractvalue %EnumObj %eo, 0
  %fn_ = extractvalue %EnumObj %fo, 0
  %ev = extractvalue %EnumObj %eo, 1
  %fv = extractvalue %EnumObj %fo, 1
  %ep = extractvalue %EnumObj %eo, 2
  %fp_ = extractvalue %EnumObj %fo, 2
  %ncm = call i32 @strcmp(i8* %en, i8* %fn_)
  %neq2 = icmp eq i32 %ncm, 0
  %vcm = call i32 @strcmp(i8* %ev, i8* %fv)
  %veq = icmp eq i32 %vcm, 0
  %nveq = and i1 %neq2, %veq
  br i1 %nveq, label %payload, label %ret_false
payload:
  %anull = icmp eq %Value* %ep, null
  %bnull = icmp eq %Value* %fp_, null
  %bothn = and i1 %anull, %bnull
  br i1 %bothn, label %ret_true, label %somechk
somechk:
  %same = icmp eq i1 %anull, %bnull
  br i1 %same, label %pl_cmp, label %ret_false
pl_cmp:
  %pv = load %Value, %Value* %ep
  %qv = load %Value, %Value* %fp_
  %peq = call i1 @hc_eq(%Value %pv, %Value %qv)
  ret i1 %peq
ret_false:
  ret i1 false
ret_true:
  ret i1 true
}
"#;

fn emit_aggregate_helpers(out: &mut String) {
    // 聚合 tag 常量与模板字符串内的字面量保持一致（标记使用，防 dead-code 告警）。
    let _ = [T_ARR, T_SLICE, T_CLASS, T_ENUM, T_END];
    for h in [
        HC_ALLOC,
        HC_MAKE_ARR,
        HC_ARR_SET,
        HC_MAKE_CLASS,
        HC_CLASS_SET,
        HC_MAKE_ENUM,
        HC_UNWRAP,
        HC_INDEX,
        HC_STORE_INDEX,
        HC_SLICE,
        HC_STORE_SLICE,
        HC_CLASS_FIND,
        HC_FIELD,
        HC_STORE_FIELD,
        HC_SEQ_INFO,
        HC_EQ_AGG,
    ] {
        out.push_str(h);
        out.push('\n');
    }
}

// ---------- 断言内建 helper（失败写全局 @hc_fail_msg） ----------

fn emit_assert_helpers(out: &mut String) {
    let amsg = "error.AssertFailed";
    let an = amsg.len() + 1;
    let cases: &[(&str, &str, &str)] = &[
        (
            "hc_expect",
            "%Value %x",
            "%b = call i1 @hc_truthy(%Value %x)",
        ),
        (
            "hc_expect_eq",
            "%Value %x, %Value %y",
            "%b = call i1 @hc_eq(%Value %x, %Value %y)",
        ),
        (
            "hc_expect_neq",
            "%Value %x, %Value %y",
            "%c = call i1 @hc_eq(%Value %x, %Value %y)\n  %b = xor i1 %c, true",
        ),
        (
            "hc_expect_error",
            "%Value %x, %Value %y",
            "%ea = call i1 @hc_is_err(%Value %x)\n  %eb = call i1 @hc_is_err(%Value %y)\n  %eq = call i1 @hc_eq(%Value %x, %Value %y)\n  %e1 = and i1 %ea, %eb\n  %b = and i1 %e1, %eq",
        ),
        (
            "hc_expect_eq_slices",
            "%Value %x, %Value %y",
            "%b = call i1 @hc_eq(%Value %x, %Value %y)",
        ),
    ];
    for (fname, params, cond) in cases {
        let _ = writeln!(out, "define %Value @{fname}({params}) {{");
        let _ = writeln!(out, "entry:");
        for line in cond.lines() {
            let _ = writeln!(out, "  {line}");
        }
        let _ = writeln!(out, "  br i1 %b, label %ok, label %fail");
        let _ = writeln!(out, "ok:");
        let _ = writeln!(out, "  ret %Value {{ i32 0, i128 0 }}");
        let _ = writeln!(out, "fail:");
        let _ = writeln!(out, "  %fail_msg = getelementptr inbounds [{an} x i8], ptr @.msg_assert, i64 0, i64 0");
        let _ = writeln!(out, "  store i8* %fail_msg, i8** @hc_fail_msg");
        let _ = writeln!(out, "  ret %Value {{ i32 0, i128 0 }}");
        let _ = writeln!(out, "}}\n");
    }
}

// ---------- 函数发射 ----------

fn emit_func(
    out: &mut String,
    f: &IrFunc,
    idx: usize,
    strings: &[String],
    errors: &ErrorCodeTable,
    canon: &HashMap<String, String>,
) {
    let _ = writeln!(out, "; hc_fn{idx} = {}", f.name);
    let params = (0..f.params.len())
        .map(|i| format!("%Value %p{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(out, "define %Value @\"hc_fn{idx}\"({params}) {{");
    // 序言（槽数组 + 参数存槽）并入 entry 块（BodyEmitter 首个块即 entry）
    let mut be = BodyEmitter::new();
    be.emit(format!("%slots = alloca [{} x %Value], align 16", f.n_slots));
    for i in 0..f.n_slots {
        be.emit(format!(
            "%sp.{i} = getelementptr inbounds [{n} x %Value], [{n} x %Value]* %slots, i32 0, i32 {i}",
            n = f.n_slots
        ));
    }
    for (i, ps) in f.params.iter().enumerate() {
        be.emit(format!("store %Value %p{i}, %Value* %sp.{ps}"));
    }
    for inst in &f.body {
        be.inst(inst, strings, errors, canon);
    }
    out.push_str(&be.finish());
    out.push_str("}\n\n");
}

// ---------- main 包装（原生 CRT 入口） ----------

fn emit_main_wrapper(out: &mut String, module: &IrModule) {
    out.push_str("define i32 @main(i32 %argc, i8** %argv) {\n");
    if let Some(&idx) = module.func_index.get("main") {
        let nparams = module.funcs[idx].params.len();
        if nparams > 0 {
            out.push_str("  %argvoid = load %Value, %Value* @.void_value\n");
        }
        let mut arglist = String::new();
        for _ in 0..nparams {
            if !arglist.is_empty() {
                arglist.push_str(", ");
            }
            arglist.push_str("%Value %argvoid");
        }
        let _ = writeln!(out, "  %r = call %Value @\"hc_fn{idx}\"({arglist})");
        out.push_str("  %tag = extractvalue %Value %r, 0\n");
        out.push_str("  %is_err = icmp eq i32 %tag, 6\n");
        out.push_str("  br i1 %is_err, label %err_exit, label %ok\n");
        out.push_str("err_exit:\n  call void @hc_abort_unhandled()\n  unreachable\n");
        out.push_str("ok:\n  ret i32 0\n");
    } else {
        out.push_str("  ret i32 0\n");
    }
    out.push_str("}\n");
}

/// 原生测试跑器（Q-T5）：按声明序调用每个 `test fn`，全部通过 `ret 0`。
/// 断言失败在测试函数 ret 路径 abort(exit 1)；`return error.X` 由跑器检测 error tag 后
/// abort(exit 1)。因断言失败即 abort，逐测试续跑需重做 assert→返回码通路——故
/// `hc test --mode=compile` 为文件粒度交叉验证（全绿 vs 有失败）。
fn emit_test_runner(out: &mut String, module: &IrModule) {
    let tests: Vec<(usize, &IrFunc)> = module
        .funcs
        .iter()
        .enumerate()
        .filter(|(_, f)| f.is_test)
        .collect();

    // 每个 [test] fn 的运行/通过标记字符串（模块级全局）
    for (idx, f) in &tests {
        let run = format!("[RUN] {}", f.name);
        let pass = format!("[PASS] {}", f.name);
        let _ = writeln!(
            out,
            "@.test.{idx}.run = private unnamed_addr constant [{} x i8] c\"{}\\00\"",
            run.len() + 1,
            llvm_escape(run.as_bytes())
        );
        let _ = writeln!(
            out,
            "@.test.{idx}.pass = private unnamed_addr constant [{} x i8] c\"{}\\00\"",
            pass.len() + 1,
            llvm_escape(pass.as_bytes())
        );
    }

    out.push_str("define i32 @main(i32 %argc, i8** %argv) {\n");
    out.push_str("  %argvoid = load %Value, %Value* @.void_value\n");
    for (idx, f) in &tests {
        let run = format!("[RUN] {}", f.name);
        let rn = run.len() + 1;
        let _ = writeln!(
            out,
            "  %runp_{idx} = getelementptr inbounds [{rn} x i8], ptr @.test.{idx}.run, i64 0, i64 0"
        );
        let _ = writeln!(out, "  call i32 @puts(i8* %runp_{idx})");
        let arglist = (0..f.params.len())
            .map(|_| "%Value %argvoid")
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "  %r_{idx} = call %Value @\"hc_fn{idx}\"({arglist})");
        let _ = writeln!(out, "  %tag_{idx} = extractvalue %Value %r_{idx}, 0");
        let _ = writeln!(out, "  %is_err_{idx} = icmp eq i32 %tag_{idx}, 6");
        let _ = writeln!(out, "  br i1 %is_err_{idx}, label %fail_{idx}, label %ok_{idx}");
        let _ = writeln!(out, "fail_{idx}:");
        out.push_str("  call void @hc_abort_unhandled()\n  unreachable\n");
        let pass = format!("[PASS] {}", f.name);
        let pn = pass.len() + 1;
        let _ = writeln!(out, "ok_{idx}:");
        let _ = writeln!(
            out,
            "  %passp_{idx} = getelementptr inbounds [{pn} x i8], ptr @.test.{idx}.pass, i64 0, i64 0"
        );
        let _ = writeln!(out, "  call i32 @puts(i8* %passp_{idx})");
    }
    out.push_str("  ret i32 0\n}\n");
}

// ---------- 函数体发射（线性 IR → 基本块 CFG） ----------

struct BodyEmitter {
    ssa: usize,
    fresh: usize,
    blocks: Vec<String>,
    cur: String,
    terminated: bool,
}

impl BodyEmitter {
    fn new() -> Self {
        BodyEmitter {
            ssa: 0,
            fresh: 0,
            blocks: Vec::new(),
            cur: "entry:\n".to_string(),
            terminated: false,
        }
    }

    fn r(&mut self) -> String {
        let n = self.ssa;
        self.ssa += 1;
        format!("%r{n}")
    }

    fn fb(&mut self) -> String {
        let n = self.fresh;
        self.fresh += 1;
        format!("fb{n}")
    }

    fn emit(&mut self, line: String) {
        self.cur.push_str("  ");
        self.cur.push_str(&line);
        self.cur.push('\n');
    }

    fn term(&mut self, line: String) {
        self.emit(line);
        self.terminated = true;
        self.close_block();
    }

    fn close_block(&mut self) {
        if !self.terminated {
            self.cur.push_str("  unreachable\n");
        }
        self.blocks.push(std::mem::take(&mut self.cur));
        self.terminated = false;
    }

    fn label(&mut self, id: usize) {
        if !self.terminated {
            let _ = writeln!(self.cur, "  br label %L{id}");
            self.terminated = true;
        }
        self.close_block();
        self.cur = format!("L{id}:\n");
        self.terminated = false;
    }

    fn cond_br(&mut self, cond: &str, label: usize) {
        let fb = self.fb();
        self.term(format!("br i1 {cond}, label %L{label}, label %{fb}"));
        self.cur = format!("{fb}:\n");
        self.terminated = false;
    }

    fn finish(mut self) -> String {
        if !self.cur.is_empty() {
            self.close_block();
        }
        self.blocks.join("")
    }

    fn build_store(&mut self, temp: usize, tag: i32, data: String) {
        let v0 = self.r();
        self.emit(format!("{v0} = insertvalue %Value {{ i32 0, i128 0 }}, i32 {tag}, 0"));
        let v1 = self.r();
        self.emit(format!("{v1} = insertvalue %Value {v0}, i128 {data}, 1"));
        self.emit(format!("store %Value {v1}, %Value* %sp.{temp}"));
    }

    fn const_(&mut self, temp: usize, val: &IrConst, strings: &[String], errors: &ErrorCodeTable) {
        if let IrConst::Str(s) = val {
            let idx = strings.iter().position(|x| x == s).unwrap_or(0);
            let n = s.len() + 1;
            let p = self.r();
            self.emit(format!("{p} = getelementptr inbounds [{n} x i8], ptr @.str.{idx}, i64 0, i64 0"));
            let pi = self.r();
            self.emit(format!("{pi} = ptrtoint i8* {p} to i128"));
            self.build_store(temp, T_STR, pi);
            return;
        }
        let (tag, data) = match val {
            IrConst::Int(i) => (T_INT, format!("{i}")),
            IrConst::Float(f) => (T_FLOAT, format!("{}", f.to_bits() as u128)),
            IrConst::Bool(b) => (T_BOOL, if *b { "1" } else { "0" }.to_string()),
            IrConst::Void => (T_VOID, "0".to_string()),
            IrConst::Null => (T_NULL, "0".to_string()),
            IrConst::Err { name, .. } => (T_ERR, format!("{}", errors.code_of(name).unwrap_or(0))),
            IrConst::End => (T_END, "0".to_string()),
            IrConst::Str(_) => unreachable!(),
        };
        self.build_store(temp, tag, data);
    }

    fn emit_bool_store(&mut self, temp: usize, cond: &str) {
        let b = self.r();
        self.emit(format!("{b} = call %Value @hc_bool(i1 {cond})"));
        self.emit(format!("store %Value {b}, %Value* %sp.{temp}"));
    }

    fn bin(&mut self, op: IrBinOp, temp: usize, a: usize, b: usize) {
        let va = self.r();
        self.emit(format!("{va} = load %Value, %Value* %sp.{a}"));
        let vb = self.r();
        self.emit(format!("{vb} = load %Value, %Value* %sp.{b}"));
        match op {
            IrBinOp::Add | IrBinOp::Sub | IrBinOp::Mul | IrBinOp::Div | IrBinOp::Mod
            | IrBinOp::EucMod | IrBinOp::BitAnd | IrBinOp::BitOr | IrBinOp::BitXor
            | IrBinOp::Shl | IrBinOp::Shr => {
                let helper = match op {
                    IrBinOp::Add => "hc_add",
                    IrBinOp::Sub => "hc_sub",
                    IrBinOp::Mul => "hc_mul",
                    IrBinOp::Div => "hc_div",
                    IrBinOp::Mod => "hc_mod",
                    IrBinOp::EucMod => "hc_eucmod",
                    IrBinOp::BitAnd => "hc_bitand",
                    IrBinOp::BitOr => "hc_bitor",
                    IrBinOp::BitXor => "hc_bitxor",
                    IrBinOp::Shl => "hc_shl",
                    IrBinOp::Shr => "hc_shr",
                    _ => unreachable!(),
                };
                let res = self.r();
                self.emit(format!("{res} = call %Value @{helper}(%Value {va}, %Value {vb})"));
                self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            }
            IrBinOp::Eq | IrBinOp::Ne | IrBinOp::Lt | IrBinOp::Le | IrBinOp::Gt | IrBinOp::Ge => {
                match op {
                    IrBinOp::Eq => {
                        let c = self.r();
                        self.emit(format!("{c} = call i1 @hc_eq(%Value {va}, %Value {vb})"));
                        self.emit_bool_store(temp, &c);
                    }
                    IrBinOp::Ne => {
                        let c = self.r();
                        self.emit(format!("{c} = call i1 @hc_eq(%Value {va}, %Value {vb})"));
                        let n = self.r();
                        self.emit(format!("{n} = xor i1 {c}, true"));
                        self.emit_bool_store(temp, &n);
                    }
                    IrBinOp::Lt => {
                        let c = self.r();
                        self.emit(format!("{c} = call i1 @hc_lt(%Value {va}, %Value {vb})"));
                        self.emit_bool_store(temp, &c);
                    }
                    IrBinOp::Le => {
                        let l = self.r();
                        self.emit(format!("{l} = call i1 @hc_lt(%Value {va}, %Value {vb})"));
                        let e = self.r();
                        self.emit(format!("{e} = call i1 @hc_eq(%Value {va}, %Value {vb})"));
                        let o = self.r();
                        self.emit(format!("{o} = or i1 {l}, {e}"));
                        self.emit_bool_store(temp, &o);
                    }
                    IrBinOp::Gt => {
                        let l = self.r();
                        self.emit(format!("{l} = call i1 @hc_lt(%Value {va}, %Value {vb})"));
                        let e = self.r();
                        self.emit(format!("{e} = call i1 @hc_eq(%Value {va}, %Value {vb})"));
                        let o = self.r();
                        self.emit(format!("{o} = or i1 {l}, {e}"));
                        let g = self.r();
                        self.emit(format!("{g} = xor i1 {o}, true"));
                        self.emit_bool_store(temp, &g);
                    }
                    IrBinOp::Ge => {
                        let l = self.r();
                        self.emit(format!("{l} = call i1 @hc_lt(%Value {va}, %Value {vb})"));
                        let g = self.r();
                        self.emit(format!("{g} = xor i1 {l}, true"));
                        self.emit_bool_store(temp, &g);
                    }
                    _ => unreachable!(),
                }
            }
        }
    }

    fn un(&mut self, op: IrUnOp, temp: usize, a: usize) {
        let va = self.r();
        self.emit(format!("{va} = load %Value, %Value* %sp.{a}"));
        let helper = match op {
            IrUnOp::Neg => "hc_neg",
            IrUnOp::Not => "hc_not",
            IrUnOp::BitNot => "hc_bitnot",
        };
        let res = self.r();
        self.emit(format!("{res} = call %Value @{helper}(%Value {va})"));
        self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
    }

    fn call(&mut self, name: &str, args: &[usize], temp: usize, canon: &HashMap<String, String>) {
        let mut arglist = String::new();
        for a in args {
            let v = self.r();
            self.emit(format!("{v} = load %Value, %Value* %sp.{a}"));
            if !arglist.is_empty() {
                arglist.push_str(", ");
            }
            arglist.push_str(&format!("%Value {v}"));
        }
        let res = self.r();
        match canon.get(name) {
            Some(sym) => self.emit(format!("{res} = call %Value @\"{sym}\"({arglist})")),
            None => self.emit(format!("{res} = call %Value @hc_no_function()")),
        }
        self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
    }

    fn call_builtin(&mut self, name: &str, args: &[usize], temp: usize) {
        let helper = match name {
            "expect" => Some("hc_expect"),
            "expect_eq" => Some("hc_expect_eq"),
            "expect_neq" => Some("hc_expect_neq"),
            "expect_error" => Some("hc_expect_error"),
            "expect_eq_slices" => Some("hc_expect_eq_slices"),
            _ => None,
        };
        let Some(helper) = helper else {
            // 切片外内建（@ 内建等）→ void 占位（对齐 call_assert_builtin 默认 Void）
            self.emit(format!("store %Value {{ i32 0, i128 0 }}, %Value* %sp.{temp}"));
            return;
        };
        let mut arglist = String::new();
        for a in args {
            let v = self.r();
            self.emit(format!("{v} = load %Value, %Value* %sp.{a}"));
            if !arglist.is_empty() {
                arglist.push_str(", ");
            }
            arglist.push_str(&format!("%Value {v}"));
        }
        let res = self.r();
        self.emit(format!("{res} = call %Value @{helper}({arglist})"));
        self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
    }

    fn ret(&mut self, slot: usize) {
        let f = self.r();
        self.emit(format!("{f} = load i8*, i8** @hc_fail_msg"));
        let has = self.r();
        self.emit(format!("{has} = icmp ne i8* {f}, null"));
        let fb_fail = self.fb();
        let fb_ok = self.fb();
        self.term(format!("br i1 {has}, label %{fb_fail}, label %{fb_ok}"));
        self.blocks.push(format!("{fb_fail}:\n  call void @hc_abort(i8* {f})\n  unreachable\n"));
        self.cur = format!("{fb_ok}:\n");
        self.terminated = false;
        let v = self.r();
        self.emit(format!("{v} = load %Value, %Value* %sp.{slot}"));
        self.term(format!("ret %Value {v}"));
    }

    fn ret_void(&mut self) {
        let f = self.r();
        self.emit(format!("{f} = load i8*, i8** @hc_fail_msg"));
        let has = self.r();
        self.emit(format!("{has} = icmp ne i8* {f}, null"));
        let fb_fail = self.fb();
        let fb_ok = self.fb();
        self.term(format!("br i1 {has}, label %{fb_fail}, label %{fb_ok}"));
        self.blocks.push(format!("{fb_fail}:\n  call void @hc_abort(i8* {f})\n  unreachable\n"));
        self.cur = format!("{fb_ok}:\n");
        self.terminated = false;
        self.term("ret %Value { i32 0, i128 0 }".to_string());
    }

    fn inst(
        &mut self,
        inst: &IrInst,
        strings: &[String],
        errors: &ErrorCodeTable,
        canon: &HashMap<String, String>,
    ) {
        match inst {
            IrInst::Const { temp, val } => self.const_(*temp, val, strings, errors),
            IrInst::Load { temp, slot } => {
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{slot}"));
                self.emit(format!("store %Value {v}, %Value* %sp.{temp}"));
            }
            IrInst::Store { slot, temp } => {
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{temp}"));
                self.emit(format!("store %Value {v}, %Value* %sp.{slot}"));
            }
            IrInst::Bin { op, temp, a, b } => self.bin(*op, *temp, *a, *b),
            IrInst::Un { op, temp, a } => self.un(*op, *temp, *a),
            IrInst::Jump { label } => {
                self.term(format!("br label %L{label}"));
                let fb = self.fb();
                self.cur = format!("{fb}:\n");
                self.terminated = false;
            }
            IrInst::JumpIf { temp, label } => {
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{temp}"));
                let c = self.r();
                self.emit(format!("{c} = call i1 @hc_truthy(%Value {v})"));
                self.cond_br(&c, *label);
            }
            IrInst::JumpIfNot { temp, label } => {
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{temp}"));
                let c = self.r();
                self.emit(format!("{c} = call i1 @hc_truthy(%Value {v})"));
                let n = self.r();
                self.emit(format!("{n} = xor i1 {c}, true"));
                self.cond_br(&n, *label);
            }
            IrInst::JumpIfNull { temp, label } => {
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{temp}"));
                let c = self.r();
                self.emit(format!("{c} = call i1 @hc_is_null(%Value {v})"));
                self.cond_br(&c, *label);
            }
            IrInst::JumpIfErr { temp, label } => {
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{temp}"));
                let c = self.r();
                self.emit(format!("{c} = call i1 @hc_is_err(%Value {v})"));
                self.cond_br(&c, *label);
            }
            IrInst::Label { id } => self.label(*id),
            // Phase 1 指针：取址 = 槽地址入 tag 7 载荷；解引用/写穿经运行时 helper。
            // AddrValue 与 AddrSlot 同一 codegen——快照值已在临时槽中，地址即该槽。
            IrInst::AddrSlot { temp, slot } => {
                let p = self.r();
                self.emit(format!("{p} = ptrtoint %Value* %sp.{slot} to i128"));
                self.build_store(*temp, T_PTR, p);
            }
            IrInst::AddrValue { temp, value } => {
                let p = self.r();
                self.emit(format!("{p} = ptrtoint %Value* %sp.{value} to i128"));
                self.build_store(*temp, T_PTR, p);
            }
            IrInst::Deref { temp, a } => {
                let va = self.r();
                self.emit(format!("{va} = load %Value, %Value* %sp.{a}"));
                let res = self.r();
                self.emit(format!("{res} = call %Value @hc_deref(%Value {va})"));
                self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            }
            IrInst::StorePtr { target, value } => {
                let vt = self.r();
                self.emit(format!("{vt} = load %Value, %Value* %sp.{target}"));
                let vv = self.r();
                self.emit(format!("{vv} = load %Value, %Value* %sp.{value}"));
                self.emit(format!("call void @hc_store_ptr(%Value {vt}, %Value {vv})"));
            }
            // ---- Phase 2 聚合 ----
            IrInst::Field { temp, base, field } => {
                let b = self.r();
                self.emit(format!("{b} = load %Value, %Value* %sp.{base}"));
                let (si, sn) = str_idx(strings, field);
                let fg = self.r();
                self.emit(format!("{fg} = getelementptr inbounds [{sn} x i8], ptr @.str.{si}, i64 0, i64 0"));
                let res = self.r();
                self.emit(format!("{res} = call %Value @hc_field(%Value {b}, i8* {fg})"));
                self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            }
            IrInst::StoreField { base, field, value } => {
                let b = self.r();
                self.emit(format!("{b} = load %Value, %Value* %sp.{base}"));
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{value}"));
                let (si, sn) = str_idx(strings, field);
                let fg = self.r();
                self.emit(format!("{fg} = getelementptr inbounds [{sn} x i8], ptr @.str.{si}, i64 0, i64 0"));
                self.emit(format!("call void @hc_store_field(%Value {b}, i8* {fg}, %Value {v})"));
            }
            IrInst::Index { temp, base, index } => {
                let b = self.r();
                self.emit(format!("{b} = load %Value, %Value* %sp.{base}"));
                let i = self.r();
                self.emit(format!("{i} = load %Value, %Value* %sp.{index}"));
                let res = self.r();
                self.emit(format!("{res} = call %Value @hc_index(%Value {b}, %Value {i})"));
                self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            }
            IrInst::StoreIndex { base, index, value } => {
                let b = self.r();
                self.emit(format!("{b} = load %Value, %Value* %sp.{base}"));
                let i = self.r();
                self.emit(format!("{i} = load %Value, %Value* %sp.{index}"));
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{value}"));
                self.emit(format!("call void @hc_store_index(%Value {b}, %Value {i}, %Value {v})"));
            }
            IrInst::SliceOf { temp, base, lo, hi } => {
                let b = self.r();
                self.emit(format!("{b} = load %Value, %Value* %sp.{base}"));
                let lo_v = self.r();
                self.emit(format!("{lo_v} = load %Value, %Value* %sp.{lo}"));
                let hi_v = self.r();
                self.emit(format!("{hi_v} = load %Value, %Value* %sp.{hi}"));
                let res = self.r();
                self.emit(format!("{res} = call %Value @hc_slice(%Value {b}, %Value {lo_v}, %Value {hi_v})"));
                self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            }
            IrInst::StoreSlice { base, lo, hi, value } => {
                let b = self.r();
                self.emit(format!("{b} = load %Value, %Value* %sp.{base}"));
                let lo_v = self.r();
                self.emit(format!("{lo_v} = load %Value, %Value* %sp.{lo}"));
                let hi_v = self.r();
                self.emit(format!("{hi_v} = load %Value, %Value* %sp.{hi}"));
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{value}"));
                self.emit(format!("call void @hc_store_slice(%Value {b}, %Value {lo_v}, %Value {hi_v}, %Value {v})"));
            }
            IrInst::MakeArr { temp, items } => {
                let arr = self.r();
                self.emit(format!("{arr} = call %Value @hc_make_arr(i64 {})", items.len()));
                for (i, it) in items.iter().enumerate() {
                    let v = self.r();
                    self.emit(format!("{v} = load %Value, %Value* %sp.{it}"));
                    self.emit(format!("call void @hc_arr_set(%Value {arr}, i64 {i}, %Value {v})"));
                }
                self.emit(format!("store %Value {arr}, %Value* %sp.{temp}"));
            }
            IrInst::MakeClass { temp, ty, fields } => {
                let (ti, tn) = str_idx(strings, ty);
                let tyg = self.r();
                self.emit(format!("{tyg} = getelementptr inbounds [{tn} x i8], ptr @.str.{ti}, i64 0, i64 0"));
                let cls = self.r();
                self.emit(format!("{cls} = call %Value @hc_make_class(i8* {tyg}, i64 {})", fields.len()));
                for (i, (fname, vslot)) in fields.iter().enumerate() {
                    let v = self.r();
                    self.emit(format!("{v} = load %Value, %Value* %sp.{vslot}"));
                    let (fi, flen) = str_idx(strings, fname);
                    let fg = self.r();
                    self.emit(format!("{fg} = getelementptr inbounds [{flen} x i8], ptr @.str.{fi}, i64 0, i64 0"));
                    self.emit(format!("call void @hc_class_set(%Value {cls}, i64 {i}, i8* {fg}, %Value {v})"));
                }
                self.emit(format!("store %Value {cls}, %Value* %sp.{temp}"));
            }
            IrInst::MakeEnum { temp, name, variant, payload } => {
                let (ni, nn) = str_idx(strings, name);
                let ng = self.r();
                self.emit(format!("{ng} = getelementptr inbounds [{nn} x i8], ptr @.str.{ni}, i64 0, i64 0"));
                let (vi, vn) = str_idx(strings, variant);
                let vg = self.r();
                self.emit(format!("{vg} = getelementptr inbounds [{vn} x i8], ptr @.str.{vi}, i64 0, i64 0"));
                match payload {
                    Some(pslot) => {
                        let pv = self.r();
                        self.emit(format!("{pv} = load %Value, %Value* %sp.{pslot}"));
                        let sz = self.r();
                        self.emit(format!("{sz} = ptrtoint %Value* getelementptr (%Value, %Value* null, i32 1) to i64"));
                        let raw = self.r();
                        self.emit(format!("{raw} = call i8* @hc_alloc(i64 {sz})"));
                        let cell = self.r();
                        self.emit(format!("{cell} = bitcast i8* {raw} to %Value*"));
                        self.emit(format!("store %Value {pv}, %Value* {cell}"));
                        let res = self.r();
                        self.emit(format!("{res} = call %Value @hc_make_enum(i8* {ng}, i8* {vg}, %Value* {cell})"));
                        self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
                    }
                    None => {
                        let res = self.r();
                        self.emit(format!("{res} = call %Value @hc_make_enum(i8* {ng}, i8* {vg}, %Value* null)"));
                        self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
                    }
                }
            }
            IrInst::Destructure { value, slots } => {
                let n = slots.len() as i64;
                let sv = self.r();
                self.emit(format!("{sv} = load %Value, %Value* %sp.{value}"));
                let dv = self.r();
                self.emit(format!("{dv} = call %Value @hc_deref(%Value {sv})"));
                let st = self.r();
                self.emit(format!("{st} = extractvalue %Value {dv}, 0"));
                let ia = self.r();
                self.emit(format!("{ia} = icmp eq i32 {st}, 8"));
                let l_arr = self.fb();
                let l_tup = self.fb();
                self.term(format!("br i1 {ia}, label %{l_arr}, label %{l_tup}"));
                self.blocks.push(format!("{l_tup}:\n  call void @hc_abort_tuplearity()\n  unreachable\n"));
                self.cur = format!("{l_arr}:\n");
                self.terminated = false;
                let sd = self.r();
                self.emit(format!("{sd} = extractvalue %Value {dv}, 1"));
                let op = self.r();
                self.emit(format!("{op} = inttoptr i128 {sd} to %ArrObj*"));
                let ao = self.r();
                self.emit(format!("{ao} = load %ArrObj, %ArrObj* {op}"));
                let alen = self.r();
                self.emit(format!("{alen} = extractvalue %ArrObj {ao}, 0"));
                let items = self.r();
                self.emit(format!("{items} = extractvalue %ArrObj {ao}, 1"));
                let le = self.r();
                self.emit(format!("{le} = icmp eq i64 {alen}, {n}"));
                let l_slots = self.fb();
                let l_arity = self.fb();
                self.term(format!("br i1 {le}, label %{l_slots}, label %{l_arity}"));
                self.blocks.push(format!("{l_arity}:\n  call void @hc_abort_tuplearity()\n  unreachable\n"));
                self.cur = format!("{l_slots}:\n");
                self.terminated = false;
                for (i, s) in slots.iter().enumerate() {
                    if let Some(slot) = s {
                        let p = self.r();
                        self.emit(format!("{p} = getelementptr %Value, %Value* {items}, i64 {i}"));
                        let ev = self.r();
                        self.emit(format!("{ev} = load %Value, %Value* {p}"));
                        self.emit(format!("store %Value {ev}, %Value* %sp.{slot}"));
                    }
                }
            }
            IrInst::Move { temp, a } => {
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{a}"));
                self.emit(format!("store %Value {v}, %Value* %sp.{temp}"));
            }
            IrInst::Unwrap { temp, a } => {
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{a}"));
                let res = self.r();
                self.emit(format!("{res} = call %Value @hc_unwrap(%Value {v})"));
                self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            }
            IrInst::Call { name, args, temp } => self.call(name, args, *temp, canon),
            IrInst::CallBuiltin { name, args, temp } => self.call_builtin(name, args, *temp),
            IrInst::Return { temp } => self.ret(*temp),
            IrInst::ReturnVoid => self.ret_void(),
        }
    }
}

// ---------- 纯文本发射测试（不依赖 zig/clang） ----------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir;

    fn gen(src: &str) -> String {
        let p = crate::parse_source(src).expect("parse");
        let m = ir::lower(&p).expect("lower");
        let t = crate::errorcodes::collect(&p, 0);
        codegen(&m, &t)
    }

    #[test]
    fn scalar_main_defines_and_wrapper() {
        let ll = gen("fn main() i32 { return 42; }");
        assert!(ll.contains("define %Value @\"hc_fn0\"()"), "{ll}");
        assert!(ll.contains("define i32 @main(i32 %argc, i8** %argv)"), "{ll}");
        assert!(ll.contains("i128 42"), "{ll}");
        assert!(ll.contains("ret %Value"), "{ll}");
    }

    #[test]
    fn add_emits_overflow_intrinsic() {
        let ll = gen("fn add(a: i32, b: i32) i32 { return a + b; }");
        assert!(ll.contains("@llvm.sadd.with.overflow.i128"), "{ll}");
        assert!(ll.contains("define %Value @hc_add"), "{ll}");
    }

    #[test]
    fn err_literal_uses_error_code() {
        let ll = gen("fn f() !i32 { return error.NotFound; }");
        assert!(ll.contains("i32 6"), "{ll}"); // err tag
        // error.NotFound 码 = 0（包 ID 0 + 首个错误）
        assert!(ll.contains("insertvalue %Value { i32 0, i128 0 }, i32 6, 0"), "{ll}");
    }

    #[test]
    fn string_literal_emits_global() {
        let ll = gen("fn f() String { return \"hi\"; }");
        assert!(ll.contains("@.str.0 = private"), "{ll}");
        assert!(ll.contains("c\"hi\\00\""), "{ll}");
        assert!(ll.contains("ptrtoint i8*"), "{ll}");
    }

    #[test]
    fn if_while_emit_labels_and_branches() {
        let ll = gen(
            "fn sum(n: i32) i32 { var mut i: i32 = 0; var mut s: i32 = 0; while (i < n) : (i += 1) { s += i; } return s; }",
        );
        assert!(ll.contains("br label %L"), "{ll}");
        assert!(ll.contains("br i1"), "{ll}");
        assert!(ll.contains("call i1 @hc_truthy"), "{ll}");
    }

    #[test]
    fn try_catch_uses_err_channel() {
        let ll = gen("fn f() !i32 { return error.NotFound; } fn g() i32 { return f() catch 7; }");
        assert!(ll.contains("call i1 @hc_is_err"), "{ll}");
    }

    #[test]
    fn pointer_addr_of_deref_store_emit_helpers() {
        // `&mut x` → AddrSlot（槽地址入 tag 7）；`p.*` → Deref；`p.* = 7` → StorePtr
        let ll = gen("fn f() i32 { var mut x: i32 = 5; var p = &mut x; p.* = 7; return x; }");
        assert!(ll.contains("define %Value @hc_deref"), "{ll}");
        assert!(ll.contains("define void @hc_store_ptr"), "{ll}");
        assert!(ll.contains("ptrtoint %Value* %sp."), "{ll}");
        assert!(ll.contains("call %Value @hc_deref"), "{ll}");
        assert!(ll.contains("call void @hc_store_ptr"), "{ll}");
        // 写穿返回结果应等于 7（常量 7 入载荷）
        assert!(ll.contains("i128 7"), "{ll}");
    }

    #[test]
    fn pointer_bad_assign_has_message() {
        let ll = gen("fn f() void { return; }");
        assert!(ll.contains("@.msg_badassign"), "{ll}");
        assert!(ll.contains("error.BadAssign"), "{ll}");
    }

    #[test]
    fn pointer_eq_uses_identity_dispatch() {
        // 同目标指针相等 → hc_eq 分流到纯值比较（先 hc_deref 归一化）
        let ll = gen("fn f() bool { var mut x: i32 = 5; var p = &mut x; var q = &mut x; return p == q; }");
        assert!(ll.contains("define i1 @hc_eq("), "{ll}");
        assert!(ll.contains("define i1 @hc_eq_plain("), "{ll}");
        assert!(ll.contains("call i1 @hc_eq_plain"), "{ll}");
        assert!(ll.contains("call %Value @hc_deref"), "{ll}");
    }

    #[test]
    fn pointer_write_through_deref_value() {
        // 复合赋值 `p.* += 1`：Deref + Bin + StorePtr 三序列齐
        let ll = gen("fn f() i32 { var mut x: i32 = 5; var p = &mut x; p.* += 1; return p.*; }");
        assert!(ll.contains("call %Value @hc_deref"), "{ll}");
        assert!(ll.contains("call %Value @hc_add"), "{ll}");
        assert!(ll.contains("call void @hc_store_ptr"), "{ll}");
    }

    // ---- Phase 2 聚合发射 ----

    #[test]
    fn aggregate_field_emits_class_helpers() {
        // MakeClass/Field/StoreField → hc_make_class/hc_class_set/hc_field/hc_store_field
        let ll = gen(
            r#"class P { x: i32, y: i32, } fn f() i32 { var p = P{ x = 1, y = 2 }; p.y = 5; return p.x; }"#,
        );
        assert!(ll.contains("define %Value @hc_make_class"), "{ll}");
        assert!(ll.contains("define void @hc_class_set"), "{ll}");
        assert!(ll.contains("define %Value @hc_field"), "{ll}");
        assert!(ll.contains("define void @hc_store_field"), "{ll}");
        assert!(ll.contains("call %Value @hc_make_class"), "{ll}");
        assert!(ll.contains("call void @hc_class_set"), "{ll}");
        assert!(ll.contains("call %Value @hc_field"), "{ll}");
        assert!(ll.contains("call void @hc_store_field"), "{ll}");
        // 字符串全局（类型名 + 字段名）被收集
        assert!(ll.contains("@.str.0 = private"), "{ll}");
    }

    #[test]
    fn aggregate_array_index_emits_arr_helpers() {
        // MakeArr/Index/StoreIndex → hc_make_arr/hc_arr_set/hc_index/hc_store_index
        let ll = gen(
            "fn f() i32 { var a = [10, 20, 30]; a[1] = 99; return a[1]; }",
        );
        assert!(ll.contains("define %Value @hc_make_arr"), "{ll}");
        assert!(ll.contains("define void @hc_arr_set"), "{ll}");
        assert!(ll.contains("define %Value @hc_index"), "{ll}");
        assert!(ll.contains("define void @hc_store_index"), "{ll}");
        assert!(ll.contains("call %Value @hc_make_arr"), "{ll}");
        assert!(ll.contains("call void @hc_arr_set"), "{ll}");
        assert!(ll.contains("call %Value @hc_index"), "{ll}");
        assert!(ll.contains("call void @hc_store_index"), "{ll}");
    }

    #[test]
    fn aggregate_slice_emits_slice_helpers() {
        // SliceOf/StoreSlice → hc_slice/hc_store_slice + %SliceObj
        let ll = gen(
            "fn f() i32 { var a = [1, 2, 3, 4, 5]; var s = a[1..3]; a[1..3] = [8, 9]; return s[0]; }",
        );
        assert!(ll.contains("define %Value @hc_slice"), "{ll}");
        assert!(ll.contains("define void @hc_store_slice"), "{ll}");
        assert!(ll.contains("%SliceObj = type"), "{ll}");
        assert!(ll.contains("call %Value @hc_slice"), "{ll}");
        assert!(ll.contains("call void @hc_store_slice"), "{ll}");
    }

    #[test]
    fn aggregate_enum_emits_make_enum() {
        // MakeEnum → hc_make_enum（name+variant 字符串 GEP）
        let ll = gen("enum Color { red, green, blue } fn f() Color { return Color.green; }");
        assert!(ll.contains("define %Value @hc_make_enum"), "{ll}");
        assert!(ll.contains("call %Value @hc_make_enum"), "{ll}");
    }

    #[test]
    fn aggregate_unwrap_emits_hc_unwrap() {
        let ll = gen("fn boxed(x: ?i32) ?i32 { return x; } fn f() i32 { return boxed(7).?; }");
        assert!(ll.contains("define %Value @hc_unwrap"), "{ll}");
        assert!(ll.contains("call %Value @hc_unwrap"), "{ll}");
        assert!(ll.contains("@.msg_nullunwrap"), "{ll}");
    }

    #[test]
    fn aggregate_deep_eq_emits_hc_eq_agg() {
        // 类/数组/枚举深比较 → hc_eq_agg（hc_eq 分流）
        let ll = gen(
            "fn f() bool { var a = [1, 2, 3]; var b = [1, 2, 3]; return a == b; }",
        );
        assert!(ll.contains("define i1 @hc_eq_agg"), "{ll}");
        assert!(ll.contains("call i1 @hc_eq_agg"), "{ll}");
        assert!(ll.contains("define %SeqInfo @hc_seq_info"), "{ll}");
    }

    #[test]
    fn aggregate_tuple_destructure_emits_split() {
        // Destructure：返回 `(i32, i32)` → `hc_make_arr` + 按索引取值
        let ll = gen(
            "fn d(a: i32, b: i32) (i32, i32) { return (a, b); } fn f() i32 { var (q, r) = d(3, 4); return q + r; }",
        );
        assert!(ll.contains("call %Value @hc_make_arr"), "{ll}");
    }

    #[test]
    fn no_inline_constant_expr_gep() {
        // LLVM 18+ 移除 `getelementptr` 常量表达式：所有 GEP 必须发射为
        // SSA 指令（`%rN = getelementptr ...` 或 sizeof 惯用法 `ptrtoint ...`），
        // 不得作为 call/select 操作数内联（`i8* getelementptr ...`）。
        let ll = gen(
            r#"class P { x: i32, } fn boxed(x: ?i32) ?i32 { return x; } fn f() i32 {
            var p = P{ x = 1 };
            var s = "abc";
            var a = [1, 2, 3];
            var v = boxed(7).?;
            if (p.x == s.len and a[0] == v) { return 1; }
            return 0; }"#,
        );
        for line in ll.lines() {
            let t = line.trim_start();
            // 指令形式（`%rN = ...`）与全局声明不含此特征；行内常量表达式
            // GEP 必然以 `i8* getelementptr` / `i8* (getelementptr` 形态出现。
            if t.contains("i8* getelementptr") || t.contains("i8* (getelementptr") {
                panic!("行内常量表达式 GEP 残留: {line}");
            }
        }
    }
}
