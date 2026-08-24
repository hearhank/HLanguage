import H.std.{io};

// stdlib-types.hc — 标准库类型单元测试全集
//
// 覆盖所有标准库内建类型：整数/浮点/bool/数组/切片/指针/可选值/
// struct/enum/class/String/Vec/Map/元组/函数/错误联合/数值接口/迭代器
//
// 运行：hc test examples/study/stdlib-types.hc

// ============================================================
// 顶层类型定义
// ============================================================

struct Point {
    x: f64,
    y: f64,
}

struct Counter {
    mut val: i32,
}

struct Rect {
    pos: Point,
    w: f64,
    h: f64,
}

struct Item {
    id: i32,
}

struct Point2 {
    x: f32,
    y: f32,
}

enum Color {
    red,
    green,
    blue,
}

enum Value {
    int: i32,
    float: f64,
    none,
}

enum Result_ {
    ok: i32,
    err: &[u8],
}

class Person {
    name: String,
    age: i32,

    fn greet(self: *Self) String {
        return String.concat(
            String.from("hello, ", alloc),
            self.name,
        );
    }

    fn is_adult(self: *Self) bool {
        return self.age >= 18;
    }
}

class CounterClass {
    mut val: i32,

    fn inc(self: *mut Self) void {
        self.val += 1;
    }

    fn get(self: *Self) i32 {
        return self.val;
    }
}

// ============================================================
// 顶层辅助函数
// ============================================================

fn safe_div(x: i32, y: i32) bool {
    return (y != 0) and (x / y > 0);
}

fn early_return(x: i32) bool {
    return (x >= 0) or (1 / x > 0);
}

fn dist(a: *Point, b: *Point) f64 {
    var dx = b.x - a.x;
    var dy = b.y - a.y;
    return sqrt(dx * dx + dy * dy);
}

fn sum_arr(s: &[i32]) i32 {
    var total = 0;
    for (s) |v| {
        total += v;
    }
    return total;
}

fn sum_slice(s: &[i32]) i32 {
    var total = 0;
    for (s) |item| {
        total += item;
    }
    return total;
}

fn divmod(a: i32, b: i32) (i32, i32) {
    return (a / b, a % b);
}

fn triple(a: i32, b: i32, c: i32) (i32, i32, i32) {
    return (a, b, c);
}

fn lookup() (&[u8], i32, bool) {
    return ("alice", 30, true);
}

fn apply(f: Fn1<i32> i32, x: i32) i32 {
    return f(x);
}

fn compose(f: Fn1<i32> i32, g: Fn1<i32> i32, x: i32) i32 {
    return f(g(x));
}

const ParseError = error{Empty, Invalid, Overflow};

fn parse_ok(s: &[u8]) ParseError!i32 {
    if (s.len == 0) {
        return error.Empty;
    }
    return 42;
}

fn parse_fail() ParseError!i32 {
    return error.Invalid;
}

fn classify(err: ParseError) &[u8] {
    return switch (err) {
        error.Empty => "empty",
        error.Invalid => "invalid",
        error.Overflow => "overflow",
    };
}

fn sum<T>(items: &[T]) T where T: INumber {
    var total = items[0];
    for (items[1..]) |v| {
        total = total.add(v);
    }
    return total;
}

fn try_parse(s: &[u8]) ?Result_ {
    if (s.len == 0) {
        return null;
    }
    return Result_{ok = 42};
}

fn lookup_kv(key: &[u8], data: &[(&[u8], i32)]) !?i32 {
    for (data) |pair| {
        var (k, v) = pair;
        if (k == key) {
            return v;
        }
    }
    return null;
}

fn square(x: i32) i32 {
    return x * x;
}

fn cube(x: i32) i32 {
    return x * x * x;
}

// ============================================================
// 1. 整数类型（i8–i128 / u8–u128 / isize / usize）
// ============================================================

[test] fn integer_type_bounds_i8() !void {
    var min: i8 = -128;
    var max: i8 = 127;
    try expect_eq(min, -128);
    try expect_eq(max, 127);
    try expect_eq(min + max, -1);
    try expect_eq(max - min, 255);
}

[test] fn integer_type_bounds_u8() !void {
    var min: u8 = 0;
    var max: u8 = 255;
    try expect_eq(min, 0);
    try expect_eq(max, 255);
    try expect_eq(max - 1, 254);
    try expect_eq(min + 1, 1);
}

[test] fn integer_type_i16() !void {
    var min: i16 = -32768;
    var max: i16 = 32767;
    try expect_eq(min, -32768);
    try expect_eq(max, 32767);
    try expect_eq(min / -1, 32768);
}

[test] fn integer_type_u16() !void {
    var min: u16 = 0;
    var max: u16 = 65535;
    try expect_eq(max, 65535);
    try expect_eq(max / 2, 32767);
}

[test] fn integer_type_i32() !void {
    var min: i32 = -2147483648;
    var max: i32 = 2147483647;
    try expect_eq(min, -2147483648);
    try expect_eq(max, 2147483647);
    try expect_eq(max - 1, 2147483646);
}

[test] fn integer_type_u32() !void {
    var min: u32 = 0;
    var max: u32 = 4294967295;
    try expect_eq(max, 4294967295);
    try expect_eq(max / 2, 2147483647);
}

[test] fn integer_type_i64() !void {
    var max: i64 = 9223372036854775807;
    var min: i64 = -9223372036854775808;
    try expect_eq(max, 9223372036854775807);
    try expect_eq(min, -9223372036854775808);
    try expect_eq(max / 2, 4611686018427387903);
}

[test] fn integer_type_u64() !void {
    var max: u64 = 18446744073709551615;
    try expect_eq(max, 18446744073709551615);
    try expect_eq(max - 1, 18446744073709551614);
}

[test] fn integer_type_isize_usize() !void {
    var pos: isize = 100;
    var neg: isize = -100;
    var size: usize = 100;
    try expect_eq(pos, 100);
    try expect_eq(neg, -100);
    try expect_eq(size, 100);
    try expect_eq(pos + neg, 0);
}

[test] fn integer_suffix_literals() !void {
    try expect_eq(42i8, 42);
    try expect_eq(255u8, 255);
    try expect_eq(-32768i16, -32768);
    try expect_eq(65535u16, 65535);
    try expect_eq(1000000i32, 1000000);
    try expect_eq(4294967295u32, 4294967295);
    try expect_eq(-1isize, -1);
    try expect_eq(1usize, 1);
}

[test] fn integer_arithmetic_chain() !void {
    var a: i32 = 100;
    var b: i32 = 30;
    var c: i32 = 7;
    try expect_eq(a + b + c, 137);
    try expect_eq(a - b - c, 63);
    try expect_eq(a * b * c, 21000);
    try expect_eq(a / b / c, 0);
    try expect_eq(a % b, 10);
    try expect_eq(a % c, 2);
}

[test] fn integer_negation_and_abs() !void {
    try expect_eq(-42i32, -42);
    try expect_eq((-42).abs(), 42);
    try expect_eq(42i32.neg(), -42);
    try expect_eq(42.neg().abs(), 42);
}

[test] fn integer_comparison_chain() !void {
    try expect(1 < 2 and 2 < 3 and 3 < 4);
    try expect(4 > 3 and 3 > 2 and 2 > 1);
    try expect(1 <= 1 and 2 <= 3);
    try expect(3 >= 3 and 2 >= 1);
    try expect_eq(1 == 1, true);
    try expect_eq(1 != 2, true);
}

[test] fn integer_mixed_operations() !void {
    var a: i32 = 10;
    var b: i32 = 3;
    var sum = a + b;
    var diff = a - b;
    var prod = a * b;
    var quot = a / b;
    var rem = a % b;
    try expect_eq(sum, 13);
    try expect_eq(diff, 7);
    try expect_eq(prod, 30);
    try expect_eq(quot, 3);
    try expect_eq(rem, 1);
}

// ============================================================
// 2. 浮点类型（f32 / f64 / f128）
// ============================================================

[test] fn float_type_f32() !void {
    var pi: f32 = 3.14159;
    var half: f32 = 0.5;
    try expect(pi > 3.14 and pi < 3.15);
    try expect(half == 0.5);
    var prod = pi * half;
    try expect(prod > 1.5707 and prod < 1.5709);
}

[test] fn float_type_f64() !void {
    var pi: f64 = 3.14159265358979;
    var area = pi * 2.0 * 2.0;
    try expect(area > 12.56 and area < 12.57);
    try expect_eq(pi + pi, 2.0 * pi);
}

[test] fn float_suffix_literals() !void {
    try expect(3.14 > 3.13 and 3.14 < 3.15);
    try expect(3.1415926535 > 3.14159 and 3.1415926535 < 3.14160);
}

[test] fn float_arithmetic_chain() !void {
    var a: f64 = 10.5;
    var b: f64 = 3.0;
    try expect_eq(a + b, 13.5);
    try expect_eq(a - b, 7.5);
    try expect_eq(a * b, 31.5);
    try expect_eq(a / b, 3.5);
}

[test] fn float_special_values() !void {
    var nan = math.nan(f64);
    try expect(nan != nan);  // NaN 不等于自身
    var inf = math.inf(f32);
    try expect(inf > 1.0e30);
    var inf_neg = math.inf_neg(f64);
    try expect(inf_neg < -1.0e30);
}

[test] fn float_scientific_notation() !void {
    var big = 1.5e9;
    var small = 2.5e-4;
    try expect(big > 1.49e9 and big < 1.51e9);
    try expect(small > 2.4e-4 and small < 2.6e-4);
}

[test] fn float_comparison_with_tolerance() !void {
    var a: f64 = 1.0 / 3.0;
    var b: f64 = 0.3333333333333333;
    var diff = a - b;
    try expect(diff.abs() < 1e-15);
}

// ============================================================
// 3. 布尔类型（bool）
// ============================================================

[test] fn bool_basic_operations() !void {
    var t: bool = true;
    var f: bool = false;
    try expect(t);
    try expect(!f);
    try expect(t and t);
    try expect(!(t and f));
    try expect(t or f);
    try expect(!(f or f));
}

[test] fn bool_short_circuit_and() !void {
    try expect(safe_div(10, 2));
    try expect(!safe_div(10, 0));  // 短路保护
    try expect(!safe_div(-5, 2));
}

[test] fn bool_short_circuit_or() !void {
    try expect(early_return(10));   // 左边 true，短路
    try expect(!early_return(-5));  // 左边 false，不求值
    try expect(early_return(0));    // 0 ≥ 0 → true，短路
}

[test] fn bool_negation_and_comparison() !void {
    try expect(!false);
    try expect(!true == false);
    try expect(!false == true);
    try expect(!!true);
    try expect((1 > 0) == true);
    try expect((1 < 0) == false);
    try expect((1 == 1) and (2 != 3));
}

// ============================================================
// 4. 数组类型（[N]T）
// ============================================================

[test] fn array_one_dimensional() !void {
    var flat = [1, 2, 3, 4, 5];
    try expect_eq(flat.len, 5);
    try expect_eq(flat[0], 1);
    try expect_eq(flat[4], 5);
}

[test] fn array_multi_dimensional() !void {
    var grid = [[1, 2], [3, 4], [5, 6]];
    try expect_eq(grid.len, 3);
    try expect_eq(grid[0][0], 1);
    try expect_eq(grid[1][1], 4);
    try expect_eq(grid[2][0], 5);
}

[test] fn array_explicit_type() !void {
    var arr: [4]i32 = [10, 20, 30, 40];
    try expect_eq(arr[0], 10);
    try expect_eq(arr[3], 40);
    try expect_eq(arr.len, 4);
}

[test] fn array_2d_explicit_type() !void {
    var matrix: [2][3]i32 = [[1, 2, 3], [4, 5, 6]];
    try expect_eq(matrix[0][2], 3);
    try expect_eq(matrix[1][1], 5);
}

[test] fn array_iteration() !void {
    var arr = [1, 2, 3, 4, 5];
    var sum = 0;
    for (arr) |v| {
        sum += v;
    }
    try expect_eq(sum, 15);
}

[test] fn array_mutable_iteration() !void {
    var mut arr = [1, 2, 3];
    for (arr) |mut item| {
        item *= 10;
    }
    try expect_eq(arr[0], 10);
    try expect_eq(arr[1], 20);
    try expect_eq(arr[2], 30);
}

[test] fn array_of_floats() !void {
    var arr = [1.5, 2.5, 3.5];
    try expect_eq(arr.len, 3);
    try expect(arr[0] > 1.4 and arr[0] < 1.6);
    try expect(arr[2] > 3.4 and arr[2] < 3.6);
}

[test] fn array_sum_helper() !void {
    var arr = [5, 10, 15];
    try expect_eq(sum_arr(&arr), 30);
}

// ============================================================
// 5. 切片类型（&[T] / &mut [T]）
// ============================================================

[test] fn slice_readonly_view() !void {
    var arr = [10, 20, 30, 40, 50];
    var s: &[i32] = &arr[1..4];
    try expect_eq(s.len, 3);
    try expect_eq(s[0], 20);
    try expect_eq(s[2], 40);
}

[test] fn slice_full_range() !void {
    var arr = [1, 2, 3];
    var s: &[i32] = &arr[0..3];
    try expect_eq(s.len, 3);
    try expect_eq(s[0], 1);
    try expect_eq(s[2], 3);
}

[test] fn slice_writable() !void {
    var mut arr = [1, 2, 3, 4, 5];
    var s: &mut [i32] = &mut arr[0..3];
    for (s) |mut item| {
        item = 0;
    }
    try expect_eq(arr[0], 0);
    try expect_eq(arr[1], 0);
    try expect_eq(arr[2], 0);
    try expect_eq(arr[3], 4);
    try expect_eq(arr[4], 5);
}

[test] fn slice_sum_test() !void {
    var arr = [2, 4, 6, 8];
    try expect_eq(sum_slice(&arr[0..2]), 6);
    try expect_eq(sum_slice(&arr[2..4]), 14);
    try expect_eq(sum_slice(&arr), 20);
}

// ============================================================
// 6. 指针类型（*T / *mut T）
// ============================================================

[test] fn pointer_readonly() !void {
    var x: i32 = 42;
    var p: *i32 = &x;
    try expect_eq(p.*, 42);
}

[test] fn pointer_writable() !void {
    var mut x: i32 = 42;
    var w: *mut i32 = &mut x;
    w.* = 100;
    try expect_eq(x, 100);
    try expect_eq(w.*, 100);
}

[test] fn pointer_auto_deref_index() !void {
    var arr = [1, 2, 3];
    var sp: *[3]i32 = &arr;
    try expect_eq(sp[0], 1);
    try expect_eq(sp[1], 2);
    try expect_eq(sp[2], 3);
}

[test] fn pointer_chain() !void {
    var mut x: i32 = 42;
    var p: *mut i32 = &mut x;
    p.* = 100;
    try expect_eq(x, 100);
}

[test] fn pointer_read_downgrade() !void {
    var mut x: i32 = 42;
    var p: *i32 = &x;
    var w: *mut i32 = &mut x;
    w.* = 50;
    try expect_eq(p.*, 50);
}

// ============================================================
// 7. 可选值类型（?T）
// ============================================================

[test] fn optional_orelse_default() !void {
    var maybe: ?i32 = null;
    var val = maybe orelse -1;
    try expect_eq(val, -1);
}

[test] fn optional_orelse_with_value() !void {
    var maybe: ?i32 = 42;
    var val = maybe orelse -1;
    try expect_eq(val, 42);
}

[test] fn optional_if_capture_some() !void {
    var maybe: ?i32 = 42;
    var captured = if (maybe) |v| v else 0;
    try expect_eq(captured, 42);
}

[test] fn optional_if_capture_null() !void {
    var maybe: ?i32 = null;
    var captured = if (maybe) |v| v else 0;
    try expect_eq(captured, 0);
}

[test] fn optional_unwrap_assert() !void {
    var maybe: ?i32 = 42;
    try expect_eq(maybe.?, 42);
}

[test] fn optional_chain_defaults() !void {
    var a: ?i32 = null;
    var b: ?i32 = 10;
    var result = a orelse b orelse 0;
    try expect_eq(result, 10);
}

// ============================================================
// 8. 结构体类型（struct）
// ============================================================

[test] fn struct_construction_and_access() !void {
    var p = Point{x = 3.0, y = 4.0};
    try expect_eq(p.x, 3.0);
    try expect_eq(p.y, 4.0);
}

[test] fn struct_method_dual_call() !void {
    var p = Point{x = 1.0, y = 2.0};
    var q = Point{x = 4.0, y = 6.0};
    var d = dist(p, q);
    try expect(d > 4.99 and d < 5.01);
}

[test] fn struct_value_copy() !void {
    var p = Point{x = 1.0, y = 2.0};
    var p2 = copy(&p);
    p2.x = 99.0;
    try expect_eq(p.x, 1.0);
    try expect_eq(p2.x, 99.0);
}

[test] fn struct_mutable_field() !void {
    var mut c = Counter{val = 0};
    c.val = 10;
    try expect_eq(c.val, 10);
    c.val += 5;
    try expect_eq(c.val, 15);
}

[test] fn struct_nested() !void {
    var r = Rect{pos = Point{x = 1.0, y = 2.0}, w = 5.0, h = 3.0};
    try expect_eq(r.pos.x, 1.0);
    try expect_eq(r.pos.y, 2.0);
    try expect_eq(r.w, 5.0);
    try expect_eq(r.h, 3.0);
}

// ============================================================
// 9. 枚举类型（enum）
// ============================================================

[test] fn enum_payloadless_constant() !void {
    var c = Color.red;
    var label = switch (c) {
        Color.red => "red",
        Color.green => "green",
        Color.blue => "blue",
    };
    try expect_eq_slices(label, "red");
}

[test] fn enum_switch_exhaustive() !void {
    var c = Color.green;
    var code = switch (c) {
        Color.red => 0,
        Color.green => 1,
        Color.blue => 2,
    };
    try expect_eq(code, 1);
}

[test] fn enum_with_payload() !void {
    var v = Value{int = 42};
    var label = switch (v) {
        Value.int => |i| i,
        Value.float => |f| 0,
        Value.none => 0,
    };
    try expect_eq(label, 42);
}

[test] fn enum_float_payload() !void {
    var v = Value{float = 3.14};
    var label = switch (v) {
        Value.int => |i| 0.0,
        Value.float => |f| f,
        Value.none => 0.0,
    };
    try expect(label > 3.13 and label < 3.15);
}

[test] fn enum_payloadless_constant_form() !void {
    var n = Value.none;
    var is_none = switch (n) {
        Value.none => true,
        else => false,
    };
    try expect(is_none);
}

// ============================================================
// 10. 类类型（class）
// ============================================================

[test] fn class_construction_and_method() !void {
    var p = alloc.init(Person{
        name = String.from("alice", alloc),
        age = 30,
    });
    try expect_eq_slices(p.name.as_slice(), "alice");
    try expect_eq(p.age, 30);
    try expect(p.is_adult());
}

[test] fn class_method_greet() !void {
    var p = alloc.init(Person{
        name = String.from("bob", alloc),
        age = 25,
    });
    var greeting = p.greet();
    try expect_eq_slices(greeting.as_slice(), "hello, bob");
}

[test] fn class_boxing() !void {
    var p = Point2{x = 1.0, y = 2.0};
    var hp: owned *mut Point2 = box(p, alloc);
    hp.x = 100.0;
    try expect_eq(hp.x, 100.0);
}

[test] fn class_mutable_state() !void {
    var c = alloc.init(CounterClass{val = 0});
    try expect_eq(c.get(), 0);
    c.inc();
    c.inc();
    c.inc();
    try expect_eq(c.get(), 3);
}

// ============================================================
// 11. String 类型
// ============================================================

[test] fn string_from_literal() !void {
    var s = String.from("hello", alloc);
    try expect_eq_slices(s.as_slice(), "hello");
}

[test] fn string_concat_method() !void {
    var s = String.from("hello, ", alloc).concat(String.from("world", alloc));
    try expect_eq_slices(s.as_slice(), "hello, world");
}

[test] fn string_concat_function() !void {
    var a = String.from("abc", alloc);
    var b = String.from("def", alloc);
    var c = String.concat(a, b);
    try expect_eq_slices(c.as_slice(), "abcdef");
}

[test] fn string_content_equals() !void {
    var a = String.from("abc", alloc);
    var b = String.from("abc", alloc);
    try expect_eq(a == b, true);
    var c = String.from("xyz", alloc);
    try expect_eq(a == c, false);
}

[test] fn string_split() !void {
    var csv = String.from("a,b,c,d", alloc);
    var parts = csv.split(',');
    try expect_eq(parts.len, 4);
    try expect_eq_slices(parts[0].as_slice(), "a");
    try expect_eq_slices(parts[3].as_slice(), "d");
}

[test] fn string_join() !void {
    var parts = Vec<String>.init(alloc);
    parts.append(String.from("x", alloc));
    parts.append(String.from("y", alloc));
    parts.append(String.from("z", alloc));
    var joined = String.join(&parts, " - ");
    try expect_eq_slices(joined.as_slice(), "x - y - z");
}

[test] fn string_find() !void {
    var text = String.from("hello world", alloc);
    var pos = text.find("world");
    try expect_eq(pos.?, 6);
    var not_found = text.find("xyz");
    try expect_eq(not_found orelse -1, -1);
}

[test] fn string_substring() !void {
    var text = String.from("hello world", alloc);
    var sub = text.substring(0, 5);
    try expect_eq_slices(sub.as_slice(), "hello");
    var sub2 = text.substring(6, 11);
    try expect_eq_slices(sub2.as_slice(), "world");
}

[test] fn string_replace() !void {
    var text = String.from("hello world", alloc);
    var replaced = text.replace("world", "h");
    try expect_eq_slices(replaced.as_slice(), "hello h");
}

[test] fn string_copy_owns() !void {
    var s1 = String.from("original", alloc);
    var s2 = copy(&s1);
    try expect_eq_slices(s2.as_slice(), "original");
    try expect_eq_slices(s1.as_slice(), "original");
}

[test] fn string_compare() !void {
    var a = String.from("abc", alloc);
    var b = String.from("abd", alloc);
    var c = String.from("abc", alloc);
    try expect_eq(String.compare(a, a), 0);
    try expect_eq(String.compare(a, c), 0);
    try expect(String.compare(a, b) < 0);
    try expect(String.compare(b, a) > 0);
}

[test] fn string_empty() !void {
    var s = String.from("", alloc);
    try expect_eq(s.as_slice().len, 0);
}

// ============================================================
// 12. Vec<T> 类型
// ============================================================

[test] fn vec_init_and_append() !void {
    var v = Vec<i32>.init(alloc);
    try expect_eq(v.len, 0);
    v.append(1);
    v.append(2);
    v.append(3);
    try expect_eq(v.len, 3);
    try expect_eq(v[0], 1);
    try expect_eq(v[2], 3);
}

[test] fn vec_iteration() !void {
    var v = Vec<i32>.init(alloc);
    v.append(10);
    v.append(20);
    v.append(30);
    var sum = 0;
    for (v) |item| {
        sum += item;
    }
    try expect_eq(sum, 60);
}

[test] fn vec_mutable_iteration() !void {
    var mut v = Vec<i32>.init(alloc);
    v.append(1);
    v.append(2);
    v.append(3);
    for (v) |mut item| {
        item *= 10;
    }
    try expect_eq(v[0], 10);
    try expect_eq(v[1], 20);
    try expect_eq(v[2], 30);
}

[test] fn vec_multiple_appends() !void {
    var v = Vec<i32>.init(alloc);
    var i: i32 = 0;
    while (i < 100) {
        v.append(i);
        i += 1;
    }
    try expect_eq(v.len, 100);
    try expect_eq(v[0], 0);
    try expect_eq(v[99], 99);
}

[test] fn vec_of_strings() !void {
    var v = Vec<String>.init(alloc);
    v.append(String.from("a", alloc));
    v.append(String.from("b", alloc));
    v.append(String.from("c", alloc));
    try expect_eq(v.len, 3);
    try expect_eq_slices(v[0].as_slice(), "a");
    try expect_eq_slices(v[2].as_slice(), "c");
}

[test] fn vec_to_bytes_roundtrip() !void {
    var v = Vec<i32>.init(alloc);
    v.append(1);
    v.append(2);
    v.append(3);
    var bytes = v.to_bytes();
    try expect_eq(bytes.len, 8 + 12);
    var v2 = try Vec<i32>.from_bytes(bytes);
    try expect_eq(v2.len, 3);
    try expect_eq(v2[0], 1);
    try expect_eq(v2[2], 3);
}

[test] fn vec_sort() !void {
    var mut v = Vec<i32>.init(alloc);
    v.append(5);
    v.append(2);
    v.append(8);
    v.append(1);
    sort(&mut v);
    try expect_eq(v[0], 1);
    try expect_eq(v[1], 2);
    try expect_eq(v[2], 5);
    try expect_eq(v[3], 8);
}

[test] fn vec_explicit_copy() !void {
    var v1 = Vec<i32>.init(alloc);
    v1.append(1);
    v1.append(2);
    var v2 = copy(&v1);
    v2.append(3);
    try expect_eq(v1.len, 2);
    try expect_eq(v2.len, 3);
}

// ============================================================
// 13. Map<K, V> 类型
// ============================================================

[test] fn map_put_and_get() !void {
    var m = Map<&[u8], i32>.init(alloc);
    m.put("apple", 5);
    m.put("banana", 7);
    try expect_eq(m.get("apple").?, 5);
    try expect_eq(m.get("banana").?, 7);
}

[test] fn map_contains() !void {
    var m = Map<&[u8], i32>.init(alloc);
    m.put("apple", 5);
    try expect(m.contains("apple"));
    try expect(!m.contains("pear"));
}

[test] fn map_remove() !void {
    var m = Map<&[u8], i32>.init(alloc);
    m.put("apple", 5);
    m.put("banana", 7);
    try expect_eq(m.len, 2);
    m.remove("apple");
    try expect_eq(m.len, 1);
    try expect(!m.contains("apple"));
    try expect(m.contains("banana"));
}

[test] fn map_iterate() !void {
    var m = Map<&[u8], i32>.init(alloc);
    m.put("a", 1);
    m.put("b", 2);
    m.put("c", 3);
    var total = 0;
    for (m) |kv| {
        total += kv.value;
    }
    try expect_eq(total, 6);
}

[test] fn map_update_value() !void {
    var m = Map<&[u8], i32>.init(alloc);
    m.put("key", 10);
    try expect_eq(m.get("key").?, 10);
    m.put("key", 20);
    try expect_eq(m.get("key").?, 20);
}

[test] fn map_multiple_entries() !void {
    var m = Map<&[u8], i32>.init(alloc);
    m.put("a", 1);
    m.put("b", 2);
    m.put("c", 3);
    m.put("d", 4);
    m.put("e", 5);
    try expect_eq(m.len, 5);
}

// ============================================================
// 14. 元组类型（tuple）
// ============================================================

[test] fn tuple_multi_return() !void {
    var (q, r) = divmod(17, 5);
    try expect_eq(q, 3);
    try expect_eq(r, 2);
}

[test] fn tuple_three_values() !void {
    var (x, y, z) = triple(1, 2, 3);
    try expect_eq(x, 1);
    try expect_eq(y, 2);
    try expect_eq(z, 3);
}

[test] fn tuple_discard_with_underscore() !void {
    var (q, _) = divmod(17, 5);
    try expect_eq(q, 3);
}

[test] fn tuple_mixed_types() !void {
    var (name, age, active) = lookup();
    try expect_eq_slices(name, "alice");
    try expect_eq(age, 30);
    try expect(active);
}

// ============================================================
// 15. 函数类型（Fn1<T> R）
// ============================================================

[test] fn function_as_value() !void {
    try expect_eq(apply(square, 5), 25);
    try expect_eq(apply(cube, 3), 27);
}

[test] fn function_variable() !void {
    var f: Fn1<i32> i32 = square;
    try expect_eq(f(4), 16);
    f = cube;
    try expect_eq(f(2), 8);
}

[test] fn function_composition() !void {
    try expect_eq(compose(square, cube, 2), 64);
    try expect_eq(compose(cube, square, 2), 64);
}

[test] fn closure_as_comparator() !void {
    var mut arr = [5, 2, 8, 1, 9];
    sort(&mut arr, |a, b| a - b);
    try expect_eq(arr[0], 1);
    try expect_eq(arr[4], 9);
}

// ============================================================
// 16. 错误联合类型（Error!T）
// ============================================================

[test] fn error_union_try_propagation() !void {
    var val = try parse_ok("ok");
    try expect_eq(val, 42);
}

[test] fn error_union_catch_default() !void {
    var val = parse_ok("") catch 0;
    try expect_eq(val, 0);
}

[test] fn error_union_catch_with_err() !void {
    var val = parse_ok("") catch |err| 0;
    try expect_eq(val, 0);
}

[test] fn expect_error_assertion() !void {
    try expect_error(error.Invalid, parse_fail());
}

[test] fn error_set_switch() !void {
    try expect_eq_slices(classify(error.Empty), "empty");
    try expect_eq_slices(classify(error.Invalid), "invalid");
}

// ============================================================
// 17. 数值接口（INumber / ICompare / IInt / IUint / IFloat）
// ============================================================

[test] fn scalar_interface_methods_i32() !void {
    var a: i32 = 7;
    var b: i32 = 5;
    try expect_eq(a.add(b), 12);
    try expect_eq(a.sub(b), 2);
    try expect_eq(a.mul(b), 35);
    try expect_eq(a.div(b), 1);
    try expect_eq(a.neg(), -7);
    try expect_eq(a.mod(b), 2);
    try expect_eq((-7).abs(), 7);
}

[test] fn scalar_interface_compare() !void {
    var a: i32 = 7;
    var b: i32 = 5;
    try expect_eq(a.eq(b), false);
    try expect_eq(a.lt(b), false);
    try expect_eq(a == b, false);
    try expect_eq(a < b, false);
    try expect_eq(a > b, true);
}

[test] fn scalar_interface_methods_f64() !void {
    var a: f64 = 3.5;
    var b: f64 = 2.0;
    try expect_eq(a.add(b), 5.5);
    try expect_eq(a.sub(b), 1.5);
    try expect_eq(a.mul(b), 7.0);
    try expect_eq(a.div(b), 1.75);
    try expect_eq(a.neg(), -3.5);
}

[test] fn scalar_interface_methods_u8() !void {
    var a: u8 = 10;
    var b: u8 = 3;
    try expect_eq(a.add(b), 13);
    try expect_eq(a.sub(b), 7);
    try expect_eq(a.mul(b), 30);
    try expect_eq(a.div(b), 3);
    try expect_eq(a.mod(b), 1);
}

[test] fn generic_sum_over_numbers() !void {
    var ints = [10, 20, 30];
    try expect_eq(sum(&ints), 60);
    var floats = [1.5, 2.5, 3.0];
    try expect_eq(sum(&floats), 7.0);
}

[test] fn scalar_interface_methods_u64() !void {
    var a: u64 = 100;
    var b: u64 = 25;
    try expect_eq(a.add(b), 125);
    try expect_eq(a.sub(b), 75);
    try expect_eq(a.mul(b), 2500);
    try expect_eq(a.div(b), 4);
    try expect_eq(a.mod(b), 0);
}

// ============================================================
// 18. 迭代器接口（IIterable<T>）
// ============================================================

[test] fn builtin_for_loop_readonly() !void {
    var arr = [2, 4, 6, 8];
    var sum = 0;
    for (arr) |n| {
        sum += n;
    }
    try expect_eq(sum, 20);
}

[test] fn builtin_for_loop_mutable() !void {
    var mut arr = [1, 2, 3];
    for (arr) |mut item| {
        item *= 10;
    }
    try expect_eq(arr[0], 10);
    try expect_eq(arr[1], 20);
    try expect_eq(arr[2], 30);
}

[test] fn for_loop_over_slice() !void {
    var arr = [5, 10, 15];
    var s: &[i32] = &arr[0..3];
    var sum = 0;
    for (s) |v| {
        sum += v;
    }
    try expect_eq(sum, 30);
}

[test] fn for_loop_over_vec() !void {
    var v = Vec<i32>.init(alloc);
    v.append(3);
    v.append(6);
    v.append(9);
    var sum = 0;
    for (v) |item| {
        sum += item;
    }
    try expect_eq(sum, 18);
}

[test] fn for_loop_over_map() !void {
    var m = Map<&[u8], i32>.init(alloc);
    m.put("a", 1);
    m.put("b", 2);
    var count = 0;
    for (m) |_| {
        count += 1;
    }
    try expect_eq(count, 2);
}

// ============================================================
// 19. 组合类型综合测试
// ============================================================

[test] fn vec_of_structs() !void {
    var v = Vec<Item>.init(alloc);
    v.append(Item{id = 1});
    v.append(Item{id = 2});
    try expect_eq(v.len, 2);
    try expect_eq(v[0].id, 1);
    try expect_eq(v[1].id, 2);
}

[test] fn map_of_vectors() !void {
    var m = Map<&[u8], Vec<i32>>.init(alloc);
    var evens = Vec<i32>.init(alloc);
    evens.append(2);
    evens.append(4);
    m.put("evens", copy(&evens));
    var odds = Vec<i32>.init(alloc);
    odds.append(1);
    odds.append(3);
    m.put("odds", copy(&odds));
    try expect(m.contains("evens"));
    try expect(m.contains("odds"));
    try expect_eq(m.get("evens").?.len, 2);
}

[test] fn optional_enum_combined() !void {
    var r1 = try_parse("abc");
    var val1 = if (r1) |r| switch (r) {
        Result_.ok => |v| v,
        Result_.err => |_| 0,
    } else 0;
    try expect_eq(val1, 42);
    var r2 = try_parse("");
    try expect_eq(r2 orelse null, null);
}

[test] fn error_optional_nested() !void {
    var data = [("a", 1), ("b", 2), ("c", 3)];
    var found = try lookup_kv("b", &data);
    try expect_eq(found.?, 2);
    var missing = try lookup_kv("z", &data);
    try expect_eq(missing orelse -1, -1);
}