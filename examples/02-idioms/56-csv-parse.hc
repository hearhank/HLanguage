// 56-csv-parse.hc — CSV 处理（String split + class）
//
//   - split / trim 组合；行 → class（定义数据）
//   - 格式错误 → error union（显式错误集）

const CsvError = error{ BadRow };

class Row {   // 含 String 字段 → 非 Continuous（默认 class，堆上）
    name: String,
    age: i32,
}

fn parse_csv(data: &[u8]) CsvError!Vec(Row) {
    var rows = Vec(Row).init(alloc);
    var text = String.from(data, alloc);
    var lines = text.split('\n');

    for (lines) |line| {
        var cols = line.split(',');
        if (cols.len != 2) {
            return error.BadRow;
        }
        var age = parse_int(cols[1]) orelse return error.BadRow;
        rows.append(alloc.init(Row{name = String.from(cols[0], alloc), age = age}));   // 带参构造（C1'）
    }
    return rows;
}

fn main(io: Io) !void {
    var csv = "alice,30\nbob,25";
    var rows = try parse_csv(csv);
    for (rows) |row| {
        io.print("{}: {}\n", row.name, row.age);
    }
}

test fn csv_parse() !void {
    var csv = "alice,30\nbob,25";
    var rows = try parse_csv(csv);
    try expect_eq(rows.len, 2);
    try expect_eq_slices(rows[0].name.as_slice(), "alice");
    try expect_eq(rows[0].age, 30);
}

test fn csv_format_error() !void {
    var bad = "alice,30,extra\n";
    try expect_error(error.BadRow, parse_csv(bad));
}
