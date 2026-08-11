// 并发：协作式调度 + Channel 通信
// 运行：node src/h.js run examples/concurrency.hc
//
// 生产者向 Channel(2) 发送 3 个值；消费者接收 3 个值。
// Channel 容量 2：第 3 次 send 时缓冲满 → 生产者挂起；
// 消费者 recv 腾出空位 → 唤醒生产者。交替推进。

global ch: Channel<u64> = Channel(2)

fun producer() {
    print("生产者启动")
    ch.send(1)
    print("生产者发送 1")
    ch.send(2)
    print("生产者发送 2")
    ch.send(3)
    print("生产者发送 3")
    print("生产者完成")
}

fun consumer() {
    print("消费者启动")
    a = ch.recv()
    print("消费者收到", a.to_str())
    b = ch.recv()
    print("消费者收到", b.to_str())
    c = ch.recv()
    print("消费者收到", c.to_str())
    print("消费者完成")
}

spawn producer()
spawn consumer()
