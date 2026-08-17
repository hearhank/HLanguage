//! G4/mem：集合持有分配器引用（`08-mem-allocator-design.md` §7 定案落地）
//!
//! 覆盖（tree-walking interp）：
//! - `Vec(T).init(alloc)` / `Map(K,V).init(alloc)` / `Table(T).init(alloc, rows, cols, init)`
//!   把 alloc 存为内部字段，`.alloc()` 可观测（返回构造时携带的分配器引用）
//! - 未显式传 alloc（含裸类型表达式 `Vec(i32)` 实例化）回退全局 alloc
//! - 扩容/子对象分配走该分配器（tag1 中 items 为 Rc 存储，可观测面即 `.alloc()`；
//!   真实后端按 §7 在扩容/销毁时经它分配/递归销毁）
//! - 集合可遍历（Vec 句柄 / Map 句柄）

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
fn vec_init_captures_global_alloc() {
    // `Vec(T).init(alloc)`：携带全局分配器——`v.alloc()` 返回它，可继续分配 8 字节
    run_ok(
        "[test] fn t() !void {\n    var v = Vec(i32).init(alloc);\n    var q = v.alloc();\n    var buf = q.alloc(8);\n    try expect_eq(buf, \"\\x00\\x00\\x00\\x00\\x00\\x00\\x00\\x00\");\n}\n",
    );
}

#[test]
fn vec_init_captures_arena() {
    // `Vec(T).init(arena)`：携带 arena——类型可见（Arena），未分配过字节则为 0
    run_ok(
        "[test] fn t() !void {\n    var arena = Arena.init(alloc);\n    var v = Vec(i32).init(arena);\n    try expect_eq(@typeOf(v.alloc()), \"Arena\");\n    try expect_eq(v.alloc().bytes(), 0);\n}\n",
    );
}

#[test]
fn vec_default_carries_global_alloc() {
    // 裸类型表达式 `Vec(i32)`（无显式 init）→ 回退全局 alloc（§3 隐式环境）
    run_ok(
        "[test] fn t() !void {\n    var v = Vec(i32);\n    var q = v.alloc();\n    var buf = q.alloc(4);\n    try expect_eq(buf, \"\\x00\\x00\\x00\\x00\");\n}\n",
    );
}

#[test]
fn vec_stores_and_grows_with_stored_alloc() {
    // 携带的分配器随集合存在：扩容（append）后 `.alloc()` 仍可观测、可分配
    run_ok(
        "[test] fn t() !void {\n    var v = Vec(i32).init(alloc);\n    v.append(1);\n    v.append(2);\n    try expect_eq(v.len(), 2);\n    try expect_eq(v[0], 1);\n    try expect_eq(v[1], 2);\n    var buf = v.alloc().alloc(8);\n    try expect_eq(buf, \"\\x00\\x00\\x00\\x00\\x00\\x00\\x00\\x00\");\n}\n",
    );
}

#[test]
fn map_init_captures_alloc() {
    // `Map(K,V).init(alloc)`：携带分配器 + put/get/len 正常
    run_ok(
        "[test] fn t() !void {\n    var m = Map(i32, i32).init(alloc);\n    m.put(1, 2);\n    m.put(3, 4);\n    try expect_eq(m.len(), 2);\n    try expect_eq(m.get(1).?, 2);\n    try expect_eq(m.get(3).?, 4);\n    var buf = m.alloc().alloc(4);\n    try expect_eq(buf, \"\\x00\\x00\\x00\\x00\");\n}\n",
    );
}

#[test]
fn map_init_captures_arena() {
    // `Map(K,V).init(arena)`：携带 arena——类型可见
    run_ok(
        "[test] fn t() !void {\n    var arena = Arena.init(alloc);\n    var m = Map(i32, i32).init(arena);\n    m.put(1, 2);\n    try expect_eq(@typeOf(m.alloc()), \"Arena\");\n    try expect_eq(m.get(1).?, 2);\n}\n",
    );
}

#[test]
fn map_iterates_kv_pairs() {
    // Map 句柄可遍历：`for (m) |kv|` → kv.key / kv.value（对齐 Class("Map") 遍历）
    run_ok(
        "[test] fn t() !void {\n    var m = Map(i32, i32).init(alloc);\n    m.put(10, 1);\n    m.put(20, 2);\n    var sum = 0;\n    for (m) |kv| {\n        sum += kv.value;\n    }\n    try expect_eq(sum, 3);\n}\n",
    );
}

#[test]
fn table_init_captures_alloc() {
    // `Table(T).init(alloc, rows, cols, init)`：外层 Vec 持分配器引用，grid 二维
    run_ok(
        "[test] fn t() !void {\n    var t = Table(i32).init(alloc, 2, 3, 7);\n    try expect_eq(t.len(), 2);\n    try expect_eq(t[0].len(), 3);\n    try expect_eq(t[0][1], 7);\n    try expect_eq(t[1][2], 7);\n    var buf = t.alloc().alloc(8);\n    try expect_eq(buf, \"\\x00\\x00\\x00\\x00\\x00\\x00\\x00\\x00\");\n}\n",
    );
}
