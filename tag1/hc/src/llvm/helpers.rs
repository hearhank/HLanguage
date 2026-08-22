//! helper 发射：聚合/比较/指针/迭代/打印等运行时 helper 的注入函数。

use super::*;

pub(crate) fn emit_arith_helpers(out: &mut String) {
    for (fname, intr, fop) in [
        ("hc_add", "llvm.sadd.with.overflow.i128", "fadd"),
        ("hc_sub", "llvm.ssub.with.overflow.i128", "fsub"),
        ("hc_mul", "llvm.smul.with.overflow.i128", "fmul"),
    ] {
        let fname = format!("@{fname}");
        let intr = format!("@{intr}");
        out.push_str(&tpl(
            TPL_OVERFLOW,
            &[("@FNAME@", &fname), ("@INTRINSIC@", &intr), ("@FOP@", fop)],
        ));
        out.push('\n');
    }
    for (fname, iop, fop) in [("hc_div", "sdiv", "fdiv"), ("hc_mod", "srem", "frem")] {
        let fname = format!("@{fname}");
        out.push_str(&tpl(
            TPL_DIVMOD,
            &[("@FNAME@", &fname), ("@IOP@", iop), ("@FOP@", fop)],
        ));
        out.push('\n');
    }
    out.push_str(TPL_EUCMOD);
    out.push_str("\n\n");
}

pub(crate) fn emit_bit_helpers(out: &mut String) {
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

pub(crate) fn emit_cmp_helpers(out: &mut String) {
    out.push_str(HC_EQ_PLAIN);
    out.push('\n');
    out.push_str(HC_EQ_DISPATCH);
    out.push('\n');
    out.push_str(HC_LT);
    out.push('\n');
    out.push_str(HC_TRUTHY);
    out.push('\n');
    out.push_str(HC_BOOL);
    out.push('\n');
}

pub(crate) fn emit_unary_helpers(out: &mut String) {
    out.push_str(HC_NEG);
    out.push('\n');
    out.push_str(HC_BITNOT);
    out.push('\n');
    out.push_str(HC_NOT);
    out.push('\n');
}

pub(crate) fn emit_pointer_helpers(out: &mut String) {
    out.push_str(HC_DEREF);
    out.push('\n');
    out.push_str(HC_STORE_PTR);
    out.push('\n');
}

pub(crate) fn emit_deep_copy_gate(
    out: &mut String,
    strings: &[String],
    continuous: &HashSet<String>,
) {
    let mut names: Vec<&String> = continuous.iter().collect();
    names.sort();
    // 无连续类：恒等门（`needs_deep_copy` 未发射指令，保守兜底），不引入递归 helper
    if names.is_empty() {
        out.push_str(
            "define %Value @hc_deep_copy_cont(%Value %v) {\nentry:\n  ret %Value %v\n}\n\n",
        );
        return;
    }
    out.push_str(HC_DEEP_COPY);
    out.push('\n');
    let mut b = String::new();
    b.push_str("define %Value @hc_deep_copy_cont(%Value %v) {\n");
    b.push_str("entry:\n");
    b.push_str("  %t = extractvalue %Value %v, 0\n");
    b.push_str("  %is_cls = icmp eq i32 %t, 10\n");
    b.push_str("  br i1 %is_cls, label %getname, label %id\n");
    b.push_str("getname:\n");
    b.push_str("  %d = extractvalue %Value %v, 1\n");
    b.push_str("  %op = inttoptr i128 %d to %ClassObj*\n");
    b.push_str("  %co = load %ClassObj, %ClassObj* %op\n");
    b.push_str("  %cname = extractvalue %ClassObj %co, 0\n");
    b.push_str("  br label %cmp0\n");
    for (i, name) in names.iter().enumerate() {
        let (si, sn) = str_idx(strings, name);
        b.push_str(&format!("cmp{i}:\n"));
        b.push_str(&format!(
            "  %g{i} = getelementptr inbounds [{sn} x i8], ptr @.str.{si}, i64 0, i64 0\n"
        ));
        b.push_str(&format!(
            "  %c{i} = call i32 @strcmp(i8* %cname, i8* %g{i})\n"
        ));
        b.push_str(&format!("  %e{i} = icmp eq i32 %c{i}, 0\n"));
        if i + 1 < names.len() {
            b.push_str(&format!(
                "  br i1 %e{i}, label %copy, label %cmp{}\n",
                i + 1
            ));
        } else {
            b.push_str(&format!("  br i1 %e{i}, label %copy, label %id\n"));
        }
    }
    b.push_str("copy:\n");
    b.push_str("  %c = call %Value @hc_deep_copy(%Value %v)\n");
    b.push_str("  ret %Value %c\n");
    b.push_str("id:\n");
    b.push_str("  ret %Value %v\n");
    b.push_str("}\n\n");
    out.push_str(&b);
}

pub(crate) fn emit_aggregate_helpers(out: &mut String) {
    // 聚合 tag 常量与模板字符串内的字面量保持一致（标记使用，防 dead-code 告警）。
    let _ = [T_ARR, T_SLICE, T_CLASS, T_ENUM, T_END];
    for h in [
        HC_ALLOC,
        HC_MAKE_ARR,
        HC_ARR_SET,
        HC_APPEND,
        HC_APPEND_U64,
        HC_EXTEND,
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

pub(crate) fn emit_switch_helpers(out: &mut String) {
    out.push_str(HC_MATCH_TEST);
    out.push('\n');
    out.push_str(HC_ENUM_PAYLOAD);
    out.push('\n');
    out.push_str(HC_MAKE_RANGE);
    out.push('\n');
}

pub(crate) fn emit_iter_helpers(out: &mut String) {
    for h in [
        HC_ITER_ALLOC,
        HC_ITER_SET,
        HC_ITER_NEXT,
        HC_ITER_WRITE_BACK,
        HC_ITER_MAKE,
    ] {
        out.push_str(h);
        out.push('\n');
    }
}

pub(crate) fn emit_print_helpers(out: &mut String) {
    for h in [
        HC_WRITE_BYTES,
        HC_WRITE_STRZ,
        HC_WRITE_U128_BASE,
        HC_WRITE_I128_DEC,
        HC_WRITE_INT,
        HC_WRITE_TYPENAME,
        HC_WRITE_VALUE,
    ] {
        out.push_str(h);
        out.push('\n');
    }
}

pub(crate) fn emit_scalar_builtin_helpers(out: &mut String) {
    for h in [
        HC_MIN,
        HC_MAX,
        HC_SQRT,
        HC_BOX,
        HC_COPY,
        HC_INTCAST,
        HC_TYPEOF,
        HC_READ_U64_LE,
        HC_FMT_INT,
        HC_FMT_FLOAT,
        HC_ADD_OVERFLOW,
        HC_SUB_OVERFLOW,
        HC_MUL_OVERFLOW,
    ] {
        out.push_str(h);
        out.push('\n');
    }
}

pub(crate) fn emit_io_helper(out: &mut String) {
    out.push_str(HC_MAKE_IO);
    out.push('\n');
}

pub(crate) fn emit_assert_helpers(out: &mut String) {
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
            "%ta = extractvalue %Value %x, 0\n  %ea = icmp eq i32 %ta, 6\n  %tb = extractvalue %Value %y, 0\n  %eb = icmp eq i32 %tb, 6\n  %eq = call i1 @hc_eq(%Value %x, %Value %y)\n  %e1 = and i1 %ea, %eb\n  %b = and i1 %e1, %eq",
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
        let _ = writeln!(
            out,
            "  %fail_msg = getelementptr inbounds [{an} x i8], ptr @.msg_assert, i64 0, i64 0"
        );
        let _ = writeln!(out, "  store i8* %fail_msg, i8** @hc_fail_msg");
        let _ = writeln!(out, "  ret %Value {{ i32 0, i128 0 }}");
        let _ = writeln!(out, "}}\n");
    }
}
