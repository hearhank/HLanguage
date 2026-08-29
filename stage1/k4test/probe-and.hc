// probe-and.hc — 验证 while 条件里裸 and 连接两个比较的 AST 形态
fn main() !void {
    var mut i: i32 = 0;
    var mut j: i32 = 0;
    while (i < 3 and j < 5) {
        i += 1;
        j += 1;
    }
    io.print("{} {}\n", i,j);
}
