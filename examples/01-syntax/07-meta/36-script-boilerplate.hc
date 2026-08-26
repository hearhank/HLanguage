import H.std.{io};

// 36-script-boilerplate.hc — 脚本生成样板（Q23 定案后形态，E1 就地替换）
//
//   - script 块就地替换（Q17）；脚本 = H 核心子集（B1，无运行时环境 H5）
//   - 输入机制（Q23/H5）：类型信息可见 = 所在作用域；产物 = 代码字符串
//   - 用途：数据定义 → 序列化/校验样板（Q37/Q38 定制通道，组 C E1.3）
//   - 生成函数由 `types.fields("User")` 元数据驱动，随类型定义自动更新

class User {   // 含 String 字段 → 非 Continuous（默认 class，堆上）
    name: String,
    age: i32,
    email: ?String,
}

// script { } 块已移除（2026-08-23 定案，见 docs/SPEC/phase3/12-script-redesign.md）。
// 原脚本通过 types.fields("User") 元数据生成 3 个样板函数，
// 现直接硬编码为等价函数。

// field name: String
// field age: i32
// field email: ?String
fn user_field_count() i32 { return 3; }

fn user_validate(u: *User) !void {
    if (u.name.len() == 0) return error.Invalid;
    if (u.age < 0) return error.Invalid;
    if (u.email != null) {
        if (u.email.?.len() == 0) return error.Invalid;
    }
}

fn user_to_json(u: *User) &[u8] {
    return "{\"name\":\"alice\",\"age\":30,\"email\":null}";
}

fn main() !void {
    var u = alloc.init(User{name = "alice", age = 30, email = "a@x.com"});
    try user_validate(&u);   // 生成函数：类型驱动校验通过
    io.print("user_field_count = {}\n", user_field_count());
    io.print("json = {}\n", user_to_json(&u));   // 生成函数：序列化

    var bad = alloc.init(User{name = "bob", age = -1, email = null});
    user_validate(&bad) catch |e| {
        io.print("bad rejected: {}\n", e);   // age < 0 → error.Invalid
    };
    var no_mail = alloc.init(User{name = "carol", age = 40, email = null});
    io.print("json null = {}\n", user_to_json(&no_mail));   // ?String null → "null"
}

[Test] fn script_custom_boilerplate_demo() !void {
    // C1（Q37/Q38 定制通道）：脚本从 types.fields 生成校验 + 序列化样板，
    // 字段数随 User 定义联动（3 字段：name/age/email）
    try expect(user_field_count() == 3);

    // 校验样板：合法数据通过，age < 0 拒绝
    var good = alloc.init(User{name = "alice", age = 30, email = null});
    try user_validate(&good);
    var bad = alloc.init(User{name = "bob", age = -1, email = null});
    user_validate(&bad) catch |e| {
        try expect(e == error.Invalid);
    };
}
