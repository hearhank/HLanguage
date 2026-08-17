import H.std.{io};
import jsonlib.{parse};

// 02-packages/app/main.hc — 跨包依赖演示
//
//   - app/build.zon 声明本地依赖 jsonlib（path = "../jsonlib"）
//   - import jsonlib.{parse}; 符号选择导入（ADR-0010 取代 using）；
//     jsonlib.parse(...) 限定访问
//   - jsonlib 的 fn secret（私有）跨包不可见

fn main(args: o Vec(String)) !void {
    var n = parse("{}");
    io.print("jsonlib.parse = {}\n", n);
}

[test] fn cross_package_pub_call() !void {
    try expect_eq(parse("{}"), 42);
}
