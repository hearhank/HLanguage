// 33-script.hc — 脚本生成样板（数据定义 → 序列化）
//
// Q17 定案（2026-08-13）：就地替换
//   - script { ... } 块在编译前被生成结果整体替换
//   - 编辑器分屏实时显示「脚本 ↔ 生成物」；编译时生成物参与编译
//   - 脚本 = H 核心子集（特定分配器/IO 下运行，Q13/B1）
// Q23 定案（2026-08-13）：输入机制 = 隐式 types 元数据对象
//   - types.fields("Person") 返回字段 [名, 类型] 列表；产物 = 代码字符串就地替换

struct Person {
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
    var p = Person{ name = String.from("alice", alloc), age = 30 };
    var json = person_to_json(&p);   // 脚本生成的函数
    io.print("{}\n", json);
}
