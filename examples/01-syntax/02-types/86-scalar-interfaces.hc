import H.std.{io};

// 86-scalar-interfaces.hc — 标量类型体系（ICompare / INumber 族，2026-08-14 定案）
//
//   - 内建标量（i32/u64/f64 等）实现数字接口族：IInt / IUint / IFloat → INumber : ICompare
//   - 运算符绑定：a + b ≡ a.add(b)（方法 = 运算符的显式形式）
//   - == 值比较，内部调用 ICompare（H3）：a == b ≡ a.eq(b)；序比较绑定 ICompare
//   - 泛型约束 where T: INumber 可作用于所有标量；元组 = 多值返回

// 泛型约束：任意数字标量求和（IInt/IUint/IFloat 均满足 INumber）
fn sum<T>(items: &[T]) T where T: INumber {
    var total = items[0];
    for (items[1..]) |v| {
        total = total.add(v);   // ≡ total + v
    }
    return total;
}

// 多值返回：元组（2026-08-14 定案）
fn divmod(a: i32, b: i32) (i32, i32) {
    return (a / b, a % b);
}

fn main() !void {
    // 方法形式（a.add(b) ≡ a + b，双向一致）
    var a: i32 = 7;
    var b: i32 = 5;
    io.print("{} {} {}\n", a.add(b), a.sub(b), a.mul(b));   // 12 2 35
    io.print("{} {}\n", a.div(b), a.neg());                 // 1 -7

    // 比较：eq/lt 来自 ICompare（INumber 继承）；== 值比较内部调用 eq（H3）
    io.print("{} {}\n", a.eq(b), a.lt(b));                  // false false
    io.print("{} {}\n", a == b, a < b);                     // false false

    // 泛型约束：INumber 族作用于标量
    var ints = [10, 20, 30];
    io.print("int sum = {}\n", sum(&ints));                 // 60
    var floats = [1.5, 2.5, 3.0];
    io.print("float sum = {}\n", sum(&floats));             // 7.0

    // 子接口方法：IInt 追加 mod/abs
    io.print("{} {}\n", a.mod(b), (-7).abs());              // 2 7

    // 多值返回 + 解构（_ 占位符放弃值）
    var (q, r) = divmod(17, 5);
    io.print("{} {}\n", q, r);                              // 3 2

    // 标量装箱：接口 = 类型标注（*INumber 只读引用 / *mut INumber 可写引用）
    var hp: *INumber = box(a, alloc);
    io.print("{}\n", hp.add(b));                            // 12（动态分发）
}

[test] fn scalar_methods() !void {
    var a: i32 = 7;
    var b: i32 = 5;
    try expect_eq(a.add(b), 12);
    try expect_eq(a.sub(b), 2);
    try expect_eq(a.mul(b), 35);
    try expect_eq(a.div(b), 1);
    try expect_eq(a.neg(), -7);
    try expect_eq(a.eq(b), false);   // ICompare 继承方法
    try expect_eq(a.lt(b), false);
    try expect_eq(a == b, false);    // 运算符形式（通用相等）
    try expect_eq(a.mod(b), 2);
    try expect_eq((-7).abs(), 7);
}

[test] fn generic_sum_over_numbers() !void {
    var ints = [10, 20, 30];
    try expect_eq(sum(&ints), 60);
    var floats = [1.5, 2.5, 3.0];
    try expect_eq(sum(&floats), 7.0);
}

[test] fn tuple_multi_return() !void {
    var (q, r) = divmod(17, 5);
    try expect_eq(q, 3);
    try expect_eq(r, 2);
}
