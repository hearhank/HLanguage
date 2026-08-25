import H.std.{io};

// 09-arrays.hc — 数组类型（12.3/B3/Q4/Q42）
//
//   - 一维字面量 [1, 2, 3]（Q4 推断式）
//   - 嵌套字面量 = 多维数组（Q42）：内层长度必须一致，锯齿编译期报错
//   - 数组 = 引用类型（B3）：传参走引用，复制需显式

fn sum_row(row: &[i32]) i32 {
    var total = 0;
    for (row) |v| {
        total += v;
    }
    return total;
}

fn main() !void {
    // 一维（推断式，Q4）
    var flat = [1, 2, 3];

    // 多维：嵌套字面量推断 [2][2]i32（Q42）
    var grid = [[1, 2], [3, 4]];
    io.print("grid[1][0] = {}\n", grid[1][0]);

    // 显式类型（需要时）
    var matrix: [2][2]i32 = [[5, 6], [7, 8]];

    // 长度/遍历
    io.print("flat len = {}\n", flat.len);
    io.print("row sum = {}\n", sum_row(&flat));

    // 锯齿：编译期报错（内层长度不一致；锯齿数据用 Vec<&[i32]>）
    // var bad = [[1, 2], [3, 4, 5]];  // 错误（Q42）
}

[Test] fn multi_dimensional_arrays() !void {
    var grid = [[1, 2], [3, 4]];
    try expect_eq(grid[1][0], 3);
    try expect_eq(grid.len, 2);
}

[Test] fn sum_row_test() !void {
    var flat = [1, 2, 3];
    try expect_eq(sum_row(&flat), 6);
}

[Test] fn explicit_array_type() !void {
    var matrix: [2][2]i32 = [[5, 6], [7, 8]];
    try expect_eq(matrix[0][1], 6);
}
