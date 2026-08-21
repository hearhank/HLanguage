import H.std.{io};

// 63-template-render.hc — 模板渲染（String 替换，52 延伸）
//
//   - 占位符替换 {{name}} → 值（replace 返回新 String）

fn render(template: &[u8], name: &[u8], age: i32) String {
    var text = String.from(template, alloc);
    text = text.replace("{{name}}", name);       // replace 返回新 String
    text = text.replace("{{age}}", fmt_int(age));
    return text;
}

fn main(args: o Vec<String>) !void {
    var tmpl = "Hello, {{name}}! You are {{age}} years old.";
    var out = render(tmpl, "alice", 30);
    io.print("{}\n", out);
}

[test] fn template_render() !void {
    var tmpl = "Hello, {{name}}! You are {{age}} years old.";
    var out = render(tmpl, "alice", 30);
    try expect(out.find("alice") != null);      // 值已替换进去
    try expect(out.find("{{name}}") == null);   // 占位符已替换
}
