import H.std.{io};

// 30-interface.hc — 接口（特性标）与实现
//
// Q14 定案（2026-08-13；2026-08-14 修订）：implements 标注 = 冒号后缀（已定案）
//   - class Rect: IShape { ... }（存储形态由特性标注决定，H1）
//   - 接口内 self = 实现类型的实例（接收者）；Self = 实现类型
//   - 接口方法签名：fn area(self: *Self) f32（首参即被处理的数据——函数 = 唯一处理逻辑）

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

fn main() !void {
    var rect = Rect{w = 3.0, h = 4.0};
    var circ = Circle{r = 2.0};

    // 方法调用双语：rect.area() ≡ Rect.area(&rect)
    io.print("rect area = {}\n", rect.area());
    io.print("circle area = {}\n", circ.area());
}

[test] fn interface_implementation() !void {
    var rect = Rect{w = 3.0, h = 4.0};
    var circ = Circle{r = 2.0};
    try expect(rect.area() > 11.99 and rect.area() < 12.01);   // 12
    try expect(circ.area() > 12.56 and circ.area() < 12.57);   // 4π ≈ 12.566
}
