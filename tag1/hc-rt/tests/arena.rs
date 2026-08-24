//! hc-rt/tests/arena.rs
//!
//! 定义：结构体：Point

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

/// 断言 main 运行期错误（错误名精确匹配）
fn run_main_err(src: &str, err_name: &str) {
    let program = hc::parse_source(src).expect("parse");
    let mut interp = Interp::new(src);
    interp.load(&program).expect("load");
    let e = interp.run_main().expect_err("应抛运行期错误");
    assert_eq!(e.name, err_name, "错误名不匹配: {}", e.message);
}

#[test]
fn arena_bump_reuses_block() {
    // 小块多次分配：同一块内 bump，块链表不增长
    run_ok(
        "[test] fn t() !void {\n    var arena = Arena.init(alloc);\n    var a = arena.alloc(16);\n    var b = arena.alloc(16);\n    var c = arena.alloc(16);\n    try expect_eq(arena.blocks(), 1);\n    try expect_eq(arena.bytes(), 48);\n}\n",
    );
}

#[test]
fn arena_grows_block_list() {
    // 单次分配超过默认块大小（1024）→ 当前块不足，申请新块
    run_ok(
        "[test] fn t() !void {\n    var arena = Arena.init(alloc);\n    var a = arena.alloc(5000);\n    try expect_eq(arena.blocks(), 1);\n    try expect_eq(arena.bytes(), 5000);\n    var b = arena.alloc(16);\n    try expect_eq(arena.blocks(), 2);\n}\n",
    );
}

#[test]
fn arena_alloc_zero_init_and_len() {
    // 分配内容零初始化、长度正确（&[u8] 与同长零串比较）
    run_ok(
        "[test] fn t() !void {\n    var arena = Arena.init(alloc);\n    var buf = arena.alloc(8);\n    try expect_eq(buf, \"\\x00\\x00\\x00\\x00\\x00\\x00\\x00\\x00\");\n}\n",
    );
}

#[test]
fn arena_distinct_regions() {
    // 相邻分配区域独立：各自零初始化、互不干扰
    // （G5 对齐后：第二块从 16 对齐处切，bytes 含对齐填充——alloc(4)+alloc(4) = 4 + 16 = 20）
    run_ok(
        "[test] fn t() !void {\n    var arena = Arena.init(alloc);\n    var a = arena.alloc(4);\n    var b = arena.alloc(4);\n    try expect_eq(a, \"\\x00\\x00\\x00\\x00\");\n    try expect_eq(b, \"\\x00\\x00\\x00\\x00\");\n    try expect_eq(arena.bytes(), 20);\n}\n",
    );
}

#[test]
fn arena_deinit_releases() {
    // deinit 批量归还全部块、重置统计
    run_ok(
        "[test] fn t() !void {\n    var arena = Arena.init(alloc);\n    var a = arena.alloc(16);\n    var b = arena.alloc(16);\n    try expect_eq(arena.blocks(), 1);\n    try expect_eq(arena.bytes(), 32);\n    arena.deinit();\n    try expect_eq(arena.blocks(), 0);\n    try expect_eq(arena.bytes(), 0);\n}\n",
    );
}

#[test]
fn arena_deinit_idempotent() {
    // deinit 幂等：二次调用安全（不重复释放）
    run_ok(
        "[test] fn t() !void {\n    var arena = Arena.init(alloc);\n    arena.deinit();\n    arena.deinit();\n    try expect_eq(arena.blocks(), 0);\n    try expect_eq(arena.bytes(), 0);\n}\n",
    );
}

#[test]
fn arena_alloc_after_deinit_errors() {
    // deinit 后 alloc → 运行期错误 ArenaDeinitialized（不可静默分配到失效内存）
    run_main_err(
        "fn main(io: Io) !void {\n    var arena = Arena.init(alloc);\n    arena.deinit();\n    var b = arena.alloc(8);\n}\n",
        "ArenaDeinitialized",
    );
}

#[test]
fn arena_oom_still_catchable() {
    // G2 回归：arena.alloc 超大（1 << 63 超 Vec 容量）→ error.OutOfMemory 可 catch
    run_ok(
        "[test] fn t() !void {\n    var arena = Arena.init(alloc);\n    var buf = arena.alloc(1 << 63) catch |err| {\n        try expect_eq(err, error.OutOfMemory);\n        return;\n    };\n}\n",
    );
}

// ---- E1：arena.init(T) typed 构造（M5.1/08 §4：按类型大小对齐后 bump + 字段默认值填充） ----

#[test]
fn arena_init_typed_default() {
    // arena.init(T)：类型名构造——字段逐默认值 + bump 记账（堆上 class = 指针宽 8）
    run_ok(
        r#"
class Node {
    mut x: i32,
    mut y: i32,
}
[test] fn t() !void {
    var arena = Arena.init(alloc);
    var node = arena.init(Node);
    try expect_eq(node.x, 0);
    try expect_eq(node.y, 0);
    try expect_eq(arena.bytes(), 8);
    try expect_eq(arena.blocks(), 1);
}
"#,
    );
}

#[test]
fn arena_init_typed_literal() {
    // arena.init(T{...})：类型字面量构造——求值即实例 + bump 记账
    run_ok(
        r#"
class Node {
    mut x: i32,
    mut y: i32,
}
[test] fn t() !void {
    var arena = Arena.init(alloc);
    var node = arena.init(Node{ x = 1, y = 2 });
    try expect_eq(node.x, 1);
    try expect_eq(node.y, 2);
    try expect_eq(arena.bytes(), 8);
    var node2 = arena.init(Node{ x = 3, y = 4 });
    try expect_eq(node2.x, 3);
    // 连续分配：第二次 bump 对齐到 16 处切，bytes 含对齐填充（8 + 16 = 24）
    try expect_eq(arena.bytes(), 24);
}
"#,
    );
}

#[test]
fn arena_init_continuous_size() {
    // 连续 class（struct）：arena.init 按布局总大小 bump（与 @sizeOf 同源）
    run_ok(
        r#"
struct Point {
    x: i32,
    y: i32,
}
[test] fn t() !void {
    var arena = Arena.init(alloc);
    var p = arena.init(Point);
    try expect_eq(p.x, 0);
    try expect_eq(p.y, 0);
    try expect_eq(@sizeOf(Point), 8);
    try expect_eq(arena.bytes(), 8);
}
"#,
    );
}

#[test]
fn arena_init_after_deinit_errors() {
    // deinit 后 init → ArenaDeinitialized（与 alloc 同规则）
    run_main_err(
        "class Node { mut x: i32 }\nfn main(io: Io) !void {\n    var arena = Arena.init(alloc);\n    arena.deinit();\n    var n = arena.init(Node);\n}\n",
        "ArenaDeinitialized",
    );
}

#[test]
fn arena_init_unknown_type_errors() {
    // 未知类型名 → UnknownType（不 bump、不静默 Void）
    run_main_err(
        "fn main(io: Io) !void {\n    var arena = Arena.init(alloc);\n    var n = arena.init(NoSuchType);\n}\n",
        "UnknownType",
    );
}
