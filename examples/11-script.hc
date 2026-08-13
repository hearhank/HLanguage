// 11-script.hc — 脚本生成样板（数据定义 → 序列化）
//
// Q17 定案（2026-08-13）：就地替换
//   - script { ... } 块在编译前被生成结果整体替换
//   - 编辑器分屏实时显示「脚本 ↔ 生成物」；编译时生成物参与编译
//   - 脚本 = H 核心子集（特定分配器/IO 下运行，Q13/B1）
//   - 脚本读取数据定义的机制（A4：DSL 或 AST 查询）待细化

struct Person {
    name: String,
    age: i32,
}

// 数据定义 → 生成序列化样板（就地替换本块）
script {
    // 读取上方 Person 定义（机制待细化：DSL 或 AST 查询）
    // 生成代码（本位置）：
    //   fn person_to_json(p: *Person) String { ... }
    //   fn person_from_json(data: &[u8]) !Person { ... }
    //   fn person_to_bytes(p: *Person, alloc: Allocator) o Vec(u8) { ... }
}

fn main(io: Io) !void {
    var p = Person{ name = String.from("alice", alloc), age = 30 };
    var json = person_to_json(&p);   // 脚本生成的函数
    io.print("{}\n", json);
}
