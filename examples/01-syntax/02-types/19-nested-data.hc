// 19-nested-data.hc — 嵌套数据结构（定义数据深度）
//
//   - struct 内嵌 struct / enum / 数组（组合，12.12/12.13）
//   - 字面量嵌套：Point{...} / Value{...} / [1,2,3] 组合
//   - 含数组字段的 struct：数组为引用类型 → struct 归其它对象（B3）

struct Position {
    x: f32,
    y: f32,
}

enum Kind {
    player,
    enemy,
    item,
}

struct Entity {
    kind: Kind,
    pos: Position,          // struct 内嵌 struct（值）
    tags: Vec(&[u8]),       // 复杂类型字段（堆，默认拥有 Q16）
    history: [8]f32,        // 数组字段（引用类型，B3）
}

fn main(io: Io) !void {
    // 嵌套字面量：struct{ enum, struct, 数组 }
    var e = Entity{
        kind = Kind.enemy,
        pos = Position{ x = 1.0, y = 2.0 },
        tags = Vec(&[u8]).init(alloc),
        history = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    };
    e.tags.append("boss");

    // 访问链（自动解引用，A3）
    io.print("kind = {}, pos.x = {}\n", e.kind, e.pos.x);
    io.print("tags = {}\n", e.tags.len);

    // switch 表达式 + 嵌套访问
    var desc = switch (e.kind) {
        Kind.player => "player",
        Kind.enemy => "enemy",
        Kind.item => "item",
    };
    io.print("{}\n", desc);
}
