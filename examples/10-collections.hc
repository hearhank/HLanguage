// 10-collections.hc — 集合与字符串（修改数据）
//
// Q15 定案（2026-08-13）：集合/字符串构造
//   - Vec(i32) 是 comptime 类型应用；构造 = 普通函数（12.20）
//   - 字符串字面量保持 &[u8] 静态切片；String.from(&[u8]) 显式转换（分配是显式动作）
//
// Q16 定案（2026-08-13）：所有权默认 + String 值语义
//   - 复杂类型（分配器创建）除 Arena 外默认拥有——作用域退出自动销毁（无需显式 o/deinit）
//   - String 默认值复制（传参深拷贝语义）

fn main(io: Io) !void {
    // Vec：构造 + 追加（非 arena 分配器 → 默认拥有，作用域退出自动销毁）
    var v = Vec(i32).init(alloc);
    v.append(1);
    v.append(2);
    v.append(3);

    // 迭代（迭代契约，12.8）
    var sum = 0;
    for (v) |item| {
        sum += item;
    }
    io.print("sum = {}\n", sum);

    // Map
    var m = Map(&[u8], i32).init(alloc);
    m.put("apple", 5);
    io.print("apple = {}\n", m.get("apple").?);

    // String：从静态切片显式转换（分配 + 复制）；值复制语义
    var name = String.from("hello", alloc);
    var name2 = name;   // 值复制（深拷贝语义）
    io.print("{}\n", name2);
}
