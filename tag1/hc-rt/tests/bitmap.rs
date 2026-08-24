//! hc-rt/tests/bitmap.rs

use hc_rt::Interp;

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

#[test]
fn bitmap_init_len() {
    run_ok(
        r#"[test] fn t() !void {
    var bm = try io.bitmap.init(64);
    try expect_eq(bm.len(), 64);
    var bm2 = try io.bitmap.init(100);
    try expect_eq(bm2.len(), 128);
    var bm3 = try io.bitmap.init(0);
    try expect_eq(bm3.len(), 0);
}
"#,
    );
}

#[test]
fn bitmap_set_get_clear() {
    run_ok(
        r#"[test] fn t() !void {
    var bm = try io.bitmap.init(200);
    try expect_eq(bm.get(0), false);
    try expect_eq(bm.get(42), false);
    bm.set(42);
    try expect_eq(bm.get(42), true);
    try expect_eq(bm.get(0), false);
    try expect_eq(bm.get(41), false);
    try expect_eq(bm.get(43), false);
    bm.clear(42);
    try expect_eq(bm.get(42), false);
}
"#,
    );
}

#[test]
fn bitmap_set_edge() {
    // 边界位：0 和 63（word 内首尾）
    run_ok(
        r#"[test] fn t() !void {
    var bm = try io.bitmap.init(64);
    bm.set(0);
    bm.set(63);
    try expect_eq(bm.get(0), true);
    try expect_eq(bm.get(63), true);
    bm.clear(63);
    try expect_eq(bm.get(63), false);
    try expect_eq(bm.get(0), true);
}
"#,
    );
}

#[test]
fn bitmap_count() {
    run_ok(
        r#"[test] fn t() !void {
    var bm = try io.bitmap.init(256);
    try expect_eq(bm.count(), 0);
    bm.set(0);
    bm.set(1);
    bm.set(100);
    bm.set(200);
    try expect_eq(bm.count(), 4);
    bm.clear(1);
    try expect_eq(bm.count(), 3);
    bm.clear(100);
    bm.clear(200);
    bm.clear(0);
    try expect_eq(bm.count(), 0);
}
"#,
    );
}

#[test]
fn bitmap_multi_word() {
    // 跨多个 u64 的位操作
    run_ok(
        r#"[test] fn t() !void {
    var bm = try io.bitmap.init(300);
    bm.set(0);
    bm.set(64);
    bm.set(128);
    bm.set(192);
    bm.set(256);
    try expect_eq(bm.count(), 5);
    try expect_eq(bm.get(0), true);
    try expect_eq(bm.get(64), true);
    try expect_eq(bm.get(128), true);
    try expect_eq(bm.get(192), true);
    try expect_eq(bm.get(256), true);
    try expect_eq(bm.get(63), false);
    try expect_eq(bm.get(65), false);
    try expect_eq(bm.get(255), false);
}
"#,
    );
}
