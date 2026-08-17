//! G1/mem：Arena 真实内存管理（bump + 块链表 + deinit 批量归还 + 统计）
//!
//! 覆盖设计文档 `08-mem-allocator-design.md` §4 G1 差距落地：
//! - 小块多次分配复用同一块（bump，不逐次开块）
//! - 单次分配超过默认块大小（1024）→ 块链表增长
//! - 分配内容零初始化、长度正确、相邻区域互不干扰
//! - `deinit` 批量归还 backing（blocks/bytes 归零）、幂等
//! - deinit 后 alloc → 运行期错误 `ArenaDeinitialized`
//! - OOM（`1 << 63` 超容量）仍返回可 catch 的 `error.OutOfMemory`

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
