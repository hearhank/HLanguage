import H.std.{io};

// 64-interface-poly.hc — 接口使用形态（Q22/Q22b 定案 2026-08-13）
//
//   - 静态路径（主）：fn describe(shape: *T) where T: IShape——单态化无虚表
//   - 异构集合：显式装箱到接口对象（虚表作为装箱一部分，非隐藏）
//   - 双路径与「方法调用双语」同精神

interface IShape {
    fn area(self: *Self) f32;
}

[continuous] class Rect: IShape {
    w: f32,
    h: f32,
    fn area(self: *Self) f32 {
        return self.w * self.h;
    }
}

[continuous] class Circle: IShape {
    r: f32,
    fn area(self: *Self) f32 {
        return pi * self.r * self.r;
    }
}

// 静态路径（主）：接口约束参数（Q22b：where 子句 + 虚拟类型，单态化无虚表）
fn describe<T>(shape: *T) f32 where T: IShape {
    return shape.area();
}

// 异构集合：接口对象（显式装箱；*Rect → *IShape 收窄待细化）
fn total_area(shapes: &Vec<*IShape>) f32 {
    var total = 0.0;
    for (shapes) |s| {
        total += s.area();
    }
    return total;
}

fn main() !void {
    var rect = Rect{w = 3.0, h = 4.0};
    var circ = Circle{r = 2.0};

    // 静态路径（单态化，无虚表）
    io.print("rect = {}\n", describe(&rect));
    io.print("circle = {}\n", describe(&circ));

    // 异构集合：显式装箱
    var shapes: owned Vec<*IShape> = Vec<*IShape>.init(alloc);
    shapes.append(box(rect, alloc));
    shapes.append(box(circ, alloc));
    io.print("total = {}\n", total_area(&shapes));
}

[test] fn static_path_monomorphization() !void {
    var rect = Rect{w = 3.0, h = 4.0};
    var circ = Circle{r = 2.0};
    try expect(describe(&rect) > 11.99 and describe(&rect) < 12.01);
    try expect(describe(&circ) > 12.56 and describe(&circ) < 12.57);
}

[test] fn heterogeneous_boxing() !void {
    var rect = Rect{w = 3.0, h = 4.0};
    var circ = Circle{r = 2.0};
    var shapes: owned Vec<*IShape> = Vec<*IShape>.init(alloc);
    shapes.append(box(rect, alloc));
    shapes.append(box(circ, alloc));
    var total = total_area(&shapes);
    try expect(total > 24.55 and total < 24.57);   // 12 + 4π ≈ 24.566
}
