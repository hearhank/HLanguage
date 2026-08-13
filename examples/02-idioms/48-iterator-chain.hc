// 48-iterator-chain.hc — 迭代器链与集合变换（12.8）
//
//   - 立即求值：每步变换产生新数据对象（TS 式）
//   - filter → map 链；迭代器是数据对象（可传递）

fn main(io: Io) !void {
    var scores = [92, 45, 78, 61, 88, 30];

    // 过滤 + 映射（立即求值，产生新集合）
    var passed = scores.iter().filter(|s| s >= 60).map(|s| s + 10);
    io.print("passed = {}\n", passed.len);

    // 归约
    var total = 0;
    for (passed) |s| {
        total += s;
    }
    io.print("total = {}\n", total);

    // 字符串变换（String = u8[] 别名，Q3）
    var names = ["alice", "bob", "carol"];
    var upper = names.iter().map(|n| n.to_upper());
    for (upper) |n| {
        io.print("{}\n", n);
    }
}

test "filter/map 链" {
    var scores = [92, 45, 78, 61, 88, 30];
    var passed = scores.iter().filter(|s| s >= 60).map(|s| s + 10);
    try expect_eq(passed.len, 4);   // 92,78,61,88 通过
    var total = 0;
    for (passed) |s| {
        total += s;
    }
    try expect_eq(total, 359);   // 102+88+71+98
}
