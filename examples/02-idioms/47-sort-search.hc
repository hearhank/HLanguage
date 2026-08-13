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
