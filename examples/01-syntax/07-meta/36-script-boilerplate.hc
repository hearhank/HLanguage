// 36-script-boilerplate.hc — 脚本生成样板（A4 机制定前的形态）
//
//   - script 块就地替换（Q17）；脚本 = H 核心子集（B1）
//   - 输入机制（A4：DSL 或 AST 查询）待定——此处示形态
//   - 用途：数据定义 → 序列化/校验样板（Q37/Q38 定制通道）

struct User {
    name: String,
    age: i32,
    email: ?String,
}

script {
    // 读取上方 User 定义（A4 机制待定）
    // 生成（本位置）：
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
