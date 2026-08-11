// 动态块 [T]：C 后端验证（双后端一致性）
// h run examples/array.h 与 h build examples/array.h --exec 输出必须完全一致
// [T] 在 C 中 = 连续数据区 + 长度（动态块三件套的 C 兑现）

struct Scores {
    values: [f64]
    label: Str
}

fun total(s: Scores) -> f64 {
    a = s.values[0]
    b = s.values[1]
    c = s.values[2]
    return a + b + c
}

fun main() -> void {
    s = Scores{ values: [1.5, 2.5, 3.0], label: "成绩" }
    print("总数:", total(s))
    print("长度:", s.values.len)
    print("首个:", s.values[0])
    if s.values[1] > s.values[0] {
        print("第二个更大")
    }
    nums = [10, 20, 30]
    print("整数数组:", nums[0] + nums[1] + nums[2])
}
