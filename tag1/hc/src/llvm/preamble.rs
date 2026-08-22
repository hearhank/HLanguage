//! 导言发射：LLVM 头部 + 类型声明 + helper 模板注入。

use super::helpers::*;
use super::*;
use std::fmt::Write as _;

pub(crate) fn emit_preamble(
    out: &mut String,
    strings: &[String],
    continuous: &HashSet<String>,
    helpers: bool,
) {
    out.push_str("; H M3.3 LLVM 原生后端（自动生成；`zig cc file.ll -o file.exe`）\n\n");
    out.push_str("%Value = type { i32, i128 }\n");
    // Phase 2 聚合堆对象（聚合 `%Value` 的 data = 堆对象指针）
    out.push_str("%ArrObj = type { i64, %Value* }\n");
    out.push_str("%SliceObj = type { %Value*, i64, i64 }\n");
    out.push_str("%Field = type { i8*, %Value }\n");
    out.push_str("%ClassObj = type { i8*, i64, %Field* }\n");
    out.push_str("%EnumObj = type { i8*, i8*, %Value* }\n");
    out.push_str("%SeqInfo = type { %Value*, i64, i64 }\n");
    out.push_str("%FindRes = type { i1, %Value }\n");
    // Phase 3 迭代器（`for` 展开：源指针 + 写回目标）
    out.push_str("%IterItemObj = type { %Value*, i1 }\n");
    out.push_str("%IterObj = type { %IterItemObj*, i64, i64, i64 }\n\n");

    // 外部符号（libc + 溢出内建）
    out.push_str("declare i32 @strcmp(i8*, i8*)\n");
    out.push_str("declare i32 @memcmp(i8*, i8*, i64)\n");
    out.push_str("declare i32 @puts(i8*)\n");
    out.push_str("declare void @exit(i32) noreturn\n");
    out.push_str("declare i64 @strlen(i8*)\n");
    out.push_str("declare noalias i8* @malloc(i64)\n");
    out.push_str("declare noalias i8* @realloc(i8*, i64)\n");
    out.push_str("declare void @llvm.memcpy.p0i8.p0i8.i64(i8*, i8*, i64, i1)\n");
    out.push_str("declare { i128, i1 } @llvm.sadd.with.overflow.i128(i128, i128)\n");
    out.push_str("declare { i128, i1 } @llvm.ssub.with.overflow.i128(i128, i128)\n");
    out.push_str("declare { i128, i1 } @llvm.smul.with.overflow.i128(i128, i128)\n");
    // Phase 7 io.print / 数值显示辅助（libc 可变参 printf；fmod/fabs 判浮点整值；
    // sprintf 供 hc_fmt_float 数字→字符串）
    out.push_str("declare i32 @printf(i8*, ...)\n");
    out.push_str("declare i32 @sprintf(i8*, ...)\n");
    out.push_str("declare double @fmod(double, double)\n");
    out.push_str("declare double @fabs(double)\n");
    out.push_str("declare double @sqrt(double)\n");
    out.push_str("declare double @floor(double)\n");
    out.push_str("declare double @ceil(double)\n");
    out.push_str("declare double @round(double)\n\n");

    // 断言失败标志（全局；单线程顺序执行）
    out.push_str("@hc_fail_msg = global i8* null\n");
    out.push_str("@.void_value = private unnamed_addr constant %Value { i32 0, i128 0 }\n");
    out.push_str("@.empty_str_s = private unnamed_addr constant [1 x i8] c\"\\00\"\n");
    // `.len` 内建字段名字符串（`hc_field` 对 Str/Arr/Slice 判定用）
    out.push_str("@.hc_len = private unnamed_addr constant [4 x i8] c\"len\\00\"\n");
    // Phase 3 switch 模式 / 迭代器判型字符串
    out.push_str("@.hc_true = private unnamed_addr constant [5 x i8] c\"true\\00\"\n");
    out.push_str("@.hc_false = private unnamed_addr constant [6 x i8] c\"false\\00\"\n");
    out.push_str("@.hc_null = private unnamed_addr constant [5 x i8] c\"null\\00\"\n");
    out.push_str("@.hc_map = private unnamed_addr constant [4 x i8] c\"Map\\00\"\n");
    out.push_str("@.hc_kv = private unnamed_addr constant [3 x i8] c\"KV\\00\"\n");
    out.push_str("@.hc_key = private unnamed_addr constant [4 x i8] c\"key\\00\"\n");
    out.push_str("@.hc_value = private unnamed_addr constant [6 x i8] c\"value\\00\"\n");
    // Phase 7 io.print 显示格式常量（printf 格式串 + 定长字节写辅助）
    out.push_str("@.fmt_pct = private unnamed_addr constant [5 x i8] c\"%.*s\\00\"\n");
    out.push_str("@.fmt_s = private unnamed_addr constant [3 x i8] c\"%s\\00\"\n");
    out.push_str("@.fmt_one = private unnamed_addr constant [5 x i8] c\"%.1f\\00\"\n");
    out.push_str("@.fmt_g15 = private unnamed_addr constant [6 x i8] c\"%.15g\\00\"\n");
    out.push_str("@.fmt_e = private unnamed_addr constant [3 x i8] c\"%e\\00\"\n");
    out.push_str("@.hc_dash = private unnamed_addr constant [2 x i8] c\"-\\00\"\n");
    out.push_str("@.hc_lb = private unnamed_addr constant [2 x i8] c\"[\\00\"\n");
    out.push_str("@.hc_rb = private unnamed_addr constant [2 x i8] c\"]\\00\"\n");
    out.push_str("@.hc_comma = private unnamed_addr constant [3 x i8] c\", \\00\"\n");
    out.push_str("@.hc_bra_l = private unnamed_addr constant [4 x i8] c\" { \\00\"\n");
    out.push_str("@.hc_bra_r = private unnamed_addr constant [3 x i8] c\" }\\00\"\n");
    out.push_str("@.hc_eqs = private unnamed_addr constant [4 x i8] c\" = \\00\"\n");
    out.push_str("@.hc_errpre = private unnamed_addr constant [7 x i8] c\"error.\\00\"\n");
    out.push_str("@.hc_dot = private unnamed_addr constant [2 x i8] c\".\\00\"\n");
    out.push_str("@.hc_shallow = private unnamed_addr constant [8 x i8] c\"shallow\\00\"\n\n");

    // @typeOf 类型名常量（hc_typeof 返回值）
    out.push_str("@.t_i128 = private unnamed_addr constant [5 x i8] c\"i128\\00\"\n");
    out.push_str("@.t_f64 = private unnamed_addr constant [4 x i8] c\"f64\\00\"\n");
    out.push_str("@.t_bool = private unnamed_addr constant [5 x i8] c\"bool\\00\"\n");
    out.push_str("@.t_str = private unnamed_addr constant [6 x i8] c\"&[u8]\\00\"\n");
    out.push_str("@.t_arr = private unnamed_addr constant [6 x i8] c\"array\\00\"\n");
    out.push_str("@.t_slice = private unnamed_addr constant [6 x i8] c\"slice\\00\"\n");
    out.push_str("@.t_opt = private unnamed_addr constant [9 x i8] c\"optional\\00\"\n");
    out.push_str("@.t_err = private unnamed_addr constant [6 x i8] c\"error\\00\"\n");
    out.push_str("@.t_ptr = private unnamed_addr constant [8 x i8] c\"pointer\\00\"\n");
    out.push_str("@.t_fn = private unnamed_addr constant [3 x i8] c\"fn\\00\"\n");
    out.push_str("@.t_closure = private unnamed_addr constant [8 x i8] c\"closure\\00\"\n");
    out.push_str("@.t_end = private unnamed_addr constant [4 x i8] c\"end\\00\"\n");
    out.push_str("@.t_void = private unnamed_addr constant [5 x i8] c\"void\\00\"\n");
    out.push_str("@.t_iter = private unnamed_addr constant [7 x i8] c\"<iter>\\00\"\n\n");

    // main(io: Io) 单参入口：Io 值构造（fs/time/net 空子类，与 IR io_value_ir 对齐）
    out.push_str("@.t_io = private unnamed_addr constant [3 x i8] c\"Io\\00\"\n");
    out.push_str("@.t_fs = private unnamed_addr constant [3 x i8] c\"Fs\\00\"\n");
    out.push_str("@.t_time = private unnamed_addr constant [5 x i8] c\"Time\\00\"\n");
    out.push_str("@.t_net = private unnamed_addr constant [4 x i8] c\"Net\\00\"\n");
    out.push_str("@.f_fs = private unnamed_addr constant [3 x i8] c\"fs\\00\"\n");
    out.push_str("@.f_time = private unnamed_addr constant [5 x i8] c\"time\\00\"\n");
    out.push_str("@.f_net = private unnamed_addr constant [4 x i8] c\"net\\00\"\n\n");

    // 硬错误消息全局
    for m in MSGS {
        let n = m.text.len() + 1;
        let esc = llvm_escape(m.text.as_bytes());
        let _ = writeln!(
            out,
            "@.msg_{} = private unnamed_addr constant [{n} x i8] c\"{esc}\\00\"",
            m.key
        );
    }
    out.push('\n');

    // 字符串常量全局（去重后）
    for (i, s) in strings.iter().enumerate() {
        let n = s.len() + 1;
        let esc = llvm_escape(s.as_bytes());
        let _ = writeln!(
            out,
            "@.str.{i} = private unnamed_addr constant [{n} x i8] c\"{esc}\\00\""
        );
    }
    out.push('\n');

    // 中止 + 各硬错误无参包装
    out.push_str("define void @hc_abort(i8* %msg) {\n  call i32 @puts(i8* %msg)\n  call void @exit(i32 1)\n  unreachable\n}\n\n");
    for m in MSGS {
        let n = m.text.len() + 1;
        let _ = writeln!(out, "define void @hc_abort_{}() {{", m.key);
        let _ = writeln!(
            out,
            "  %p = getelementptr inbounds [{n} x i8], ptr @.msg_{}, i64 0, i64 0",
            m.key
        );
        let _ = writeln!(out, "  call void @hc_abort(i8* %p)");
        out.push_str("  unreachable\n}\n\n");
    }
    // 切片外函数调用（运行时 NoFunction 硬错误）
    out.push_str(
        "define %Value @hc_no_function() {\n  call void @hc_abort_nofunc()\n  unreachable\n}\n\n",
    );

    // 计算 helper（C3 库形态跳过——由链接的 exe 提供符号；中止基建保留）
    if helpers {
        emit_arith_helpers(out);
        emit_bit_helpers(out);
        emit_cmp_helpers(out);
        emit_unary_helpers(out);
        emit_assert_helpers(out);
        emit_pointer_helpers(out);
        emit_aggregate_helpers(out);
        emit_switch_helpers(out);
        emit_iter_helpers(out);
        emit_print_helpers(out);
        emit_scalar_builtin_helpers(out);
        emit_io_helper(out);
        emit_deep_copy_gate(out, strings, continuous);
    }
}
