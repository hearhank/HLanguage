// 15-pointers.hc — 指针与引用（& / &mut / *T / *mut T）
//
// Q10 定案（2026-08-13）：引用 = 指针（合一）
//   - &x 产生 *T：无 Rust 式「安全引用 vs 裸指针」之分，无 unsafe 关键字
//   - 安全性由运行时登记统一承担（双向注册、Debug 悬垂检测）

fn main(io: Io) !void {
    var mut x: o i32 = 42;
    var p: *i32 = &x;           // 只读指针（& 对只读/可写变量均合法，读权限可降级）
    var w: *mut i32 = &mut x;   // 可写指针（&mut 仅对 var mut 变量合法）

    io.print("{} {}\n", p.*, w.*);   // 显式解引用取值
    w.* = 100;                       // 解引用赋值（通过可写指针）
    io.print("{}\n", x);

    // 自动解引用字段/索引访问（评审 A3）：p.x、s[i]
    var arr = [1, 2, 3];
    var sp: *[3]i32 = &arr;
    io.print("{}\n", sp[1]);

    // 唯一写者：同一变量同一时间最多一个 &mut（运行时登记）
    // var w2: *mut i32 = &mut x;  // 运行时错误！x 已有写者 w（携带位置）
}
