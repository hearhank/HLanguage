import H.std.{io};

// 73-rate-limit.hc — 令牌桶限流（算术 + class 状态）
//
//   - 令牌桶：每毫秒补令牌，容量上限
//   - allow() 返回是否放行；io.time 显式传递

class TokenBucket {
    mut capacity: i32,
    mut tokens: i32,
    mut last_refill: i64,

    fn new(capacity: i32, now: i64) owned TokenBucket {
        return TokenBucket{capacity = capacity, tokens = capacity, last_refill = now};
    }

fn allow<T>(self: *mut Self, io: *T) bool where T: Io {
        // 补充令牌（按流逝时间）
        var elapsed = io.time.now() - self.last_refill;
        self.tokens = min(self.capacity, self.tokens + elapsed);
        self.last_refill = io.time.now();

        if (self.tokens > 0) {
            self.tokens -= 1;
            return true;
        }
        return false;
    }
}

fn main() !void {
    var bucket: TokenBucket = TokenBucket.new(3, io.time.now());
    for (0..5) |_| {
        io.print("allowed = {}\n", bucket.allow(&io));
    }
}

[test] fn token_bucket() !void {
    var bucket: TokenBucket = TokenBucket.new(3, io.time.now());
    var allowed = 0;
    for (0..5) |_| {
        if (bucket.allow(&io)) {
            allowed += 1;
        }
    }
    try expect(allowed >= 3);   // 至少初始 3 令牌；时间流逝可能补充更多
}
