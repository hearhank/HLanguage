import H.std.{io};

// 20-errors.hc — 错误处理（error union / error set / try / catch）
//
// Q11 定案（2026-08-13）：Zig 式
//   - 错误集显式声明：error{ NotFound, ... }
//   - 处理：x catch 默认值 / x catch |err| { ... }
//   - try 传播；无异常；无 ? 运算符别名
//   - const 为常量/类型别名声明（Q12 定案，2026-08-13）：const 不可变，var mut 可变

const FileError = error{NotFound, PermissionDenied, Io};

fn read_config(io: *T, path: &[u8]) FileError!&[u8] where T: Io {
    var f = io.fs.open(path) catch |err| switch (err) {
        error.NotFound => return error.NotFound,
        error.PermissionDenied => return error.PermissionDenied,
        else => return error.Io,
    };
    defer f.close();
    return io.fs.read_all(f, alloc);
}

fn main(args: o Vec(String)) !void {
    var data = read_config(&io, "config.txt") catch |err| {
        io.print("config error: {}\n", err);
        return;
    };
    io.print("config: {}\n", data);
}

[test] fn read_config_missing_not_found() !void {
    // 真实 IO（Q-T4）：随机文件名保证不存在 → 期望 NotFound
    try expect_error(error.NotFound, read_config(io, "config_missing_42.txt"));
}
