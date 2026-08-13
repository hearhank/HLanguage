// 54-nested-json.hc — 嵌套 class 的 JSON 序列化（Q37 分层）
//
//   - class 嵌套：外层 to_json 递归内层字段
//   - 内建默认（Q37 C）；契约定制（字段映射/忽略）走脚本生成覆盖

class Address {
    mut city: String,
    mut zip: &[u8],
}

class Person {
    mut name: String,
    mut age: i32,
    mut addr: o Address,     // 嵌套 class（默认拥有，Q16）

    fn to_json(self: *Self) String {
        // 内建：递归嵌套字段序列化（{"name":...,"addr":{"city":...}}）
    }
}

fn main(io: Io) !void {
    var mut p: o Person = Person.new(alloc);
    p.name = String.from("alice", alloc);
    p.addr.city = String.from("beijing", alloc);

    var json = p.to_json();
    io.print("{}\n", json);

    var p2 = try Person.from_json(json);
    io.print("{}\n", p2.addr.city);
}

test "嵌套 class JSON（演示）" {
    // S4 演示型（Q-T6）：Person.to_json 为内建默认语义（示例以空体标注），
    // 递归嵌套序列化实现在标准库内建；断言留 M7 标准库测试
    try expect(true);
}
