import H.std.{io};

// 60-binary-search.hc — 二分查找（递归 + 切片只读访问）
//
//   - 递归 + 切片（&[i32]，只读）
//   - 返回 ?usize（未命中 null）

fn binary_search(data: &[i32], target: i32) ?usize {
    return bs(data, target, 0, data.len);
}

fn bs(data: &[i32], target: i32, lo: usize, hi: usize) ?usize {
    if (lo >= hi) {
        return null;
    }
    var mid = lo + (hi - lo) / 2;
    if (data[mid] == target) {
        return mid;
    }
    if (data[mid] > target) {
        return bs(data, target, lo, mid);
    }
    return bs(data, target, mid + 1, hi);
}

fn main() !void {
    var sorted = [1, 3, 5, 7, 9, 11];
    io.print("find 7: {}\n", binary_search(&sorted, 7) orelse -1);
    io.print("find 6: {}\n", binary_search(&sorted, 6) orelse -1);
}

[test] fn binary_search_hit_miss() !void {
    var sorted = [1, 3, 5, 7, 9, 11];
    try expect_eq(binary_search(&sorted, 7).?, 3);
    try expect_eq(binary_search(&sorted, 1).?, 0);
    try expect_eq(binary_search(&sorted, 11).?, 5);
    try expect_eq(binary_search(&sorted, 6) orelse 0, 0);
    try expect_eq(binary_search(&sorted, 12) orelse 0, 0);
}
