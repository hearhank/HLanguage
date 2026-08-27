//! hc-rt/tests/treemap.rs

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
fn treemap_init_empty() {
    run_ok(
        r#"[test] fn t() !void {
    var map = try io.treemap.init();
    try expect_eq(map.len(), 0);
    try expect_eq(map.is_empty(), true);
    try expect_eq(map.get(42), null);
    try expect_eq(map.contains(42), false);
}
"#,
    );
}

#[test]
fn treemap_insert_and_get() {
    run_ok(
        r#"[test] fn t() !void {
    var map = try io.treemap.init();
    map.insert(10, 100);
    map.insert(20, 200);
    map.insert(30, 300);
    try expect_eq(map.len(), 3);
    try expect_eq(map.get(10), 100);
    try expect_eq(map.get(20), 200);
    try expect_eq(map.get(30), 300);
    try expect_eq(map.get(99), null);
}
"#,
    );
}

#[test]
fn treemap_update_existing_key() {
    run_ok(
        r#"[test] fn t() !void {
    var map = try io.treemap.init();
    map.insert(10, 100);
    try expect_eq(map.get(10), 100);
    map.insert(10, 999);
    try expect_eq(map.get(10), 999);
    try expect_eq(map.len(), 1);
}
"#,
    );
}

#[test]
fn treemap_contains() {
    run_ok(
        r#"[test] fn t() !void {
    var map = try io.treemap.init();
    map.insert(5, 50);
    map.insert(15, 150);
    try expect_eq(map.contains(5), true);
    try expect_eq(map.contains(15), true);
    try expect_eq(map.contains(0), false);
    try expect_eq(map.contains(10), false);
}
"#,
    );
}

#[test]
fn treemap_insert_descending() {
    run_ok(
        r#"[test] fn t() !void {
    var map = try io.treemap.init();
    map.insert(30, 300);
    map.insert(20, 200);
    map.insert(10, 100);
    try expect_eq(map.len(), 3);
    try expect_eq(map.get(30), 300);
    try expect_eq(map.get(20), 200);
    try expect_eq(map.get(10), 100);
}
"#,
    );
}

#[test]
fn treemap_insert_ascending() {
    run_ok(
        r#"[test] fn t() !void {
    var map = try io.treemap.init();
    map.insert(10, 100);
    map.insert(20, 200);
    map.insert(30, 300);
    try expect_eq(map.len(), 3);
    try expect_eq(map.get(10), 100);
    try expect_eq(map.get(20), 200);
    try expect_eq(map.get(30), 300);
}
"#,
    );
}

#[test]
fn treemap_clear() {
    run_ok(
        r#"[test] fn t() !void {
    var map = try io.treemap.init();
    map.insert(1, 10);
    map.insert(2, 20);
    map.insert(3, 30);
    try expect_eq(map.len(), 3);
    map.clear();
    try expect_eq(map.is_empty(), true);
    try expect_eq(map.len(), 0);
    try expect_eq(map.get(1), null);
    try expect_eq(map.get(2), null);
}
"#,
    );
}

#[test]
fn treemap_large_keys() {
    run_ok(
        r#"[test] fn t() !void {
    var map = try io.treemap.init();
    map.insert(1000000, 1);
    map.insert(-1000000, 2);
    try expect_eq(map.len(), 2);
    try expect_eq(map.get(1000000), 1);
    try expect_eq(map.get(-1000000), 2);
    try expect_eq(map.contains(0), false);
}
"#,
    );
}
