import H.std.{io};

// 15-pointers.hc — 指针与引用（& / &mut / *T / *mut T）
//
// Q10 定案（2026-08-13）：引用 = 指针（合一）
//   - &x 产生 *T：无 Rust 式「安全引用 vs 裸指针」之分，无 unsafe 关键字
//   - 安全性由运行时登记统一承担（双向注册、Debug 悬垂检测）

fn main(args: o Vec(String)) !void {
    var mut x: i32 = 42;
    var p: *i32 = &x;           // 只读指针（& 对只读/可写变量均合法，读权限可降级）
    var w: *mut i32 = &mut x;   // 可写指针（&mut 仅对 var mut 变量合法）

    io.print("{} {}\n", p.*, w.*);   // 显式解引用取值
    w.* = 100;                       // 解引用赋值（通过可写指针）
    io.print("{}\n", x);

    // 自动解引用字段/索引访问（评审 A3）：p.x、s[i]
    var arr = [1, 2, 3];
    var sp: *[3]i32 = &arr;
    io.print("{}\n", sp[1]);

    // 指针自由（Q-S11）：多个 &mut 可同时存在（唯一写者概念取消，指针问题用户负责）
    // var w2: *mut i32 = &mut x;  // 合法（无唯一写者限制）
}

[test] fn pointer_read_write_downgrade() !void {
    var mut x: i32 = 42;
    var p: *i32 = &x;
    var w: *mut i32 = &mut x;
    w.* = 100;
    try expect_eq(p.*, 100);   // 读权限可降级：& 与 &mut 共存
    try expect_eq(x, 100);
}

[test] fn auto_deref_index() !void {
    var arr = [1, 2, 3];
    var sp: *[3]i32 = &arr;
    try expect_eq(sp[1], 2);
}
