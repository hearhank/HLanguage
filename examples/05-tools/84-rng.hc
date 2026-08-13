// 84-rng.hc — 伪随机数发生器（位运算 + 算术，12.2）
//
//   - xorshift64：位运算实战（56 延伸）
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
        return lo + (self.next() % (hi - lo));
    }
}

fn main(io: Io) !void {
    var rng: o Rng = Rng.new(0x1234_5678_9abc_def0);

    // 骰子模拟（1..6）
    var sum = 0;
    for (0..10) |_| {
        sum += rng.between(1, 7);
    }
    io.print("dice sum = {}\n", sum);
}
