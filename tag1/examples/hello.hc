// hello.hc — H 语言 Hello World（tag1 演示）
//
// 双模式：hc run 脚本模式解释执行；hc build 生成字节码镜像 + 启动器
// 入口：fn main(io: Io) !void（io 显式传入；io.print 带格式串）
class ABC{
    pub fn print(self: *Self, io: *Io){
        io.print("引用在此打印");
    }
}
fn main(io: Io) !void {
    io.print("hello, world\n");
    io.print("H 语言：定义数据 / 修改数据 / 传输数据 / 保存数据\n");
    io.print("x = {}, y = {}\n", 42, 3.14);

    var mut abc:ABC = alloc.init(ABC);
    abc.print(&io);
}

[test] fn hello_smoke() !void {
    try expect(true);
}
