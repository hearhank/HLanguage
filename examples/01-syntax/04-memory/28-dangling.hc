// 28-dangling.hc — 悬垂检测与唯一写者（运行时登记，ADR-0003/0005）
//
// 语义（已定）：
//   - *mut 强制双向注册：唯一写者由运行时登记强制（可写包含可读）
//   - *mut 不可拷贝，只能 move 传递（评审 A5）
//   - Debug：悬垂访问抛错带位置；Release：裸读（用户负责）
//   - 只读指针：Debug 默认注册、Release 默认裸读
// Q18 定案（2026-08-13）：悬垂唯一产生路径 = 引用逃逸到比目标更长寿的容器/全局
//   （返回值引用被编译期禁止）；容器元素「取指针」不抛错，「解引用访问」才触发检测

fn fill(buf: *mut Vec(*i32), alloc: Allocator) void {
    var temp: i32 = 7;
    buf.append(&temp);      // 登记 &temp；fill 返回后 temp 销毁 → 容器内引用被标记悬垂
}

fn main(io: Io) !void {
    // 唯一写者：同一变量同一时间最多一个 &mut（运行时登记）
    var mut x: i32 = 42;
    var w: *mut i32 = &mut x;
    w.* = 100;
    io.print("x = {}\n", x);
    // var w2: *mut i32 = &mut x;   // 运行时错误！x 已有写者 w（Debug 抛错带位置）

    // 只读指针与可写指针可共存（读权限可降级）
    var p: *i32 = &x;
    io.print("read via p: {}\n", p.*);

    // 悬垂（Debug 演示）：引用逃逸进容器 → 目标销毁 → 解引用抛错
    var mut buf = Vec(*i32).init(alloc);
    fill(&mut buf, alloc);
    var d = buf[0];          // 取出悬垂引用（取指针本身不抛错）
    // io.print("{}\n", d.*);  // Debug：悬垂访问抛错（携带位置）；Release：UB（用户负责）
}

[test] fn dangling_marked_not_accessed() !void {
    // Debug：fill 返回后 temp 已销毁 → buf[0] 的引用被标记悬垂；
    // 解引用访问（d.*）会抛错带位置——本测试不触发访问（触发演示见主程序注释）
    var mut buf = Vec(*i32).init(alloc);
    fill(&mut buf, alloc);
    try expect_eq(buf.len, 1);   // 引用被登记并标记，但取出/持有不抛错
}
