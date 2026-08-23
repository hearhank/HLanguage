//! G5/mem：对齐保证与 Debug 泄漏检测（`08-mem-allocator-design.md` §2.3 / §8.3 定案落地）
//!
//! 覆盖（tree-walking interp）：
//! - **对齐（§2.3）**：Arena bump 切出前游标圆整到 16 字节（`ALLOC_ALIGN`），返回区域
//!   起始相对块起点恒为 16 的倍数；对齐填充计入 `arena.bytes()`（真实 bump 语义）
//! - **泄漏检测（§8.3）**：全局 `alloc.alloc(n)` 登记分配记录（大小 + 调用行号），
//!   值销毁（作用域退出自动销毁）后弱引用失效自动视为释放；
//!   `alloc.leaks()` 当前活跃数、`alloc.leak_report()` 清单文本

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
fn arena_bump_aligned_to_16() {
    // 对齐（§2.3）：连续小分配每次从 16 对齐处切——alloc(1)+alloc(1)+alloc(16) 游标推进
    // 0 → 1 → 16 → 17 → 32 → 48；bytes 含对齐填充（真实 bump 语义）
    run_ok(
        "[test] fn t() !void {\n    var arena = Arena.init(alloc);\n    var a = arena.alloc(1);\n    var b = arena.alloc(1);\n    var c = arena.alloc(16);\n    try expect_eq(arena.bytes(), 48);\n}\n",
    );
}

#[test]
fn arena_aligned_region_distinct() {
    // 对齐后区域互不干扰：alloc(1) 后 alloc(16) 从 16 对齐处切（跳过对齐填充），各自零初始化
    run_ok(
        "[test] fn t() !void {\n    var arena = Arena.init(alloc);\n    var a = arena.alloc(1);\n    var b = arena.alloc(16);\n    try expect_eq(a, \"\\x00\");\n    try expect_eq(b, \"\\x00\\x00\\x00\\x00\\x00\\x00\\x00\\x00\\x00\\x00\\x00\\x00\\x00\\x00\\x00\\x00\");\n    try expect_eq(arena.bytes(), 32);\n}\n",
    );
}

#[test]
fn alloc_leaks_tracks_active_and_release() {
    // 泄漏检测（§8.3）：`alloc.alloc(n)` 登记；作用域退出自动销毁 → 弱引用失效 → 注销
    run_ok(
        "[test] fn t() !void {\n    var n0 = alloc.leaks();\n    {\n        var buf = alloc.alloc(8);\n        try expect_eq(alloc.leaks(), n0 + 1);\n    }\n    try expect_eq(alloc.leaks(), n0);\n}\n",
    );
}

#[test]
fn alloc_leak_report_lists_size_and_line() {
    // 泄漏检测（§8.3）：`alloc.leak_report()` 输出清单——大小 + 调用行号（第 2 行）
    run_ok(
        "[test] fn t() !void {\n    var buf = alloc.alloc(8);\n    try expect_eq(alloc.leak_report(), \"leak: line 2: 8 bytes\\n\");\n}\n",
    );
}

#[test]
fn alloc_multiple_leaks_tracked() {
    // 多笔分配分别登记；报告按分配顺序列出（两次 alloc 各一行）
    run_ok(
        "[test] fn t() !void {\n    var a = alloc.alloc(4);\n    var b = alloc.alloc(16);\n    try expect_eq(alloc.leaks(), 2);\n    try expect_eq(alloc.leak_report(), \"leak: line 2: 4 bytes\\nleak: line 3: 16 bytes\\n\");\n}\n",
    );
}
