// 55-time.hc — 时间模块（12.18 stdlib time）
//
//   - io.time：时间戳/计时/延时（io 显式传递，Q35 同款）
//   - 场景：计时、延时

fn main(io: Io) !void {
    // 计时
    var start = io.time.now();
    // ... 工作
    var elapsed = io.time.now() - start;
    io.print("elapsed = {} ms\n", elapsed);

    // 延时（毫秒）
    io.time.sleep(50);
    io.print("slept 50ms\n");
}
