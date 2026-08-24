//! A6 标准库数据结构：IntrList 侵入式链表验收测试
//!
//! API：`io.intrlist.init()` → IntrList → `.push_front(v) usize` /
//! `.pop_front() ?T` / `.push_back(v) usize` / `.pop_back() ?T` /
//! `.remove(idx) ?T` / `.len() usize` / `.is_empty() bool` / `.clear()`

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
fn intrlist_init_empty() {
    run_ok(
        r#"[test] fn t() !void {
    var list = try io.intrlist.init();
    try expect_eq(list.len(), 0);
    try expect_eq(list.is_empty(), true);
    try expect_eq(list.pop_front(), null);
    try expect_eq(list.pop_back(), null);
}
"#,
    );
}

#[test]
fn intrlist_push_front_pop_front() {
    run_ok(
        r#"[test] fn t() !void {
    var list = try io.intrlist.init();
    var a = list.push_front(10);
    var b = list.push_front(20);
    var c = list.push_front(30);
    try expect_eq(list.len(), 3);
    try expect_eq(list.pop_front(), 30);
    try expect_eq(list.pop_front(), 20);
    try expect_eq(list.pop_front(), 10);
    try expect_eq(list.is_empty(), true);
}
"#,
    );
}

#[test]
fn intrlist_push_back_pop_back() {
    run_ok(
        r#"[test] fn t() !void {
    var list = try io.intrlist.init();
    var a = list.push_back(10);
    var b = list.push_back(20);
    var c = list.push_back(30);
    try expect_eq(list.len(), 3);
    try expect_eq(list.pop_back(), 30);
    try expect_eq(list.pop_back(), 20);
    try expect_eq(list.pop_back(), 10);
    try expect_eq(list.is_empty(), true);
}
"#,
    );
}

#[test]
fn intrlist_push_front_pop_back() {
    run_ok(
        r#"[test] fn t() !void {
    var list = try io.intrlist.init();
    list.push_front(10);
    list.push_front(20);
    list.push_front(30);
    // list: 30 ↔ 20 ↔ 10
    try expect_eq(list.pop_back(), 10);
    try expect_eq(list.pop_back(), 20);
    try expect_eq(list.pop_back(), 30);
    try expect_eq(list.is_empty(), true);
}
"#,
    );
}

#[test]
fn intrlist_push_back_pop_front() {
    run_ok(
        r#"[test] fn t() !void {
    var list = try io.intrlist.init();
    list.push_back(10);
    list.push_back(20);
    list.push_back(30);
    // list: 10 ↔ 20 ↔ 30
    try expect_eq(list.pop_front(), 10);
    try expect_eq(list.pop_front(), 20);
    try expect_eq(list.pop_front(), 30);
    try expect_eq(list.is_empty(), true);
}
"#,
    );
}

#[test]
fn intrlist_remove_middle() {
    run_ok(
        r#"[test] fn t() !void {
    var list = try io.intrlist.init();
    var a = list.push_back(10);
    var b = list.push_back(20);
    var c = list.push_back(30);
    try expect_eq(list.remove(b), 20);
    try expect_eq(list.len(), 2);
    // list: 10 ↔ 30
    try expect_eq(list.pop_front(), 10);
    try expect_eq(list.pop_front(), 30);
    try expect_eq(list.is_empty(), true);
}
"#,
    );
}

#[test]
fn intrlist_remove_head() {
    run_ok(
        r#"[test] fn t() !void {
    var list = try io.intrlist.init();
    var a = list.push_back(10);
    var b = list.push_back(20);
    try expect_eq(list.remove(a), 10);
    try expect_eq(list.len(), 1);
    try expect_eq(list.pop_front(), 20);
}
"#,
    );
}

#[test]
fn intrlist_remove_tail() {
    run_ok(
        r#"[test] fn t() !void {
    var list = try io.intrlist.init();
    var a = list.push_back(10);
    var b = list.push_back(20);
    try expect_eq(list.remove(b), 20);
    try expect_eq(list.len(), 1);
    try expect_eq(list.pop_front(), 10);
}
"#,
    );
}

#[test]
fn intrlist_remove_invalid() {
    run_ok(
        r#"[test] fn t() !void {
    var list = try io.intrlist.init();
    try expect_eq(list.remove(0), null);
    try expect_eq(list.remove(100), null);
}
"#,
    );
}

#[test]
fn intrlist_clear() {
    run_ok(
        r#"[test] fn t() !void {
    var list = try io.intrlist.init();
    list.push_back(1);
    list.push_back(2);
    list.push_back(3);
    try expect_eq(list.len(), 3);
    list.clear();
    try expect_eq(list.is_empty(), true);
    try expect_eq(list.pop_front(), null);
}
"#,
    );
}

#[test]
fn intrlist_node_reuse() {
    run_ok(
        r#"[test] fn t() !void {
    var list = try io.intrlist.init();
    var a = list.push_back(10);
    var b = list.push_back(20);
    try expect_eq(list.len(), 2);
    try expect_eq(list.remove(a), 10);
    try expect_eq(list.remove(b), 20);
    try expect_eq(list.is_empty(), true);
    // 节点应被重用（LIFO：b 先释放，所以 b 先被重用）
    var c = list.push_back(30);
    var d = list.push_back(40);
    try expect_eq(list.len(), 2);
    try expect_eq(list.pop_front(), 30);
    try expect_eq(list.pop_front(), 40);
}
"#,
    );
}
