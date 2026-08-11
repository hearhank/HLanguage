// class ref 字段（双向引用通知）：C 后端验证（双后端一致性）
// h run examples/ref.hc 与 h build examples/ref.hc --exec 输出必须完全一致
// 覆盖：构造连接 / 赋值连接 / 目标销毁→ref 字段自动置空（通知）/ 重赋值注销 / move 后 ref 仍有效

class Node {
    val: u64
    mut next: ref Node
}

fun main() -> void {
    mut a = Node{ val: 1 }
    mut b = Node{ val: 2, next: a }    // 构造连接：b.next 注册到 a
    print("构造连接:", b)
    {
        mut c = Node{ val: 3 }
        b.next = c                     // 赋值连接：从 a 注销，注册到 c
        print("赋值连接:", b)
    }                                  // c 随块销毁 → 通知 b.next 置空
    print("通知后:", b)
    print("直接访问失效字段:", b.next)   // 通知后 = null
    mut d = Node{ val: 4 }
    b.next = d                         // 注册到 d
    b.next = a                         // 重赋值：从 d 注销，改注册 a
    x = move a                         // 所有权转移（数据存活）→ b.next 仍有效
    print("move 后:", b)
    print("完成")
}
