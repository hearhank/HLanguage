// 33-script.hc — 脚本生成样板（数据定义 → 序列化）——**E1 示例（第三块，最小集不实现）**
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

// 数据定义 → 生成序列化样板（就地替换本块）
script {
    var fields = types.fields("Person");   // [["name", "String"], ["age", "i32"]]
    // 遍历 fields 拼接生成（示意）：
    //   fn person_to_json(p: *Person) String { ... }
    //   fn person_from_json(data: &[u8]) !Person { ... }
    //   fn person_to_bytes(p: *Person, alloc: Allocator) o Vec(u8) { ... }
}

fn main(io: Io) !void {
    var p = alloc.init(Person{name = String.from("alice", alloc), age = 30});   // 带参构造（C1'）
    var json = person_to_json(&p);   // 脚本生成的函数（E1）
    io.print("{}\n", json);
}

[test] fn script_generation_demo() !void {
    // S4 演示型（Q-T6）：person_to_json 由脚本生成（Q23 types 元数据），
    // 示例中未展开实现；生成物验证在 M3 脚本生成测试套件中覆盖
    try expect(true);
}
