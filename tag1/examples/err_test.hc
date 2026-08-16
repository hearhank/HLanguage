const FileError = error{ NotFound, PermissionDenied, Io };
fn read_config(io: *T, path: &[u8]) FileError!&[u8] where T: Io {
    var f = io.fs.open(path) catch |err| switch (err) {
        error.NotFound => return error.NotFound,
        error.PermissionDenied => return error.PermissionDenied,
        else => return error.Io,
    };
    defer f.close();
    return io.fs.read_all(f, alloc);
}
[test] fn t() !void {
    try expect_error(error.NotFound, read_config(test_io, "config_missing_42.txt"));
}
