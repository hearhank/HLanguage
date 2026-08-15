// 59-pipeline.hc — 数据管道（读 → 变换 → 写）
//
//   - 函数链：数据流经管道（每步一个函数）
//   - 综合：io.fs / 迭代器链 / parse

fn read_numbers(io: *T, path: &[u8]) !Vec(i32) where T: Io {
    var data = try io.fs.read_file(path, alloc);
    var nums = Vec(i32).init(alloc);
    var parts = String.from(data, alloc).split(',');
    for (parts) |p| {
        var n = parse_int(p) orelse continue;
        nums.append(n);
    }
    return nums;
}

fn transform(nums: &Vec(i32)) Vec(i32) {
    // 立即求值变换（12.8）
    return nums.iter().filter(|n| n % 2 == 0).map(|n| n * 10);
}

fn main(io: Io) !void {
    // 管道：读 → 变换 → 汇总
    var nums = try read_numbers(&io, "data.txt");
    var evens = transform(&nums);

    var sum = 0;
    for (evens) |n| {
        sum += n;
    }
    io.print("even*10 sum = {}\n", sum);
}

test fn data_pipeline_transform() !void {
    var nums = Vec(i32).init(alloc);
    nums.append(1);
    nums.append(2);
    nums.append(3);
    nums.append(4);
    var evens = transform(&nums);
    var sum = 0;
    for (evens) |n| {
        sum += n;
    }
    try expect_eq(sum, 60);   // (2 + 4) × 10
}
