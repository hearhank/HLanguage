// 69-config-load.hc — 配置加载综合（optional + error + 默认值）
//
//   - 文件缺失 → 默认配置（catch 返回默认值）
//   - 字段缺失 → 默认值（orelse 链）
//   - 格式错误 → error（显式错误集）

const ConfigError = error{ InvalidConfig };

struct Config {
    host: String,
    port: i32,
    timeout_ms: i32,
}

fn default_config() Config {
    return Config{ host = String.from("localhost", alloc), port = 8080, timeout_ms = 1000 };
}

fn load_config(io: *T, path: &[u8]) ConfigError!Config where T: Io {
    var data = io.fs.read_file(path) catch return default_config();   // 缺失 → 默认
    var json = json.parse(data) catch return error.InvalidConfig;

    return Config{                              // 字段缺失 → 默认值（orelse 链）
        host = json.get("host") orelse String.from("localhost", alloc),
        port = json.get("port") orelse 8080,
        timeout_ms = json.get("timeout_ms") orelse 1000,
    };
}

fn main(io: Io) !void {
    var cfg = try load_config(&io, "config.json");
    io.print("{}:{}\n", cfg.host, cfg.port);
}
