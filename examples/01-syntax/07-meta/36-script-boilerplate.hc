// 36-script-boilerplate.hc — 脚本生成样板（Q23 定案后形态）——**E1 示例（第三块，最小集不实现）**
//
//   - script 块就地替换（Q17）；脚本 = H 核心子集（B1，无运行时环境 H5）
//   - 输入机制（Q23/H5）：类型信息可见 = 所在作用域；产物 = 代码字符串
//   - 用途：数据定义 → 序列化/校验样板（Q37/Q38 定制通道）

class User {   // 含 String 字段 → 非 Continuous（默认 class，堆上）
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
    var u = alloc.init(User{name = String.from("alice", alloc), age = 30, email = null});   // 带参构造（C1'）

    try user_validate(&u);         // 脚本生成的校验（E1）
    var json = user_to_json(&u);   // 脚本生成的 JSON（定制字段，E1）
    io.print("{}\n", json);
}

test fn script_custom_boilerplate_demo() !void {
    // S4 演示型（Q-T6）：user_to_json/user_validate 由脚本生成（Q23），
    // 示例中未展开实现；生成物验证在 M3 脚本生成测试套件中覆盖
    try expect(true);
}
