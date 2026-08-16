//! M4.6 迭代内建方法链（iter/filter/map 统一覆盖内建可迭代类型）
//!
//! tag1 采用「立即求值链」形态：`iter()` 返回元素数组（数据对象），`filter`/`map`
//! 在该数组上立即求值变换（与「显式迭代器对象」的 tag1 近似一致）。本套件覆盖
//! 新增的 Str（字节 Int）/ Map（KV 条目）/ 切片（`arr[lo..hi]`）方法链。

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
fn str_iter_bytes() {
    // String = 字节串：iter() 按字节 Int 立即求值
    run_ok(
        r#"
[test] fn t() !void {
    var s = "abc";
    var bs = s.iter();
    try expect_eq(bs.len, 3);
    try expect_eq(bs[0], 97);
    try expect_eq(bs[2], 99);
}
"#,
    );
}

#[test]
fn str_filter_map() {
    // String 方法链：map 变换 / filter 筛选（字节 Int）
    run_ok(
        r#"
[test] fn t() !void {
    var s = "a1b2";
    var upper = s.map(|b| b - 32);
    try expect_eq(upper.len, 4);
    try expect_eq(upper[0], 65);
    var digits = s.filter(|b| b >= 48 and b <= 57);
    try expect_eq(digits.len, 2);
    try expect_eq(digits[0], 49);
}
"#,
    );
}

#[test]
fn map_iter_entries() {
    // Map.iter() → KV 条目数组（key/value 字段，与 for |kv| 捕获一致）
    run_ok(
        r#"
[test] fn t() !void {
    var m = Map(&[u8], i32).init(alloc);
    m.put("a", 1);
    m.put("b", 2);
    var keys = m.iter().map(|kv| kv.key);
    try expect_eq(keys.len, 2);
    var vals = m.iter().map(|kv| kv.value);
    try expect_eq(vals.len, 2);
}
"#,
    );
}

#[test]
fn slice_filter_map() {
    // 切片（arr[lo..hi]）方法链：filter/map
    run_ok(
        r#"
[test] fn t() !void {
    var arr = [1, 2, 3, 4, 5];
    var sub = arr[1..4];
    var doubled = sub.map(|x| x * 2);
    try expect_eq(doubled.len, 3);
    try expect_eq(doubled[0], 4);
    var evens = arr.filter(|x| x % 2 == 0);
    try expect_eq(evens.len, 2);
}
"#,
    );
}
