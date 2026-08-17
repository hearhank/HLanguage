import H.std.{io};

// 74-state-machine-adv.hc — 带数据的状态机（enum 负载 + switch 表达式）
//
//   - enum 变体带负载（12.13）；switch 穷举 + 负载捕获（Q27）
//   - 场景：HTTP 请求生命周期

enum HttpState {
    idle,
    connecting: i32,        // 携带尝试次数
    sending: usize,         // 携带已发送字节
    done,
    failed: &[u8],          // 携带错误信息
}

fn describe(state: HttpState) &[u8] {
    return switch (state) {
        HttpState.idle => "idle",
        HttpState.connecting => |attempt| "connecting",
        HttpState.sending => |sent| "sending",
        HttpState.done => "done",
        HttpState.failed => |msg| "failed",
    };
}

fn main(args: o Vec(String)) !void {
    var state = HttpState{connecting = 3};
    io.print("{}\n", describe(state));

    var failed = HttpState{failed = "timeout"};
    io.print("{}\n", describe(failed));
}

[test] fn state_machine_description() !void {
    var state = HttpState{connecting = 3};
    try expect_eq_slices(describe(state), "connecting");
    var failed = HttpState{failed = "timeout"};
    try expect_eq_slices(describe(failed), "failed");
    try expect_eq_slices(describe(HttpState.done), "done");
}
