import H.std.{io};

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

    fn log(self: *Self, io: *T, level: Level, msg: &[u8]) void where T: Io {
        if (level_rank(level) >= level_rank(self.min_level)) {
            io.print("[{}] {}\n", level, msg);
        }
    }
}

fn main(args: o Vec(String)) !void {
    var logger: o Logger = alloc.init(Logger);   // 无参构造（C1'）
    logger.min_level = Level.info;

    logger.log(&io, Level.debug, "hidden");    // 低于 min_level
    logger.log(&io, Level.info, "started");
    logger.log(&io, Level.error, "boom");
}

[test] fn log_levels() !void {
    var logger: o Logger = alloc.init(Logger);
    logger.min_level = Level.info;
    try expect_eq(level_rank(Level.error), 3);
    try expect_eq(level_rank(Level.debug), 0);
    // log 输出到 stdout（不捕获，Q-T6）；级别过滤由 level_rank 断言覆盖
    try expect(level_rank(Level.info) >= level_rank(logger.min_level));
}
