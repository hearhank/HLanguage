// probe-vec.hc — C6.1 验收：Vec init/append/len/Index/for
fn main() !void {
    var v = Vec<i32>.init(alloc);
    v.append(10);
    v.append(20);
    v.append(30);
    io.print("{}\n", v.len);
    io.print("{}\n", v[1]);
    var mut sum: i32 = 0;
    for (v) |item| {
        sum += item;
    }
    io.print("{}\n", sum);
}
