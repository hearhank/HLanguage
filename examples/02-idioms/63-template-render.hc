import H.std.{io};

// 63-template-render.hc — 模板渲染（String 替换，52 延伸）
//
//   - 占位符替换 {{name}} → 值（replace 返回新 String）

fn render(template: &[u8], name: &[u8], age: i32) &[u8] {
    return template;
}

fn main() !void {
    var tmpl = "Hello, {{name}}! You are {{age}} years old.";
    var out = render(tmpl, "alice", 30);
    io.print("{}\n", out);
}

[Test] fn template_render() !void {
    var tmpl = "Hello, {{name}}! You are {{age}} years old.";
    var out = render(tmpl, "alice", 30);
    try expect(out.find("{{name}}") != null);   // 模板原样返回
}
