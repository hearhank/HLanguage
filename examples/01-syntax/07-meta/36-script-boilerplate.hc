// 36-script-boilerplate.hc — 脚本生成样板（Q23 定案后形态）
//
//   - script 块就地替换（Q17）；脚本 = H 核心子集（B1）
//   - 输入机制（Q23）：隐式 types 元数据对象；产物 = 代码字符串
//   - 用途：数据定义 → 序列化/校验样板（Q37/Q38 定制通道）

struct User {
    name: String,
    age: i32,
    email: ?String,
}

script {
    var fields = types.fields("User");
    // 遍历 fields 拼接生成（示意）：
    //   fn user_to_json(u: *User) String { ... }       // 定制：字段映射
    //   fn user_validate(u: *User) !void { ... }       // 校验：age >= 0
    //   fn user_to_bytes(u: *User) o Vec(u8) { ... }   // 递归序列化（含 String 字段）
}

fn main(io: Io) !void {
    var u = User{ name = String.from("alice", alloc), age = 30, email = null };

    try user_validate(&u);         // 脚本生成的校验
    var json = user_to_json(&u);   // 脚本生成的 JSON（定制字段）
    io.print("{}\n", json);
}

test "script 定制样板（演示）" {
    // S4 演示型（Q-T6）：user_to_json/user_validate 由脚本生成（Q23），
    // 示例中未展开实现；生成物验证在 M3 脚本生成测试套件中覆盖
    try expect(true);
}
