// 19-nested-data.hc — 嵌套数据结构（定义数据深度）
//
//   - class 内嵌 class(连续) / enum / 数组（组合，12.12/12.13）
//   - 字面量嵌套：Position{...} / Value{...} / [1,2,3] 组合
//   - 含数组字段的 class：数组为引用类型 → 未标 continuous（堆上，B3）

[continuous]   // 连续内存值类型（H1 特性标注）
class Position {
    x: f32,
    y: f32,
}

enum Kind {
    player,
    enemy,
    item,
}

class Entity {   // 含 Vec/数组字段 → 未标 continuous（堆上）
    kind: Kind,
    pos: Position,          // 连续类型内嵌（值）
    tags: Vec(&[u8]),       // 复杂类型字段（堆，默认拥有 Q16）
    history: [8]f32,        // 数组字段（引用类型，B3）
}

fn main(io: Io) !void {
    // 嵌套构造：连续类型用字面量，Entity 用 new 样板（Q22）
    var e = Entity.new(
        alloc,
        Kind.enemy,
        Position{ x = 1.0, y = 2.0 },
        Vec(&[u8]).init(alloc),
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    );
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

[test] fn nested_struct_and_switch() !void {
    var e = Entity.new(
        alloc,
        Kind.enemy,
        Position{ x = 1.0, y = 2.0 },
        Vec(&[u8]).init(alloc),
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    );
    e.tags.append("boss");

    try expect_eq(e.kind == Kind.enemy, true);
    try expect_eq(e.pos.x, 1.0);
    try expect_eq(e.tags.len, 1);
    try expect_eq(e.history.len, 8);

    var desc = switch (e.kind) {
        Kind.player => "player",
        Kind.enemy => "enemy",
        Kind.item => "item",
    };
    try expect_eq_slices(desc, "enemy");
}
