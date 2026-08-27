//! hc-rt/tests/mutex.rs

use hc_rt::Interp;

/// 运行源码中所有 test fn；断言全部通过
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
fn mutex_init_and_lock() {
    // 基本 Mutex.init/lock：加锁后返回内部值
    run_ok(
        r#"
[test] fn t() !void {
    var m = Mutex.init(42);
    var v = try m.lock();
    try expect_eq(v, 42);
}
"#,
    );
}

#[test]
fn mutex_try_lock_acquires() {
    // try_lock 在未锁定时返回 ?T（Some），用 null 检查非空
    run_ok(
        r#"
[test] fn t() !void {
    var m = Mutex.init(42);
    var v = try m.try_lock();
    try expect_neq(v, null);
}
"#,
    );
}

#[test]
fn mutex_lock_returns_clone() {
    // lock 返回内部值的克隆，修改后不影响 Mutex 内部
    run_ok(
        r#"
[test] fn t() !void {
    var m = Mutex.init(100);
    var mut v = try m.lock();
    v = v + 1;
    // 重新加锁获取原始值（不受外部修改影响）
    var v2 = try m.lock();
    try expect_eq(v2, 100);
}
"#,
    );
}

#[test]
fn mutex_spawn_shared_access() {
    // 多线程通过 Mutex 共享数据
    run_ok(
        r#"
fn worker(m: Mutex) !i32 {
    var v = try m.lock();
    return v + 1;
}
[test] fn t() !void {
    var m = Mutex.init(41);
    var th = spawn(worker, m);
    var r = try th.join();
    try expect_eq(r, 42);
}
"#,
    );
}
