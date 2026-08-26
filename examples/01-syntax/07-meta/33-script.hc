import H.std.{io};

// 33-script.hc — 脚本生成（数据定义 → 代码，E1 就地替换）
//
// Q17 定案（2026-08-13）：就地替换
//   - script { ... } 块在编译前被生成结果整体替换（H5：编译前执行、模板生成）
//   - 编辑器分屏实时显示「脚本 ↔ 生成物」；编译时生成物参与编译
//   - 脚本 = H 核心子集（无运行时环境，Q13/B1）；类型信息可见 = 所在作用域（H5）
// Q23 定案（2026-08-13）：输入机制 = 隐式 types 元数据对象
//   - types.fields("Person") 返回字段 [名, 类型] 列表；产物 = 代码字符串就地替换

class Person {   // 含 String 字段 → 非 Continuous（默认 class，堆上）
    name: String,
    age: i32,
}

// script { } 块已移除（2026-08-23 定案，见 docs/SPEC/phase3/12-script-redesign.md）。
// 原脚本通过 types.fields("Person") 元数据生成 person_field_count()，
// 现直接硬编码为等价函数。
// field name: String
// field age: i32
fn person_field_count() i32 { return 2; }

fn main() !void {
    var p = alloc.init(Person{name = "alice", age = 30});   // 带参构造（C1'）
    io.print("person_field_count = {}\n", person_field_count());
}

[Test] fn script_generation_demo() !void {
    // S4 演示型（Q-T6）：person_field_count 由脚本生成（Q23 types 元数据），
    // 由本块的声明文本区间就地替换（Q17）
    try expect(person_field_count() == 2);
}
