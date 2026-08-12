// 元组 + 切片：C 后端验证（双后端一致性）
// h run examples/tuple_slice.hc 与 h build examples/tuple_slice.hc --exec 输出必须完全一致

fun divmod(a: u64, b: u64) -> (u64, u64) {
    return (a / b, a % b)
}

fun named() -> (x: u64, y: u64) {
    return (x: 10, y: 20)
}

fun sum(xs: []u64) -> u64 {
    if xs.len == 0 {
        return 0
    }
    return xs[0] + sum(xs[1..])
}

fun main() -> void {
    // 多返回 + 位置元组 + 解构
    (q, r) = divmod(7, 3)
    print("多返回:", q.to_str(), r.to_str())

    // 命名元组
    t = named()
    print("命名元组:", t.x.to_str(), t.y.to_str())

    // 位置元组访问 .0/.1
    p = (5, 6)
    print("位置:", p.0.to_str(), p.1.to_str())

    // 解构交换
    mut a = 1
    mut b = 2
    (a, b) = (b, a)
    print("交换:", a.to_str(), b.to_str())

    // 嵌套元组
    nest = ((1, 2), (3, 4))
    print("嵌套:", nest.0.1.to_str())

    // 单元素元组
    one = (7,)
    print("单元素:", one.0.to_str())

    // 切片：range、len、二次切片、写透
    mut arr = [1, 2, 3, 4, 5]
    mut s = arr[1..4]
    print("切片:", s, "len:", s.len.to_str())
    s[0] = 9
    print("写透:", arr)
    s2 = s[1..2]
    print("二次切片:", s2)

    // 完整切片
    full = arr[..]
    print("完整:", full, "len:", full.len.to_str())

    // 参数自动借用 [T] → []T
    print("求和:", sum(arr).to_str())

    // clone 独立
    mut c = s.clone()
    c[0] = 77
    print("clone 独立:", arr, c)

    // 元组字节化
    by = t.to_bytes()
    print("元组字节:", by)
}
