import H.std.{io};

// test/main.hc — 项目入口（hc init 脚手架）
//
//   - 源码约定：`.hc` 文件位于包根（目录 = 包，M1.4）
//   - 测试约定：`[Test]` 标注函数与源码同文件（Q-T1）
//   - 运行：`hc run test`   测试：`hc test test`

[module] namespace HP {
    interface ISharpe {
        fn area() i32;
    }

    class Rect: ISharpe {
        mut x: i32,

        pub fn area() i32 {
            return x*x;
        }
    }
}

fn main() !void {
    io.print("hello, test111!\n");
    io.print("{}\n", 255);
    var rect = HP.Rect.new(alloc);
    var r = rect.area();
    //io.print(“{}”,100);
}

[Test] fn scaffold_smoke() !void {

    try expect_eq(1 + 1, 2);
}
