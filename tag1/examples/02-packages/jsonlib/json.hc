// 02-packages/jsonlib/json.hc — 依赖包：pub 边界演示
//
//   - pub fn parse：跨包可见（app 经 `using jsonlib;` / `jsonlib.parse(...)` 访问）
//   - fn secret：包内私有（无 pub，跨包不可见）

pub fn parse(json: String) i32 {
    return 42;   // tag1 简化：固定解析结果，不真正解析 JSON
}

fn secret() i32 {
    return 99;
}

test fn jsonlib_self() !void {
    try expect_eq(parse("{}"), 42);
    try expect_eq(secret(), 99);
}
