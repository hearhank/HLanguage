// 70-logger.hc — 日志工具（枚举级别 + 格式输出）
//
//   - 枚举级别 + switch 表达式排序（Q27）
//   - io 显式传递（12.18）

enum Level {
    debug,
    info,
    warn,
    error,
}

fn level_rank(level: Level) i32 {
    return switch (level) {                 // switch 表达式（Q27）
        Level.debug => 0,
        Level.info => 1,
        Level.warn => 2,
        Level.error => 3,
    };
}

class Logger {
    mut min_level: Level,

    fn log(self: *Self, io: Io, level: Level, msg: &[u8]) void {
        if (level_rank(level) >= level_rank(self.min_level)) {
            io.print("[{}] {}\n", level, msg);
        }
    }
}

fn main(io: Io) !void {
    var logger: o Logger = Logger.new(alloc);
    logger.min_level = Level.info;

    logger.log(io, Level.debug, "hidden");    // 低于 min_level
    logger.log(io, Level.info, "started");
    logger.log(io, Level.error, "boom");
}
