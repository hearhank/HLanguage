// 标识符形态 + @ 内置名
fn main(args) !void {
    var snake_case_name = 0;
    var CamelCaseName = 1;
    var _leading_under = 2;
    var a1b2 = 3;
    var abc变量 = 4;        // ASCII ident 后接 CJK → 整体一个 Ident（is_alphanumeric）
    var PascalCase，如 = 5;  // 全角逗号（U+FF0C）非字母数字 → 断开 ident，逗号与「如」各报错
    io.print("{}\n", @sizeOf(i32));
    io.print("{}\n", @alignOf(u8));
    io.print("{}\n", @offsetOf(Point, x));
    io.print("{}\n", @hasField(Point, "y"));
    var v = @intCast(i32, x);
}
