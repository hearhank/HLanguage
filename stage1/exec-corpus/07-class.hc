// 07-class.hc — 类与方法
// 覆盖：class 字段、alloc.init(ClassLit)、方法调用（无参/带参）、self 字段读写、mut 字段（C8）
// 预期 stdout：
// 2
// 7
// 5
class Counter {
    mut n: i32,
    mut step: i32,
    fn inc(self: *mut Self) void {
        self.n += self.step;
    }
    fn bump(self: *mut Self, by: i32) void {
        self.n += by;
    }
    fn get(self: *Self) i32 {
        return self.n;
    }
}
fn main() !void {
    var mut c = alloc.init(Counter{n = 0, step = 1});
    c.inc();
    c.inc();
    io.print("{}\n", c.get());
    c.bump(5);
    io.print("{}\n", c.get());
    var d = alloc.init(Counter{n = 5, step = 1});
    io.print("{}\n", d.get());
}
