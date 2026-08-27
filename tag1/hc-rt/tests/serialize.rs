//! hc-rt/tests/serialize.rs
//!
//! 定义：结构体：Packed, Aligned, Inner, Outer

use hc_rt::Interp;

fn run_ok(src: &str) {
    let program = hc::parse_source(src).unwrap_or_else(|d| panic!("parse: {:?}", d));
    let mut interp = Interp::new(src);
    interp
        .load(&program)
        .unwrap_or_else(|e| panic!("load: {} {}", e.name, e.message));
    let (p, f, _s) = interp.run_tests();
    assert_eq!(f, 0, "failed: {:?}", interp.test_out);
    assert!(p >= 1, "no tests ran");
}

#[test]
fn pad_packed_layout() {
    // [pad]：紧凑布局，字段间无填充，alignOf = 1，sizeOf = 字段宽度和
    run_ok(
        r#"
[pad]
struct Packed {
    a: i8,
    b: i32,
}
[test] fn t() !void {
    try expect_eq(@sizeOf(Packed), 5);
    try expect_eq(@offsetOf(Packed, "a"), 0);
    try expect_eq(@offsetOf(Packed, "b"), 1);
    try expect_eq(@alignOf(Packed), 1);
    var p = Packed{ a = 7, b = 300 };
    var bytes = p.to_bytes();
    try expect_eq(bytes.len, 5);
    var p2 = try Packed.from_bytes(bytes);
    try expect_eq(p2.a, 7);
    try expect_eq(p2.b, 300);
}
"#,
    );
}

#[test]
fn align_type_level_alignment() {
    // [align(u64)]：类型级对齐 8，尾部圆整到 8；alignOf = 8
    run_ok(
        r#"
[align(8)]
struct Aligned {
    a: i8,
}
[test] fn t() !void {
    try expect_eq(@alignOf(Aligned), 8);
    try expect_eq(@sizeOf(Aligned), 8);
    var a = Aligned{ a = 9 };
    var bytes = a.to_bytes();
    try expect_eq(bytes.len, 8);
    var a2 = try Aligned.from_bytes(bytes);
    try expect_eq(a2.a, 9);
}
"#,
    );
}

#[test]
fn nested_continuous_roundtrip() {
    // 嵌套连续类型：字段对齐 = 自身 alignOf，round-trip 还原嵌套字段
    run_ok(
        r#"
struct Inner {
    x: i32,
    y: i32,
}
struct Outer {
    a: i8,
    b: Inner,
}
[test] fn t() !void {
    var outer = Outer{ a = 1, b = Inner{ x = 10, y = 20 } };
    try expect_eq(@sizeOf(Outer), 12);
    try expect_eq(@offsetOf(Outer, "a"), 0);
    try expect_eq(@offsetOf(Outer, "b"), 4);
    var bytes = outer.to_bytes();
    try expect_eq(bytes.len, 12);
    var outer2 = try Outer.from_bytes(bytes);
    try expect_eq(outer2.a, 1);
    try expect_eq(outer2.b.x, 10);
    try expect_eq(outer2.b.y, 20);
}
"#,
    );
}

#[test]
fn heap_json_nested_roundtrip() {
    // 堆类型 JSON：Vec 字段（嵌套数组）+ 嵌套 class 字段（嵌套对象）round-trip
    run_ok(
        r#"
class Tag {
    mut score: i32,
}
class Doc {
    mut id: i32,
    mut tag: Tag,
    mut nums: Vec<i32>,
}
[test] fn t() !void {
    var mut tag: Tag = alloc.init(Tag);
    tag.score = 5;
    var mut doc: Doc = alloc.init(Doc);
    doc.id = 1;
    doc.tag = tag;
    doc.nums.append(7);
    doc.nums.append(8);
    var json = doc.to_json();
    var doc2 = try Doc.from_json(json);
    try expect_eq(doc2.id, 1);
    try expect_eq(doc2.tag.score, 5);
    try expect_eq(doc2.nums.len, 2);
    try expect_eq(doc2.nums[0], 7);
    try expect_eq(doc2.nums[1], 8);
}
"#,
    );
}

// ---- D2：serialize 命名空间（M5.3 库封装）——解析辅助组 ----

#[test]
fn serialize_parse_int_float() {
    // serialize.parse_int / serialize.parse_float：文本 → ?i32 / ?f64
    // F4 补全：负数 / 显式 + 号 / i32 上界 / 科学计数法与尾随空格
    run_ok(
        r#"
[test] fn parse_int_float() !void {
    try expect_eq(serialize.parse_int("42") orelse -1, 42);
    try expect_eq(serialize.parse_int(" 7 ") orelse -1, 7);
    try expect_eq(serialize.parse_int("x") orelse -1, -1);
    try expect_eq(serialize.parse_int("") orelse -1, -1);
    try expect_eq(serialize.parse_int("-42") orelse -1, -42);
    try expect_eq(serialize.parse_int("+99") orelse -1, 99);
    try expect_eq(serialize.parse_int("2147483647") orelse -1, 2147483647);
    try expect_eq(serialize.parse_float("3.5") orelse -1.0, 3.5);
    try expect_eq(serialize.parse_float("2.0") orelse -1.0, 2.0);
    try expect_eq(serialize.parse_float("z") orelse -1.0, -1.0);
    try expect_eq(serialize.parse_float("-3.5") orelse -1.0, -3.5);
    try expect_eq(serialize.parse_float("1e3") orelse -1.0, 1000.0);
    try expect_eq(serialize.parse_float(" 3.50 ") orelse -1.0, 3.5);
}
"#,
    );
}

#[test]
fn serialize_json_csv_parse() {
    // serialize.json.parse / serialize.csv.parse：与虚拟根 json.parse/csv.parse 等价
    run_ok(
        r#"
[test] fn json_csv() !void {
    var obj = serialize.json.parse("{\"a\":1,\"b\":2}");
    try expect_eq(obj.get("a").?, 1);
    try expect_eq(obj.get("b").?, 2);
    var rows = serialize.csv.parse("x,y\n1,2");
    try expect_eq(rows.len, 2);
    try expect_eq(rows[0][1], "y");
    try expect_eq(rows[1][0], "1");
    // 与既有虚拟根形式等价
    var obj2 = json.parse("{\"a\":1}");
    try expect_eq(obj2.get("a").?, 1);
}
"#,
    );
}

#[test]
fn serialize_parser_helpers() {
    // serialize.skip_space/peek/advance/is_digit/parse_number/expect：&[u8] + *usize
    run_ok(
        r#"
[test] fn parser_helpers() !void {
    var data: &[u8] = "  42,";
    var pos: usize = 0;
    serialize.skip_space(data, &pos);
    try expect_eq(pos, 2);
    var c = serialize.peek(data, &pos) orelse return error.End;
    try expect_eq(c, '4');
    try expect_eq(serialize.is_digit(c), true);
    try expect_eq(serialize.is_digit(' '), false);
    var n = serialize.parse_number(data, &pos);
    try expect_eq(n, 42);
    try expect_eq(pos, 4);
    serialize.expect(data, &pos, ',') catch return error.Token;
    try expect_eq(pos, 5);
    serialize.advance(data, &pos);
    try expect_eq(pos, 6);
    // 自由内建形态保持可用（既有示例依赖）
    var pos2: usize = 0;
    skip_space(data, &pos2);
    try expect_eq(pos2, 2);
}
"#,
    );
}
