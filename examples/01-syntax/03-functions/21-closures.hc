import H.std.{io};

// 21-closures.hc — 闭包与迭代
//
// Q13 定案（2026-08-13）：函数是唯一处理逻辑
//   - 接口描述功能；复杂类型 = 函数的组合；数据为中心的唯一处理机制就是函数
//   - 闭包参数类型 = 内置调用接口类型 Fn1(参数) 返回（捕获不参与签名）
//   - 闭包捕获：默认只读 |x|；可写 mut |x|；转移 move |x|

fn apply(f: Fn1(i32) i32, x: i32) i32 {
    return f(x);
}

fn main(args: o Vec(String)) !void {
    var a = 10;

    var double = |v| v * 2;              // 无环境捕获
    var add_a = |v| v + a;               // 只读捕获 a（默认）
    var mut total = 0;
    var accum = mut |v| { // 可写捕获（mut |x|）
        total += v;
        return v;
    };

    io.print("{}\n", apply(double, 5));
    io.print("{}\n", apply(add_a, 5));

    // 迭代 + 立即求值变换（12.8：变换产生新数据对象）
    var arr = [1, 2, 3, 4, 5];
    var evens = arr.iter().filter(|v| v % 2 == 0).map(|v| v * v);
    for (evens) |item| {
        io.print("{}\n", item);
    }
}

[test] fn closure_capture() !void {
    var a = 10;
    var add_a = |v| v + a;               // 只读捕获（默认）
    try expect_eq(apply(add_a, 5), 15);

    var mut total = 0;
    var accum = mut |v| { // 可写捕获
        total += v;
        return v;
    };
    try expect_eq(apply(accum, 3), 3);
    try expect_eq(total, 3);
}

[test] fn iterator_chain_eager() !void {
    var arr = [1, 2, 3, 4, 5];
    var evens = arr.iter().filter(|v| v % 2 == 0).map(|v| v * v);
    var sum = 0;
    for (evens) |item| {
        sum += item;
    }
    try expect_eq(sum, 20);   // 2² + 4² = 20
}
