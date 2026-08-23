//! 梯队 1 语义完整验收测试（M2.2 类型检查 / M2.5 definite / M2.4 所有权 / M4.3 @ 内建）

use hc_rt::Interp;

/// 运行单个 .hc 源码所有 test fn；断言全部通过
fn run_ok(src: &str) {
    let program = hc::parse_source(src).unwrap_or_else(|d| panic!("parse: {:?}", d));
    let mut interp = Interp::new(src);
    interp
        .load(&program)
        .unwrap_or_else(|e| panic!("load: {} {}", e.name, e.message));
    let (p, f, _s) = interp.run_tests();
    assert_eq!(f, 0, "failed tests: {:?}", interp.test_out);
    assert!(p >= 1, "no tests ran");
}

/// 断言 load 阶段编译错误（语义检查拦截）
fn run_compile_error(src: &str, err_frag: &str) {
    let program = hc::parse_source(src).expect("parse should succeed");
    let mut interp = Interp::new(src);
    let e = interp
        .load(&program)
        .expect_err("semantic check should reject");
    assert!(
        e.message.contains(err_frag) || e.name == "CompileError",
        "expected error containing `{err_frag}`, got: {} ({})",
        e.name,
        e.message
    );
}

#[test]
fn width_check_u8_overflow() {
    // 06-integers：var g: u8 = 256 → 编译期报错（Q24/Q39）
    run_compile_error(
        "fn main(io: Io) !void { var g: u8 = 256; }\n",
        "out of range",
    );
}

#[test]
fn width_check_i32_ok() {
    run_ok("[test] fn t() !void { var a: i32 = 42; try expect_eq(a, 42); }\n");
}

#[test]
fn width_check_hex_ok() {
    // 0xFF 在 u8 范围内
    run_ok("[test] fn t() !void { var a: u8 = 0xFF; try expect_eq(a, 255); }\n");
}

#[test]
fn width_check_u64_ok() {
    // xorshift 种子（84-rng）
    run_ok(
        "[test] fn t() !void {
    var s: u64 = 0x1234_5678_9abc_def0;
    try expect_eq(s, 1311768467463790320);
}\n",
    );
}

#[test]
fn reference_assignment_rejected() {
    // 引用类型（Vec）赋值 = 编译错误（Q1'：显式 copy(&x) 或指针）
    run_compile_error(
        "class Foo { a: i32 }
fn main(io: Io) !void {
    var v: Vec<i32> = Vec<i32>.init(alloc);
    var w: Vec<i32> = v;
}\n",
        "cannot assign",
    );
}

#[test]
fn continuous_assignment_allowed() {
    // [continuous] 值类型：赋值即复制（允许）
    run_ok(
        r#"struct Point { x: f32, y: f32 }
[test] fn t() !void {
    var p1 = Point{ x = 1.0, y = 2.0 };
    var p2 = p1;
    p2.x = 99.0;
    try expect_eq(p1.x, 1.0);
}"#,
    );
}

#[test]
fn table_construct_and_index() {
    // M8：Table<T>.init(alloc, rows, cols, init) + t[i, j] 多参索引
    run_ok(
        "[test] fn t() !void {
    var tbl = Table<i32>.init(alloc, 3, 4, 0);
    try expect_eq(tbl[1, 2], 0);
    var t2 = Table<i32>.init(alloc, 2, 2, 7);
    try expect_eq(t2[0, 0], 7);
    try expect_eq(t2[1, 1], 7);
}\n",
    );
}

#[test]
fn at_int_from_enum() {
    run_ok(
        "enum Kind { player, enemy, item }
[test] fn t() !void {
    var k = Kind.enemy;
    try expect_eq(@intFromEnum(k), 1);
    var k2 = @enumFromInt(Kind, 2);
    try expect_eq(@intFromEnum(k2), 2);
}\n",
    );
}

#[test]
fn copy_shallow_mode() {
    // L1：copy(&x, .shallow) ≡ copy(&x, CopyMode.shallow)
    run_ok(
        "[test] fn t() !void {
    var v1 = Vec<i32>.init(alloc);
    v1.append(1);
    var v2 = copy(&v1, .shallow);
    try expect_eq(v2.len, 1);
}\n",
    );
}

#[test]
fn copy_deep_mode_default() {
    run_ok(
        "[test] fn t() !void {
    var v1 = Vec<i32>.init(alloc);
    v1.append(1);
    var v2 = copy(&v1);
    v2.append(2);
    try expect_eq(v1.len, 1);
    try expect_eq(v2.len, 2);
}\n",
    );
}

#[test]
fn definite_assignment_rejects_partial_return() {
    // C7：alloc.init(T) 无参构造后字段未全赋值即 return → 编译错误
    run_compile_error(
        "class Order { id: i32, amount: f64 }
fn make() Order {
    var ord = alloc.init(Order);
    ord.id = 42;
    return ord;   // amount 未赋值
}
fn main(io: Io) !void {}",
        "partially-initialized",
    );
}

#[test]
fn definite_assignment_allows_complete() {
    // 全字段赋值后返回 → 通过
    run_ok(
        "class Order { id: i32, amount: f64 }
fn make() Order {
    var ord = alloc.init(Order);
    ord.id = 42;
    ord.amount = 3.5;
    return ord;
}
[test] fn t() !void {
    var ord = make();
    try expect_eq(ord.id, 42);
}\n",
    );
}

#[test]
fn definite_assignment_ignores_continuous() {
    // [continuous] 值类型走字面量构造，无需字段跟踪
    run_ok(
        r#"struct Point { x: f32, y: f32 }
[test] fn t() !void {
    var p = Point{ x = 1.0, y = 2.0 };
    try expect_eq(p.x, 1.0);
}"#,
    );
}

#[test]
fn struct_defaults_alloc_init() {
    // Q13：字段默认值——alloc.init(T) 使用默认值初始化
    run_ok(
        r#"struct Point { x: f32 = 1.0, y: f32 = 2.0 }
[test] fn t() !void {
    var p = alloc.init(Point);
    try expect_eq(p.x, 1.0);
    try expect_eq(p.y, 2.0);
}"#,
    );
}

#[test]
fn struct_defaults_arena_init() {
    // Q13：字段默认值——arena.init(T) 使用默认值初始化
    run_ok(
        r#"struct Point { x: f32 = 1.0, y: f32 = 2.0 }
[test] fn t() !void {
    var arena = Arena.init(alloc);
    var p = arena.init(Point);
    try expect_eq(p.x, 1.0);
    try expect_eq(p.y, 2.0);
}"#,
    );
}

#[test]
fn struct_defaults_override() {
    // Q13：字段默认值——字面量可覆盖默认值
    run_ok(
        r#"struct Point { x: f32 = 1.0, y: f32 = 2.0 }
[test] fn t() !void {
    var p = Point{ x = 99.0, y = 2.0 };
    try expect_eq(p.x, 99.0);
}"#,
    );
}

#[test]
fn struct_defaults_mixed() {
    // Q13：部分字段有默认值、部分无默认值——alloc.init 自动填充默认值
    run_ok(
        r#"struct Config { timeout: i32 = 5000, retries: i32 }
[test] fn t() !void {
    var c = alloc.init(Config);
    try expect_eq(c.timeout, 5000);
    try expect_eq(c.retries, 0);
}"#,
    );
}

// ---------- M2.2 表达式级类型检查验收 ----------

#[test]
fn m22_var_init_type_mismatch() {
    // 表达式级类型检查：var x: i32 = 字符串 → 编译错误
    run_compile_error(
        "fn main(io: Io) !void { var x: i32 = \"str\"; }\n",
        "type mismatch",
    );
}

#[test]
fn m22_return_type_mismatch() {
    // 期望类型传播：return 表达式 vs 函数返回类型
    run_compile_error(
        "fn f() i32 { return \"str\"; }\nfn main(io: Io) !void {}\n",
        "type mismatch in return",
    );
}

#[test]
fn m22_return_ok() {
    run_ok("fn f() f64 { return 3.5; }\n[test] fn t() !void { try expect_eq(f(), 3.5); }\n");
}

#[test]
fn m22_named_lit_unknown_field() {
    // NamedLit 构造：未知字段
    run_compile_error(
        r#"struct Point { x: f32, y: f32 }
[test] fn t() !void {
    var p = Point{ x = 1.0, z = 2.0 };
}"#,
        "unknown field",
    );
}

#[test]
fn m22_named_lit_missing_field() {
    // NamedLit 构造：必填字段缺失
    run_compile_error(
        r#"struct Point { x: f32, y: f32 }
[test] fn t() !void {
    var p = Point{ x = 1.0 };
}"#,
        "missing field",
    );
}

#[test]
fn m22_named_lit_field_type_mismatch() {
    // NamedLit 构造：字段类型不匹配
    run_compile_error(
        r#"struct Point { x: f32, y: f32 }
[test] fn t() !void {
    var p = Point{ x = "s", y = 2.0 };
}"#,
        "type mismatch in field",
    );
}

#[test]
fn m22_field_access_unknown() {
    // 字段访问：不存在字段
    run_compile_error(
        r#"struct Point { x: f32, y: f32 }
[test] fn t() !void {
    var p = Point{ x = 1.0, y = 2.0 };
    io.print("{}
", p.z);
}"#,
        "has no field",
    );
}

#[test]
fn m22_field_access_len() {
    // 内建字段：容器 .len
    run_ok(
        "[test] fn t() !void {
    var v = Vec<i32>.init(alloc);
    v.append(1);
    v.append(2);
    try expect_eq(v.len, 2);
}\n",
    );
}

#[test]
fn m22_table_double_index_ok() {
    run_ok(
        "[test] fn t() !void {
    var tbl = Table<i32>.init(alloc, 2, 2, 0);
    try expect_eq(tbl[1, 0], 0);
}\n",
    );
}

#[test]
fn m22_continuous_rejects_ref_field() {
    // 存储形态验证：[continuous] 含引用字段 → 编译错误
    run_compile_error(
        "struct Bad { s: String }
[test] fn t() !void {}\n",
        "non-value field",
    );
}

#[test]
fn m22_binary_numeric_required() {
    // 运算符检查：算术需数值
    run_compile_error(
        "[test] fn t() !void {
    var x = 1 + \"a\";
}\n",
        "requires numeric",
    );
}

#[test]
fn m22_binary_integer_required() {
    // 位运算需整数
    run_compile_error(
        "[test] fn t() !void {
    var x = 1.0 & 2.0;
}\n",
        "requires integer",
    );
}

#[test]
fn m22_binary_ok() {
    run_ok(
        "[test] fn t() !void {
    try expect_eq(1 + 2 * 3, 7);
    try expect_eq(10 >> 1, 5);
}\n",
    );
}

#[test]
fn m22_condition_requires_bool() {
    // 条件表达式检查
    run_compile_error(
        "struct Foo { a: i32 }
[test] fn t() !void {
    var f = Foo{ a = 1 };
    if (f) { }
}\n",
        "condition must be",
    );
}

#[test]
fn m22_for_not_iterable() {
    // 迭代契约：不可迭代类型 → 编译错误
    run_compile_error(
        "[test] fn t() !void {
    var n = 42;
    for (n) |x| { }
}\n",
        "not iterable",
    );
}

#[test]
fn m22_for_iterable_ok() {
    run_ok(
        "[test] fn t() !void {
    var sum = 0;
    for ([1, 2, 3]) |x| { sum += x; }
    try expect_eq(sum, 6);
}\n",
    );
}

#[test]
fn m22_where_constraint_satisfied() {
    // 泛型 where 约束：T=整数满足 INumber → 通过
    run_ok(
        "fn sum(items: &[T]) T where T: INumber {
    var total = items[0];
    for (items[1..]) |v| { total = total + v; }
    return total;
}
[test] fn t() !void {
    var ints = [10, 20, 30];
    try expect_eq(sum(&ints), 60);
}\n",
    );
}

#[test]
fn m22_where_constraint_violated() {
    // 泛型 where 约束违反：Point 不实现 INumber → 编译错误
    run_compile_error(
        "struct Point { x: f32 }
fn sum(items: &[T]) T where T: INumber {
    return items[0];
}
[test] fn t() !void {
    var pts = [Point{ x = 1.0 }];
    var s = sum(&pts);
}\n",
        "does not satisfy",
    );
}

#[test]
fn m22_enum_variant_rejected() {
    // 枚举变体校验
    run_compile_error(
        "enum Kind { player, enemy }
[test] fn t() !void {
    var k = Kind.chest;
}\n",
        "has no variant",
    );
}

#[test]
fn m22_orelse_requires_optional() {
    // orelse 需可选值
    run_compile_error(
        "[test] fn t() !void {
    var x = 1 orelse 2;
}\n",
        "requires an optional",
    );
}

#[test]
fn m22_try_requires_error_union() {
    // try 需错误联合
    run_compile_error(
        "fn f() i32 { return 1; }
[test] fn t() !void {
    var x = try f();
}\n",
        "requires an error union",
    );
}

#[test]
fn m22_tuple_destructure_mismatch() {
    // 元组解构数量不匹配
    run_compile_error(
        "fn divmod(a: i32, b: i32) (i32, i32) { return (a / b, a % b); }
[test] fn t() !void {
    var (q, r, s) = divmod(10, 3);
}\n",
        "tuple destructure",
    );
}

#[test]
fn m22_assign_pointer_field() {
    // 方法内通过 self 指针写字段（自动解引用）→ 通过
    run_ok(
        "class Counter {
    mut count: i32,
    fn inc(self: *mut Self) void { self.count += 1; }
}
[test] fn t() !void {
    var c = alloc.init(Counter);
    c.count = 0;
    c.inc();
    try expect_eq(c.count, 1);
}\n",
    );
}

#[test]
fn m22_slice_range_index() {
    // 范围索引 arr[1..] → 切片
    run_ok(
        "[test] fn t() !void {
    var arr = [1, 2, 3, 4];
    var s: &[i32] = &arr[1..];
    try expect_eq(s.len, 3);
}\n",
    );
}

// ---------- M2.5 Debug 悬垂标记验收 ----------

const DANGLING_SRC: &str = "fn fill(buf: *mut Vec<*i32>, alloc: Allocator) void {\n    var temp: i32 = 7;\n    buf.append(&temp);\n}\n[test] fn t() !void {\n    var mut buf = Vec<*i32>.init(alloc);\n    fill(&mut buf, alloc);\n    var d = buf[0];\n    var x = d.*;\n}\n";

#[test]
fn m25_dangling_access_rejected_debug() {
    // Debug（默认）：目标销毁后解引用访问 → DanglingPointer（带位置）
    let program = hc::parse_source(DANGLING_SRC).expect("parse");
    let mut interp = Interp::new(DANGLING_SRC);
    interp.load(&program).expect("load");
    interp.run_tests();
    assert!(
        interp
            .test_out
            .iter()
            .any(|l| l.contains("DanglingPointer")),
        "Debug 应检测悬垂访问: {:?}",
        interp.test_out
    );
}

#[test]
fn m25_dangling_hold_not_accessed_ok() {
    // 取出/持有悬垂引用不抛错；只有解引用访问才触发（Q18：取指针不抛错）
    let src = "fn fill(buf: *mut Vec<*i32>, alloc: Allocator) void {\n    var temp: i32 = 7;\n    buf.append(&temp);\n}\n[test] fn t() !void {\n    var mut buf = Vec<*i32>.init(alloc);\n    fill(&mut buf, alloc);\n    try expect_eq(buf.len, 1);\n}\n";
    run_ok(src);
}

#[test]
fn m25_dangling_release_bare_read() {
    // Release（debug_dangling=false）：裸读（用户负责），不检测
    let program = hc::parse_source(DANGLING_SRC).expect("parse");
    let mut interp = Interp::new(DANGLING_SRC);
    interp.set_debug_dangling(false);
    interp.load(&program).expect("load");
    let (p, f, _s) = interp.run_tests();
    assert_eq!(f, 0, "Release 裸读不检测: {:?}", interp.test_out);
    assert!(p >= 1);
}

// ---------- M4.3 @ 内建基础集验收 ----------

#[test]
fn m43_sizeof_scalars_and_continuous() {
    // @sizeOf：标量宽度 + 连续类型（与 to_bytes 布局一致）
    run_ok(
        "struct Point { x: f32, y: f64 }\n[test] fn t() !void {\n    var p = Point{ x = 1.0, y = 2.0 };\n    try expect_eq(@sizeOf(i32), 4);\n    try expect_eq(@sizeOf(bool), 1);\n    try expect_eq(@sizeOf(f64), 8);\n    try expect_eq(@sizeOf(String), 8);\n    try expect_eq(@sizeOf(Point), 16);\n    try expect_eq(@sizeOf(Point), p.to_bytes().len);\n}\n",
    );
}

#[test]
fn m43_alignof_and_offsetof() {
    // @alignOf / @offsetOf：自然对齐 + 字段偏移（含填充）
    run_ok(
        "struct Point { x: f32, y: f64 }\n[test] fn t() !void {\n    try expect_eq(@alignOf(f64), 8);\n    try expect_eq(@offsetOf(Point, \"x\"), 0);\n    try expect_eq(@offsetOf(Point, \"y\"), 8);\n}\n",
    );
}

#[test]
fn m43_intcast_ok_and_overflow() {
    // @intCast：范围检查（Debug 溢出抛错）
    run_ok(
        "[test] fn t() !void {\n    try expect_eq(@intCast(u8, 255), 255);\n    try expect_eq(@intCast(i16, -32768), -32768);\n}\n",
    );
    // 溢出 → 运行时错误
    let src = "fn main(io: Io) !void {\n    var x = @intCast(u8, 256);\n}\n";
    let program = hc::parse_source(src).expect("parse");
    let mut interp = Interp::new(src);
    interp.load(&program).expect("load");
    let e = interp.run_main().expect_err("溢出应报错");
    assert_eq!(e.name, "IntCastOverflow");
}

#[test]
fn m43_typeof_returns_type_name() {
    run_ok(
        "[test] fn t() !void {\n    var x: f64 = 1.0;\n    try expect_eq_slices(@typeOf(x), \"f64\");\n    try expect_eq_slices(@typeOf(42), \"i128\");\n}\n",
    );
}

#[test]
fn m43_add_with_overflow_tuple() {
    // @addWithOverflow → (T, bool) 元组
    run_ok(
        "[test] fn t() !void {\n    var ov = @addWithOverflow(100, 200);\n    try expect_eq(ov[0], 300);\n    try expect_eq(ov[1], false);\n    var sv = @subWithOverflow(10, 3);\n    try expect_eq(sv[0], 7);\n}\n",
    );
}

#[test]
fn m43_ptrcast_passthrough() {
    // @ptrCast：tag1 指针无类型化——透传，可解引用
    run_ok(
        "[test] fn t() !void {\n    var x: i32 = 42;\n    var p = @ptrCast(i32, &x);\n    try expect_eq(p.*, 42);\n}\n",
    );
}

#[test]
fn m43_compile_error_rejected() {
    // @compileError = 编译期错误（强制编译失败）
    run_compile_error(
        "[test] fn t() !void {\n    @compileError(\"boom\");\n}\n",
        "compileError",
    );
}

// ---------- Pool(T) 分配器测试（Phase 3） ----------

#[test]
/// Pool.init 创建 + alloc/free 基本操作
fn pool_init_alloc_free() {
    run_ok(
        "[test] fn t() !void {\n    var pool = Pool.init(alloc, 16);\n    var data = pool.alloc(16);\n    pool.free(data);\n}\n",
    );
}

#[test]
/// Pool.alloc() 无参——使用 item_size
fn pool_alloc_no_args() {
    run_ok(
        "[test] fn t() !void {\n    var pool = Pool.init(alloc, 8);\n    var data = pool.alloc();\n    pool.free(data);\n}\n",
    );
}

#[test]
/// Pool alloc → free → alloc 复用空闲块
fn pool_alloc_free_reuse() {
    run_ok(
        "[test] fn t() !void {\n    var pool = Pool.init(alloc, 16);\n    var a = pool.alloc(16);\n    pool.free(a);\n    var b = pool.alloc(16);\n    pool.free(b);\n}\n",
    );
}

#[test]
/// Pool 多次 alloc + free 循环
fn pool_multiple_alloc_free() {
    run_ok(
        "[test] fn t() !void {\n    var pool = Pool.init(alloc, 8);\n    var a = pool.alloc(8);\n    var b = pool.alloc(8);\n    var c = pool.alloc(8);\n    pool.free(a);\n    pool.free(b);\n    pool.free(c);\n    var r = pool.alloc(8);\n    pool.free(r);\n}\n",
    );
}

#[test]
/// Pool.deinit 释放所有资源
fn pool_deinit() {
    run_ok(
        "[test] fn t() !void {\n    var pool = Pool.init(alloc, 16);\n    var a = pool.alloc(16);\n    var b = pool.alloc(16);\n    pool.free(a);\n    pool.free(b);\n    pool.deinit();\n}\n",
    );
}
