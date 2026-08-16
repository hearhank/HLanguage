//! M3.3 LLVM 原生后端（emit-.ll 文本 + 外部 `zig cc` 驱动）
//!
//! 与 M3.1 IR 参考解释器（`ir::run_ir`）共用 `IrModule`（ADR-0004 唯一语义源），
//! 逐条对齐 `exec_func` 的动态语义。IR 槽是无类型的 `IrValue`，首轮用**统一带标签
//! 值表示** `%Value = { i32 tag, i64 data }`（正确性优先），动态运算集中到导言 helper，
//! 避免每个 `Bin` 内联 tag-dispatch。
//!
//! 覆盖（tag1 切片）：标量 / 控制流 / 函数调用 / 错误值通道 / 断言内建。
//! 已知简化见 07-bootstrap-plan.md：i64 载荷（非 i128）、NUL 结尾字符串字面量、
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

/// 收集全部字符串常量（去重、保序）。
fn collect_strings(module: &IrModule) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<String> = Vec::new();
    for f in &module.funcs {
        for inst in &f.body {
            if let IrInst::Const { val: IrConst::Str(s), .. } = inst {
                if !seen.contains_key(s) {
                    seen.insert(s.clone(), out.len());
                    out.push(s.clone());
                }
            }
        }
    }
    out
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
    Msg { key: "unhandled", text: "error: unhandled error value reached entry point" },
];

/// `@.msg_{key}` 数组全局的 `getelementptr` 常量表达式（取 i8*）。
fn msg_gep(key: &str, text: &str) -> String {
    let n = text.len() + 1;
    format!("getelementptr inbounds ([{n} x i8], [{n} x i8]* @.msg_{key}, i64 0, i64 0)")
}

// ---------- 导言 ----------

fn emit_preamble(out: &mut String, strings: &[String]) {
    out.push_str("; H M3.3 LLVM 原生后端（自动生成；`zig cc file.ll -o file.exe`）\n\n");
    out.push_str("%Value = type { i32, i64 }\n\n");

    // 外部符号（libc + 溢出内建）
    out.push_str("declare i32 @strcmp(i8*, i8*)\n");
    out.push_str("declare i32 @puts(i8*)\n");
    out.push_str("declare void @exit(i32) noreturn\n");
    out.push_str("declare { i64, i1 } @llvm.sadd.with.overflow.i64(i64, i64)\n");
    out.push_str("declare { i64, i1 } @llvm.ssub.with.overflow.i64(i64, i64)\n");
    out.push_str("declare { i64, i1 } @llvm.smul.with.overflow.i64(i64, i64)\n\n");

    // 断言失败标志（全局；单线程顺序执行）
    out.push_str("@hc_fail_msg = global i8* null\n");
    out.push_str("@.void_value = private unnamed_addr constant %Value { i32 0, i64 0 }\n");
    out.push_str("@.empty_str_s = private unnamed_addr constant [1 x i8] c\"\\00\"\n\n");

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
        let gep = msg_gep(m.key, m.text);
        let _ = writeln!(out, "define void @hc_abort_{}() {{", m.key);
        let _ = writeln!(out, "  call void @hc_abort(i8* {gep})");
        out.push_str("  unreachable\n}\n\n");
    }
    // 切片外函数调用（运行时 NoFunction 硬错误）
    out.push_str("define %Value @hc_no_function() {\n  call void @hc_abort_nofunc()\n  unreachable\n}\n\n");

    emit_arith_helpers(out);
    emit_bit_helpers(out);
    emit_cmp_helpers(out);
    emit_unary_helpers(out);
    emit_assert_helpers(out);
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
  %res = call { i64, i1 } @INTRINSIC@(i64 %da, i64 %db)
  %rv = extractvalue { i64, i1 } %res, 0
  %ov = extractvalue { i64, i1 } %res, 1
  br i1 %ov, label %ovf, label %int_ok
int_ok:
  %i0 = insertvalue %Value { i32 0, i64 0 }, i32 2, 0
  %i1 = insertvalue %Value %i0, i64 %rv, 1
  ret %Value %i1
ovf:
  call void @hc_abort_overflow()
  unreachable
float_op:
  %fa_raw = bitcast i64 %da to double
  %fa_int = sitofp i64 %da to double
  %fa = select i1 %af, double %fa_raw, double %fa_int
  %fb_raw = bitcast i64 %db to double
  %fb_int = sitofp i64 %db to double
  %fb = select i1 %bf, double %fb_raw, double %fb_int
  %fr = @FOP@ double %fa, %fb
  %fr_bits = bitcast double %fr to i64
  %f0 = insertvalue %Value { i32 0, i64 0 }, i32 3, 0
  %f1 = insertvalue %Value %f0, i64 %fr_bits, 1
  ret %Value %f1
other:
  ret %Value { i32 2, i64 0 }
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
  %bz = icmp eq i64 %db, 0
  br i1 %bz, label %divzero, label %int_ok
int_ok:
  %rv = @IOP@ i64 %da, %db
  %i0 = insertvalue %Value { i32 0, i64 0 }, i32 2, 0
  %i1 = insertvalue %Value %i0, i64 %rv, 1
  ret %Value %i1
divzero:
  call void @hc_abort_divzero()
  unreachable
float_op:
  %fa_raw = bitcast i64 %da to double
  %fa_int = sitofp i64 %da to double
  %fa = select i1 %af, double %fa_raw, double %fa_int
  %fb_raw = bitcast i64 %db to double
  %fb_int = sitofp i64 %db to double
  %fb = select i1 %bf, double %fb_raw, double %fb_int
  %fr = @FOP@ double %fa, %fb
  %fr_bits = bitcast double %fr to i64
  %f0 = insertvalue %Value { i32 0, i64 0 }, i32 3, 0
  %f1 = insertvalue %Value %f0, i64 %fr_bits, 1
  ret %Value %f1
other:
  ret %Value { i32 2, i64 0 }
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
  %bz = icmp eq i64 %db, 0
  br i1 %bz, label %divzero, label %int_ok
int_ok:
  %rm = srem i64 %da, %db
  %rneg = icmp slt i64 %rm, 0
  %dbneg = icmp slt i64 %db, 0
  %dbnegv = sub i64 0, %db
  %mabs = select i1 %dbneg, i64 %dbnegv, i64 %db
  %rm2 = add i64 %rm, %mabs
  %rv = select i1 %rneg, i64 %rm2, i64 %rm
  %i0 = insertvalue %Value { i32 0, i64 0 }, i32 2, 0
  %i1 = insertvalue %Value %i0, i64 %rv, 1
  ret %Value %i1
divzero:
  call void @hc_abort_divzero()
  unreachable
float_op:
  %fa_raw = bitcast i64 %da to double
  %fa_int = sitofp i64 %da to double
  %fa = select i1 %af, double %fa_raw, double %fa_int
  %fb_raw = bitcast i64 %db to double
  %fb_int = sitofp i64 %db to double
  %fb = select i1 %bf, double %fb_raw, double %fb_int
  %fr = frem double %fa, %fb
  %fr_bits = bitcast double %fr to i64
  %f0 = insertvalue %Value { i32 0, i64 0 }, i32 3, 0
  %f1 = insertvalue %Value %f0, i64 %fr_bits, 1
  ret %Value %f1
other:
  ret %Value { i32 2, i64 0 }
}
"#;

fn emit_arith_helpers(out: &mut String) {
    for (fname, intr, fop) in [
        ("hc_add", "llvm.sadd.with.overflow.i64", "fadd"),
        ("hc_sub", "llvm.ssub.with.overflow.i64", "fsub"),
        ("hc_mul", "llvm.smul.with.overflow.i64", "fmul"),
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
  %r = @BOP@ i64 %da, %db
  %v0 = insertvalue %Value { i32 0, i64 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i64 %r, 1
  ret %Value %v1
other:
  ret %Value { i32 2, i64 0 }
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
  %sh = and i64 %db, 63
  %r = @SHOP@ i64 %da, %sh
  %v0 = insertvalue %Value { i32 0, i64 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i64 %r, 1
  ret %Value %v1
other:
  ret %Value { i32 2, i64 0 }
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

const HC_EQ: &str = r#"define i1 @hc_eq(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %a_int = icmp eq i32 %ta, 2
  %b_int = icmp eq i32 %tb, 2
  %ii_case = and i1 %a_int, %b_int
  %ii_eq = icmp eq i64 %da, %db
  %ii_res = and i1 %ii_case, %ii_eq
  %b_float = icmp eq i32 %tb, 3
  %if_case = and i1 %a_int, %b_float
  %if_fa = sitofp i64 %da to double
  %if_fb = bitcast i64 %db to double
  %if_eq = fcmp oeq double %if_fa, %if_fb
  %if_res = and i1 %if_case, %if_eq
  %a_float = icmp eq i32 %ta, 3
  %fi_case = and i1 %a_float, %b_int
  %fi_fa = bitcast i64 %da to double
  %fi_fb = sitofp i64 %db to double
  %fi_eq = fcmp oeq double %fi_fa, %fi_fb
  %fi_res = and i1 %fi_case, %fi_eq
  %ff_case = and i1 %a_float, %b_float
  %ff_eq = fcmp oeq double %fi_fa, %if_fb
  %ff_res = and i1 %ff_case, %ff_eq
  %a_bool = icmp eq i32 %ta, 4
  %b_bool = icmp eq i32 %tb, 4
  %bb_case = and i1 %a_bool, %b_bool
  %bb_eq = icmp eq i64 %da, %db
  %bb_res = and i1 %bb_case, %bb_eq
  %a_str = icmp eq i32 %ta, 5
  %b_str = icmp eq i32 %tb, 5
  %ss_case = and i1 %a_str, %b_str
  %ss_pa = inttoptr i64 %da to i8*
  %ss_pb = inttoptr i64 %db to i8*
  %ss_pa_s = select i1 %ss_case, i8* %ss_pa, i8* getelementptr inbounds ([1 x i8], [1 x i8]* @.empty_str_s, i64 0, i64 0)
  %ss_pb_s = select i1 %ss_case, i8* %ss_pb, i8* getelementptr inbounds ([1 x i8], [1 x i8]* @.empty_str_s, i64 0, i64 0)
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
  %ee_eq = icmp eq i64 %da, %db
  %ee_res = and i1 %ee_case, %ee_eq
  %r1 = or i1 %ii_res, %if_res
  %r2 = or i1 %r1, %fi_res
  %r3 = or i1 %r2, %ff_res
  %r4 = or i1 %r3, %bb_res
  %r5 = or i1 %r4, %ss_res
  %r6 = or i1 %r5, %nn_res
  %r7 = or i1 %r6, %vv_res
  %r8 = or i1 %r7, %ee_res
  ret i1 %r8
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
  %ii_lt = icmp slt i64 %da, %db
  %ii_res = and i1 %ii_case, %ii_lt
  %b_float = icmp eq i32 %tb, 3
  %if_case = and i1 %a_int, %b_float
  %if_fa = sitofp i64 %da to double
  %if_fb = bitcast i64 %db to double
  %if_lt = fcmp olt double %if_fa, %if_fb
  %if_res = and i1 %if_case, %if_lt
  %a_float = icmp eq i32 %ta, 3
  %fi_case = and i1 %a_float, %b_int
  %fi_fa = bitcast i64 %da to double
  %fi_fb = sitofp i64 %db to double
  %fi_lt = fcmp olt double %fi_fa, %fi_fb
  %fi_res = and i1 %fi_case, %fi_lt
  %ff_case = and i1 %a_float, %b_float
  %ff_lt = fcmp olt double %fi_fa, %if_fb
  %ff_res = and i1 %ff_case, %ff_lt
  %a_bool = icmp eq i32 %ta, 4
  %b_bool = icmp eq i32 %tb, 4
  %bb_case = and i1 %a_bool, %b_bool
  %bb_lt = icmp slt i64 %da, %db
  %bb_res = and i1 %bb_case, %bb_lt
  %a_str = icmp eq i32 %ta, 5
  %b_str = icmp eq i32 %tb, 5
  %ss_case = and i1 %a_str, %b_str
  %ss_pa = inttoptr i64 %da to i8*
  %ss_pb = inttoptr i64 %db to i8*
  %ss_pa_s = select i1 %ss_case, i8* %ss_pa, i8* getelementptr inbounds ([1 x i8], [1 x i8]* @.empty_str_s, i64 0, i64 0)
  %ss_pb_s = select i1 %ss_case, i8* %ss_pb, i8* getelementptr inbounds ([1 x i8], [1 x i8]* @.empty_str_s, i64 0, i64 0)
  %ss_cmp = call i32 @strcmp(i8* %ss_pa_s, i8* %ss_pb_s)
  %ss_lt = icmp slt i32 %ss_cmp, 0
  %ss_res = and i1 %ss_case, %ss_lt
  %r1 = or i1 %ii_res, %if_res
  %r2 = or i1 %r1, %fi_res
  %r3 = or i1 %r2, %ff_res
  %r4 = or i1 %r3, %bb_res
  %r5 = or i1 %r4, %ss_res
  ret i1 %r5
}
"#;

const HC_TRUTHY: &str = r#"define i1 @hc_truthy(%Value %v) {
entry:
  %t = extractvalue %Value %v, 0
  %d = extractvalue %Value %v, 1
  %is_bool = icmp eq i32 %t, 4
  br i1 %is_bool, label %bool_, label %chk_int
bool_:
  %b = icmp ne i64 %d, 0
  ret i1 %b
chk_int:
  %is_int = icmp eq i32 %t, 2
  br i1 %is_int, label %int_, label %chk_float
int_:
  %i = icmp ne i64 %d, 0
  ret i1 %i
chk_float:
  %is_float = icmp eq i32 %t, 3
  br i1 %is_float, label %float_, label %chk_str
float_:
  %f = bitcast i64 %d to double
  %fn = fcmp une double %f, 0.000000e+00
  ret i1 %fn
chk_str:
  %is_str = icmp eq i32 %t, 5
  br i1 %is_str, label %str_, label %chk_null
str_:
  %p = inttoptr i64 %d to i8*
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
  %d = zext i1 %b to i64
  %v0 = insertvalue %Value { i32 0, i64 0 }, i32 4, 0
  %v1 = insertvalue %Value %v0, i64 %d, 1
  ret %Value %v1
}
"#;

fn emit_cmp_helpers(out: &mut String) {
    out.push_str(HC_EQ);
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
  %n = sub i64 0, %d
  %v0 = insertvalue %Value { i32 0, i64 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i64 %n, 1
  ret %Value %v1
chk_float:
  %is_float = icmp eq i32 %t, 3
  br i1 %is_float, label %float_, label %err
float_:
  %f = bitcast i64 %d to double
  %nf = fneg double %f
  %bits = bitcast double %nf to i64
  %f0 = insertvalue %Value { i32 0, i64 0 }, i32 3, 0
  %f1 = insertvalue %Value %f0, i64 %bits, 1
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
  %n = xor i64 %d, -1
  %v0 = insertvalue %Value { i32 0, i64 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i64 %n, 1
  ret %Value %v1
err:
  call void @hc_abort_typeerr()
  unreachable
}
"#;

const HC_NOT: &str = r#"define %Value @hc_not(%Value %v) {
  %b = call i1 @hc_truthy(%Value %v)
  %n = xor i1 %b, true
  %d = zext i1 %n to i64
  %v0 = insertvalue %Value { i32 0, i64 0 }, i32 4, 0
  %v1 = insertvalue %Value %v0, i64 %d, 1
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

// ---------- 断言内建 helper（失败写全局 @hc_fail_msg） ----------

fn emit_assert_helpers(out: &mut String) {
    let gep = msg_gep("assert", "error.AssertFailed");
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
        let _ = writeln!(out, "  ret %Value {{ i32 0, i64 0 }}");
        let _ = writeln!(out, "fail:");
        let _ = writeln!(out, "  store i8* {gep}, i8** @hc_fail_msg");
        let _ = writeln!(out, "  ret %Value {{ i32 0, i64 0 }}");
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
    be.emit(format!("%slots = alloca [{} x %Value], align 8", f.n_slots));
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
        self.emit(format!("{v0} = insertvalue %Value {{ i32 0, i64 0 }}, i32 {tag}, 0"));
        let v1 = self.r();
        self.emit(format!("{v1} = insertvalue %Value {v0}, i64 {data}, 1"));
        self.emit(format!("store %Value {v1}, %Value* %sp.{temp}"));
    }

    fn const_(&mut self, temp: usize, val: &IrConst, strings: &[String], errors: &ErrorCodeTable) {
        if let IrConst::Str(s) = val {
            let idx = strings.iter().position(|x| x == s).unwrap_or(0);
            let n = s.len() + 1;
            let p = self.r();
            self.emit(format!("{p} = getelementptr inbounds [{n} x i8], [{n} x i8]* @.str.{idx}, i64 0, i64 0"));
            let pi = self.r();
            self.emit(format!("{pi} = ptrtoint i8* {p} to i64"));
            self.build_store(temp, T_STR, pi);
            return;
        }
        let (tag, data) = match val {
            IrConst::Int(i) => (T_INT, format!("{}", *i as i64)),
            IrConst::Float(f) => (T_FLOAT, format!("0x{:016x}", f.to_bits())),
            IrConst::Bool(b) => (T_BOOL, if *b { "1" } else { "0" }.to_string()),
            IrConst::Void => (T_VOID, "0".to_string()),
            IrConst::Null => (T_NULL, "0".to_string()),
            IrConst::Err(name) => (T_ERR, format!("{}", errors.code_of(name).unwrap_or(0) as i64)),
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
            self.emit(format!("store %Value {{ i32 0, i64 0 }}, %Value* %sp.{temp}"));
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
        self.term("ret %Value { i32 0, i64 0 }".to_string());
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
        let m = ir::lower(&p);
        let t = crate::errorcodes::collect(&p, 0);
        codegen(&m, &t)
    }

    #[test]
    fn scalar_main_defines_and_wrapper() {
        let ll = gen("fn main() i32 { return 42; }");
        assert!(ll.contains("define %Value @\"hc_fn0\"()"), "{ll}");
        assert!(ll.contains("define i32 @main(i32 %argc, i8** %argv)"), "{ll}");
        assert!(ll.contains("i64 42"), "{ll}");
        assert!(ll.contains("ret %Value"), "{ll}");
    }

    #[test]
    fn add_emits_overflow_intrinsic() {
        let ll = gen("fn add(a: i32, b: i32) i32 { return a + b; }");
        assert!(ll.contains("@llvm.sadd.with.overflow.i64"), "{ll}");
        assert!(ll.contains("define %Value @hc_add"), "{ll}");
    }

    #[test]
    fn err_literal_uses_error_code() {
        let ll = gen("fn f() !i32 { return error.NotFound; }");
        assert!(ll.contains("i32 6"), "{ll}"); // err tag
        // error.NotFound 码 = 0（包 ID 0 + 首个错误）
        assert!(ll.contains("insertvalue %Value { i32 0, i64 0 }, i32 6, 0"), "{ll}");
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
}
