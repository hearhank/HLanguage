import H.std.{io};

// 24-interface-errors.hc — 接口与错误（Q34 定案 2026-08-13）
//
//   - 接口方法错误返回 = anyerror（Zig 式任意错误类型，契约不约束具体错误集）
//   - 实现方返回具体错误；调用方 catch 按需匹配
//   - 普通函数仍用显式错误集（12.15）；anyerror 仅用于接口方法契约

interface IParse {
    fn parse(self: *Self, data: &[u8]) anyerror!Value;
}

[continuous] class JsonParser: IParse {   // 无字段（连续）；实现 IParse
    fn parse(self: *Self, data: &[u8]) anyerror!Value {
        return json.parse(data) catch return error.InvalidJson;
    }
}

[continuous] class CsvParser: IParse {   // 无字段（连续）；实现 IParse
    fn parse(self: *Self, data: &[u8]) anyerror!Value {
        return csv.parse(data) catch return error.BadRow;
    }
}

fn main() !void {
    var json_p = JsonParser{};   // 连续类型（空字段）：字面量构造
    var csv_p = CsvParser{};

    // anyerror：调用方按具体实现 catch 处理
    var v = json_p.parse("{\"a\":1}") catch |err| {
        io.print("json error: {}\n", err);
        return;
    };
    io.print("parsed: {}\n", v);

    var v2 = csv_p.parse("a,b,c") catch |err| {
        io.print("csv error: {}\n", err);
        return;
    };
    io.print("parsed2: {}\n", v2);
}

[Test] fn interface_error_contract() !void {
    var json_p = JsonParser{};
    var v = json_p.parse("{\"a\":1}") catch |err| {
        try expect(false);   // 合法 JSON 不应失败
        return;
    };
    // 成功路径：v 已解析（契约成立）

    var csv_p = CsvParser{};
    var v2 = csv_p.parse("a,b,c") catch |err| {
        try expect(false);   // 合法 CSV 不应失败
        return;
    };
}
