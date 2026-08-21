import H.std.{io};

// 16-slices.hc — 切片与数组（12.11）
//
// Q24 定案（2026-08-13）：越界检查按模式
//   - Debug：运行时检查抛错带位置；Release：裸性能
//   - 编译期可证越界（arr[10] 对 [5]i32）：所有模式编译期报错
//
// 切片（Q6/R2）：&[T] 只读 / &mut [T] 可写；取段 &arr[1..3] / &mut arr[0..2]

fn sum_slice(s: &[i32]) i32 {
    var total = 0;
    for (s) |item| {
        total += item;
    }
    return total;
}

fn zero_out(s: &mut [i32]) void {
    for (s) |mut item| {
        item = 0;
    }
}

fn main(args: o Vec<String>) !void {
    var arr = [1, 2, 3, 4, 5];

    // 只读切片视图（不拥有数据，无 o）
    var s: &[i32] = &arr[1..3];        // [2, 3]
    io.print("slice sum = {}\n", sum_slice(s));

    // 可写切片视图（唯一写者登记）
    var s2: &mut [i32] = &mut arr[0..2];
    zero_out(s2);
    io.print("arr[0] = {}, arr[1] = {}\n", arr[0], arr[1]);

    // 越界：编译期可证 → 所有模式编译期报错
    // var v = arr[10];  // 错误（编译期，Q24）

    // 字符串字面量 = &[u8] 静态只读切片
    var msg: &[u8] = "hello";
    io.print("{}\n", msg);
}

[test] fn read_only_slice_view() !void {
    var arr = [1, 2, 3, 4, 5];
    var s: &[i32] = &arr[1..3];
    try expect_eq(s.len, 2);
    try expect_eq(sum_slice(s), 5);
}

[test] fn writable_slice() !void {
    var arr = [1, 2, 3, 4, 5];
    var s2: &mut [i32] = &mut arr[0..2];
    zero_out(s2);
    try expect_eq(arr[0], 0);
    try expect_eq(arr[1], 0);
    try expect_eq(arr[2], 3);   // 切片外不受影响
}

[test] fn string_literal_slice() !void {
    var msg: &[u8] = "hello";
    try expect_eq(msg.len, 5);
    try expect_eq_slices(msg, "hello");
}
