// 13-struct.hc — 类型定义：class + Continuous（定义数据，2026-08-14 合并定案）
//
// Q5 定案（2026-08-13）：方法调用双语
//   - 定义：Zig 式函数成员（无 impl）
//   - 调用：p.dist(q) 与 Point.dist(p, q) 等价
//   - 接收者自动取引用：首参为 *Point 时取 &p；*mut Point 时取 &mut p
//
// Q6 定案（2026-08-13；2026-08-14 修订）：字面量构造 Point{x = 1.0, y = 2.0}
//   - 仅 Continuous 类型（值语义）保留字面量构造；大括号紧贴类型名；字段 名 = 值
//
// Q7/Q15 定案（2026-08-13；2026-08-14 H1 修订）：o 与分配器绑定
//   - 所有权管理只存在于「分配内存」时；[continuous] 连续类型（栈上）无分配 → 无 o
//   - 堆上 class（未标 continuous）需分配器 → 有所有权；装箱 box(p, alloc) → o *mut Point
//
// Q8 定案（2026-08-13）：装箱 box(p, alloc) → o *mut Point
//   - 堆内存随作用域退出自动归还给该分配器

[continuous]   // 连续内存值类型（H1 特性标注）
class Point {
    x: f32,
    y: f32,

    // 方法 = 函数成员
    fn dist(a: *Point, b: *Point) f32 {
        var dx = b.x - a.x;
        var dy = b.y - a.y;
        return sqrt(dx * dx + dy * dy);
    }
}

fn main(io: Io) !void {
    var p: Point = Point{x = 1.0, y = 2.0};
    var q: Point = Point{x = 4.0, y = 6.0};

    // 双语调用（等价）
    var d1 = p.dist(q);
    var d2 = Point.dist(p, q);
    io.print("dist = {}\n", d1);
    io.print("same = {}\n", d1 == d2);

    // 连续类型可复制（字段全为标量，自动判定）
    var p2: Point = p;
    io.print("{}\n", p2.x);

    // 装箱：值 → 堆引用（Q8，获得所有权，作用域退出自动归还）
    var hp: o *mut Point = box(p, alloc);
    hp.x = 100.0;
    io.print("{}\n", hp.x);
}

[test] fn dist_calc_and_dual_call() !void {
    var p: Point = Point{x = 1.0, y = 2.0};
    var q: Point = Point{x = 4.0, y = 6.0};
    var d1 = p.dist(q);
    var d2 = Point.dist(p, q);
    try expect(d1 > 4.99 and d1 < 5.01);   // √(3²+4²) = 5
    try expect_eq(d1 == d2, true);         // 双语等价
}

[test] fn pure_value_copy() !void {
    var p: Point = Point{x = 1.0, y = 2.0};
    var p2: Point = p;
    p2.x = 99.0;
    try expect_eq(p.x, 1.0);               // 复制互不影响
}

[test] fn boxing() !void {
    var p: Point = Point{x = 1.0, y = 2.0};
    var hp: o *mut Point = box(p, alloc);
    hp.x = 100.0;
    try expect_eq(hp.x, 100.0);
}
