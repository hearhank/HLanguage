// 04-struct.hc — struct 与方法（定义数据）
//
// Q5 定案（2026-08-13）：方法调用双语
//   - 定义：Zig 式函数成员（无 impl）
//   - 调用：p.dist(q) 与 Point.dist(p, q) 等价
//   - 接收者自动取引用：首参为 *Point 时取 &p；*mut Point 时取 &mut p
//
// Q6 定案（2026-08-13）：struct 字面量 Point{x = 1.0, y = 2.0}
//   - 大括号紧贴类型名；字段 名 = 值（Zig 式赋值风格，无点号前缀）；空结构 Point{}
//
// Q7 定案（2026-08-13）：o 与分配器绑定
//   - 所有权管理只存在于「分配内存」时；值类型（栈上）无分配 → 无 o
//   - 强制转引用类型（默认分配器分配堆内存、值写入堆）→ 获得所有权（o 逻辑）
//
// Q8 定案（2026-08-13）：装箱 box(p, alloc) → o *mut Point
//   - 堆内存随作用域退出自动归还给该分配器

struct Point {
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

    // 纯值 struct 可复制（字段全为标量）
    var p2: Point = p;
    io.print("{}\n", p2.x);

    // 装箱：值 → 堆引用（Q8，获得所有权，作用域退出自动归还）
    var hp: o *mut Point = box(p, alloc);
    hp.x = 100.0;
    io.print("{}\n", hp.x);
}
