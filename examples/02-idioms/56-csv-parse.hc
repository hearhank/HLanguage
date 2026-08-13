// 56-csv-parse.hc — CSV 处理（String split + struct）
//
//   - split / trim 组合；行 → struct（定义数据）
//   - 格式错误 → error union（显式错误集）

const CsvError = error{ BadRow };

struct Row {
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
        rows.append(Row{ name = String.from(cols[0], alloc), age = age });
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
