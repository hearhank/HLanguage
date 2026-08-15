// 02-packages/app/main.hc — 跨包依赖演示
//
//   - app/build.zon 声明本地依赖 jsonlib（path = "../jsonlib"）
//   - using jsonlib; 平铺 pub 符号；jsonlib.parse(...) 限定访问
//   - jsonlib 的 fn secret（私有）跨包不可见

using jsonlib;

fn main(io: Io) !void {
    var n = jsonlib.parse("{}");
    io.print("jsonlib.parse = {}\n", n);
}

test fn cross_package_pub_call() !void {
    try expect_eq(jsonlib.parse("{}"), 42);
    try expect_eq(parse("{}"), 42);   // using 平铺后可直接调用
}
