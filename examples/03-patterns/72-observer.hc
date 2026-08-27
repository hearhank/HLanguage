import H.std.{io};

// 72-observer.hc — 发布/订阅（观察者模式，67 延伸）
//
//   - 主题 + 订阅者列表（闭包回调）
//   - 事件即数据：事件名 + 负载（&[u8]）

class Subject {
    mut subscribers: Vec<Fn1<&[u8], &[u8]> void>,   // (事件, 负载)

    fn subscribe(self: *mut Self, handler: Fn1<&[u8], &[u8]> void) void {
        self.subscribers.append(handler);
    }

    fn publish(self: *Self, event: &[u8], payload: &[u8]) void {
        for (self.subscribers) |h| {
            h(event, payload);
        }
    }
}

fn main() !void {
    var subject: Subject = alloc.init(Subject);   // 无参构造（C1'）

    subject.subscribe(|event, payload| io.print("[{}] {}\n", event, payload));

    subject.publish("user.login", "alice");
    subject.publish("user.logout", "alice");
}

[Test] fn publish_subscribe() !void {
    var subject: Subject = alloc.init(Subject);
    var mut received = 0;
    subject.subscribe(mut |event, payload| { // 可写捕获（mut）
        received += 1;
    });
    subject.publish("user.login", "alice");
    subject.publish("user.logout", "alice");
    try expect_eq(received, 2);
}
