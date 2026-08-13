// 30-interface.hc — 接口（特性标）与实现
//
// Q14 定案（2026-08-13）：
//   - implements 标注 = 冒号后缀：class Rect: Shape { ... }（struct 同样适用）
//   - 接口内 self = 实现类型的实例（接收者）；Self = 实现类型
//   - 接口方法签名：fn area(self: *Self) f32（首参即被处理的数据——函数 = 唯一处理逻辑）

interface Shape {
    fn area(self: *Self) f32;
}

struct Rect: Shape {
    w: f32,
    h: f32,

    fn area(self: *Self) f32 {
        return self.w * self.h;
    }
}

struct Circle: Shape {
    r: f32,

    fn area(self: *Self) f32 {
        return pi * self.r * self.r;
    }
}

fn main(io: Io) !void {
    var rect = Rect{ w = 3.0, h = 4.0 };
    var circ = Circle{ r = 2.0 };

    // 方法调用双语：rect.area() ≡ Rect.area(&rect)
    io.print("rect area = {}\n", rect.area());
    io.print("circle area = {}\n", circ.area());
}
