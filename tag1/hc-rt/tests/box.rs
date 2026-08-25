//! hc-rt/tests/box.rs

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
fn box_single_arg_falls_back_global_alloc() {
    // box(v) 单参 → 回退全局 alloc（设计文档 §6：`box` 的 alloc 参数显式传入；未传时回退 `alloc`）
    run_ok("[test] fn t() !void {\n    var p = box(42);\n    try expect_eq(p.*, 42);\n}\n");
}

#[test]
fn box_carries_explicit_alloc() {
    // box(v, alloc)：携带全局分配器——p.alloc() 返回它，可继续分配 8 字节
    run_ok(
        "[test] fn t() !void {\n    var p = box(42, alloc);\n    var q = p.alloc();\n    var buf = q.alloc(8);\n    try expect_eq(buf, \"\\x00\\x00\\x00\\x00\\x00\\x00\\x00\\x00\");\n}\n",
    );
}

#[test]
fn box_carries_arena() {
    // box(v, arena)：携带 arena——类型可见（Arena），且 box 不占用 arena 字节
    // （tag1 data 走 Rc，alloc 为元数据引用；真实后端按 §6 销毁 owned *I 时用它释放 data）
    run_ok(
        "[test] fn t() !void {\n    var arena = Arena.init(alloc);\n    var p = box(42, arena);\n    try expect_eq(@typeOf(p.alloc()), \"Arena\");\n    try expect_eq(p.alloc().bytes(), 0);\n}\n",
    );
}

#[test]
fn box_deref_read_write() {
    // p.* 读/写穿透到 pointee（对齐 Ptr 语义）
    run_ok(
        "[test] fn t() !void {\n    var p = box(7);\n    p.* = 9;\n    try expect_eq(p.*, 9);\n}\n",
    );
}

#[test]
fn box_auto_release_on_scope_exit() {
    // Q14：Boxed 值离开作用域自动释放（在块内装箱，块外不可用）
    run_ok(
        "[test] fn t() !void {\n\
         var outer = 0;\n\
         {\n\
             var p = box(42);\n\
             outer = p.*;\n\
         }\n\
         try expect_eq(outer, 42);\n\
         }\n",
    );
}

#[test]
fn box_unbox_returns_inner_value() {
    // unbox(box(v)) 返回内部值
    run_ok(
        "[test] fn t() !void {\n    var p = box(42);\n    var v = unbox(p);\n    try expect_eq(v, 42);\n}\n",
    );
}

#[test]
fn box_compare_with_plain_value() {
    // Boxed 与普通值比较：解引用后比较（对齐 Ptr 语义）
    run_ok("[test] fn t() !void {\n    var p = box(42);\n    try expect_eq(p, 42);\n}\n");
}

#[test]
fn box_interface_dispatch() {
    // 装箱 class → *I 胖指针：s.area() 鸭子类型分派到具体实现（Rect/Circle）
    run_ok(
        "interface IShape { fn area(self: *Self) f32; }\n\
         class Rect: IShape {\n\
             w: f32,\n\
             h: f32,\n\
             fn area(self: *Self) f32 { return self.w * self.h; }\n\
         }\n\
         class Circle: IShape {\n\
             r: f32,\n\
             fn area(self: *Self) f32 { return pi * self.r * self.r; }\n\
         }\n\
         fn total_area(shapes: &Vec<*IShape>) f32 {\n\
             var total = 0.0;\n\
             for (shapes) |s| {\n\
                 total += s.area();\n\
             }\n\
             return total;\n\
         }\n\
         [test] fn t() !void {
             var rect = Rect{ w = 3.0, h = 4.0 };
             var circ = Circle{ r = 2.0 };
             var shapes: owned Vec<*IShape> = Vec<*IShape>.init(alloc);
             defer shapes.deinit();
             shapes.append(box(rect, alloc));
             shapes.append(box(circ, alloc));
             var total = total_area(&shapes);
             try expect(total > 24.55 and total < 24.57);
         }\n",
    );
}
