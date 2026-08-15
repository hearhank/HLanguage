// 47-sort-search.hc — 切片算法（修改数据支柱）
//
//   - 原地排序：sort(&mut arr)（可写切片）
//   - 只读查找：binary_search(&arr, v) → ?usize

fn main(io: Io) !void {
    var mut arr = [5, 2, 8, 1, 9, 3];

    sort(&mut arr);                    // 可写切片（唯一写者）
    for (arr) |v| {
        io.print("{}, ", v);
    }
    io.print("\n");

    var idx = binary_search(&arr, 8);  // 只读查找 → 可选值
    io.print("index of 8 = {}\n", idx orelse -1);
}

test fn inplace_sort() !void {
    var mut arr = [5, 2, 8, 1, 9, 3];
    sort(&mut arr);
    try expect_eq(arr[0], 1);
    try expect_eq(arr[5], 9);
}

test fn binary_search_find() !void {
    var sorted = [1, 3, 5, 7, 9, 11];
    try expect_eq(binary_search(&sorted, 7).?, 3);
    try expect_eq(binary_search(&sorted, 6) orelse 0, 0);   // 未命中 → null
}
