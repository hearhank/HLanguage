// 28-dangling.hc — 悬垂检测与唯一写者（运行时登记，ADR-0003/0005）
//
// 语义（已定）：
//   - *mut 强制双向注册：唯一写者由运行时登记强制（可写包含可读）
//   - *mut 不可拷贝，只能 move 传递（评审 A5）
//   - Debug：悬垂访问抛错带位置；Release：裸读（用户负责）
//   - 只读指针：Debug 默认注册、Release 默认裸读

fn main(io: Io) !void {
    // 唯一写者：同一变量同一时间最多一个 &mut（运行时登记）
    var mut x: o i32 = 42;
    var w: *mut i32 = &mut x;
    w.* = 100;
    io.print("x = {}\n", x);
    // var w2: *mut i32 = &mut x;   // 运行时错误！x 已有写者 w（Debug 抛错带位置）

    // 只读指针与可写指针可共存（读权限可降级）
    var p: *i32 = &x;
    io.print("read via p: {}\n", p.*);

    // 悬垂（Debug 演示）：块内引用指向块外将被销毁的变量
    {
        var temp: o i32 = 7;
        var d: *i32 = &temp;   // 登记到 temp
        io.print("temp = {}\n", d.*);
    }   // temp 销毁 → d 被标记悬垂
    // io.print("{}\n", d.*);  // Debug：悬垂访问抛错（携带位置）；Release：UB（用户负责）
}
