// 05-vec.hc — Vec 动态数组
// 覆盖：Vec<T>.init、append、len、索引 []、get().? 可选解包、for 迭代（C6）
// 预期 stdout：
// 3
// 20
// 30
// 60
// 10
// 20
// 30
fn main() !void {
    var v = Vec<i32>.init(alloc);
    v.append(10);
    v.append(20);
    v.append(30);
    io.print("{}\n", v.len);
    io.print("{}\n", v[1]);
    io.print("{}\n", v.get(2).?);
    var mut sum: i32 = 0;
    for (v) |item| {
        sum += item;
    }
    io.print("{}\n", sum);
    for (v) |item| {
        io.print("{}\n", item);
    }
}
