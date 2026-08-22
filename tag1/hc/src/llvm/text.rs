//! LLVM 模板常量：运行时 helper 的 `.ll` 文本（`tpl` 占位替换）。

pub(crate) const TPL_OVERFLOW: &str = r#"define %Value @FNAME@(%Value %a, %Value %b) {
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

pub(crate) const TPL_DIVMOD: &str = r#"define %Value @FNAME@(%Value %a, %Value %b) {
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

pub(crate) const TPL_EUCMOD: &str = r#"define %Value @hc_eucmod(%Value %a, %Value %b) {
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

pub(crate) const TPL_BITOP: &str = r#"define %Value @FNAME@(%Value %a, %Value %b) {
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

pub(crate) const TPL_SHIFT: &str = r#"define %Value @FNAME@(%Value %a, %Value %b) {
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

pub(crate) const HC_EQ_PLAIN: &str = r#"define i1 @hc_eq_plain(%Value %a, %Value %b) {
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

pub(crate) const HC_LT: &str = r#"define i1 @hc_lt(%Value %a, %Value %b) {
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

pub(crate) const HC_TRUTHY: &str = r#"define i1 @hc_truthy(%Value %v) {
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

pub(crate) const HC_BOOL: &str = r#"define %Value @hc_bool(i1 zeroext %b) {
  %d = zext i1 %b to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 4, 0
  %v1 = insertvalue %Value %v0, i128 %d, 1
  ret %Value %v1
}
"#;

/// 相等分派（Phase 1 指针）：两指针 → 载荷地址身份；否则解引用归一化后走纯值比较
/// （对齐 `IrValue::value_eq`：`(Ptr,Ptr)` 身份、`(Ptr,b)` 解引用）。

pub(crate) const HC_EQ_DISPATCH: &str = r#"define i1 @hc_eq(%Value %a, %Value %b) {
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

pub(crate) const HC_NEG: &str = r#"define %Value @hc_neg(%Value %v) {
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

pub(crate) const HC_BITNOT: &str = r#"define %Value @hc_bitnot(%Value %v) {
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

pub(crate) const HC_NOT: &str = r#"define %Value @hc_not(%Value %v) {
  %b = call i1 @hc_truthy(%Value %v)
  %n = xor i1 %b, true
  %d = zext i1 %n to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 4, 0
  %v1 = insertvalue %Value %v0, i128 %d, 1
  ret %Value %v1
}
"#;

pub(crate) const HC_DEREF: &str = r#"define %Value @hc_deref(%Value %v) {
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

pub(crate) const HC_STORE_PTR: &str = r#"define void @hc_store_ptr(%Value %p, %Value %v) {
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

/// K2（ADR-0014）：@volatileLoad——LLVM `load volatile`（防优化掉副作用/重排，
/// MMIO 场景）。非指针恒等（对齐 `hc_deref` 的 identity 分支）。

/// 比较前归一化：指针解引用、普通值恒等。与 [`HC_EQ_DISPATCH`] 配合，
/// 让 `hc_eq` 在指针与非指针混合时对齐 `IrValue::value_eq`。

pub(crate) const HC_ALLOC: &str = r#"define i8* @hc_alloc(i64 %size) {
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

pub(crate) const HC_MAKE_ARR: &str = r#"define %Value @hc_make_arr(i64 %n) {
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

pub(crate) const HC_ARR_SET: &str = r#"define void @hc_arr_set(%Value %arr, i64 %i, %Value %v) {
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

/// `append` 的原生实现：ArrObj（固定容量，无 cap 字段）原地扩容——分配新 items 缓冲、
/// 拷贝旧元素、`store` 回同一 ArrObj 指针。接收者槽/字段持有同一堆指针，写入即刻对所有
/// 别名可见（对齐 run_ir 的共享 cell 语义）。旧缓冲泄漏（本阶段整体无 free，一致取舍）。

pub(crate) const HC_APPEND: &str = r#"define void @hc_append(%Value %arr, %Value %v) {
entry:
  %d = extractvalue %Value %arr, 1
  %op = inttoptr i128 %d to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %op
  %oldlen = extractvalue %ArrObj %ao, 0
  %olditems = extractvalue %ArrObj %ao, 1
  %newlen = add i64 %oldlen, 1
  %vsz = ptrtoint %Value* getelementptr (%Value, %Value* null, i32 1) to i64
  %isz = mul i64 %vsz, %newlen
  %iraw = call i8* @hc_alloc(i64 %isz)
  %newitems = bitcast i8* %iraw to %Value*
  br label %ccond
ccond:
  %i = phi i64 [ 0, %entry ], [ %inext, %cbody ]
  %c = icmp ult i64 %i, %oldlen
  br i1 %c, label %cbody, label %cdone
cbody:
  %op1 = getelementptr %Value, %Value* %olditems, i64 %i
  %ov = load %Value, %Value* %op1
  %np1 = getelementptr %Value, %Value* %newitems, i64 %i
  store %Value %ov, %Value* %np1
  %inext = add i64 %i, 1
  br label %ccond
cdone:
  %last = getelementptr %Value, %Value* %newitems, i64 %oldlen
  store %Value %v, %Value* %last
  %a0 = insertvalue %ArrObj %ao, i64 %newlen, 0
  %a1 = insertvalue %ArrObj %a0, %Value* %newitems, 1
  store %ArrObj %a1, %ArrObj* %op
  ret void
}
"#;

/// `append_u64` 的原生实现：Int 值低 64 位按 LE 展开 8 字节，逐字节作为 Int 追加。
/// 复用 `hc_append`（逐次扩容，阶段内可接受；无自引用/别名迭代正确性要求）。

pub(crate) const HC_APPEND_U64: &str = r#"define void @hc_append_u64(%Value %arr, %Value %n) {
entry:
  %nd = extractvalue %Value %n, 1
  %nlow = trunc i128 %nd to i64
  %b0 = and i64 %nlow, 255
  %b0z = zext i64 %b0 to i128
  %v0a = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v0b = insertvalue %Value %v0a, i128 %b0z, 1
  call void @hc_append(%Value %arr, %Value %v0b)
  %s1 = lshr i64 %nlow, 8
  %b1 = and i64 %s1, 255
  %b1z = zext i64 %b1 to i128
  %v1a = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1b = insertvalue %Value %v1a, i128 %b1z, 1
  call void @hc_append(%Value %arr, %Value %v1b)
  %s2 = lshr i64 %nlow, 16
  %b2 = and i64 %s2, 255
  %b2z = zext i64 %b2 to i128
  %v2a = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v2b = insertvalue %Value %v2a, i128 %b2z, 1
  call void @hc_append(%Value %arr, %Value %v2b)
  %s3 = lshr i64 %nlow, 24
  %b3 = and i64 %s3, 255
  %b3z = zext i64 %b3 to i128
  %v3a = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v3b = insertvalue %Value %v3a, i128 %b3z, 1
  call void @hc_append(%Value %arr, %Value %v3b)
  %s4 = lshr i64 %nlow, 32
  %b4 = and i64 %s4, 255
  %b4z = zext i64 %b4 to i128
  %v4a = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v4b = insertvalue %Value %v4a, i128 %b4z, 1
  call void @hc_append(%Value %arr, %Value %v4b)
  %s5 = lshr i64 %nlow, 40
  %b5 = and i64 %s5, 255
  %b5z = zext i64 %b5 to i128
  %v5a = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v5b = insertvalue %Value %v5a, i128 %b5z, 1
  call void @hc_append(%Value %arr, %Value %v5b)
  %s6 = lshr i64 %nlow, 48
  %b6 = and i64 %s6, 255
  %b6z = zext i64 %b6 to i128
  %v6a = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v6b = insertvalue %Value %v6a, i128 %b6z, 1
  call void @hc_append(%Value %arr, %Value %v6b)
  %s7 = lshr i64 %nlow, 56
  %b7 = and i64 %s7, 255
  %b7z = zext i64 %b7 to i128
  %v7a = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v7b = insertvalue %Value %v7a, i128 %b7z, 1
  call void @hc_append(%Value %arr, %Value %v7b)
  ret void
}
"#;

/// `extend` 的原生实现：`other` 为 Str（&[u8] 字节）或 Arr 时逐元素追加为 Int/元素值。
/// 对齐 run_ir `extend`（Str 逐字节 Int；Arr 元素克隆）。复用 `hc_append`。

pub(crate) const HC_EXTEND: &str = r#"define void @hc_extend(%Value %arr, %Value %other) {
entry:
  %ot = extractvalue %Value %other, 0
  %is_str = icmp eq i32 %ot, 5
  br i1 %is_str, label %estr, label %earr
estr:
  %od = extractvalue %Value %other, 1
  %osp = inttoptr i128 %od to i8*
  %cnt = call i64 @strlen(i8* %osp)
  br label %scond
scond:
  %si = phi i64 [ 0, %estr ], [ %snext, %sbody ]
  %sd = icmp ult i64 %si, %cnt
  br i1 %sd, label %sbody, label %sdone
sbody:
  %sp = getelementptr i8, i8* %osp, i64 %si
  %sb = load i8, i8* %sp
  %sz = zext i8 %sb to i128
  %sv0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %sv1 = insertvalue %Value %sv0, i128 %sz, 1
  call void @hc_append(%Value %arr, %Value %sv1)
  %snext = add i64 %si, 1
  br label %scond
sdone:
  ret void
earr:
  %od2 = extractvalue %Value %other, 1
  %op2 = inttoptr i128 %od2 to %ArrObj*
  %ao2 = load %ArrObj, %ArrObj* %op2
  %elen = extractvalue %ArrObj %ao2, 0
  %eitems = extractvalue %ArrObj %ao2, 1
  br label %econd
econd:
  %ei = phi i64 [ 0, %earr ], [ %enext, %ebody ]
  %ed = icmp ult i64 %ei, %elen
  br i1 %ed, label %ebody, label %edone
ebody:
  %ep = getelementptr %Value, %Value* %eitems, i64 %ei
  %ev = load %Value, %Value* %ep
  call void @hc_append(%Value %arr, %Value %ev)
  %enext = add i64 %ei, 1
  br label %econd
edone:
  ret void
}
"#;

pub(crate) const HC_MAKE_CLASS: &str = r#"define %Value @hc_make_class(i8* %ty, i64 %n) {
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

pub(crate) const HC_CLASS_SET: &str = r#"define void @hc_class_set(%Value %obj, i64 %i, i8* %fname, %Value %v) {
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

pub(crate) const HC_MAKE_ENUM: &str = r#"define %Value @hc_make_enum(i8* %name, i8* %variant, %Value* %payload) {
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

pub(crate) const HC_UNWRAP: &str = r#"define %Value @hc_unwrap(%Value %v) {
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

pub(crate) const HC_INDEX: &str = r#"define %Value @hc_index(%Value %base, %Value %idx) {
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

pub(crate) const HC_STORE_INDEX: &str = r#"define void @hc_store_index(%Value %base, %Value %idx, %Value %v) {
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

pub(crate) const HC_SLICE: &str = r#"define %Value @hc_slice(%Value %base, %Value %lo, %Value %hi) {
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

pub(crate) const HC_STORE_SLICE: &str = r#"define void @hc_store_slice(%Value %base, %Value %lo, %Value %hi, %Value %v) {
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

pub(crate) const HC_CLASS_FIND: &str = r#"define %FindRes @hc_class_find(%Value %base, i8* %field) {
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

pub(crate) const HC_FIELD: &str = r#"define %Value @hc_field(%Value %base, i8* %field) {
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

pub(crate) const HC_STORE_FIELD: &str = r#"define void @hc_store_field(%Value %base, i8* %field, %Value %v) {
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

pub(crate) const HC_SEQ_INFO: &str = r#"define %SeqInfo @hc_seq_info(%Value %v) {
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

pub(crate) const HC_EQ_AGG: &str = r#"define i1 @hc_eq_agg(%Value %a, %Value %b) {
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

/// P11d [continuous] 递归深拷贝（对齐 oracle `deep_copy` `interp.rs:5651-5695`）：
/// Arr/Class 逐元素/字段递归拷贝；Ptr 新建 cell 拷贝所指值；Str 原生为不可变全局
/// 字符串（拷贝指针即等价，无 Rc 生命周期），恒等。Closure 原生不存在
/// （`MakeClosure` 硬拒绝），其余（标量/枚举/End 等）按值恒等。

pub(crate) const HC_DEEP_COPY: &str = r#"define %Value @hc_deep_copy(%Value %v) {
entry:
  %t = extractvalue %Value %v, 0
  %is_arr = icmp eq i32 %t, 8
  br i1 %is_arr, label %arr, label %chk_cls
chk_cls:
  %is_cls = icmp eq i32 %t, 10
  br i1 %is_cls, label %cls, label %chk_str
chk_str:
  %is_str = icmp eq i32 %t, 5
  br i1 %is_str, label %id, label %chk_ptr
chk_ptr:
  %is_ptr = icmp eq i32 %t, 7
  br i1 %is_ptr, label %ptr, label %id
arr:
  %ad = extractvalue %Value %v, 1
  %aop = inttoptr i128 %ad to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %aop
  %alen = extractvalue %ArrObj %ao, 0
  %abase = extractvalue %ArrObj %ao, 1
  %anew = call %Value @hc_make_arr(i64 %alen)
  br label %aloop
aloop:
  %i = phi i64 [ 0, %arr ], [ %i2, %anext ]
  %idone = icmp uge i64 %i, %alen
  br i1 %idone, label %aret, label %abody
abody:
  %ap = getelementptr %Value, %Value* %abase, i64 %i
  %av = load %Value, %Value* %ap
  %ac = call %Value @hc_deep_copy(%Value %av)
  call void @hc_arr_set(%Value %anew, i64 %i, %Value %ac)
  br label %anext
anext:
  %i2 = add i64 %i, 1
  br label %aloop
aret:
  ret %Value %anew
cls:
  %cd = extractvalue %Value %v, 1
  %cop = inttoptr i128 %cd to %ClassObj*
  %co = load %ClassObj, %ClassObj* %cop
  %cn = extractvalue %ClassObj %co, 0
  %ccnt = extractvalue %ClassObj %co, 1
  %cfs = extractvalue %ClassObj %co, 2
  %cnew = call %Value @hc_make_class(i8* %cn, i64 %ccnt)
  br label %cloop
cloop:
  %j = phi i64 [ 0, %cls ], [ %j2, %cnext ]
  %jdone = icmp uge i64 %j, %ccnt
  br i1 %jdone, label %cret, label %cbody
cbody:
  %fp = getelementptr %Field, %Field* %cfs, i64 %j
  %f = load %Field, %Field* %fp
  %fname = extractvalue %Field %f, 0
  %fval = extractvalue %Field %f, 1
  %fcp = call %Value @hc_deep_copy(%Value %fval)
  call void @hc_class_set(%Value %cnew, i64 %j, i8* %fname, %Value %fcp)
  br label %cnext
cnext:
  %j2 = add i64 %j, 1
  br label %cloop
cret:
  ret %Value %cnew
ptr:
  %pd = extractvalue %Value %v, 1
  %pop = inttoptr i128 %pd to %Value*
  %pv = load %Value, %Value* %pop
  %pc = call %Value @hc_deep_copy(%Value %pv)
  %sz = ptrtoint %Value* getelementptr (%Value, %Value* null, i32 1) to i64
  %raw = call i8* @hc_alloc(i64 %sz)
  %ncell = bitcast i8* %raw to %Value*
  store %Value %pc, %Value* %ncell
  %np = ptrtoint %Value* %ncell to i128
  %nv0 = insertvalue %Value { i32 0, i128 0 }, i32 7, 0
  %nv1 = insertvalue %Value %nv0, i128 %np, 1
  ret %Value %nv1
id:
  ret %Value %v
}
"#;

/// P11d [continuous] DeepCopy 运行时门 `hc_deep_copy_cont`：值非 Class 恒等；
/// Class 名 ∈ `module.continuous` → `hc_deep_copy` 递归拷贝；否则恒等
/// （对齐 oracle `type_is_continuous` 仅对 Named 连续类生效，非连续类/数组 = 引用别名）。
/// LLVM 18 禁止常量表达式 GEP 于全局初始化器——名检查以 strcmp 链内联（同方法分派模式）。

pub(crate) const HC_MATCH_TEST: &str = r#"define %Value @hc_match_test(%Value %subj, i8 %tag, i128 %data, i8* %str, i64 %len) {
entry:
  %slot = alloca %Value, align 8
  %f0 = insertvalue %Value { i32 0, i128 0 }, i32 4, 0
  store %Value %f0, %Value* %slot
  %b = call %Value @hc_deref(%Value %subj)
  %t = extractvalue %Value %b, 0
  %d = extractvalue %Value %b, 1
  switch i8 %tag, label %done [
    i8 0, label %t_err
    i8 1, label %t_ident
    i8 2, label %t_int
    i8 3, label %t_float
    i8 4, label %t_str
    i8 5, label %t_char
  ]
t_err:
  %is_err = icmp eq i32 %t, 6
  br i1 %is_err, label %err_cmp, label %done
err_cmp:
  %eqc = icmp eq i128 %d, %data
  br label %store_res
t_ident:
  %is_bool = icmp eq i32 %t, 4
  br i1 %is_bool, label %ident_bool, label %chk_null
ident_bool:
  %truep = getelementptr inbounds [5 x i8], ptr @.hc_true, i64 0, i64 0
  %c1 = call i32 @strcmp(i8* %str, i8* %truep)
  %is_true = icmp eq i32 %c1, 0
  br i1 %is_true, label %bool_true, label %chk_false
bool_true:
  %eqt = icmp eq i128 %d, 1
  br label %store_res
chk_false:
  %falsep = getelementptr inbounds [6 x i8], ptr @.hc_false, i64 0, i64 0
  %c2 = call i32 @strcmp(i8* %str, i8* %falsep)
  %is_false = icmp eq i32 %c2, 0
  br i1 %is_false, label %bool_false, label %chk_null
bool_false:
  %eqf = icmp eq i128 %d, 0
  br label %store_res
chk_null:
  %is_null = icmp eq i32 %t, 1
  br i1 %is_null, label %null_cmp, label %chk_enum
null_cmp:
  %nullp = getelementptr inbounds [5 x i8], ptr @.hc_null, i64 0, i64 0
  %c3 = call i32 @strcmp(i8* %str, i8* %nullp)
  %eqn = icmp eq i32 %c3, 0
  br label %store_res
chk_enum:
  %is_enum = icmp eq i32 %t, 11
  br i1 %is_enum, label %enum_cmp, label %done
enum_cmp:
  %d2 = extractvalue %Value %b, 1
  %op = inttoptr i128 %d2 to %EnumObj*
  %eo = load %EnumObj, %EnumObj* %op
  %vn = extractvalue %EnumObj %eo, 1
  %c4 = call i32 @strcmp(i8* %vn, i8* %str)
  %eqv = icmp eq i32 %c4, 0
  br label %store_res
t_int:
  %is_int = icmp eq i32 %t, 2
  br i1 %is_int, label %int_cmp, label %done
int_cmp:
  %eqi = icmp eq i128 %d, %data
  br label %store_res
t_float:
  %is_float = icmp eq i32 %t, 3
  br i1 %is_float, label %float_cmp, label %done
float_cmp:
  %eqf2 = icmp eq i128 %d, %data
  br label %store_res
t_str:
  %is_str = icmp eq i32 %t, 5
  br i1 %is_str, label %str_cmp, label %done
str_cmp:
  %sp = inttoptr i128 %d to i8*
  %mc = call i32 @memcmp(i8* %str, i8* %sp, i64 %len)
  %eqs = icmp eq i32 %mc, 0
  br label %store_res
t_char:
  %is_int2 = icmp eq i32 %t, 2
  br i1 %is_int2, label %char_cmp, label %done
char_cmp:
  %eqc2 = icmp eq i128 %d, %data
  br label %store_res
store_res:
  %res = phi i1 [ %eqc, %err_cmp ], [ %eqt, %bool_true ], [ %eqf, %bool_false ], [ %eqn, %null_cmp ], [ %eqv, %enum_cmp ], [ %eqi, %int_cmp ], [ %eqf2, %float_cmp ], [ %eqs, %str_cmp ], [ %eqc2, %char_cmp ]
  %d3 = zext i1 %res to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 4, 0
  %v1 = insertvalue %Value %v0, i128 %d3, 1
  store %Value %v1, %Value* %slot
  br label %done
done:
  %rv = load %Value, %Value* %slot
  ret %Value %rv
}
"#;

pub(crate) const HC_ENUM_PAYLOAD: &str = r#"define %Value @hc_enum_payload(%Value %v) {
entry:
  %b = call %Value @hc_deref(%Value %v)
  %t = extractvalue %Value %b, 0
  %is_enum = icmp eq i32 %t, 11
  br i1 %is_enum, label %enum_, label %identity
enum_:
  %d = extractvalue %Value %b, 1
  %op = inttoptr i128 %d to %EnumObj*
  %eo = load %EnumObj, %EnumObj* %op
  %pp = extractvalue %EnumObj %eo, 2
  %isnull = icmp eq %Value* %pp, null
  br i1 %isnull, label %identity, label %payload
payload:
  %pv = load %Value, %Value* %pp
  ret %Value %pv
identity:
  ret %Value %b
}
"#;

/// `lo..hi` → Arr（[lo, hi) 元素，对齐 run_ir `MakeRange`；lo ≥ hi → 空数组）。

pub(crate) const HC_MAKE_RANGE: &str = r#"define %Value @hc_make_range(%Value %lo, %Value %hi) {
entry:
  %l = call %Value @hc_deref(%Value %lo)
  %lt = extractvalue %Value %l, 0
  %lt2 = icmp eq i32 %lt, 2
  br i1 %lt2, label %l_ok, label %typeerr
l_ok:
  %h = call %Value @hc_deref(%Value %hi)
  %ht = extractvalue %Value %h, 0
  %ht2 = icmp eq i32 %ht, 2
  br i1 %ht2, label %h_ok, label %typeerr
h_ok:
  %lv = extractvalue %Value %l, 1
  %hv = extractvalue %Value %h, 1
  %sub = sub i128 %hv, %lv
  %neg = icmp slt i128 %sub, 0
  %cnt = select i1 %neg, i128 0, i128 %sub
  %cnt64 = trunc i128 %cnt to i64
  %arr = call %Value @hc_make_arr(i64 %cnt64)
  br label %loop
loop:
  %i = phi i64 [ 0, %h_ok ], [ %i2, %next ]
  %done = icmp uge i64 %i, %cnt64
  br i1 %done, label %fin, label %body
body:
  %vi = zext i64 %i to i128
  %lv2 = add i128 %lv, %vi
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %lv2, 1
  call void @hc_arr_set(%Value %arr, i64 %i, %Value %v1)
  br label %next
next:
  %i2 = add i64 %i, 1
  br label %loop
typeerr:
  call void @hc_abort_typeerr()
  unreachable
fin:
  ret %Value %arr
}
"#;

pub(crate) const HC_ITER_ALLOC: &str = r#"define %IterObj* @hc_iter_alloc(i64 %n) {
entry:
  %osz = ptrtoint %IterObj* getelementptr (%IterObj, %IterObj* null, i32 1) to i64
  %oraw = call i8* @hc_alloc(i64 %osz)
  %op = bitcast i8* %oraw to %IterObj*
  %isz = ptrtoint %IterItemObj* getelementptr (%IterItemObj, %IterItemObj* null, i32 1) to i64
  %is = mul i64 %isz, %n
  %iraw = call i8* @hc_alloc(i64 %is)
  %ip = bitcast i8* %iraw to %IterItemObj*
  %o0 = insertvalue %IterObj undef, %IterItemObj* %ip, 0
  %o1 = insertvalue %IterObj %o0, i64 %n, 1
  %o2 = insertvalue %IterObj %o1, i64 0, 2
  %o3 = insertvalue %IterObj %o2, i64 -1, 3
  store %IterObj %o3, %IterObj* %op
  ret %IterObj* %op
}
"#;

pub(crate) const HC_ITER_SET: &str = r#"define void @hc_iter_set(%IterObj* %iter, i64 %i, %Value* %src, i1 %is_ref) {
entry:
  %it = load %IterObj, %IterObj* %iter
  %items = extractvalue %IterObj %it, 0
  %ip = getelementptr %IterItemObj, %IterItemObj* %items, i64 %i
  %it0 = insertvalue %IterItemObj undef, %Value* %src, 0
  %it1 = insertvalue %IterItemObj %it0, i1 %is_ref, 1
  store %IterItemObj %it1, %IterItemObj* %ip
  ret void
}
"#;

/// 取下一项：`has`（Bool）写入迭代器当前项值副本到捕获槽；无下一项 → false。
/// Mut/Move 捕获由 `hc_iter_write_back` 在循环体末尾写回（拷贝进出——LLVM 槽模型无
/// 单元间接层；run_ir 侧槽 cell 即源 cell，写回为无操作）。

pub(crate) const HC_ITER_NEXT: &str = r#"define %Value @hc_iter_next(%IterObj* %iter, %Value* %slot) {
entry:
  %it = load %IterObj, %IterObj* %iter
  %count = extractvalue %IterObj %it, 1
  %next = extractvalue %IterObj %it, 2
  %done = icmp uge i64 %next, %count
  br i1 %done, label %ret_false, label %got
got:
  %items = extractvalue %IterObj %it, 0
  %ip = getelementptr %IterItemObj, %IterItemObj* %items, i64 %next
  %item = load %IterItemObj, %IterItemObj* %ip
  %src = extractvalue %IterItemObj %item, 0
  %val = load %Value, %Value* %src
  store %Value %val, %Value* %slot
  %i0 = insertvalue %IterObj %it, i64 %next, 3
  %next2 = add i64 %next, 1
  %i1 = insertvalue %IterObj %i0, i64 %next2, 2
  store %IterObj %i1, %IterObj* %iter
  ret %Value { i32 4, i128 1 }
ret_false:
  ret %Value { i32 4, i128 0 }
}
"#;

pub(crate) const HC_ITER_WRITE_BACK: &str = r#"define void @hc_iter_write_back(%IterObj* %iter, %Value* %slot) {
entry:
  %it = load %IterObj, %IterObj* %iter
  %wb = extractvalue %IterObj %it, 3
  %none = icmp eq i64 %wb, -1
  br i1 %none, label %done, label %wb2
wb2:
  %items = extractvalue %IterObj %it, 0
  %ip = getelementptr %IterItemObj, %IterItemObj* %items, i64 %wb
  %item = load %IterItemObj, %IterItemObj* %ip
  %src = extractvalue %IterItemObj %item, 0
  %val = load %Value, %Value* %slot
  store %Value %val, %Value* %src
  br label %done
done:
  ret void
}
"#;

/// 展开可迭代值（对齐 run_ir `make_iter`）：Arr/Slice 元素源指针 `is_ref=true`（写穿）；
/// Str → 字节新单元；Map → KV 类新单元（key 字段 + value 字段副本，`is_ref=false`——
/// 原生 Map 写回为 KV 副本，收敛于 Phase 7 标准库）；其它 Class（用户 IIterable）→
/// NotIterable 硬错误（方法动态分派属 Phase 4）。

pub(crate) const HC_ITER_MAKE: &str = r#"define %Value @hc_iter_make(%Value %base) {
entry:
  %b = call %Value @hc_deref(%Value %base)
  %t = extractvalue %Value %b, 0
  %is_arr = icmp eq i32 %t, 8
  br i1 %is_arr, label %arr, label %chk_slice
arr:
  %d = extractvalue %Value %b, 1
  %op = inttoptr i128 %d to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %op
  %cnt = extractvalue %ArrObj %ao, 0
  %items = extractvalue %ArrObj %ao, 1
  %iter = call %IterObj* @hc_iter_alloc(i64 %cnt)
  br label %arr_loop
arr_loop:
  %i = phi i64 [ 0, %arr ], [ %i2, %arr_next ]
  %adone = icmp uge i64 %i, %cnt
  br i1 %adone, label %fin_arr, label %arr_body
arr_body:
  %asrc = getelementptr %Value, %Value* %items, i64 %i
  call void @hc_iter_set(%IterObj* %iter, i64 %i, %Value* %asrc, i1 true)
  br label %arr_next
arr_next:
  %i2 = add i64 %i, 1
  br label %arr_loop
fin_arr:
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 13, 0
  %ip0 = ptrtoint %IterObj* %iter to i128
  %v1 = insertvalue %Value %v0, i128 %ip0, 1
  ret %Value %v1
chk_slice:
  %is_slc = icmp eq i32 %t, 9
  br i1 %is_slc, label %slc, label %chk_str
slc:
  %d3 = extractvalue %Value %b, 1
  %sop = inttoptr i128 %d3 to %SliceObj*
  %so = load %SliceObj, %SliceObj* %sop
  %sdata = extractvalue %SliceObj %so, 0
  %sstart = extractvalue %SliceObj %so, 1
  %slen = extractvalue %SliceObj %so, 2
  %iter3 = call %IterObj* @hc_iter_alloc(i64 %slen)
  br label %slc_loop
slc_loop:
  %k = phi i64 [ 0, %slc ], [ %k2, %slc_next ]
  %sdone = icmp uge i64 %k, %slen
  br i1 %sdone, label %fin_slc, label %slc_body
slc_body:
  %sidx = add i64 %sstart, %k
  %ssrc = getelementptr %Value, %Value* %sdata, i64 %sidx
  call void @hc_iter_set(%IterObj* %iter3, i64 %k, %Value* %ssrc, i1 true)
  br label %slc_next
slc_next:
  %k2 = add i64 %k, 1
  br label %slc_loop
fin_slc:
  %u0 = insertvalue %Value { i32 0, i128 0 }, i32 13, 0
  %uip = ptrtoint %IterObj* %iter3 to i128
  %u1 = insertvalue %Value %u0, i128 %uip, 1
  ret %Value %u1
chk_str:
  %is_str = icmp eq i32 %t, 5
  br i1 %is_str, label %str_, label %chk_class
str_:
  %d2 = extractvalue %Value %b, 1
  %sp2 = inttoptr i128 %d2 to i8*
  %blen = call i64 @strlen(i8* %sp2)
  %iter2 = call %IterObj* @hc_iter_alloc(i64 %blen)
  br label %str_loop
str_loop:
  %j = phi i64 [ 0, %str_ ], [ %j2, %str_next ]
  %bdone = icmp uge i64 %j, %blen
  br i1 %bdone, label %fin_str, label %str_body
str_body:
  %cp = getelementptr i8, i8* %sp2, i64 %j
  %c = load i8, i8* %cp
  %ci = zext i8 %c to i128
  %bv0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %bv1 = insertvalue %Value %bv0, i128 %ci, 1
  %vsz = ptrtoint %Value* getelementptr (%Value, %Value* null, i32 1) to i64
  %craw = call i8* @hc_alloc(i64 %vsz)
  %cell = bitcast i8* %craw to %Value*
  store %Value %bv1, %Value* %cell
  call void @hc_iter_set(%IterObj* %iter2, i64 %j, %Value* %cell, i1 false)
  br label %str_next
str_next:
  %j2 = add i64 %j, 1
  br label %str_loop
fin_str:
  %w0 = insertvalue %Value { i32 0, i128 0 }, i32 13, 0
  %wip = ptrtoint %IterObj* %iter2 to i128
  %w1 = insertvalue %Value %w0, i128 %wip, 1
  ret %Value %w1
chk_class:
  %is_cls = icmp eq i32 %t, 10
  br i1 %is_cls, label %cls, label %notiter
cls:
  %d4 = extractvalue %Value %b, 1
  %cop = inttoptr i128 %d4 to %ClassObj*
  %co = load %ClassObj, %ClassObj* %cop
  %tyn = extractvalue %ClassObj %co, 0
  %mapp = getelementptr inbounds [4 x i8], ptr @.hc_map, i64 0, i64 0
  %mcmp = call i32 @strcmp(i8* %tyn, i8* %mapp)
  %is_map = icmp eq i32 %mcmp, 0
  br i1 %is_map, label %map_, label %notiter
map_:
  %mcnt = extractvalue %ClassObj %co, 1
  %mfields = extractvalue %ClassObj %co, 2
  %iter4 = call %IterObj* @hc_iter_alloc(i64 %mcnt)
  br label %map_loop
map_loop:
  %m = phi i64 [ 0, %map_ ], [ %m2, %map_next ]
  %mdone = icmp uge i64 %m, %mcnt
  br i1 %mdone, label %fin_map, label %map_body
map_body:
  %mfp = getelementptr %Field, %Field* %mfields, i64 %m
  %mf = load %Field, %Field* %mfp
  %mfn = extractvalue %Field %mf, 0
  %mfv = extractvalue %Field %mf, 1
  ; KV 类：key = 字段名 Str，value = 字段值副本（is_ref=false）
  %kvp = getelementptr inbounds [3 x i8], ptr @.hc_kv, i64 0, i64 0
  %kv = call %Value @hc_make_class(i8* %kvp, i64 2)
  %keyp = getelementptr inbounds [4 x i8], ptr @.hc_key, i64 0, i64 0
  %mfnptr = ptrtoint i8* %mfn to i128
  %ks0 = insertvalue %Value { i32 0, i128 0 }, i32 5, 0
  %ks1 = insertvalue %Value %ks0, i128 %mfnptr, 1
  call void @hc_class_set(%Value %kv, i64 0, i8* %keyp, %Value %ks1)
  %valp = getelementptr inbounds [6 x i8], ptr @.hc_value, i64 0, i64 0
  call void @hc_class_set(%Value %kv, i64 1, i8* %valp, %Value %mfv)
  ; 每项独立 cell 持有 KV 副本（写回不传播到源 Map——与 oracle KV 新 cell 一致）
  %vsz2 = ptrtoint %Value* getelementptr (%Value, %Value* null, i32 1) to i64
  %kraw = call i8* @hc_alloc(i64 %vsz2)
  %kcell = bitcast i8* %kraw to %Value*
  store %Value %kv, %Value* %kcell
  call void @hc_iter_set(%IterObj* %iter4, i64 %m, %Value* %kcell, i1 false)
  br label %map_next
map_next:
  %m2 = add i64 %m, 1
  br label %map_loop
fin_map:
  %x0 = insertvalue %Value { i32 0, i128 0 }, i32 13, 0
  %xip = ptrtoint %IterObj* %iter4 to i128
  %x1 = insertvalue %Value %x0, i128 %xip, 1
  ret %Value %x1
notiter:
  call void @hc_abort_notiter()
  unreachable
}
"#;

pub(crate) const HC_WRITE_BYTES: &str = r#"define void @hc_write_bytes(i8* %p, i64 %n) {
entry:
  %n32 = trunc i64 %n to i32
  %f = getelementptr inbounds [5 x i8], ptr @.fmt_pct, i64 0, i64 0
  call i32 (i8*, ...) @printf(i8* %f, i32 %n32, i8* %p)
  ret void
}
"#;

pub(crate) const HC_WRITE_STRZ: &str = r#"define void @hc_write_strz(i8* %p) {
entry:
  %f = getelementptr inbounds [3 x i8], ptr @.fmt_s, i64 0, i64 0
  call i32 (i8*, ...) @printf(i8* %f, i8* %p)
  ret void
}
"#;

pub(crate) const HC_WRITE_U128_BASE: &str = r#"define void @hc_write_u128_base(i128 %v, i32 %base, i32 %upper) {
entry:
  %buf = alloca [64 x i8], align 1
  %b64 = zext i32 %base to i128
  %up_i1 = icmp ne i32 %upper, 0
  br label %loop
loop:
  %val = phi i128 [ %v, %entry ], [ %quot, %loop ]
  %pos = phi i64 [ 63, %entry ], [ %dec, %loop ]
  %quot = udiv i128 %val, %b64
  %rem = urem i128 %val, %b64
  %rem64 = trunc i128 %rem to i64
  %lt10 = icmp ult i64 %rem64, 10
  %dig = add i64 %rem64, 48
  %off = select i1 %up_i1, i64 55, i64 87
  %up = add i64 %rem64, %off
  %ch64 = select i1 %lt10, i64 %dig, i64 %up
  %ch = trunc i64 %ch64 to i8
  %dst = getelementptr inbounds [64 x i8], ptr %buf, i64 0, i64 %pos
  store i8 %ch, i8* %dst
  %dec = sub i64 %pos, 1
  %is0 = icmp eq i128 %quot, 0
  br i1 %is0, label %write, label %loop
write:
  %sp = getelementptr inbounds [64 x i8], ptr %buf, i64 0, i64 %pos
  %len = sub i64 64, %pos
  call void @hc_write_bytes(i8* %sp, i64 %len)
  ret void
}
"#;

pub(crate) const HC_WRITE_I128_DEC: &str = r#"define void @hc_write_i128_dec(i128 %n) {
entry:
  %is_neg = icmp slt i128 %n, 0
  br i1 %is_neg, label %neg, label %mag
neg:
  %dash = getelementptr inbounds [2 x i8], ptr @.hc_dash, i64 0, i64 0
  call void @hc_write_bytes(i8* %dash, i64 1)
  br label %mag
mag:
  %negv = sub i128 0, %n
  %m = select i1 %is_neg, i128 %negv, i128 %n
  call void @hc_write_u128_base(i128 %m, i32 10, i32 0)
  ret void
}
"#;

pub(crate) const HC_WRITE_INT: &str = r#"define void @hc_write_int(%Value %v, i32 %mode) {
entry:
  %d = extractvalue %Value %v, 1
  %m0 = icmp eq i32 %mode, 0
  %m3 = icmp eq i32 %mode, 3
  %is_dec = or i1 %m0, %m3
  br i1 %is_dec, label %dec, label %basefmt
dec:
  call void @hc_write_i128_dec(i128 %d)
  ret void
basefmt:
  %is_bin = icmp eq i32 %mode, 2
  %is_hex = icmp eq i32 %mode, 1
  %is_hexu = icmp eq i32 %mode, 4
  %is_hex2 = or i1 %is_hex, %is_hexu
  %b1 = select i1 %is_hex2, i32 16, i32 10
  %base = select i1 %is_bin, i32 2, i32 %b1
  %upper = select i1 %is_hexu, i32 1, i32 0
  call void @hc_write_u128_base(i128 %d, i32 %base, i32 %upper)
  ret void
}
"#;

pub(crate) const HC_WRITE_TYPENAME: &str = r#"define void @hc_write_typename(%Value %v) {
entry:
  %tag = extractvalue %Value %v, 0
  %d = extractvalue %Value %v, 1
  %tc = icmp eq i32 %tag, 10
  br i1 %tc, label %cls, label %t_en
t_en:
  %te = icmp eq i32 %tag, 11
  br i1 %te, label %en, label %t2
cls:
  %cop = inttoptr i128 %d to %ClassObj*
  %co = load %ClassObj, %ClassObj* %cop
  %tn = extractvalue %ClassObj %co, 0
  call void @hc_write_strz(i8* %tn)
  ret void
en:
  %eop = inttoptr i128 %d to %EnumObj*
  %eo = load %EnumObj, %EnumObj* %eop
  %n = extractvalue %EnumObj %eo, 0
  call void @hc_write_strz(i8* %n)
  ret void
t2:
  %t2c = icmp eq i32 %tag, 2
  br i1 %t2c, label %n_i128, label %t3
t3:
  %t3c = icmp eq i32 %tag, 3
  br i1 %t3c, label %n_f64, label %t4
t4:
  %t4c = icmp eq i32 %tag, 4
  br i1 %t4c, label %n_bool, label %t5
t5:
  %t5c = icmp eq i32 %tag, 5
  br i1 %t5c, label %n_str, label %t8
t8:
  %t8c = icmp eq i32 %tag, 8
  br i1 %t8c, label %n_arr, label %t9
t9:
  %t9c = icmp eq i32 %tag, 9
  br i1 %t9c, label %n_slice, label %t1
t1:
  %t1c = icmp eq i32 %tag, 1
  br i1 %t1c, label %n_opt, label %t6
t6:
  %t6c = icmp eq i32 %tag, 6
  br i1 %t6c, label %n_err, label %t7
t7:
  %t7c = icmp eq i32 %tag, 7
  br i1 %t7c, label %n_ptr, label %t14
t14:
  %t14c = icmp eq i32 %tag, 14
  br i1 %t14c, label %n_fn, label %t15
t15:
  %t15c = icmp eq i32 %tag, 15
  br i1 %t15c, label %n_closure, label %t12
t12:
  %t12c = icmp eq i32 %tag, 12
  br i1 %t12c, label %n_end, label %t13
t13:
  %t13c = icmp eq i32 %tag, 13
  br i1 %t13c, label %n_iter, label %n_void
n_i128:
  call void @hc_write_strz(i8* @.t_i128)
  ret void
n_f64:
  call void @hc_write_strz(i8* @.t_f64)
  ret void
n_bool:
  call void @hc_write_strz(i8* @.t_bool)
  ret void
n_str:
  call void @hc_write_strz(i8* @.t_str)
  ret void
n_arr:
  call void @hc_write_strz(i8* @.t_arr)
  ret void
n_slice:
  call void @hc_write_strz(i8* @.t_slice)
  ret void
n_opt:
  call void @hc_write_strz(i8* @.t_opt)
  ret void
n_err:
  call void @hc_write_strz(i8* @.t_err)
  ret void
n_ptr:
  call void @hc_write_strz(i8* @.t_ptr)
  ret void
n_fn:
  call void @hc_write_strz(i8* @.t_fn)
  ret void
n_closure:
  call void @hc_write_strz(i8* @.t_closure)
  ret void
n_end:
  call void @hc_write_strz(i8* @.t_end)
  ret void
n_iter:
  call void @hc_write_strz(i8* @.t_iter)
  ret void
n_void:
  call void @hc_write_strz(i8* @.t_void)
  ret void
}
"#;

pub(crate) const HC_WRITE_VALUE: &str = r#"define void @hc_write_value(%Value %v, i32 %mode) {
entry:
  %tag = extractvalue %Value %v, 0
  %d = extractvalue %Value %v, 1
  %ti = icmp eq i32 %tag, 2
  br i1 %ti, label %int_case, label %ck_f
ck_f:
  %tf = icmp eq i32 %tag, 3
  br i1 %tf, label %float_case, label %ck_b
ck_b:
  %tb = icmp eq i32 %tag, 4
  br i1 %tb, label %bool_case, label %ck_s
ck_s:
  %ts = icmp eq i32 %tag, 5
  br i1 %ts, label %str_case, label %ck_n
ck_n:
  %tn = icmp eq i32 %tag, 1
  br i1 %tn, label %null_case, label %ck_e
ck_e:
  %te = icmp eq i32 %tag, 6
  br i1 %te, label %err_case, label %ck_a
ck_a:
  %ta = icmp eq i32 %tag, 8
  br i1 %ta, label %arr_case, label %ck_sl
ck_sl:
  %tsl = icmp eq i32 %tag, 9
  br i1 %tsl, label %slice_case, label %ck_c
ck_c:
  %tc = icmp eq i32 %tag, 10
  br i1 %tc, label %class_case, label %ck_en
ck_en:
  %ten = icmp eq i32 %tag, 11
  br i1 %ten, label %enum_case, label %ck_p
ck_p:
  %tp = icmp eq i32 %tag, 7
  br i1 %tp, label %ptr_case, label %fallback
fallback:
  call void @hc_write_typename(%Value %v)
  ret void
int_case:
  call void @hc_write_int(%Value %v, i32 %mode)
  ret void
float_case:
  %me5 = icmp eq i32 %mode, 5
  br i1 %me5, label %exp_f, label %norm_f
exp_f:
  %dt2 = trunc i128 %d to i64
  %fve = bitcast i64 %dt2 to double
  %pe = getelementptr inbounds [3 x i8], ptr @.fmt_e, i64 0, i64 0
  call i32 (i8*, ...) @printf(i8* %pe, double %fve)
  ret void
norm_f:
  %dt = trunc i128 %d to i64
  %fv = bitcast i64 %dt to double
  %fr = call double @fmod(double %fv, double 1.0)
  %fz = fcmp oeq double %fr, 0.0
  %fa = call double @fabs(double %fv)
  %fl = fcmp olt double %fa, 1.0e15
  %whole = and i1 %fz, %fl
  br i1 %whole, label %whole_f, label %frac_f
whole_f:
  %p1 = getelementptr inbounds [5 x i8], ptr @.fmt_one, i64 0, i64 0
  call i32 (i8*, ...) @printf(i8* %p1, double %fv)
  ret void
frac_f:
  %pg = getelementptr inbounds [6 x i8], ptr @.fmt_g15, i64 0, i64 0
  call i32 (i8*, ...) @printf(i8* %pg, double %fv)
  ret void
bool_case:
  %is_t = icmp eq i128 %d, 1
  %bp = select i1 %is_t, i8* @.hc_true, i8* @.hc_false
  call void @hc_write_strz(i8* %bp)
  ret void
str_case:
  %sp = trunc i128 %d to i64
  %pp = inttoptr i64 %sp to i8*
  %n = call i64 @strlen(i8* %pp)
  call void @hc_write_bytes(i8* %pp, i64 %n)
  ret void
null_case:
  call void @hc_write_strz(i8* @.hc_null)
  ret void
err_case:
  %ep = getelementptr inbounds [7 x i8], ptr @.hc_errpre, i64 0, i64 0
  call void @hc_write_bytes(i8* %ep, i64 6)
  call void @hc_write_i128_dec(i128 %d)
  ret void
ptr_case:
  %pd = call %Value @hc_deref(%Value %v)
  call void @hc_write_value(%Value %pd, i32 %mode)
  ret void
arr_case:
  %ap = inttoptr i128 %d to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %ap
  %alen = extractvalue %ArrObj %ao, 0
  %items = extractvalue %ArrObj %ao, 1
  %lb = getelementptr inbounds [2 x i8], ptr @.hc_lb, i64 0, i64 0
  call void @hc_write_bytes(i8* %lb, i64 1)
  br label %aloop
aloop:
  %i = phi i64 [ 0, %arr_case ], [ %inext, %aelem ]
  %gt0 = icmp ugt i64 %i, 0
  br i1 %gt0, label %acomma, label %aelem
acomma:
  %cma = getelementptr inbounds [3 x i8], ptr @.hc_comma, i64 0, i64 0
  call void @hc_write_bytes(i8* %cma, i64 2)
  br label %aelem
aelem:
  %ep2 = getelementptr %Value, %Value* %items, i64 %i
  %ev = load %Value, %Value* %ep2
  call void @hc_write_value(%Value %ev, i32 0)
  %inext = add i64 %i, 1
  %adone = icmp uge i64 %inext, %alen
  br i1 %adone, label %aend, label %aloop
aend:
  %rb = getelementptr inbounds [2 x i8], ptr @.hc_rb, i64 0, i64 0
  call void @hc_write_bytes(i8* %rb, i64 1)
  ret void
slice_case:
  %sop = inttoptr i128 %d to %SliceObj*
  %so = load %SliceObj, %SliceObj* %sop
  %sdata = extractvalue %SliceObj %so, 0
  %sstart = extractvalue %SliceObj %so, 1
  %slen = extractvalue %SliceObj %so, 2
  %lb2 = getelementptr inbounds [2 x i8], ptr @.hc_lb, i64 0, i64 0
  call void @hc_write_bytes(i8* %lb2, i64 1)
  br label %sloop
sloop:
  %si = phi i64 [ 0, %slice_case ], [ %sinext, %selem ]
  %sgt0 = icmp ugt i64 %si, 0
  br i1 %sgt0, label %scomma, label %selem
scomma:
  %cma2 = getelementptr inbounds [3 x i8], ptr @.hc_comma, i64 0, i64 0
  call void @hc_write_bytes(i8* %cma2, i64 2)
  br label %selem
selem:
  %sidx = add i64 %sstart, %si
  %sep = getelementptr %Value, %Value* %sdata, i64 %sidx
  %sev = load %Value, %Value* %sep
  call void @hc_write_value(%Value %sev, i32 0)
  %sinext = add i64 %si, 1
  %sdone = icmp uge i64 %sinext, %slen
  br i1 %sdone, label %send, label %sloop
send:
  %rb2 = getelementptr inbounds [2 x i8], ptr @.hc_rb, i64 0, i64 0
  call void @hc_write_bytes(i8* %rb2, i64 1)
  ret void
class_case:
  %cop = inttoptr i128 %d to %ClassObj*
  %co = load %ClassObj, %ClassObj* %cop
  %tyname = extractvalue %ClassObj %co, 0
  call void @hc_write_strz(i8* %tyname)
  %clen = extractvalue %ClassObj %co, 1
  %cfields = extractvalue %ClassObj %co, 2
  %bl = getelementptr inbounds [4 x i8], ptr @.hc_bra_l, i64 0, i64 0
  call void @hc_write_bytes(i8* %bl, i64 3)
  br label %cloop
cloop:
  %ci = phi i64 [ 0, %class_case ], [ %cinext, %celem ]
  %cgt0 = icmp ugt i64 %ci, 0
  br i1 %cgt0, label %ccomma, label %celem
ccomma:
  %cma3 = getelementptr inbounds [3 x i8], ptr @.hc_comma, i64 0, i64 0
  call void @hc_write_bytes(i8* %cma3, i64 2)
  br label %celem
celem:
  %cfp = getelementptr %Field, %Field* %cfields, i64 %ci
  %cfv = load %Field, %Field* %cfp
  %cfname = extractvalue %Field %cfv, 0
  %cfval = extractvalue %Field %cfv, 1
  %fnl = call i64 @strlen(i8* %cfname)
  call void @hc_write_bytes(i8* %cfname, i64 %fnl)
  %eqs = getelementptr inbounds [4 x i8], ptr @.hc_eqs, i64 0, i64 0
  call void @hc_write_bytes(i8* %eqs, i64 3)
  call void @hc_write_value(%Value %cfval, i32 0)
  %cinext = add i64 %ci, 1
  %cdone = icmp uge i64 %cinext, %clen
  br i1 %cdone, label %cend, label %cloop
cend:
  %brr = getelementptr inbounds [3 x i8], ptr @.hc_bra_r, i64 0, i64 0
  call void @hc_write_bytes(i8* %brr, i64 3)
  ret void
enum_case:
  %eop = inttoptr i128 %d to %EnumObj*
  %eo = load %EnumObj, %EnumObj* %eop
  %ename = extractvalue %EnumObj %eo, 0
  %evariant = extractvalue %EnumObj %eo, 1
  %epay = extractvalue %EnumObj %eo, 2
  call void @hc_write_strz(i8* %ename)
  %dot = getelementptr inbounds [2 x i8], ptr @.hc_dot, i64 0, i64 0
  call void @hc_write_bytes(i8* %dot, i64 1)
  call void @hc_write_strz(i8* %evariant)
  %is_none = icmp eq %Value* %epay, null
  br i1 %is_none, label %en_end, label %en_pay
en_pay:
  %eqs2 = getelementptr inbounds [4 x i8], ptr @.hc_eqs, i64 0, i64 0
  call void @hc_write_bytes(i8* %eqs2, i64 3)
  %pv = load %Value, %Value* %epay
  call void @hc_write_value(%Value %pv, i32 0)
  br label %en_end
en_end:
  ret void
}
"#;

pub(crate) const HC_MIN: &str = r#"define %Value @hc_min(%Value %a, %Value %b) {
entry:
  %lt = call i1 @hc_lt(%Value %a, %Value %b)
  %r = select i1 %lt, %Value %a, %Value %b
  ret %Value %r
}
"#;

pub(crate) const HC_MAX: &str = r#"define %Value @hc_max(%Value %a, %Value %b) {
entry:
  %lt = call i1 @hc_lt(%Value %a, %Value %b)
  %r = select i1 %lt, %Value %b, %Value %a
  ret %Value %r
}
"#;

pub(crate) const HC_SQRT: &str = r#"define %Value @hc_sqrt(%Value %v) {
entry:
  %t = extractvalue %Value %v, 0
  %d = extractvalue %Value %v, 1
  %is_int = icmp eq i32 %t, 2
  %dt = trunc i128 %d to i64
  %asf = sitofp i64 %dt to double
  %raw = bitcast i64 %dt to double
  %f = select i1 %is_int, double %asf, double %raw
  %sq = call double @sqrt(double %f)
  %bits = bitcast double %sq to i64
  %z = zext i64 %bits to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 3, 0
  %v1 = insertvalue %Value %v0, i128 %z, 1
  ret %Value %v1
}
"#;

/// math 一元浮点 helper（对齐 oracle call_math interp.rs:4922-4960）：
/// Int → f64 强制 / Float 直用，应用 `op`（LLVM 计算行，结果在 `%r`）后返回 Float。
/// 与 `@hc_sqrt` 同构；`pow` 在 oracle 为 `f.powf(2.0)`（单参平方）。

pub(crate) const HC_BOX: &str = r#"define %Value @hc_box(%Value %v) {
entry:
  %sz = ptrtoint %Value* getelementptr (%Value, %Value* null, i32 1) to i64
  %raw = call i8* @hc_alloc(i64 %sz)
  %cell = bitcast i8* %raw to %Value*
  store %Value %v, %Value* %cell
  %dp = ptrtoint %Value* %cell to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 7, 0
  %v1 = insertvalue %Value %v0, i128 %dp, 1
  ret %Value %v1
}
"#;

pub(crate) const HC_COPY: &str = r#"define %Value @hc_copy(%Value %v, %Value %mode) {
entry:
  %t = extractvalue %Value %mode, 0
  %is_enum = icmp eq i32 %t, 11
  br i1 %is_enum, label %chk, label %deep
chk:
  %d = extractvalue %Value %mode, 1
  %op = inttoptr i128 %d to %EnumObj*
  %eo = load %EnumObj, %EnumObj* %op
  %vn = extractvalue %EnumObj %eo, 1
  %sp = getelementptr inbounds [8 x i8], ptr @.hc_shallow, i64 0, i64 0
  %c = call i32 @strcmp(i8* %vn, i8* %sp)
  %is_sh = icmp eq i32 %c, 0
  br i1 %is_sh, label %id, label %deep
id:
  ret %Value %v
deep:
  %t2 = extractvalue %Value %v, 0
  %t8 = icmp eq i32 %t2, 8
  %t10 = icmp eq i32 %t2, 10
  %t7 = icmp eq i32 %t2, 7
  %t1 = icmp eq i32 %t2, 1
  %is_agg = or i1 %t8, %t10
  %is_agg2 = or i1 %is_agg, %t7
  %is_agg3 = or i1 %is_agg2, %t1
  br i1 %is_agg3, label %abort, label %id
abort:
  call void @hc_abort_builtin()
  unreachable
}
"#;

pub(crate) const HC_INTCAST: &str = r#"define %Value @hc_intcast(%Value %v, i128 %min, i128 %max) {
entry:
  %t = extractvalue %Value %v, 0
  %is_int = icmp eq i32 %t, 2
  br i1 %is_int, label %chk, label %abort
chk:
  %d = extractvalue %Value %v, 1
  %lo_ok = icmp sge i128 %d, %min
  %hi_ok = icmp sle i128 %d, %max
  %ok = and i1 %lo_ok, %hi_ok
  br i1 %ok, label %retv, label %abort
retv:
  ret %Value %v
abort:
  call void @hc_abort_intcast()
  unreachable
}
"#;

pub(crate) const HC_TYPEOF: &str = r#"define %Value @hc_typeof(%Value %v) {
entry:
  %tag = extractvalue %Value %v, 0
  %d = extractvalue %Value %v, 1
  %tc = icmp eq i32 %tag, 10
  br i1 %tc, label %cls, label %t_en
t_en:
  %te = icmp eq i32 %tag, 11
  br i1 %te, label %en, label %t2
cls:
  %cop = inttoptr i128 %d to %ClassObj*
  %co = load %ClassObj, %ClassObj* %cop
  %tn = extractvalue %ClassObj %co, 0
  br label %ret_str
en:
  %eop = inttoptr i128 %d to %EnumObj*
  %eo = load %EnumObj, %EnumObj* %eop
  %en_tn = extractvalue %EnumObj %eo, 0
  br label %ret_str
t2:
  %t2c = icmp eq i32 %tag, 2
  br i1 %t2c, label %n_i128, label %t3
t3:
  %t3c = icmp eq i32 %tag, 3
  br i1 %t3c, label %n_f64, label %t4
t4:
  %t4c = icmp eq i32 %tag, 4
  br i1 %t4c, label %n_bool, label %t5
t5:
  %t5c = icmp eq i32 %tag, 5
  br i1 %t5c, label %n_str, label %t8
t8:
  %t8c = icmp eq i32 %tag, 8
  br i1 %t8c, label %n_arr, label %t9
t9:
  %t9c = icmp eq i32 %tag, 9
  br i1 %t9c, label %n_slice, label %t1
t1:
  %t1c = icmp eq i32 %tag, 1
  br i1 %t1c, label %n_opt, label %t6
t6:
  %t6c = icmp eq i32 %tag, 6
  br i1 %t6c, label %n_err, label %t7
t7:
  %t7c = icmp eq i32 %tag, 7
  br i1 %t7c, label %n_ptr, label %t14
t14:
  %t14c = icmp eq i32 %tag, 14
  br i1 %t14c, label %n_fn, label %t15
t15:
  %t15c = icmp eq i32 %tag, 15
  br i1 %t15c, label %n_closure, label %t12
t12:
  %t12c = icmp eq i32 %tag, 12
  br i1 %t12c, label %n_end, label %t13
t13:
  %t13c = icmp eq i32 %tag, 13
  br i1 %t13c, label %n_iter, label %n_void
n_i128:
  br label %ret_global
n_f64:
  br label %ret_global
n_bool:
  br label %ret_global
n_str:
  br label %ret_global
n_arr:
  br label %ret_global
n_slice:
  br label %ret_global
n_opt:
  br label %ret_global
n_err:
  br label %ret_global
n_ptr:
  br label %ret_global
n_fn:
  br label %ret_global
n_closure:
  br label %ret_global
n_end:
  br label %ret_global
n_iter:
  br label %ret_global
n_void:
  br label %ret_global
ret_global:
  %gp = phi i8* [ @.t_i128, %n_i128 ], [ @.t_f64, %n_f64 ], [ @.t_bool, %n_bool ], [ @.t_str, %n_str ], [ @.t_arr, %n_arr ], [ @.t_slice, %n_slice ], [ @.t_opt, %n_opt ], [ @.t_err, %n_err ], [ @.t_ptr, %n_ptr ], [ @.t_fn, %n_fn ], [ @.t_closure, %n_closure ], [ @.t_end, %n_end ], [ @.t_iter, %n_iter ], [ @.t_void, %n_void ]
  br label %ret_str
ret_str:
  %tp = phi i8* [ %tn, %cls ], [ %en_tn, %en ], [ %gp, %ret_global ]
  %pi = ptrtoint i8* %tp to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 5, 0
  %v1 = insertvalue %Value %v0, i128 %pi, 1
  ret %Value %v1
}
"#;

pub(crate) const HC_READ_U64_LE: &str = r#"define %Value @hc_read_u64_le(%Value %v) {
entry:
  %t = extractvalue %Value %v, 0
  %is_str = icmp eq i32 %t, 5
  br i1 %is_str, label %read, label %abort
read:
  %d = extractvalue %Value %v, 1
  %p = trunc i128 %d to i64
  %pp = inttoptr i64 %p to i8*
  %b0 = load i8, i8* %pp
  %u0 = zext i8 %b0 to i64
  %p1 = getelementptr i8, i8* %pp, i64 1
  %b1 = load i8, i8* %p1
  %u1 = zext i8 %b1 to i64
  %s1 = shl i64 %u1, 8
  %o1 = or i64 %u0, %s1
  %p2 = getelementptr i8, i8* %pp, i64 2
  %b2 = load i8, i8* %p2
  %u2 = zext i8 %b2 to i64
  %s2 = shl i64 %u2, 16
  %o2 = or i64 %o1, %s2
  %p3 = getelementptr i8, i8* %pp, i64 3
  %b3 = load i8, i8* %p3
  %u3 = zext i8 %b3 to i64
  %s3 = shl i64 %u3, 24
  %o3 = or i64 %o2, %s3
  %p4 = getelementptr i8, i8* %pp, i64 4
  %b4 = load i8, i8* %p4
  %u4 = zext i8 %b4 to i64
  %s4 = shl i64 %u4, 32
  %o4 = or i64 %o3, %s4
  %p5 = getelementptr i8, i8* %pp, i64 5
  %b5 = load i8, i8* %p5
  %u5 = zext i8 %b5 to i64
  %s5 = shl i64 %u5, 40
  %o5 = or i64 %o4, %s5
  %p6 = getelementptr i8, i8* %pp, i64 6
  %b6 = load i8, i8* %p6
  %u6 = zext i8 %b6 to i64
  %s6 = shl i64 %u6, 48
  %o6 = or i64 %o5, %s6
  %p7 = getelementptr i8, i8* %pp, i64 7
  %b7 = load i8, i8* %p7
  %u7 = zext i8 %b7 to i64
  %s7 = shl i64 %u7, 56
  %o7 = or i64 %o6, %s7
  %z = zext i64 %o7 to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %z, 1
  ret %Value %v1
abort:
  call void @hc_abort_typeerr()
  unreachable
}
"#;

/// fmt_int<i32> String：i128 → 十进制 → 堆缓冲 Str 值（对齐 oracle display）

pub(crate) const HC_FMT_INT: &str = r#"define %Value @hc_fmt_int(%Value %v) {
entry:
  %d = extractvalue %Value %v, 1
  %is_neg = icmp slt i128 %d, 0
  %negv = sub i128 0, %d
  %mag = select i1 %is_neg, i128 %negv, i128 %d
  %buf = alloca [64 x i8], align 1
  br label %loop
loop:
  %val = phi i128 [ %mag, %entry ], [ %quot, %loop ]
  %pos = phi i64 [ 63, %entry ], [ %dec, %loop ]
  %quot = udiv i128 %val, 10
  %rem = urem i128 %val, 10
  %rem64 = trunc i128 %rem to i64
  %ch64 = add i64 %rem64, 48
  %ch = trunc i64 %ch64 to i8
  %dst = getelementptr inbounds [64 x i8], ptr %buf, i64 0, i64 %pos
  store i8 %ch, i8* %dst
  %dec = sub i64 %pos, 1
  %is0 = icmp eq i128 %quot, 0
  br i1 %is0, label %done, label %loop
done:
  %dcount = sub i64 64, %pos
  %extra = select i1 %is_neg, i64 1, i64 0
  %nbytes = add i64 %dcount, %extra
  %allocn = add i64 %nbytes, 1
  %bufh = call i8* @hc_alloc(i64 %allocn)
  %dp = getelementptr inbounds [64 x i8], ptr %buf, i64 0, i64 %pos
  %dstd = getelementptr i8, i8* %bufh, i64 %extra
  call void @llvm.memcpy.p0i8.p0i8.i64(i8* %dstd, i8* %dp, i64 %dcount, i1 false)
  %nuloff = add i64 %dcount, %extra
  %nulp = getelementptr i8, i8* %bufh, i64 %nuloff
  store i8 0, i8* %nulp
  br i1 %is_neg, label %sig, label %mk
sig:
  store i8 45, i8* %bufh
  br label %mk
mk:
  %pi = ptrtoint i8* %bufh to i128
  %x0 = insertvalue %Value { i32 0, i128 0 }, i32 5, 0
  %x1 = insertvalue %Value %x0, i128 %pi, 1
  ret %Value %x1
}
"#;

/// fmt_float(f64) String：sprintf 格式（整值 `%.1f`，否则 `%.15g`，对齐 oracle display）；
/// 接受 Int 实参（对齐 interp/IR 的数值提升）

pub(crate) const HC_FMT_FLOAT: &str = r#"define %Value @hc_fmt_float(%Value %v) {
entry:
  %tag = extractvalue %Value %v, 0
  %d = extractvalue %Value %v, 1
  %is_int = icmp eq i32 %tag, 2
  %as_double = sitofp i128 %d to double
  %dt = trunc i128 %d to i64
  %fbits = bitcast i64 %dt to double
  %fv = select i1 %is_int, double %as_double, double %fbits
  %buf = alloca [64 x i8], align 1
  %fr = call double @fmod(double %fv, double 1.0)
  %fz = fcmp oeq double %fr, 0.0
  %fa = call double @fabs(double %fv)
  %fl = fcmp olt double %fa, 1.0e15
  %whole = and i1 %fz, %fl
  br i1 %whole, label %whole_f, label %frac_f
whole_f:
  %p1 = getelementptr inbounds [5 x i8], ptr @.fmt_one, i64 0, i64 0
  call i32 (i8*, ...) @sprintf(i8* %buf, i8* %p1, double %fv)
  br label %mk
frac_f:
  %pg = getelementptr inbounds [6 x i8], ptr @.fmt_g15, i64 0, i64 0
  call i32 (i8*, ...) @sprintf(i8* %buf, i8* %pg, double %fv)
  br label %mk
mk:
  %len = call i64 @strlen(i8* %buf)
  %allocn = add i64 %len, 1
  %bufh = call i8* @hc_alloc(i64 %allocn)
  call void @llvm.memcpy.p0i8.p0i8.i64(i8* %bufh, i8* %buf, i64 %len, i1 false)
  %nulp = getelementptr i8, i8* %bufh, i64 %len
  store i8 0, i8* %nulp
  %pi = ptrtoint i8* %bufh to i128
  %x0 = insertvalue %Value { i32 0, i128 0 }, i32 5, 0
  %x1 = insertvalue %Value %x0, i128 %pi, 1
  ret %Value %x1
}
"#;

pub(crate) const HC_ADD_OVERFLOW: &str = r#"define %Value @hc_add_overflow(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %ai = icmp eq i32 %ta, 2
  %bi = icmp eq i32 %tb, 2
  %both = and i1 %ai, %bi
  br i1 %both, label %int_op, label %abort
int_op:
  %res = call { i128, i1 } @llvm.sadd.with.overflow.i128(i128 %da, i128 %db)
  %rv = extractvalue { i128, i1 } %res, 0
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %rv, 1
  %arr = call %Value @hc_make_arr(i64 2)
  call void @hc_arr_set(%Value %arr, i64 0, %Value %v1)
  %boolv = call %Value @hc_bool(i1 false)
  call void @hc_arr_set(%Value %arr, i64 1, %Value %boolv)
  ret %Value %arr
abort:
  call void @hc_abort_typeerr()
  unreachable
}
"#;

pub(crate) const HC_SUB_OVERFLOW: &str = r#"define %Value @hc_sub_overflow(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %ai = icmp eq i32 %ta, 2
  %bi = icmp eq i32 %tb, 2
  %both = and i1 %ai, %bi
  br i1 %both, label %int_op, label %abort
int_op:
  %res = call { i128, i1 } @llvm.ssub.with.overflow.i128(i128 %da, i128 %db)
  %rv = extractvalue { i128, i1 } %res, 0
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %rv, 1
  %arr = call %Value @hc_make_arr(i64 2)
  call void @hc_arr_set(%Value %arr, i64 0, %Value %v1)
  %boolv = call %Value @hc_bool(i1 false)
  call void @hc_arr_set(%Value %arr, i64 1, %Value %boolv)
  ret %Value %arr
abort:
  call void @hc_abort_typeerr()
  unreachable
}
"#;

pub(crate) const HC_MUL_OVERFLOW: &str = r#"define %Value @hc_mul_overflow(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %ai = icmp eq i32 %ta, 2
  %bi = icmp eq i32 %tb, 2
  %both = and i1 %ai, %bi
  br i1 %both, label %int_op, label %abort
int_op:
  %res = call { i128, i1 } @llvm.smul.with.overflow.i128(i128 %da, i128 %db)
  %rv = extractvalue { i128, i1 } %res, 0
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %rv, 1
  %arr = call %Value @hc_make_arr(i64 2)
  call void @hc_arr_set(%Value %arr, i64 0, %Value %v1)
  %boolv = call %Value @hc_bool(i1 false)
  call void @hc_arr_set(%Value %arr, i64 1, %Value %boolv)
  ret %Value %arr
abort:
  call void @hc_abort_typeerr()
  unreachable
}
"#;

pub(crate) const HC_MAKE_IO: &str = r#"define %Value @hc_make_io() {
entry:
  %fs = call %Value @hc_make_class(i8* @.t_fs, i64 0)
  %time = call %Value @hc_make_class(i8* @.t_time, i64 0)
  %net = call %Value @hc_make_class(i8* @.t_net, i64 0)
  %io = call %Value @hc_make_class(i8* @.t_io, i64 3)
  %fp = getelementptr inbounds [3 x i8], ptr @.f_fs, i64 0, i64 0
  call void @hc_class_set(%Value %io, i64 0, i8* %fp, %Value %fs)
  %tp = getelementptr inbounds [5 x i8], ptr @.f_time, i64 0, i64 0
  call void @hc_class_set(%Value %io, i64 1, i8* %tp, %Value %time)
  %np = getelementptr inbounds [4 x i8], ptr @.f_net, i64 0, i64 0
  call void @hc_class_set(%Value %io, i64 2, i8* %np, %Value %net)
  ret %Value %io
}
"#;
