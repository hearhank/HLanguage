import H.std.{io};

// 68-memoize.hc — 记忆化缓存（Map + optional）
//
//   - 以输入为键缓存结果；if (opt) |v| 判断命中
//   - 函数作计算器（Q43：函数自动满足 FnN）

class Memo {
    mut cache: Map<i32, i32>,

    fn get_or_compute(self: *mut Self, key: i32, compute: Fn1<i32> i32) i32 {
        if (self.cache.get(key)) |v| {
            return v;                // 命中缓存
        }
        var value = compute(key);
        self.cache.put(key, value);
        return value;
    }
}

fn slow_square(x: i32) i32 {
    return x * x;                    // 模拟慢计算（纯函数）
}

fn main() !void {
    var memo: Memo = alloc.init(Memo);   // 无参构造（C1'）

    var r1 = memo.get_or_compute(5, slow_square);
    var r2 = memo.get_or_compute(5, slow_square);   // 命中缓存
    var r3 = memo.get_or_compute(7, slow_square);   // 未命中
    io.print("{} {} {}\n", r1, r2, r3);
}

[Test] fn memoized_cache() !void {
    var memo: Memo = alloc.init(Memo);
    var r1 = memo.get_or_compute(5, slow_square);
    var r2 = memo.get_or_compute(5, slow_square);   // 命中缓存
    var r3 = memo.get_or_compute(7, slow_square);   // 未命中
    try expect_eq(r1, 25);
    try expect_eq(r2, 25);
    try expect_eq(r3, 49);
}
