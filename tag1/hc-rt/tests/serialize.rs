//! M4.4 序列化内建（packed/align 尊重 + 嵌套连续 round-trip + 堆类型 JSON 完整）
//!
//! 覆盖：`[pad]`（紧凑布局）/ `[align(T)]`（类型级对齐）对 `to_bytes`/`from_bytes`
//! 与 `@sizeOf`/`@offsetOf`/`@alignOf` 的一致映射；嵌套连续类型字段的字节 round-trip；
//! 堆上 class 的 `to_json`/`from_json` 嵌套（Vec 字段 / 嵌套 class 字段）还原。

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
[continuous] [pad]
class Packed {
    a: i8,
    b: i32,
}
test fn t() !void {
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
[continuous] [align(u64)]
class Aligned {
    a: i8,
}
test fn t() !void {
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
[continuous]
class Inner {
    x: i32,
    y: i32,
}
[continuous]
class Outer {
    a: i8,
    b: Inner,
}
test fn t() !void {
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
    mut nums: Vec(i32),
}
test fn t() !void {
    var mut tag: o Tag = alloc.init(Tag);
    tag.score = 5;
    var mut doc: o Doc = alloc.init(Doc);
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
