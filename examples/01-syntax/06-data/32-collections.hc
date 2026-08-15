// 32-collections.hc — 集合与字符串（修改数据）
//
// Q15 定案（2026-08-13）：集合/字符串构造
//   - Vec(i32) 是 comptime 类型应用；构造 = 普通函数（12.20）
//   - 字符串字面量保持 &[u8] 静态切片；String.from(&[u8]) 显式转换（分配是显式动作）
//
// Q16 定案（2026-08-13）：所有权默认 + String = u8[] 别名（Q3；Q1' 2026-08-14 修订赋值语义）
//   - 复杂类型（分配器创建）除 Arena 外默认拥有——作用域退出自动销毁（无需显式 o/deinit）
//   - String = u8[] 别名（Q3）：引用类型、赋值 = 编译错误（共享走指针、复制走显式 copy）

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

    // String = u8[] 别名（Q3）：从静态切片显式转换；复制走显式 copy（Q1'）
    var name = String.from("hello", alloc);
    var name2 = copy(&name);   // 深复制（Q1'）：新建内存，name2 有所有权
    io.print("{}\n", name2);
}

test fn vec_append_and_iterate() !void {
    var v = Vec(i32).init(alloc);
    v.append(1);
    v.append(2);
    v.append(3);
    var sum = 0;
    for (v) |item| {
        sum += item;
    }
    try expect_eq(sum, 6);
}

test fn map_key_value_ops() !void {
    var m = Map(&[u8], i32).init(alloc);
    m.put("apple", 5);
    try expect_eq(m.get("apple").?, 5);
    try expect(m.contains("apple"));
    try expect(!m.contains("pear"));
}

test fn string_copy_owns() !void {
    var name = String.from("hello", alloc);
    var name2 = copy(&name);   // 深复制（Q1'）：新建内存、有所有权
    try expect_eq_slices(name2.as_slice(), "hello");
    try expect_eq_slices(name.as_slice(), "hello");   // 原变量不受影响
}
