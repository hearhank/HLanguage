import H.std.{io};

// 71-recursive-parser.hc — 递归下降解析器（综合示例）
//
//   - 综合：切片 + 递归 + enum + error union + arena 回收
//   - 微型表达式：数字 / 括号嵌套（语法可扩展 + -）
//   - AST 节点从 arena 分配（统一回收，49 延伸）——避开复杂所有权链接

const ParseError = error{UnexpectedToken, UnexpectedEnd};

enum Kind {
    num,
    // add, sub,     // 扩展：语法加项后启用
}

class Node {                     // AST 节点
    kind: Kind,
    value: i64,
    // left/right 扩展后启用（arena 持有）
}

fn parse<T>(io: *T, data: &[u8], pos: *usize, arena: *Arena) ParseError!*Node where T: Io {
    skip_space(data, pos);
    var c = peek(data, pos) orelse return error.UnexpectedEnd;

    if (c == '(') {
        advance(data, pos);
        var inner = try parse(&io, data, pos, arena);      // 递归
        expect(data, pos, ')') catch return error.UnexpectedToken;
        return inner;
    }

    if (is_digit(c)) {
        var n = parse_number(data, pos);
        return arena.alloc(Node{kind = Kind.num, value = n});
    }

    return error.UnexpectedToken;
}

fn eval(n: *Node) i64 {
    return switch (n.kind) {     // switch 表达式（Q27）
        Kind.num => n.value,
    };
}

fn main() !void {
    var arena = Arena.init(alloc);
    var pos = 0;

    var node = try parse(&io, "(5)", &pos, &arena);
    io.print("result = {}\n", eval(node));
}

[Test] fn recursive_parser() !void {
    var arena = Arena.init(alloc);
    var pos = 0;
    var node = try parse(&io, "(5)", &pos, &arena);
    try expect_eq(eval(node), 5);
}

[Test] fn parse_error() !void {
    var arena = Arena.init(alloc);
    var pos = 0;
    try expect_error(error.UnexpectedToken, parse(&io, ")", &pos, &arena));
}
