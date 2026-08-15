//! M5.2 Deque 运行时实现（双端队列 + 最小方法集 get/put/remove）
//!
//! tag1：Vec/Deque 共享 `Value::Arr` 值模型；Deque 方法按名分派
//! （push_front/pop_front/push_back/pop_back/front/back + get/put/remove）。

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
fn deque_push_pop_order() {
    run_ok(
        r#"
test fn deque_push_pop_order() !void {
    var dq = Deque(i32).init(alloc);
    dq.push_back(1);
    dq.push_back(2);
    dq.push_front(0);
    try expect_eq(dq.len, 3);
    try expect_eq(dq[0], 0);
    try expect_eq(dq[1], 1);
    try expect_eq(dq[2], 2);
    try expect_eq(dq.pop_front().?, 0);
    try expect_eq(dq.pop_back().?, 2);
    try expect_eq(dq.len, 1);
    try expect_eq(dq.front().?, 1);
    try expect_eq(dq.back().?, 1);
}
"#,
    );
}

#[test]
fn deque_get_put_remove() {
    run_ok(
        r#"
test fn deque_get_put_remove() !void {
    var dq = Deque(i32).init(alloc);
    dq.append(10);
    dq.append(20);
    dq.append(30);
    try expect_eq(dq.get(1).?, 20);
    dq.put(1, 99);
    try expect_eq(dq.get(1).?, 99);
    try expect_eq(dq.remove(0), 10);
    try expect_eq(dq.len, 2);
    try expect_eq(dq[0], 99);
}
"#,
    );
}

#[test]
fn deque_empty_returns_null() {
    run_ok(
        r#"
test fn deque_empty_returns_null() !void {
    var dq = Deque(i32).init(alloc);
    try expect_eq(dq.pop_front(), null);
    try expect_eq(dq.pop_back(), null);
    try expect_eq(dq.front(), null);
    try expect_eq(dq.back(), null);
    try expect_eq(dq.get(0), null);
}
"#,
    );
}

#[test]
fn deque_len_iter() {
    run_ok(
        r#"
test fn deque_len_iter() !void {
    var dq = Deque(i32).init(alloc);
    dq.append(1);
    dq.append(2);
    dq.append(3);
    try expect_eq(dq.len, 3);
    var sum = 0;
    for (dq) |v| {
        sum += v;
    }
    try expect_eq(sum, 6);
}
"#,
    );
}
