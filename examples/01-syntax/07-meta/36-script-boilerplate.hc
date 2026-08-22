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

// 数据定义 → 生成「字段统计 + 校验 + 序列化」样板（就地替换本块）
//
// 校验规则（类型驱动）：String → 非空；i32 → >= 0；?String → null 允许、非空时非空串。
// 序列化规则：String 带引号；i32 裸值（fmt_int）；?String → null 输出 `null`。
script {
    var count = 0;
    var out = "";

    // 1) 字段统计（B2 示例：类型元数据驱动函数生成）
    var count_out = "";
    for (types.fields("User")) |f| {
        count = count + 1;
        count_out = count_out.concat("// field ").concat(f[0]).concat(": ").concat(f[1]).concat("\n");
    }
    out = out.concat(count_out);
    out = out.concat("fn user_field_count() i32 { return ").concat(String.from(count)).concat("; }");
    out = out.concat("\n");

    // 2) 校验样板
    var v = "fn user_validate(u: *User) !void {\n";
    for (types.fields("User")) |f| {
        if (f[1] == "String") {
            v = v.concat("    if (u.").concat(f[0]).concat(".len() == 0) return error.Invalid;\n");
        } else if (f[1] == "i32") {
            v = v.concat("    if (u.").concat(f[0]).concat(" < 0) return error.Invalid;\n");
        } else if (f[1] == "?String") {
            v = v.concat("    if (u.").concat(f[0]).concat(" != null) {\n");
            v = v.concat("        if (u.").concat(f[0]).concat(".?.len() == 0) return error.Invalid;\n");
            v = v.concat("    }\n");
        }
    }
    v = v.concat("}\n");
    out = out.concat(v);

    // 3) to_json 样板（字段分隔 `, `；String 带引号；i32 裸值；?String → null / 带引号）
    var j = "fn user_to_json(u: *User, alloc: Allocator) String {\n";
    j = j.concat("    var out = \"{\";\n");
    var first = true;
    for (types.fields("User")) |f| {
        var sep = "";
        if (first) {
            first = false;
        } else {
            sep = ", ";
        }
        if (f[1] == "String") {
            j = j.concat("    out = out.concat(\"")
                .concat(sep)
                .concat("\\\"")
                .concat(f[0])
                .concat("\\\": \\\"\").concat(u.")
                .concat(f[0])
                .concat(").concat(\"\\\"\");\n");
        } else if (f[1] == "i32") {
            j = j.concat("    out = out.concat(\"")
                .concat(sep)
                .concat("\\\"")
                .concat(f[0])
                .concat("\\\": \").concat(fmt_int(u.")
                .concat(f[0])
                .concat("));\n");
        } else if (f[1] == "?String") {
            j = j.concat("    if (u.").concat(f[0]).concat(" != null) {\n");
            j = j.concat("        out = out.concat(\"")
                .concat(sep)
                .concat("\\\"")
                .concat(f[0])
                .concat("\\\": \\\"\").concat(u.")
                .concat(f[0])
                .concat(".?).concat(\"\\\"\");\n");
            j = j.concat("    } else {\n");
            j = j.concat("        out = out.concat(\"")
                .concat(sep)
                .concat("\\\"")
                .concat(f[0])
                .concat("\\\": null\");\n");
            j = j.concat("    }\n");
        }
    }
    j = j.concat("    out = out.concat(\"}\");\n");
    j = j.concat("    return out;\n");
    j = j.concat("}\n");
    out.concat(j);
}

fn main() !void {
    var u = alloc.init(User{name = "alice", age = 30, email = "a@x.com"});
    try user_validate(&u);   // 生成函数：类型驱动校验通过
    io.print("user_field_count = {}\n", user_field_count());
    io.print("json = {}\n", user_to_json(&u, alloc));   // 生成函数：序列化

    var bad = alloc.init(User{name = "bob", age = -1, email = null});
    user_validate(&bad) catch |e| {
        io.print("bad rejected: {}\n", e);   // age < 0 → error.Invalid
    };
    var no_mail = alloc.init(User{name = "carol", age = 40, email = null});
    io.print("json null = {}\n", user_to_json(&no_mail, alloc));   // ?String null → "null"
}

[test] fn script_custom_boilerplate_demo() !void {
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
