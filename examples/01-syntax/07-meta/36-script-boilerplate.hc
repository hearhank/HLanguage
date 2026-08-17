import H.std.{io};

// 36-script-boilerplate.hc — 脚本生成样板（Q23 定案后形态，E1 就地替换）
//
//   - script 块就地替换（Q17）；脚本 = H 核心子集（B1，无运行时环境 H5）
//   - 输入机制（Q23/H5）：类型信息可见 = 所在作用域；产物 = 代码字符串
//   - 用途：数据定义 → 序列化/校验样板（Q37/Q38 定制通道）
//   - 生成函数由 `types.fields("User")` 元数据驱动，随类型定义自动更新

class User {   // 含 String 字段 → 非 Continuous（默认 class，堆上）
    name: String,
    age: i32,
    email: ?String,
}

// 数据定义 → 生成「字段统计」函数（就地替换本块）
script {
    var count = 0;
    var out = "";
    for (types.fields("User")) |f| {
        count = count + 1;
        out = out.concat("// field ").concat(f[0]).concat(": ").concat(f[1]).concat("\n");
    }
    out.concat("fn user_field_count() i32 { return ").concat(String.from(count)).concat("; }");
}

fn main(args: o Vec(String)) !void {
    var u = alloc.init(User{name = String.from("alice", alloc), age = 30, email = null});   // 带参构造（C1'）
    io.print("user_field_count = {}\n", user_field_count());
}

[test] fn script_custom_boilerplate_demo() !void {
    // S4 演示型（Q-T6）：user_field_count 由脚本生成（Q23 types 元数据），
    // 生成物验证 = 字段数随 User 定义联动（3 字段：name/age/email）
    try expect(user_field_count() == 3);
}
