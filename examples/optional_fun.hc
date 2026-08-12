// 可选类型 ?T + 函数作为参数：双后端一致性验证
// h run examples/optional_fun.hc 与 h build examples/optional_fun.hc --exec 输出必须完全一致

fun twice(x: u64) -> u64 {
    return x * 2
}

fun find(arr: [u64], target: u64) -> ?u64 {
    for i in 0..arr.len {
        if arr[i] == target {
            return arr[i]
        }
    }
    return null
}

fun apply(f: fun(u64) -> u64, x: u64) -> u64 {
    return f(x)
}

fun show(x: ?u64) -> u64 {
    if x == null {
        return 0
    }
    return x.?
}

struct Config {
    name: Str
    timeout: ?u64
}

fun main() -> void {
    // 可选：标注 + 提升 + 置空 + 重新赋值
    mut a: ?u64 = 5
    print("可选:", a)
    a = null
    print("置空:", a)
    a = 9
    print("提升:", a)

    // 可选：判断 + 解包
    mut v: ?u64 = 3
    if v != null {
        print("解包:", v.?.to_str())
    }

    // 可选：函数返回
    arr = [1, 2, 3]
    r = find(arr, 2)
    if r != null {
        print("找到:", r.?.to_str())
    }
    r2 = find(arr, 99)
    if r2 == null {
        print("未找到")
    }

    // 可选：参数 + null
    print("参数:", show(42).to_str(), show(null).to_str())

    // 可选：struct 字段
    c = Config{ name: "srv", timeout: 30 }
    print("字段:", c.name, c.timeout)
    c2 = Config{ name: "srv2", timeout: null }
    print("字段空:", c2.timeout)

    // 可选：字节化往返
    b = c.to_bytes()
    print("字节:", b)
    c3 = Config.from_bytes(b)
    print("恢复:", c3.timeout.?.to_str())

    // 函数作为参数 + 函数值变量
    print("apply:", apply(twice, 5).to_str())
    g = twice
    print("函数值:", g(7).to_str())
    print("再传递:", apply(g, 3).to_str())
}
