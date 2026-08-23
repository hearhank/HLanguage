//! A6 标准库数据结构：RingBuf 环形缓冲验收测试
//!
//! API：`io.ringbuf.init(cap)` → RingBuf → `.push(v)` / `.pop() ?T` /
//! `.len() usize` / `.capacity() usize` / `.is_full() bool` / `.is_empty() bool` /
//! `.clear()` / `.peek(idx) ?T`

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
fn ringbuf_init_len() {
    run_ok(
        r#"[test] fn t() !void {
    var rb = try io.ringbuf.init(10);
    try expect_eq(rb.len(), 0);
    try expect_eq(rb.capacity(), 10);
    try expect_eq(rb.is_empty(), true);
    try expect_eq(rb.is_full(), false);
}
"#,
    );
}

#[test]
fn ringbuf_push_pop() {
    run_ok(
        r#"[test] fn t() !void {
    var rb = try io.ringbuf.init(5);
    try expect_eq(rb.push(42), true);
    try expect_eq(rb.push(99), true);
    try expect_eq(rb.len(), 2);
    try expect_eq(rb.pop(), 42);
    try expect_eq(rb.pop(), 99);
    try expect_eq(rb.is_empty(), true);
}
"#,
    );
}

#[test]
fn ringbuf_pop_empty() {
    run_ok(
        r#"[test] fn t() !void {
    var rb = try io.ringbuf.init(3);
    try expect_eq(rb.pop(), null);
}
"#,
    );
}

#[test]
fn ringbuf_push_when_full() {
    run_ok(
        r#"[test] fn t() !void {
    var rb = try io.ringbuf.init(2);
    try expect_eq(rb.push(1), true);
    try expect_eq(rb.push(2), true);
    try expect_eq(rb.is_full(), true);
    try expect_eq(rb.push(3), false);
    try expect_eq(rb.len(), 2);
}
"#,
    );
}

#[test]
fn ringbuf_clear() {
    run_ok(
        r#"[test] fn t() !void {
    var rb = try io.ringbuf.init(5);
    try expect_eq(rb.push(10), true);
    try expect_eq(rb.push(20), true);
    try expect_eq(rb.len(), 2);
    rb.clear();
    try expect_eq(rb.len(), 0);
    try expect_eq(rb.is_empty(), true);
    try expect_eq(rb.pop(), null);
}
"#,
    );
}

#[test]
fn ringbuf_fifo_order() {
    run_ok(
        r#"[test] fn t() !void {
    var rb = try io.ringbuf.init(10);
    try expect_eq(rb.push(1), true);
    try expect_eq(rb.push(2), true);
    try expect_eq(rb.push(3), true);
    try expect_eq(rb.pop(), 1);
    try expect_eq(rb.pop(), 2);
    try expect_eq(rb.pop(), 3);
    try expect_eq(rb.is_empty(), true);
}
"#,
    );
}
