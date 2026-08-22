//! LLVM 后端单元测试。

use super::*;
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
        assert!(
            ll.contains("define i32 @main(i32 %argc, i8** %argv)"),
            "{ll}"
        );
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
        assert!(
            ll.contains("insertvalue %Value { i32 0, i128 0 }, i32 6, 0"),
            "{ll}"
        );
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
    fn volatile_load_store_emit_volatile_ops() {
        // K2：@volatileLoad/@volatileStore → hc_volatile_load/hc_volatile_store——
        // 内嵌 LLVM `load volatile`/`store volatile`（防优化掉副作用，MMIO 场景）
        let ll = gen(
            "fn f() i32 { var mut x: i32 = 5; var p = &mut x; @volatileStore(p, 7); return @volatileLoad(p); }",
        );
        assert!(ll.contains("define %Value @hc_volatile_load"), "{ll}");
        assert!(ll.contains("define void @hc_volatile_store"), "{ll}");
        assert!(ll.contains("load volatile %Value"), "{ll}");
        assert!(ll.contains("store volatile %Value"), "{ll}");
    }

    #[test]
    fn ptr_from_int_int_from_ptr_emit_tag_swaps() {
        // K4：@intFromPtr(p) 提取指针 i128 载荷 → 重建 T_INT 值；@ptrFromInt(a) 提取整数
        // 载荷 → 重建 T_PTR 值（载荷搬运，extractvalue → insertvalue）。写穿重建指针经
        // hc_deref 可见（与 AddrSlot 指针同一路径）。
        let ll = gen(
            "fn f() i32 { var x: i32 = 5; var p = &mut x; var a = @intFromPtr(p); var q = @ptrFromInt(a); @volatileStore(q, 9); return x; }",
        );
        assert!(ll.contains("extractvalue %Value"), "{ll}");
        assert!(ll.contains("call %Value @hc_deref"), "{ll}");
    }

    #[test]
    fn export_fn_emits_thunk_and_manifest() {
        // K5：`export fn` → 外部 thunk `define %Value @"add"` 调用别名 `@"hc_fn0"`
        // + 符号清单注释 `; exports: add`；`_start` 导出 → `; entry: _start` 钩子。
        let ll = gen(
            "export fn add(a: i32, b: i32) i32 { return a + b; } fn main() i32 { return add(1, 2); }",
        );
        assert!(ll.contains("; exports: add"), "{ll}");
        assert!(
            ll.contains("define %Value @\"add\"(%Value %p0, %Value %p1)"),
            "{ll}"
        );
        assert!(
            ll.contains("call %Value @\"hc_fn0\"(%Value %p0, %Value %p1)"),
            "{ll}"
        );
        assert!(ll.contains("ret %Value %r"), "{ll}");
        // 非导出函数不产生 thunk
        assert!(!ll.contains("define %Value @\"main\"("), "{ll}");
    }

    #[test]
    fn export_start_marks_entry_hook() {
        // K5：`_start` 导出 = 链接脚本入口钩子标记（freestanding 完整入口属 H5/K6）
        let ll = gen("export fn _start() i32 { return 0; } fn main() i32 { return _start(); }");
        assert!(ll.contains("; exports: _start"), "{ll}");
        assert!(ll.contains("; entry: _start"), "{ll}");
        assert!(ll.contains("define %Value @\"_start\"()"), "{ll}");
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
        let ll = gen(
            "fn f() bool { var mut x: i32 = 5; var p = &mut x; var q = &mut x; return p == q; }",
        );
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
        let ll = gen("fn f() i32 { var a = [10, 20, 30]; a[1] = 99; return a[1]; }");
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
    fn aggregate_append_emits_coll_helpers() {
        // `Vec.append`（Arr 接收者内建方法）→ hc_append 定义 + 调用；append_u64/extend
        // helper 随 preamble 发射（供 57-protocol 等）。对齐 run_ir `call_builtin_method`
        // Arr 臂的原地扩容语义（同一 `%ArrObj` 指针写回，别名可见）。
        let ll = gen(
            r#"class Node { value: i32, children: Vec<Node>, } fn f() i32 {
    var r = Node.new(1, alloc);
    r.children.append(Node.new(2, alloc));
    r.children.append(Node.new(3, alloc));
    return r.children.len;
}"#,
        );
        assert!(ll.contains("define void @hc_append"), "{ll}");
        assert!(ll.contains("define void @hc_append_u64"), "{ll}");
        assert!(ll.contains("define void @hc_extend"), "{ll}");
        assert!(ll.contains("call void @hc_append"), "{ll}");
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
        let ll = gen("fn f() bool { var a = [1, 2, 3]; var b = [1, 2, 3]; return a == b; }");
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

    // ---- Phase 3 switch + range + for 发射 ----

    #[test]
    fn phase3_switch_emits_match_test_helper() {
        // MatchTest 指令 → @hc_match_test（模式描述符：tag/data/str/len）
        let ll = gen("fn f(x: i32) i32 { switch (x) { 1 => return 10, else => return 99, } }");
        assert!(ll.contains("define %Value @hc_match_test"), "{ll}");
        assert!(ll.contains("call %Value @hc_match_test"), "{ll}");
        // switch 指令：i8 类型限定的 case 列表
        assert!(ll.contains("switch i8 %tag, label %done"), "{ll}");
        assert!(ll.contains("i8 0, label %t_err"), "{ll}");
        assert!(ll.contains("i8 2, label %t_int"), "{ll}");
    }

    #[test]
    fn phase3_switch_string_pattern_collected() {
        // Str/Ident 模式字符串进入字符串表（@.str.N 全局）
        let ll =
            gen(r#"fn pick(s: String) i32 { switch (s) { "hi" => return 1, else => return 0, } }"#);
        assert!(ll.contains("@hc_match_test"), "{ll}");
        assert!(ll.contains("c\"hi\\00\""), "{ll}");
    }

    #[test]
    fn phase3_enum_payload_emits_helper() {
        // EnumPayload 指令 → @hc_enum_payload
        let ll = gen(
            r#"enum Maybe { some: i32, none } fn f(m: Maybe) i32 { switch (m) { some => |i| i, none => -1, } }"#,
        );
        assert!(ll.contains("define %Value @hc_enum_payload"), "{ll}");
        assert!(ll.contains("call %Value @hc_enum_payload"), "{ll}");
    }

    #[test]
    fn phase3_make_range_emits_helper() {
        // MakeRange 指令 → @hc_make_range（区间 [lo, hi) → Arr）
        let ll = gen("fn f() i32 { var mut s: i32 = 0; for (0..4) |i| { s += i; } return s; }");
        assert!(ll.contains("define %Value @hc_make_range"), "{ll}");
        assert!(ll.contains("call %Value @hc_make_range"), "{ll}");
    }

    #[test]
    fn phase3_iter_emits_iter_helpers() {
        // IterMake/IterNext/IterWriteBack → @hc_iter_make/@hc_iter_next/@hc_iter_write_back
        let ll = gen("fn f() i32 { var a = [1, 2]; for (a) |mut x| { x += 1; } return a[1]; }");
        assert!(ll.contains("define %IterObj* @hc_iter_alloc"), "{ll}");
        assert!(ll.contains("define void @hc_iter_set"), "{ll}");
        assert!(ll.contains("define %Value @hc_iter_make"), "{ll}");
        assert!(ll.contains("define %Value @hc_iter_next"), "{ll}");
        assert!(ll.contains("define void @hc_iter_write_back"), "{ll}");
        assert!(ll.contains("call %Value @hc_iter_make"), "{ll}");
        assert!(ll.contains("call %Value @hc_iter_next"), "{ll}");
        assert!(ll.contains("call void @hc_iter_write_back"), "{ll}");
        assert!(ll.contains("%IterItemObj = type"), "{ll}");
        assert!(ll.contains("%IterObj = type"), "{ll}");
    }

    #[test]
    fn phase3_iter_notiter_message_present() {
        // NotIterable 硬错误消息：@.msg_notiter 全局 + hc_abort_notiter helper
        let ll = gen(
            "fn f() i32 { var a = [1]; var mut s: i32 = 0; for (a) |x| { s += x; } return s; }",
        );
        assert!(ll.contains("@.msg_notiter"), "{ll}");
        assert!(ll.contains("define void @hc_abort_notiter"), "{ll}");
    }

    // ---- Phase 7 原生内建 helper（子集） ----

    #[test]
    fn phase7_io_print_emits_write_helpers() {
        // main(io: Io) 的 io.print：单参 main 注入 @hc_make_io()；格式串切分为
        // 字面量段（hc_write_bytes）+ 参数槽（hc_write_value，模式 0=显示）。
        let ll = gen("fn main(io: Io) !void { io.print(\"x = {}, y = {}\\n\", 42, 3.14); }");
        assert!(ll.contains("call %Value @hc_make_io()"), "{ll}");
        assert!(ll.contains("define %Value @hc_make_io()"), "{ll}");
        assert!(ll.contains("define void @hc_write_bytes"), "{ll}");
        assert!(ll.contains("define void @hc_write_value"), "{ll}");
        // 字面量段 "x = " 与 "\n" 登记为独立全局（不是格式串整体）
        assert!(ll.contains("c\"x = \\00\""), "{ll}");
        assert!(ll.contains("c\"\\0A\\00\""), "{ll}");
        assert!(ll.contains("call void @hc_write_bytes"), "{ll}");
        assert!(ll.contains("call void @hc_write_value"), "{ll}");
    }

    #[test]
    fn phase7_alloc_init_emits_make_class() {
        // alloc.init(ABC) → MakeClass 默认字段 → @hc_make_class(ABC 类型名全局, i64 1)（1 字段）
        let ll =
            gen("class ABC { x: i32, } fn main() i32 { var abc = alloc.init(ABC); return abc.x; }");
        assert!(ll.contains("call %Value @hc_make_class(i8*"), "{ll}");
        assert!(ll.contains("i64 1)"), "{ll}");
        assert!(ll.contains("c\"ABC\\00\""), "{ll}");
    }

    #[test]
    fn phase7_math_builtins_emit_nan_and_helpers() {
        // math.nan(f64) 类型名参数 → 忽略，直接发 NaN 位模式 Float；
        // math.sqrt/abs/pow → 一元 helper 调用；helper 定义进 preamble。
        let ll = gen(r#"fn main(io: Io) !void {
    var nan = math.nan(f64);
    var s = math.sqrt(4.0);
    var p = math.pow(3.0);
    io.print("{} {} {}\n", nan, s, p);
}"#);
        // f64::NAN.to_bits() = 0x7FF8000000000000
        assert!(ll.contains("i32 3, 0"), "{ll}");
        assert!(ll.contains("i128 9221120237041090560"), "{ll}");
        assert!(ll.contains("call %Value @hc_sqrt(%Value"), "{ll}");
        assert!(ll.contains("call %Value @hc_pow(%Value"), "{ll}");
        assert!(ll.contains("define %Value @hc_abs(%Value"), "{ll}");
        assert!(ll.contains("define %Value @hc_floor(%Value"), "{ll}");
        assert!(ll.contains("define %Value @hc_ceil(%Value"), "{ll}");
        assert!(ll.contains("define %Value @hc_round(%Value"), "{ll}");
        assert!(ll.contains("define %Value @hc_pow(%Value"), "{ll}");
    }

    #[test]
    fn phase7_user_method_dispatch_emits_strcmp_chain() {
        // abc.print(&io)：运行时分派——解引用基值 → 类名 strcmp 匹配 "ABC" →
        // 调 hc_fn{k} 且 self（解引用后值）注入首参。
        let ll = gen(r#"class ABC {
    pub fn print(self: *Self, io: *Io) { io.print("m\n"); }
}
fn main(io: Io) !void {
    var abc = alloc.init(ABC);
    abc.print(&io);
}"#);
        assert!(ll.contains("call i32 @strcmp"), "{ll}");
        assert!(ll.contains("c\"ABC\\00\""), "{ll}");
        assert!(ll.contains("call %Value @hc_deref"), "{ll}");
        assert!(ll.contains("hc_fn"), "{ll}");
        // self 注入：方法分派后至少一处以解引用值调 hc_fn{k}
        assert!(ll.contains("call %Value @\"hc_fn"), "{ll}");
    }

    #[test]
    fn phase7_scalar_builtins_emit_helpers() {
        // 标量 @ 内建 + 自由内建 → 各自 helper 调用（不再静默 Void）
        let ll = gen(r#"fn main() !void {
    try expect_eq(@sizeOf(i32), 4);
    try expect_eq(@intCast(i32, 7), 7);
    try expect_eq(@typeOf(42), "i128");
    try expect_eq(min(3, 9), 3);
    try expect_eq(max(3, 9), 9);
    var p = box(42);
    try expect_eq(p.*, 42);
}"#);
        assert!(ll.contains("call %Value @hc_intcast"), "{ll}");
        assert!(ll.contains("call %Value @hc_typeof"), "{ll}");
        assert!(ll.contains("call %Value @hc_min"), "{ll}");
        assert!(ll.contains("call %Value @hc_max"), "{ll}");
        assert!(ll.contains("call %Value @hc_box"), "{ll}");
        // @sizeOf 编译期常量折叠 → i32 槽直接存 4（非 helper 调用）
        assert!(ll.contains("store %Value"), "{ll}");
    }

    #[test]
    fn phase7_fmt_builtins_emit_str_helpers() {
        // D3：fmt_int/fmt_float 自由内建 → hc_fmt_int/hc_fmt_float helper 调用；
        // helper 定义进 preamble（hc_alloc 堆缓冲 Str 值）。
        let ll = gen(r#"fn main() !void {
    var s1 = fmt_int(30);
    var s2 = fmt_float(3.5);
    io.print("{} {}\n", s1, s2);
}"#);
        assert!(ll.contains("call %Value @hc_fmt_int(%Value"), "{ll}");
        assert!(ll.contains("call %Value @hc_fmt_float(%Value"), "{ll}");
        assert!(ll.contains("define %Value @hc_fmt_int(%Value"), "{ll}");
        assert!(ll.contains("define %Value @hc_fmt_float(%Value"), "{ll}");
        // sprintf 声明（hc_fmt_float 数字→字符串用）
        assert!(ll.contains("declare i32 @sprintf(i8*, ...)"), "{ll}");
    }

    #[test]
    fn continuous_class_copy_emits_deep_copy_gate() {
        // P11d：连续类 var 声明 → DeepCopy 指令 → `hc_deep_copy_cont` 运行时门 +
        // 递归 `hc_deep_copy` helper；连续类名进字符串表供 strcmp 链匹配。
        let ll = gen(r#"[continuous]
class Point {
    x: f32,
    y: f32,
}
fn f() f32 {
    var p: Point = Point{ x = 1.0, y = 2.0 };
    var p2: Point = p;
    p2.x = 99.0;
    return p.x;
}"#);
        assert!(ll.contains("define %Value @hc_deep_copy_cont"), "{ll}");
        assert!(ll.contains("define %Value @hc_deep_copy"), "{ll}");
        assert!(ll.contains("call %Value @hc_deep_copy_cont"), "{ll}");
        assert!(ll.contains("c\"Point\\00\""), "{ll}"); // 连续类名进字符串表
        assert!(ll.contains("call i32 @strcmp"), "{ll}"); // 门 strcmp 链
    }

    #[test]
    fn non_continuous_class_no_deep_copy_inst() {
        // 非连续类 var 声明不发射 DeepCopy 指令（引用类型，按值赋值被语义层拒绝）；
        // 无连续类模块的门为恒等（仍定义，但无调用）。
        let ll = gen(r#"class Blob {
    x: i32,
    y: i32,
}
fn f() i32 {
    var b = Blob{ x = 1, y = 2 };
    return b.x;
}"#);
        // 无连续类 → 恒等门（不含递归 helper/strcmp 链），且无 DeepCopy 调用
        assert!(!ll.contains("call %Value @hc_deep_copy_cont"), "{ll}");
        assert!(!ll.contains("define %Value @hc_deep_copy("), "{ll}");
    }

    #[test]
    fn implicit_env_pi_seeded_in_entry() {
        // P11d：30-interface 的 Circle.area 读 `pi`——原生 LoadGlobal 此前为 Void → 0.0；
        // main/test 入口播种 Float(PI) 常量到 `@.h_globals` 槽位（对齐 IrRuntime::init）。
        let ll = gen(r#"fn area(r: f64) f64 {
    return pi * r * r;
}"#);
        // Float tag=3 + PI 位模式常量；对 @.h_globals 数组 GEP 后 store
        assert!(
            ll.contains("store %Value { i32 3, i128 4614256656552045848 }"),
            "{ll}"
        );
        assert!(
            ll.contains("getelementptr inbounds [10 x %Value], ptr @.h_globals, i64 0, i64 5"),
            "{ll}"
        );
        assert!(ll.contains("call %Value @hc_make_arr(i64 0)"), "{ll}"); // Vec/Deque/Table 空 Arr 播种
    }

    // ---- 组 G 线程：原生子集边界（定案 A） ----

    #[test]
    fn g4b_thread_spawn_aborts_notcallable() {
        // G4b 定案 A：原生保持响亮拒绝线程。spawn 的 callee 以 FnRef 传递 → 原生 ABI
        // 无函数值表示（Phase 8），codegen 发射 @hc_abort_notcallable 运行时中止
        // （error.NotCallable）——不静默误编译，属原生子集边界。
        let ll = gen(r#"
fn add(a: i32, b: i32) i32 { return a + b; }
fn main() {
    var th = spawn(add, 6, 7);
    var r = th.join();
}
"#);
        assert!(ll.contains("define void @hc_abort_notcallable"), "{ll}");
        assert!(ll.contains("call void @hc_abort_notcallable"), "{ll}");
        // 禁止静默：spawn 内建也不得落入 NotBuiltin 之前被跳过——FnRef 中止先于
        // CallBuiltin spawn 执行（运行时首先命中 notcallable 中止块）。
        assert!(ll.contains("c\"error.NotCallable: function refs/closures/threads (spawn) not yet in native mode (Phase 8)\\00\""), "{ll}");
    }
}
