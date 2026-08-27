import H.std.{io};

// 56-csv-parse.hc — CSV 处理（String split + class）
//
//   - split / trim 组合；行 → class（定义数据）
//   - 格式错误 → error union（显式错误集）

const CsvError = error{BadRow};

class Row {   // 含 String 字段 → 非 Continuous（默认 class，堆上）
    name: &[u8],
    age: i32,
}

fn parse_csv(data: &[u8]) CsvError!Vec<Row> {
    var rows = Vec<Row>.init(alloc);
    var lines = data.split('\n');

    for (lines) |line| {
        var cols = line.split(',');
        if (cols.len != 2) {
            return error.BadRow;
        }
        var age = parse_int(cols[1]) orelse return error.BadRow;
        rows.append(alloc.init(Row{name = cols[0], age = age}));
    }
    return rows;
}

fn main() !void {
    var csv = "alice,30\nbob,25";
    var rows = try parse_csv(csv);
    for (rows) |row| {
        io.print("{}: {}\n", row.name, row.age);
    }
}

[Test] fn csv_parse() !void {
    var csv = "alice,30\nbob,25";
    var rows = try parse_csv(csv);
    try expect_eq(rows.len, 2);
    try expect_eq_slices(rows[0].name, "alice");
    try expect_eq(rows[0].age, 30);
}

[Test] fn csv_format_error() !void {
    var bad = "alice,30,extra\n";
    try expect_error(error.BadRow, parse_csv(bad));
}