//! G1（E3.1）net 验收：UDP（bind/send_to/recv_from/local_port/close + 命名空间双语）/
//! HTTP 客户端（io.net.get）与服务端（io.net.listen + Rust 客户端对答）。

use hc_rt::Interp;
use std::io::{Read, Write};

fn run_ok(src: &str) {
    let program = hc::parse_source(src).unwrap_or_else(|d| panic!("parse: {:?}", d));
    let mut interp = Interp::new(src);
    interp
        .load(&program)
        .unwrap_or_else(|e| panic!("load: {} {}", e.name, e.message));
    let (p, f, _s) = interp.run_tests();
    assert_eq!(f, 0, "failed tests: {:?}", interp.test_out);
    assert!(p >= 1, "no tests ran");
}

/// 端口号 → "host:port" 对端地址串（fmt_int + String.concat 拼装）
fn udp_addr(port_expr: &str) -> String {
    format!("\"127.0.0.1:\".concat(fmt_int({port_expr}))")
}

#[test]
fn udp_bind_send_recv_loopback() {
    // 双 UdpSocket 回环：bind(0) 取临时端口 → send_to 对端地址 → recv_from 取 [addr, data]
    let src = format!(
        "[test] fn t() !void {{
    var s1 = try io.net.udp.bind(0);
    var s2 = try io.net.udp.bind(0);
    defer s1.close();
    defer s2.close();
    var p1 = try s1.local_port();
    try s2.send_to({}, \"ping-udp\");
    var r = try s1.recv_from(alloc);
    try expect_eq_slices(r[1], \"ping-udp\");
}}\n",
        udp_addr("p1"),
    );
    run_ok(&src);
}

#[test]
fn udp_recv_timed_out() {
    // 空队列 recv_from：200ms 读超时 → error.TimedOut（不挂起测试）
    run_ok(
        "[test] fn t() !void {
    var s1 = try io.net.udp.bind(0);
    defer s1.close();
    expect_error(error.TimedOut, s1.recv_from(alloc));
}\n",
    );
}

#[test]
fn udp_namespace_form() {
    // Q20 双语：io.net.udp.send_to(&s2, …) / recv_from(&s1, alloc) / close(&s1)
    let src = format!(
        "[test] fn t() !void {{
    var s1 = try io.net.udp.bind(0);
    var s2 = try io.net.udp.bind(0);
    defer io.net.udp.close(&s1);
    defer io.net.udp.close(&s2);
    var p1 = try s1.local_port();
    try io.net.udp.send_to(&s2, {}, \"ns-form\");
    var r = try io.net.udp.recv_from(&s1, alloc);
    try expect_eq_slices(r[1], \"ns-form\");
}}\n",
        udp_addr("p1"),
    );
    run_ok(&src);
}

#[test]
fn tcp_namespace_form_q20() {
    // Q20 双语：io.net.read_all(&conn, alloc) ≡ conn.read_all(alloc)、
    // io.net.accept(&server) ≡ server.accept()、io.net.write(&accepted, …) 等
    run_ok(
        "[test] fn t() !void {
    var listener = try io.net.listen(\"127.0.0.1\", 0, alloc);
    var port = try listener.local_port();
    var conn = try io.net.connect(\"127.0.0.1\", port, alloc);
    var accepted = try io.net.accept(&listener);
    try io.net.write(&accepted, \"ns-tcp\");
    io.net.shutdown(&accepted);
    var reply = try io.net.read_all(&conn, alloc);
    try expect_eq_slices(reply, \"ns-tcp\");
    io.net.close(&accepted);
    io.net.close(&conn);
    io.net.close(&listener);
}\n",
    );
}

#[test]
fn http_get_loopback() {
    // Rust 侧真实 HTTP 服务线程：收到 GET /greet 后返回 200 + Content-Length 体
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut stream, _peer) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).unwrap();
        let req = String::from_utf8_lossy(&buf[..n]).to_string();
        assert!(req.starts_with("GET /greet HTTP/1.1"), "req: {req}");
        let resp = b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nhello-http";
        stream.write_all(resp).unwrap();
        // 闭包结束 drop(stream) → 连接关闭 → 客户端 read_to_end 得 EOF
    });
    let src = format!(
        "[test] fn t() !void {{
    var body = try io.net.get(\"http://127.0.0.1:{port}/greet\");
    try expect_eq_slices(body, \"hello-http\");
}}\n"
    );
    run_ok(&src);
    server.join().unwrap();
}

#[test]
fn http_server_responds() {
    // H 作 HTTP 服务端（io.net.listen）：Rust 客户端带重试连接 → GET /ping → 读响应
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let req_body = format!("GET /ping HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    let src = format!(
        "[test] fn t() !void {{
    var listener = try io.net.listen(\"127.0.0.1\", {port}, alloc);
    var accepted = try listener.accept();
    var req = try accepted.read_all();
    try expect_eq_slices(req, \"{req_body}\");
    try accepted.write(\"pong-http\");
    accepted.shutdown();
    accepted.close();
    listener.close();
}}\n"
    );
    let client = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut stream = loop {
            if let Ok(s) = std::net::TcpStream::connect(("127.0.0.1", port)) {
                break s;
            }
            if std::time::Instant::now() >= deadline {
                panic!("connect to H listener timed out");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        stream.write_all(req_body.as_bytes()).unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();
        let mut resp = Vec::new();
        stream.read_to_end(&mut resp).unwrap();
        let s = String::from_utf8_lossy(&resp);
        assert!(s.contains("pong-http"), "resp: {s}");
    });
    run_ok(&src);
    client.join().unwrap();
}
