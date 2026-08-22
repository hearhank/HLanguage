import H.std.{io};

// 67-callbacks.hc — 事件回调（闭包作处理器，12.9）
//
//   - 闭包 = 数据对象：注册回调、事件分发
//   - 捕获：默认只读 / mut / move（显式标注）

class EventBus {
    mut handlers: Vec<Fn1<&[u8]> void>,

    fn on(self: *mut Self, handler: Fn1<&[u8]> void) void {
        self.handlers.append(handler);
    }

    fn emit(self: *Self, event: &[u8]) void {
        for (self.handlers) |h| {
            h(event);
        }
    }
}

fn main() !void {
    var bus: owned EventBus = alloc.init(EventBus);   // 无参构造（C1'）
    var mut count = 0;

    // 只读捕获（默认）
    bus.on(|event| io.print("got: {}\n", event));

    // 可写捕获（mut）
    bus.on(mut |event| {
        count += 1;
    });

    bus.emit("click");
    bus.emit("key");
    io.print("handled {} events\n", count);
}

[test] fn event_callback() !void {
    var bus: owned EventBus = alloc.init(EventBus);
    var mut count = 0;
    bus.on(mut |event| { // 可写捕获（Q26：双向登记）
        count += 1;
    });
    bus.emit("click");
    bus.emit("key");
    try expect_eq(count, 2);
}
