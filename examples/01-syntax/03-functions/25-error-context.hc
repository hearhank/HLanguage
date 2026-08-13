// 25-error-context.hc — 错误返回追踪（Zig 式 error return traces）
//
//   - try 传播时记录调用链（Debug）；错误带各层调用位置
//   - 自定义错误集 + 错误值映射

const ConfigError = error{ NotFound, InvalidFormat };

fn load_config(io: *T, path: &[u8]) ConfigError!Config where T: Io {
    var data = io.fs.read_file(path) catch return error.NotFound;
    return Config.from_json(data) catch return error.InvalidFormat;
}

fn main(io: Io) !void {
    var cfg = load_config(&io, "app.json") catch |err| {
        // Debug：err 携带返回追踪（各 try/catch 位置）
        io.print("config failed: {}\n", err);
        return;
    };
    io.print("loaded\n");
}
