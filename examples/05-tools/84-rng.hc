import H.std.{io};

// 84-rng.hc — 伪随机数发生器（位运算 + 算术，12.2）
//
//   - xorshift64：位运算实战（12 延伸）
//   - class 封装状态；构造带种子（new 样板，Q22）

class Rng {
    mut state: u64,

    fn next(self: *mut Self) u64 {
        // xorshift64
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        return self.state;
    }

    fn between(self: *mut Self, lo: i32, hi: i32) i32 {
        return lo + (self.next() %(hi - lo));
    }
}

fn main() !void {
    var rng: Rng = Rng.new(0x1234_5678_9abc_def0);

    // 骰子模拟（1..6）
    var sum = 0;
    for (0..10) |_| {
        sum += rng.between(1, 7);
    }
    io.print("dice sum = {}\n", sum);
}

[test] fn rng_deterministic() !void {
    var rng: Rng = Rng.new(0x1234_5678_9abc_def0);
    var first = rng.next();
    var rng2: Rng = Rng.new(0x1234_5678_9abc_def0);
    try expect_eq(rng2.next(), first);   // 同种子同序列
}

[test] fn rng_range() !void {
    var rng: Rng = Rng.new(1);
    for (0..100) |_| {
        var v = rng.between(1, 7);
        try expect(v >= 1 and v <= 6);
    }
}
