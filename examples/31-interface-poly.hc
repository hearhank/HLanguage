// 31-interface-poly.hc — 接口使用形态（Q33 定案 2026-08-13）
//
//   - 静态路径（主）：fn describe(shape: anytype)——comptime 检查实现 Shape，单态化无虚表
//   - 异构集合：显式装箱到接口对象（虚表作为装箱一部分，非隐藏）
//   - 双路径与「方法调用双语」同精神

interface Shape {
    fn area(self: *Self) f32;
}

struct Rect: Shape {
    w: f32,
    h: f32,
    fn area(self: *Self) f32 { return self.w * self.h; }
}

struct Circle: Shape {
    r: f32,
    fn area(self: *Self) f32 { return pi * self.r * self.r; }
}

// 静态路径：comptime 约束（anytype + Shape 形状检查；bound 语法待细化）
fn describe(shape: anytype) f32 {
    return shape.area();
}

// 异构集合：接口对象（显式装箱；*Rect → *Shape 收窄待细化）
fn total_area(shapes: &Vec(*Shape)) f32 {
    var total = 0.0;
    for (shapes) |s| {
        total += s.area();
    }
    return total;
}

fn main(io: Io) !void {
    var rect = Rect{ w = 3.0, h = 4.0 };
    var circ = Circle{ r = 2.0 };

    // 静态路径（单态化，无虚表）
    io.print("rect = {}\n", describe(rect));
    io.print("circle = {}\n", describe(circ));

    // 异构集合：显式装箱
    var shapes: o Vec(*Shape) = Vec(*Shape).init(alloc);
    shapes.append(box(rect, alloc));
    shapes.append(box(circ, alloc));
    io.print("total = {}\n", total_area(&shapes));
}
