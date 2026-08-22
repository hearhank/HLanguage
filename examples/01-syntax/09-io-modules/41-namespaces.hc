import H.std.{io};

// 41-namespaces.hc — 命名空间与引入（12.14）
//
// Q21 定案（2026-08-13）：C# 式命名空间
//   - using Math; 引入（同包跨命名空间；跨包需 build.zon 依赖 + pub）
//   - 限定访问 Math.square(5) 与直接使用 square(5) 都可用
//   - 文件 = 物理单元（build.zon 声明包内文件）；命名空间 = 逻辑分组

using Math;

fn main() !void {
    io.print("{}\n", Math.square(5));   // 限定访问
    io.print("{}\n", square(5));        // using 后直接使用
}

[test] fn namespace_access() !void {
    try expect_eq(Math.square(5), 25);   // 限定访问
    try expect_eq(square(5), 25);        // using 后直接使用（Q21）
}

[test] fn cross_package_dep() !void {
    // 跨包依赖（Q26）：build.zon deps 带 path → pub 符号以包名 `jsonlib` 限定访问
    try expect_eq(jsonlib.parse("{}"), 42);
}
