//! hc-rt/tests/pagemem.rs

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
fn pagemem_init() {
    run_ok(
        r#"[test] fn t() !void {
    var pm = try io.pagemem.init(10);
    try expect_eq(pm.total(), 10);
    try expect_eq(pm.available(), 10);
}
"#,
    );
}

#[test]
fn pagemem_alloc() {
    run_ok(
        r#"[test] fn t() !void {
    var pm = try io.pagemem.init(5);
    var a = pm.alloc();
    try expect_eq(a, 0);
    var b = pm.alloc();
    try expect_eq(b, 1);
    try expect_eq(pm.available(), 3);
}
"#,
    );
}

#[test]
fn pagemem_alloc_all() {
    run_ok(
        r#"[test] fn t() !void {
    var pm = try io.pagemem.init(3);
    try expect_eq(pm.alloc(), 0);
    try expect_eq(pm.alloc(), 1);
    try expect_eq(pm.alloc(), 2);
    try expect_eq(pm.alloc(), null);
    try expect_eq(pm.available(), 0);
}
"#,
    );
}

#[test]
fn pagemem_free() {
    run_ok(
        r#"[test] fn t() !void {
    var pm = try io.pagemem.init(5);
    var a = pm.alloc();
    var b = pm.alloc();
    try expect_eq(pm.available(), 3);
    pm.free(a);
    try expect_eq(pm.available(), 4);
    var c = pm.alloc();
    try expect_eq(c, a);
    try expect_eq(pm.available(), 3);
}
"#,
    );
}

#[test]
fn pagemem_double_free_safe() {
    run_ok(
        r#"[test] fn t() !void {
    var pm = try io.pagemem.init(3);
    var a = pm.alloc();
    pm.free(a);
    pm.free(a);
    try expect_eq(pm.available(), 3);
}
"#,
    );
}

#[test]
fn pagemem_free_invalid() {
    run_ok(
        r#"[test] fn t() !void {
    var pm = try io.pagemem.init(3);
    pm.free(100);
    try expect_eq(pm.available(), 3);
}
"#,
    );
}
