// 26-error-set-union.hc — 错误集组合（12.15）
//
//   - 错误集联合：error{A} || error{B}（Zig 式）
//   - 组合错误集作为函数返回契约

const FileError = error{ NotFound, PermissionDenied };
const ParseError = error{ InvalidFormat };
const CombinedError = FileError || ParseError;   // 错误集联合

fn load_config(io: *T, path: &[u8]) CombinedError!Config where T: Io {
    var data = io.fs.read_file(path) catch return error.NotFound;
    return Config.from_json(data) catch return error.InvalidFormat;
}

fn main(io: Io) !void {
    var cfg = load_config(&io, "app.json") catch |err| {
        io.print("failed: {}\n", err);
        return;
    };
    io.print("loaded\n");
}

test fn error_set_union() !void {
    // CombinedError = FileError || ParseError（组合契约）；真实 IO：文件缺失 → NotFound
    try expect_error(error.NotFound, load_config(test_io, "app.json"));
}
