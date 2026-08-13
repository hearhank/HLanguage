// 63-template-render.hc — 模板渲染（String 替换，22 延伸）
//
//   - 占位符替换 {{name}} → 值（replace 返回新 String，Q16 值语义）

fn render(template: &[u8], name: &[u8], age: i32) String {
    var text = String.from(template, alloc);
    text = text.replace("{{name}}", name);       // 值语义：replace 返回新 String
    text = text.replace("{{age}}", fmt_int(age));
    return text;
}

fn main(io: Io) !void {
    var tmpl = "Hello, {{name}}! You are {{age}} years old.";
    var out = render(tmpl, "alice", 30);
    io.print("{}\n", out);
}
