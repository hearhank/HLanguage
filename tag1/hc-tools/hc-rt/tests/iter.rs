//! M4.6 / A7 惰性迭代器链（iter/filter/map 返回 LazyIter，next() 按需求值）
//!
//! tag1 采用「惰性组合子」形态：`iter()` 返回 LazyIter（延迟包装），`filter`/`map`
//! 链式追加变换（不立即求值），`next()` 按需逐一计算，`to_array()` 解析全部剩余项。
//! 覆盖：Arr/Slice/Str/Map/用户类型。

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

// ---------- 后向兼容：to_array() 恢复数组形态 ----------

#[test]
fn str_iter_bytes() {
    // String = 字节串：iter().to_array() 得到字节 Int 数组
    run_ok(
        r#"
[test] fn t() !void {
    var s = "abc";
    var bs = s.iter().to_array();
    try expect_eq(bs.len, 3);
    try expect_eq(bs[0], 97);
    try expect_eq(bs[2], 99);
}
"#,
    );
}

#[test]
fn str_filter_map() {
    // String 方法链：map/filter 后 to_array() 求值
    run_ok(
        r#"
[test] fn t() !void {
    var s = "a1b2";
    var upper = s.map(|b| b - 32).to_array();
    try expect_eq(upper.len, 4);
    try expect_eq(upper[0], 65);
    var digits = s.filter(|b| b >= 48 && b <= 57).to_array();
    try expect_eq(digits.len, 2);
    try expect_eq(digits[0], 49);
}
"#,
    );
}

#[test]
fn map_iter_entries() {
    // Map.iter() → KV 条目数组（key/value 字段）
    run_ok(
        r#"
[test] fn t() !void {
    var m = Map(&[u8], i32).init(alloc);
    m.put("a", 1);
    m.put("b", 2);
    var keys = m.iter().map(|kv| kv.key).to_array();
    try expect_eq(keys.len, 2);
    var vals = m.iter().map(|kv| kv.value).to_array();
    try expect_eq(vals.len, 2);
}
"#,
    );
}

#[test]
fn slice_filter_map() {
    // 切片方法链：filter/map + to_array()
    run_ok(
        r#"
[test] fn t() !void {
    var arr = [1, 2, 3, 4, 5];
    var sub = arr[1..4];
    var doubled = sub.map(|x| x * 2).to_array();
    try expect_eq(doubled.len, 3);
    try expect_eq(doubled[0], 4);
    var evens = arr.filter(|x| x % 2 == 0).to_array();
    try expect_eq(evens.len, 2);
}
"#,
    );
}

// ---------- 惰性迭代器 next() 测试 ----------

#[test]
fn lazy_next_arr() {
    // 数组迭代：next() 按需求值
    run_ok(
        r#"
[test] fn t() !void {
    var iter = [10, 20, 30].iter();
    var v = iter.next();
    try expect_eq(v.?, 10);
    var v2 = iter.next();
    try expect_eq(v2.?, 20);
    var v3 = iter.next();
    try expect_eq(v3.?, 30);
    var v4 = iter.next();
    try expect(v4 == null);
}
"#,
    );
}

#[test]
fn lazy_next_str() {
    // 字符串迭代：字节 Int
    run_ok(
        r#"
[test] fn t() !void {
    var iter = "hi".iter();
    try expect_eq(iter.next().?, 104);
    try expect_eq(iter.next().?, 105);
    try expect(iter.next() == null);
}
"#,
    );
}

#[test]
fn lazy_filter_next() {
    // filter().next() 链式求值
    run_ok(
        r#"
[test] fn t() !void {
    var arr = [1, 2, 3, 4, 5];
    var evens = arr.filter(|x| x % 2 == 0);
    try expect_eq(evens.next().?, 2);
    try expect_eq(evens.next().?, 4);
    try expect(evens.next() == null);
}
"#,
    );
}

#[test]
fn lazy_map_next() {
    // map().next() 链式求值
    run_ok(
        r#"
[test] fn t() !void {
    var arr = [1, 2, 3];
    var doubled = arr.map(|x| x * 2);
    try expect_eq(doubled.next().?, 2);
    try expect_eq(doubled.next().?, 4);
    try expect_eq(doubled.next().?, 6);
    try expect(doubled.next() == null);
}
"#,
    );
}

#[test]
fn lazy_filter_map_chain() {
    // filter + map 链式复合
    run_ok(
        r#"
[test] fn t() !void {
    var arr = [1, 2, 3, 4, 5, 6];
    // 偶数 → 乘以 10
    var result = arr.filter(|x| x % 2 == 0).map(|x| x * 10);
    try expect_eq(result.next().?, 20);
    try expect_eq(result.next().?, 40);
    try expect_eq(result.next().?, 60);
    try expect(result.next() == null);
}
"#,
    );
}

#[test]
fn lazy_iter_filter_chain() {
    // iter().filter() 链式
    run_ok(
        r#"
[test] fn t() !void {
    var arr = [1, 2, 3, 4, 5, 6];
    var result = arr.iter().filter(|x| x % 2 == 0);
    try expect_eq(result.next().?, 2);
    try expect_eq(result.next().?, 4);
    try expect_eq(result.next().?, 6);
    try expect(result.next() == null);
}
"#,
    );
}

#[test]
fn lazy_map_filter_chain_to_array() {
    // map + filter 链式复合后用 to_array() 解析
    run_ok(
        r#"
[test] fn t() !void {
    var arr = [1, 2, 3, 4, 5];
    // 乘以 3 → 筛选大于 10 的
    var result = arr.map(|x| x * 3).filter(|x| x > 10).to_array();
    try expect_eq(result.len, 2);
    try expect_eq(result[0], 12);
    try expect_eq(result[1], 15);
}
"#,
    );
}

// ---------- to_array() 集成测试 ----------

#[test]
fn lazy_to_array() {
    // to_array() 解析全部剩余项
    run_ok(
        r#"
[test] fn t() !void {
    var arr = [1, 2, 3, 4, 5];
    var evens = arr.filter(|x| x % 2 == 0).to_array();
    try expect_eq(evens.len, 2);
    try expect_eq(evens[0], 2);
    try expect_eq(evens[1], 4);
}
"#,
    );
}
