use super::*;

impl Interp {
    pub(crate) fn io_poll(&mut self, io: &Value, args: &[Expr], span: &Span) -> Result<usize> {
        if !args.is_empty() {
            return Err(RtError::new("ArityMismatch", Some(span.clone())));
        }
        let evented = match io {
            Value::Class(c) => {
                let d = c.borrow();
                matches!(
                    d.fields.get("runtime"),
                    Some(Value::Str(s)) if s.borrow().as_slice() == b"evented"
                )
            }
            _ => return Err(RtError::new("TypeError", Some(span.clone()))),
        };
        if !evented {
            return Ok(0);
        }
        let pending = std::mem::take(&mut self.root_threads);
        let n = pending.len();
        for t in pending {
            let _ = self.thread_run(&t, &Span::new(0, 0, 0, 0));
        }
        Ok(n)
    }

    pub(crate) fn call_io_print(&mut self, args: &[Expr], span: &Span) -> Result<()> {
        if args.is_empty() {
            return Err(RtError::new("ArityMismatch", Some(span.clone())));
        }
        let fmt = self.eval(&args[0])?;
        let fmt = self.deref_value(fmt);
        let fmt = match fmt {
            Value::Str(s) => s.borrow().clone(),
            _ => return Err(RtError::new("TypeError", Some(span.clone()))),
        };
        let mut out = Vec::new();
        let mut argi = 1;
        let mut i = 0;
        while i < fmt.len() {
            if fmt[i] == b'{' {
                // 找匹配 `}`；无匹配 → 按字面量
                if let Some(close) = fmt[i + 1..].iter().position(|&c| c == b'}') {
                    if argi < args.len() {
                        let v = self.eval(&args[argi])?;
                        let v = self.deref_value(v);
                        let s = self.format_spec_value(&v, &fmt[i + 1..i + 1 + close], span)?;
                        out.extend_from_slice(s.as_bytes());
                        argi += 1;
                    }
                    i += close + 2;
                    continue;
                }
            }
            out.push(fmt[i]);
            i += 1;
        }
        let line = String::from_utf8_lossy(&out).to_string();
        if self.in_main {
            print!("{line}");
        } else {
            self.test_out.push(line);
        }
        Ok(())
    }

    /// 格式说明符（B1，2026-08-17）：`{}` 默认 / `{d}` / `{x}` / `{X}` / `{b}` / `{e}` / `{s}`
    /// + 宽度/对齐/精度（`{:8}` 右对齐、`{:<6}` 左对齐、`{:.2}` 精度）——Zig 式：
    /// `{` [可选 `:`] [对齐 `<`/`>`/`^`] [宽度数字] [`.` 精度数字] [类型字符] `}`。
    /// 未知类型字符 → `FormatError`（B2：不再按字面量静默输出）。
    pub(crate) fn format_spec_value(&self, v: &Value, inner: &[u8], span: &Span) -> Result<String> {
        let mut p = if inner.first() == Some(&b':') { 1 } else { 0 };
        let align = match inner.get(p) {
            Some(b'<') | Some(b'>') | Some(b'^') => {
                let a = inner[p];
                p += 1;
                a
            }
            _ => b'>',
        };
        let mut width: Option<usize> = None;
        let mut ws = String::new();
        while p < inner.len() && inner[p].is_ascii_digit() {
            ws.push(inner[p] as char);
            p += 1;
        }
        if !ws.is_empty() {
            width = ws.parse().ok();
        }
        let mut precision: Option<usize> = None;
        if p < inner.len() && inner[p] == b'.' {
            p += 1;
            let mut ps = String::new();
            while p < inner.len() && inner[p].is_ascii_digit() {
                ps.push(inner[p] as char);
                p += 1;
            }
            precision = ps.parse().ok();
        }
        let ty = inner.get(p).copied();
        if p + usize::from(ty.is_some()) < inner.len() {
            // 多字符残留 → 未知说明符（不再静默输出）
            return Err(RtError::new("FormatError", Some(span.clone())));
        }
        let display = v.display();
        let mut s = match ty {
            Some(b'd') => match v {
                Value::Int(n) => n.to_string(),
                Value::Float(f) => f.to_string(),
                _ => display,
            },
            Some(b'x') => match v {
                Value::Int(n) => format!("{n:x}"),
                _ => display,
            },
            Some(b'X') => match v {
                Value::Int(n) => format!("{n:X}"),
                _ => display,
            },
            Some(b'b') => match v {
                Value::Int(n) => format!("{n:b}"),
                _ => display,
            },
            Some(b'e') => match v {
                Value::Float(f) => format!("{f:e}"),
                _ => display,
            },
            Some(b's') => display,
            Some(_) => return Err(RtError::new("FormatError", Some(span.clone()))),
            None => display,
        };
        // 精度（浮点）
        if let Some(pr) = precision {
            if let Value::Float(f) = v {
                s = format!("{f:.pr$}");
            }
        }
        // 宽度/对齐
        if let Some(w) = width {
            if s.len() < w {
                let pad = w - s.len();
                match align {
                    b'<' => s = format!("{s}{}", " ".repeat(pad)),
                    b'^' => {
                        let l = pad / 2;
                        s = format!("{}{s}{}", " ".repeat(l), " ".repeat(pad - l));
                    }
                    _ => s = format!("{}{s}", " ".repeat(pad)),
                }
            }
        }
        Ok(s)
    }

    // ---------- M5.4 真实 IO：io.fs / io.time / File 句柄 ----------

    /// 求值第 i 个参数并解引用为字节串（数据参数）
    pub(crate) fn eval_str_arg(&mut self, args: &[Expr], i: usize, span: &Span) -> Result<Vec<u8>> {
        let a = args
            .get(i)
            .ok_or_else(|| RtError::new("ArityMismatch", Some(span.clone())))?;
        let v = self.eval(a)?;
        match self.deref_value(v) {
            Value::Str(s) => Ok(s.borrow().clone()),
            _ => Err(RtError::new("TypeError", Some(span.clone()))),
        }
    }

    /// 求值第 i 个参数为路径字符串（fs 函数路径参数）
    pub(crate) fn eval_path_arg(&mut self, args: &[Expr], i: usize, span: &Span) -> Result<String> {
        let b = self.eval_str_arg(args, i, span)?;
        Ok(String::from_utf8_lossy(&b).into_owned())
    }

    /// 求值第 i 个参数为整数
    pub(crate) fn eval_int_arg(&mut self, args: &[Expr], i: usize, span: &Span) -> Result<i128> {
        let a = args
            .get(i)
            .ok_or_else(|| RtError::new("ArityMismatch", Some(span.clone())))?;
        let v = self.eval(a)?;
        match self.deref_value(v) {
            Value::Int(n) => Ok(n),
            _ => Err(RtError::new("TypeError", Some(span.clone()))),
        }
    }

    /// 从 File 值（或指针）提取文件描述符
    pub(crate) fn file_fd(&self, v: &Value, span: &Span) -> Result<i64> {
        match self.deref_value(v.clone()) {
            Value::Class(c) if c.borrow().name == "File" => match c.borrow().fields.get("_fd") {
                Some(Value::Int(fd)) => Ok(*fd as i64),
                _ => Err(RtError::new("BadFd", Some(span.clone()))),
            },
            _ => Err(RtError::new("TypeError", Some(span.clone()))),
        }
    }

    /// std::io::Error → H 错误名（20-errors 错误集：NotFound/PermissionDenied/其它 Io）
    pub(crate) fn io_error_name(&self, e: &std::io::Error) -> String {
        match e.kind() {
            std::io::ErrorKind::NotFound => "NotFound".into(),
            std::io::ErrorKind::PermissionDenied => "PermissionDenied".into(),
            // G1（E3.1）：UDP recv_from 读超时（200ms）/ 非阻塞 WouldBlock → error.TimedOut
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => "TimedOut".into(),
            _ => "Io".into(),
        }
    }

    /// 注册真实文件句柄 → File 值（内部 `_fd` 字段）
    pub(crate) fn register_file(&mut self, f: std::fs::File) -> Value {
        let fd = self.next_fd;
        self.next_fd += 1;
        self.files.insert(fd, f);
        let mut fields = HashMap::new();
        fields.insert("_fd".into(), Value::Int(fd as i128));
        Value::class("File", fields)
    }

    // ---------- M5.4 io.net（TCP 基础） ----------

    /// TcpConn/TcpListener 值 → 注册表 fd
    pub(crate) fn net_fd(&self, v: &Value, span: &Span) -> Result<i64> {
        if let Value::Class(c) = v {
            if let Some(Value::Int(fd)) = c.borrow().fields.get("fd") {
                return Ok(*fd as i64);
            }
        }
        Err(RtError::new("BadFd", Some(span.clone())))
    }

    pub(crate) fn call_net_method(
        &mut self,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        match field {
            // io.net.connect(host, port, alloc) !TcpConn
            "connect" => {
                let host = self.eval_str_arg(args, 0, span)?;
                let port = self.eval_int_arg(args, 1, span)? as u16;
                let host = String::from_utf8_lossy(&host).to_string();
                match std::net::TcpStream::connect((host.as_str(), port)) {
                    Ok(stream) => {
                        let fd = self.next_net_fd;
                        self.next_net_fd += 1;
                        let _ = stream.set_nodelay(true);
                        self.tcp_streams.insert(fd, stream);
                        let mut f = HashMap::new();
                        f.insert("fd".into(), Value::Int(fd as i128));
                        Ok(Some(Value::class("TcpConn", f)))
                    }
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            // io.net.listen(host, port, alloc) !TcpListener
            "listen" => {
                let host = self.eval_str_arg(args, 0, span)?;
                let port = self.eval_int_arg(args, 1, span)? as u16;
                let host = String::from_utf8_lossy(&host).to_string();
                let addr = format!("{host}:{port}");
                match std::net::TcpListener::bind(&addr) {
                    Ok(listener) => {
                        let fd = self.next_net_fd;
                        self.next_net_fd += 1;
                        self.tcp_listeners.insert(fd, listener);
                        let mut f = HashMap::new();
                        f.insert("fd".into(), Value::Int(fd as i128));
                        Ok(Some(Value::class("TcpListener", f)))
                    }
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            // G1（E3.1）：io.net.get(url) !&[u8]——HTTP GET 客户端，返回响应体字节
            "get" => {
                let url = self.eval_str_arg(args, 0, span)?;
                let url = String::from_utf8_lossy(&url).to_string();
                match self.http_get(&url) {
                    Ok(body) => Ok(Some(Value::str_bytes(body))),
                    Err(name) => Ok(Some(self.err_val(&name))),
                }
            }
            // Q20 双语：命名空间形式 io.net.read_all(&conn, alloc) ≡ conn.read_all(alloc)
            //（write/shutdown/close/local_port 同构；第一个实参解引用剥 Ptr → 实例方法）
            "read_all" | "write" | "shutdown" | "close" | "local_port" => {
                let conn = self.eval(&args[0])?;
                let conn = self.deref_value(conn);
                self.call_conn_method(field, &conn, &args[1..], span)
            }
            // io.net.accept(&server) !Conn ≡ server.accept()
            "accept" => {
                let srv = self.eval(&args[0])?;
                let srv = self.deref_value(srv);
                self.call_listener_method("accept", &srv, &args[1..], span)
            }
            _ => Ok(None),
        }
    }

    // ---------- G1（E3.1）UDP：io.net.udp 命名空间 + UdpSocket 实例方法 ----------

    /// UDP 绑定的共享实现：`udp_bind(host, port)` → UdpSocket 值（fd 注册表）；
    /// 读超时 200ms（recv_from 空队列 → error.TimedOut，不阻塞挂起测试）。
    pub(crate) fn udp_bind(&mut self, host: &str, port: u16, span: &Span) -> Result<Value> {
        let addr = format!("{host}:{port}");
        match std::net::UdpSocket::bind(&addr) {
            Ok(sock) => {
                let _ = sock.set_read_timeout(Some(std::time::Duration::from_millis(200)));
                let fd = self.next_net_fd;
                self.next_net_fd += 1;
                self.udp_sockets.insert(fd, sock);
                let mut f = HashMap::new();
                f.insert("fd".into(), Value::Int(fd as i128));
                Ok(Value::class("UdpSocket", f))
            }
            Err(e) => Err(RtError::new(&self.io_error_name(&e), Some(span.clone()))),
        }
    }

    /// 解析 UDP 对端地址串 "host:port" → (host, port)。
    pub(crate) fn parse_udp_addr(&self, s: &str, span: &Span) -> Result<(String, u16)> {
        match s.rsplit_once(':') {
            Some((host, port)) => match port.parse::<u16>() {
                Ok(p) => Ok((host.to_string(), p)),
                Err(_) => Err(RtError::new("InvalidAddress", Some(span.clone()))),
            },
            None => Err(RtError::new("InvalidAddress", Some(span.clone()))),
        }
    }

    /// `io.net.udp` 命名空间方法：bind(port) / bind(host, port) !UdpSocket；
    /// 命名空间形式 send_to(&sock, addr, data) / recv_from(&sock, alloc) / close(&sock)
    /// 委托实例方法（Q20 双语）。
    pub(crate) fn call_udp_ns_method(
        &mut self,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        match field {
            "bind" => {
                // bind(port) / bind(host, port) / bind(host, port, alloc)——args[0] 为
                // 整型字面量 → port 首参（bind(0, alloc) 亦归此，alloc 忽略）
                let is_port_first = matches!(args[0], Expr::IntLit { .. });
                let (host, port_i) = if is_port_first {
                    ("127.0.0.1".to_string(), 0)
                } else if args.len() >= 2 {
                    let h = self.eval_str_arg(args, 0, span)?;
                    (String::from_utf8_lossy(&h).to_string(), 1)
                } else {
                    ("127.0.0.1".to_string(), 0)
                };
                let port = self.eval_int_arg(args, port_i, span)? as u16;
                Ok(Some(self.udp_bind(&host, port, span)?))
            }
            "send_to" | "recv_from" | "close" => {
                let sock = self.eval(&args[0])?;
                let sock = self.deref_value(sock);
                self.call_udp_socket_method(field, &sock, &args[1..], span)
            }
            _ => Ok(None),
        }
    }

    /// UdpSocket 实例方法：send_to(addr, data) !void / recv_from(alloc) ![addr, data] /
    /// local_port() !u16 / close() !void。recv_from 空队列（200ms 读超时）→ error.TimedOut。
    pub(crate) fn call_udp_socket_method(
        &mut self,
        field: &str,
        v: &Value,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        let fd = self.net_fd(v, span)?;
        match field {
            "send_to" => {
                let addr = self.eval_str_arg(args, 0, span)?;
                let addr = String::from_utf8_lossy(&addr).to_string();
                let (host, port) = self.parse_udp_addr(&addr, span)?;
                let data = self.eval_str_arg(args, 1, span)?;
                let sock = self
                    .udp_sockets
                    .get_mut(&fd)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?;
                match sock.send_to(&data, (host.as_str(), port)) {
                    Ok(_) => Ok(Some(Value::Void)),
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            "recv_from" => {
                let sock = self
                    .udp_sockets
                    .get_mut(&fd)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?;
                let mut buf = vec![0u8; 65536];
                match sock.recv_from(&mut buf) {
                    Ok((n, peer)) => {
                        buf.truncate(n);
                        let addr = peer.to_string();
                        Ok(Some(Value::arr(vec![
                            Value::str(&addr),
                            Value::str_bytes(buf),
                        ])))
                    }
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            "local_port" => {
                let sock = self
                    .udp_sockets
                    .get(&fd)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?;
                match sock.local_addr() {
                    Ok(a) => Ok(Some(Value::Int(a.port() as i128))),
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            "close" => {
                self.udp_sockets.remove(&fd);
                Ok(Some(Value::Void))
            }
            _ => Ok(None),
        }
    }

    /// G1（E3.1）：HTTP GET 客户端——`http://host[:port][/path]` → TCP connect →
    /// `GET {path} HTTP/1.1` + Host 头 → 读响应 → 按 Content-Length 提取体。
    pub(crate) fn http_get(&self, url: &str) -> std::result::Result<Vec<u8>, String> {
        let rest = url
            .strip_prefix("http://")
            .ok_or_else(|| "InvalidUrl".to_string())?;
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) => (
                h.to_string(),
                p.parse::<u16>().map_err(|_| "InvalidUrl".to_string())?,
            ),
            None => (authority.to_string(), 80u16),
        };
        let mut stream = std::net::TcpStream::connect((host.as_str(), port))
            .map_err(|e| self.io_error_name(&e))?;
        let req = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
        stream
            .write_all(req.as_bytes())
            .map_err(|e| self.io_error_name(&e))?;
        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .map_err(|e| self.io_error_name(&e))?;
        // 状态行 + 头段由第一个空行分隔；体按 Content-Length 取（无则取空行后全部）
        let head_end = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|i| i + 4)
            .ok_or_else(|| "BadResponse".to_string())?;
        let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
        let body = &raw[head_end..];
        if !head.starts_with("HTTP/1.1 200") && !head.starts_with("HTTP/1.0 200") {
            // 非 200：体返回给调用方诊断（错误名 = Http{code}）
            let code = head.split_whitespace().nth(1).unwrap_or("000").to_string();
            return Err(format!("Http{code}"));
        }
        let mut len: Option<usize> = None;
        for line in head.lines() {
            if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                if let Ok(n) = v.trim().parse::<usize>() {
                    len = Some(n);
                }
            }
        }
        Ok(match len {
            Some(n) => body[..n.min(body.len())].to_vec(),
            None => body.to_vec(),
        })
    }

    pub(crate) fn call_conn_method(
        &mut self,
        field: &str,
        v: &Value,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        let fd = self.net_fd(v, span)?;
        match field {
            // conn.write(data)：写字节（EOF → 错误）
            "write" => {
                let data = self.eval_str_arg(args, 0, span)?;
                let stream = self
                    .tcp_streams
                    .get_mut(&fd)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?;
                match stream.write_all(&data) {
                    Ok(_) => Ok(Some(Value::Void)),
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            // conn.read(n) &[u8]：读至多 n 字节
            "read" => {
                let n = self.eval_int_arg(args, 0, span)?;
                let n = n.max(0) as usize;
                let stream = self
                    .tcp_streams
                    .get_mut(&fd)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?;
                let mut buf = vec![0u8; n];
                match stream.read(&mut buf) {
                    Ok(0) => Ok(Some(Value::str_bytes(vec![]))),
                    Ok(k) => {
                        buf.truncate(k);
                        Ok(Some(Value::str_bytes(buf)))
                    }
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            // conn.read_all() &[u8]：读到 EOF（流关闭）
            "read_all" => {
                let stream = self
                    .tcp_streams
                    .get_mut(&fd)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?;
                let mut buf = Vec::new();
                match stream.read_to_end(&mut buf) {
                    Ok(_) => Ok(Some(Value::str_bytes(buf))),
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            // 帧读写（u32 LE 前缀帧）：write_u32_le(n) / read_u32_le() !u32
            "write_u32_le" => {
                let n = self.eval_int_arg(args, 0, span)?;
                let stream = self
                    .tcp_streams
                    .get_mut(&fd)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?;
                match stream.write_all(&(n as u32).to_le_bytes()) {
                    Ok(_) => Ok(Some(Value::Void)),
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            "read_u32_le" => {
                let stream = self
                    .tcp_streams
                    .get_mut(&fd)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?;
                let mut buf = [0u8; 4];
                match stream.read_exact(&mut buf) {
                    Ok(_) => Ok(Some(Value::Int(u32::from_le_bytes(buf) as i128))),
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            // conn.shutdown()：半关闭写（对端 read 返回 EOF）
            "shutdown" => {
                let stream = self
                    .tcp_streams
                    .get_mut(&fd)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?;
                match stream.shutdown(std::net::Shutdown::Write) {
                    Ok(_) => Ok(Some(Value::Void)),
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            // conn.close()：关闭并注销
            "close" => {
                self.tcp_streams.remove(&fd);
                Ok(Some(Value::Void))
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn call_listener_method(
        &mut self,
        field: &str,
        v: &Value,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        let _ = args;
        let fd = self.net_fd(v, span)?;
        match field {
            // listener.local_port() !u16：实际监听端口（listen 0 端口动态分配用）
            "local_port" => {
                let listener = self
                    .tcp_listeners
                    .get(&fd)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?;
                match listener.local_addr() {
                    Ok(addr) => Ok(Some(Value::Int(addr.port() as i128))),
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            // listener.accept() !TcpConn：阻塞接受连接
            "accept" => {
                let listener = self
                    .tcp_listeners
                    .get_mut(&fd)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?;
                match listener.accept() {
                    Ok((stream, _peer)) => {
                        let cfd = self.next_net_fd;
                        self.next_net_fd += 1;
                        let _ = stream.set_nodelay(true);
                        self.tcp_streams.insert(cfd, stream);
                        let mut f = HashMap::new();
                        f.insert("fd".into(), Value::Int(cfd as i128));
                        Ok(Some(Value::class("TcpConn", f)))
                    }
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            // listener.close()：关闭并注销
            "close" => {
                self.tcp_listeners.remove(&fd);
                Ok(Some(Value::Void))
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn call_fs_method(
        &mut self,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        match field {
            // io.fs.open(path)：读写、不创建（缺失 → error.NotFound，Zig 式）
            "open" => {
                let path = self.eval_path_arg(args, 0, span)?;
                match std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&path)
                {
                    Ok(f) => Ok(Some(self.register_file(f))),
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            // io.fs.create(path)：创建/截断（读写权限——供写入后 seek/read 验证）
            "create" => {
                let path = self.eval_path_arg(args, 0, span)?;
                match std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&path)
                {
                    Ok(f) => Ok(Some(self.register_file(f))),
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            // io.fs.read_file(path, alloc)：整文件读取
            "read_file" => {
                let path = self.eval_path_arg(args, 0, span)?;
                match std::fs::read(&path) {
                    Ok(b) => Ok(Some(Value::str_bytes(b))),
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            // io.fs.read_all(file, alloc)：从句柄读整个文件（从头）
            "read_all" => {
                let fd = {
                    let f = self.eval(&args[0])?;
                    self.file_fd(&f, span)?
                };
                let file = self
                    .files
                    .get_mut(&fd)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?;
                file.seek(std::io::SeekFrom::Start(0))
                    .map_err(|e| RtError::msg("Io", format!("seek: {e}")))?;
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)
                    .map_err(|e| RtError::msg("Io", format!("read: {e}")))?;
                Ok(Some(Value::str_bytes(buf)))
            }
            // io.fs.write_all(file, data)：句柄写入
            "write_all" => {
                let fd = {
                    let f = self.eval(&args[0])?;
                    self.file_fd(&f, span)?
                };
                let data = self.eval_str_arg(args, 1, span)?;
                let file = self
                    .files
                    .get_mut(&fd)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?;
                file.write_all(&data)
                    .map_err(|e| RtError::msg("Io", format!("write: {e}")))?;
                Ok(Some(Value::Void))
            }
            // io.fs.append(path, data)：追加（缺失则创建）
            "append" => {
                let path = self.eval_path_arg(args, 0, span)?;
                let data = self.eval_str_arg(args, 1, span)?;
                match std::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&path)
                {
                    Ok(mut f) => {
                        f.write_all(&data)
                            .map_err(|e| RtError::msg("Io", format!("append: {e}")))?;
                        Ok(Some(Value::Void))
                    }
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            "remove" => {
                let path = self.eval_path_arg(args, 0, span)?;
                match std::fs::remove_file(&path) {
                    Ok(_) => Ok(Some(Value::Void)),
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            "rename" => {
                let from = self.eval_path_arg(args, 0, span)?;
                let to = self.eval_path_arg(args, 1, span)?;
                match std::fs::rename(&from, &to) {
                    Ok(_) => Ok(Some(Value::Void)),
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            // io.fs.read_int(path)：十进制文本 → i64
            "read_int" => {
                let path = self.eval_path_arg(args, 0, span)?;
                match std::fs::read(&path) {
                    Ok(b) => match String::from_utf8_lossy(&b).trim().parse::<i64>() {
                        Ok(n) => Ok(Some(Value::Int(n as i128))),
                        Err(_) => Ok(Some(self.err_val("InvalidFormat"))),
                    },
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            // io.fs.write_int(path, v)：十进制文本写入（创建/截断）
            "write_int" => {
                let path = self.eval_path_arg(args, 0, span)?;
                let v = self.eval_int_arg(args, 1, span)?;
                match std::fs::write(&path, v.to_string().as_bytes()) {
                    Ok(_) => Ok(Some(Value::Void)),
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            // G2（io 差异项）：io.fs.list_dir(path) / io.fs.list_dir(&dir, alloc)
            // ——返回 Vec(DirEntry)，每条 {name, is_dir}（不再只是名字数组）。
            // 双形态：第一参为 Str（路径）或 Dir 值（句柄）；Dir 值经 deref_value 剥 Ptr。
            "list_dir" => {
                let a0 = args
                    .get(0)
                    .ok_or_else(|| RtError::new("ArityMismatch", Some(span.clone())))?;
                let v0 = self.eval(a0)?;
                let v0 = self.deref_value(v0);
                match &v0 {
                    Value::Class(c) if c.borrow().name == "Dir" => {
                        let fd = self.dir_fd(&v0, span)?;
                        let path = self
                            .dirs
                            .get(&fd)
                            .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?
                            .clone();
                        let entries = self.list_dir_entries(&path)?;
                        Ok(Some(entries))
                    }
                    Value::Str(s) => {
                        let path = String::from_utf8_lossy(&s.borrow()).into_owned();
                        let entries = self.list_dir_entries(&path)?;
                        Ok(Some(entries))
                    }
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            // G2（io 差异项）：io.fs.open_dir(path) !Dir——目录句柄。
            // 读校验成功则注册 fd→path（供 dir.list_dir / dir.close），返回 Dir 值。
            "open_dir" => {
                let path = self.eval_path_arg(args, 0, span)?;
                match std::fs::read_dir(&path) {
                    Ok(_) => {
                        let fd = self.next_dir_fd;
                        self.next_dir_fd += 1;
                        self.dirs.insert(fd, path);
                        let mut fields = HashMap::new();
                        fields.insert("_fd".into(), Value::Int(fd as i128));
                        Ok(Some(Value::class("Dir", fields)))
                    }
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            _ => Ok(None),
        }
    }

    /// G2（io 差异项）：枚举目录路径为 Vec(DirEntry)——每条 = {name: 文件名, is_dir: 是否目录}。
    /// 供 io.fs.list_dir（路径/句柄双形态）与 dir.list_dir(alloc) 共用。
    pub(crate) fn list_dir_entries(&mut self, path: &str) -> Result<Value> {
        match std::fs::read_dir(path) {
            Ok(rd) => {
                let entries: Vec<Value> = rd
                    .flatten()
                    .map(|e| {
                        let mut fields = HashMap::new();
                        fields.insert("name".into(), Value::str(&e.file_name().to_string_lossy()));
                        fields.insert(
                            "is_dir".into(),
                            Value::Bool(e.file_type().map(|t| t.is_dir()).unwrap_or(false)),
                        );
                        Value::class("DirEntry", fields)
                    })
                    .collect();
                Ok(Value::arr(entries))
            }
            Err(e) => Ok(self.err_val(&self.io_error_name(&e))),
        }
    }

    /// Dir 值 → 注册表 fd（_fd 字段；先 deref_value 剥 Ptr）
    pub(crate) fn dir_fd(&self, v: &Value, span: &Span) -> Result<i64> {
        match self.deref_value(v.clone()) {
            Value::Class(c) if c.borrow().name == "Dir" => match c.borrow().fields.get("_fd") {
                Some(Value::Int(fd)) => Ok(*fd as i64),
                _ => Err(RtError::new("BadFd", Some(span.clone()))),
            },
            _ => Err(RtError::new("TypeError", Some(span.clone()))),
        }
    }

    /// G2（io 差异项）：Dir 类方法分派——`dir.list_dir(alloc) !Vec(DirEntry)`
    ///（重开枚举）/ `dir.close()`（注销句柄）。
    pub(crate) fn call_dir_method(
        &mut self,
        field: &str,
        v: &Value,
        _args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        let fd = self.dir_fd(v, span)?;
        match field {
            "list_dir" => {
                let path = self
                    .dirs
                    .get(&fd)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?
                    .clone();
                let entries = self.list_dir_entries(&path)?;
                Ok(Some(entries))
            }
            "close" => {
                self.dirs.remove(&fd);
                Ok(Some(Value::Void))
            }
            _ => Ok(None),
        }
    }

    // ---------- G3（E3.2 ipc）：管道 + 共享内存 ----------

    /// io.ipc 命名空间分派：`pipe()`（匿名管道 → `[reader, writer]`）/ `shm(name, size) !Shm`
    ///（命名共享内存）。进程内 IPC 原语——真实 OS 进程/共享内存依赖 FFI（G6 跳过）与
    /// 进程模块（无），以注册表 + 类名分派承载（Q20 双语），协作式模型下读写均不阻塞。
    pub(crate) fn call_ipc_method(
        &mut self,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        match field {
            "pipe" => {
                let pid = self.next_pipe_fd;
                self.next_pipe_fd += 1;
                self.pipes.insert(
                    pid,
                    Rc::new(RefCell::new(Pipe {
                        buf: Vec::new(),
                        writer_open: true,
                    })),
                );
                let reader = {
                    let mut fld = HashMap::new();
                    fld.insert("pipe".into(), Value::Int(pid as i128));
                    Value::class("PipeReader", fld)
                };
                let writer = {
                    let mut fld = HashMap::new();
                    fld.insert("pipe".into(), Value::Int(pid as i128));
                    Value::class("PipeWriter", fld)
                };
                Ok(Some(Value::arr(vec![reader, writer])))
            }
            "shm" => {
                // name 参数当前仅用于形态约束（命名共享内存的标识语义），区域本体按 id 注册
                let _name = self.eval_path_arg(args, 0, span)?;
                let size = self.eval_int_arg(args, 1, span)?;
                let size = size.max(0) as usize;
                let id = self.next_shm_fd;
                self.next_shm_fd += 1;
                self.shms.insert(
                    id,
                    Rc::new(RefCell::new(Shm {
                        data: vec![0u8; size],
                    })),
                );
                let mut fld = HashMap::new();
                fld.insert("shm".into(), Value::Int(id as i128));
                Ok(Some(Value::class("Shm", fld)))
            }
            _ => Ok(None),
        }
    }

    /// Pipe 值 → 管道 id（`pipe` 字段；先 deref_value 剥 Ptr）
    pub(crate) fn pipe_id_of(&self, v: &Value, span: &Span) -> Result<i64> {
        match self.deref_value(v.clone()) {
            Value::Class(c)
                if c.borrow().name == "PipeReader" || c.borrow().name == "PipeWriter" =>
            {
                match c.borrow().fields.get("pipe") {
                    Some(Value::Int(id)) => Ok(*id as i64),
                    _ => Err(RtError::new("BadFd", Some(span.clone()))),
                }
            }
            _ => Err(RtError::new("TypeError", Some(span.clone()))),
        }
    }

    /// Pipe 类方法分派（is_reader 区分读写端）：写端 `write(data) !void` / `close() !void`；
    /// 读端 `read(alloc) !&[u8]`（排空可读字节；空且写端开 → 空切片，不阻塞）/
    /// `read_all(alloc) !&[u8]` / `is_closed() bool` / `close() !void`。
    /// `close` 幂等：读端 close 注销注册表（管道随之拆除）、写端 close 仅置 writer_open=false；
    /// 管道已注销后再 close 为 no-op（不报 BadFd）。
    pub(crate) fn call_pipe_method(
        &mut self,
        is_reader: bool,
        field: &str,
        v: &Value,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        let pid = self.pipe_id_of(v, span)?;
        match field {
            "close" => {
                if let Some(pipe) = self.pipes.get(&pid).cloned() {
                    if is_reader {
                        self.pipes.remove(&pid);
                    } else {
                        pipe.borrow_mut().writer_open = false;
                    }
                }
                Ok(Some(Value::Void))
            }
            "write" if !is_reader => {
                let data = self.eval_str_arg(args, 0, span)?;
                let pipe = self
                    .pipes
                    .get(&pid)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?
                    .clone();
                pipe.borrow_mut().buf.extend_from_slice(&data);
                Ok(Some(Value::Void))
            }
            "read" | "read_all" if is_reader => {
                let pipe = self
                    .pipes
                    .get(&pid)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?
                    .clone();
                let out = std::mem::take(&mut pipe.borrow_mut().buf);
                Ok(Some(Value::str_bytes(out)))
            }
            "is_closed" if is_reader => {
                let pipe = self
                    .pipes
                    .get(&pid)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?
                    .clone();
                let closed = !pipe.borrow().writer_open;
                Ok(Some(Value::Bool(closed)))
            }
            _ => Ok(None),
        }
    }

    /// Shm 值 → 共享内存 id（`shm` 字段）
    pub(crate) fn shm_id_of(&self, v: &Value, span: &Span) -> Result<i64> {
        match self.deref_value(v.clone()) {
            Value::Class(c) if c.borrow().name == "Shm" => match c.borrow().fields.get("shm") {
                Some(Value::Int(id)) => Ok(*id as i64),
                _ => Err(RtError::new("BadFd", Some(span.clone()))),
            },
            _ => Err(RtError::new("TypeError", Some(span.clone()))),
        }
    }

    /// Shm 类方法分派：`write(data) !void`（覆盖内容，截断到 size）/ `read(alloc) !&[u8]`
    /// / `close() !void`（注销句柄）。
    pub(crate) fn call_shm_method(
        &mut self,
        field: &str,
        v: &Value,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        let id = self.shm_id_of(v, span)?;
        let shm = self
            .shms
            .get(&id)
            .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?
            .clone();
        let mut s = shm.borrow_mut();
        match field {
            "write" => {
                let data = self.eval_str_arg(args, 0, span)?;
                let cap = s.data.capacity();
                let take = data.len().min(cap);
                s.data.clear();
                s.data.extend_from_slice(&data[..take]);
                Ok(Some(Value::Void))
            }
            "read" => Ok(Some(Value::str_bytes(s.data.clone()))),
            "close" => {
                self.shms.remove(&id);
                Ok(Some(Value::Void))
            }
            _ => Ok(None),
        }
    }

    // ---------- G4（E3.3 storage）文件持久化键值存储 ----------

    /// io.storage.open(path) !KvStore——打开/创建文件持久化的键值存储。
    /// 文件存在则装载既有条目（二进制格式：u32 键长 + 键 + u32 值长 + 值，小端）；
    /// 缺文件视为空库（close 时创建）。KvStore 值持 `store` id → 注册表。
    pub(crate) fn call_storage_method(
        &mut self,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        match field {
            "open" => {
                let path = self.eval_path_arg(args, 0, span)?;
                let mut entries = HashMap::new();
                if let Ok(bytes) = std::fs::read(&path) {
                    let mut i = 0usize;
                    while i < bytes.len() {
                        // 格式：u32 键长 + 键 + u32 值长 + 值（vlen 不紧跟 klen——键在中间）
                        if i + 4 > bytes.len() {
                            return Ok(Some(self.err_val("InvalidFormat")));
                        }
                        let klen = u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
                        i += 4;
                        if i + klen + 4 > bytes.len() {
                            return Ok(Some(self.err_val("InvalidFormat")));
                        }
                        let key = bytes[i..i + klen].to_vec();
                        i += klen;
                        let vlen = u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
                        i += 4;
                        if i + vlen > bytes.len() {
                            return Ok(Some(self.err_val("InvalidFormat")));
                        }
                        let val = bytes[i..i + vlen].to_vec();
                        entries.insert(key, val);
                        i += vlen;
                    }
                }
                let id = self.next_store_fd;
                self.next_store_fd += 1;
                self.stores
                    .insert(id, Rc::new(RefCell::new(KvStore { path, entries })));
                let mut fld = HashMap::new();
                fld.insert("store".into(), Value::Int(id as i128));
                Ok(Some(Value::class("KvStore", fld)))
            }
            _ => Ok(None),
        }
    }

    /// KvStore 值 → 注册表 id（`store` 字段；先 deref_value 剥 Ptr）
    pub(crate) fn store_id_of(&self, v: &Value, span: &Span) -> Result<i64> {
        match self.deref_value(v.clone()) {
            Value::Class(c) if c.borrow().name == "KvStore" => match c.borrow().fields.get("store")
            {
                Some(Value::Int(id)) => Ok(*id as i64),
                _ => Err(RtError::new("BadFd", Some(span.clone()))),
            },
            _ => Err(RtError::new("TypeError", Some(span.clone()))),
        }
    }

    /// KvStore 落盘：二进制格式（u32 键长 + 键 + u32 值长 + 值，小端）写回 path。
    pub(crate) fn persist_store(&self, store: &KvStore) -> std::io::Result<()> {
        let mut out = Vec::new();
        for (k, v) in &store.entries {
            out.extend_from_slice(&(k.len() as u32).to_le_bytes());
            out.extend_from_slice(k);
            out.extend_from_slice(&(v.len() as u32).to_le_bytes());
            out.extend_from_slice(v);
        }
        std::fs::write(&store.path, out)
    }

    /// KvStore 实例方法：`put(key, value) !void` / `get(key) !?&[u8]`（缺失 → null）/
    /// `contains(key) bool` / `remove(key) !void`（幂等）/ `len() usize` /
    /// `close() !void`（落盘 + 注销注册表；已关闭再 close 为 no-op）。
    pub(crate) fn call_store_method(
        &mut self,
        field: &str,
        v: &Value,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        let id = self.store_id_of(v, span)?;
        match field {
            "put" => {
                let key = self.eval_str_arg(args, 0, span)?;
                let value = self.eval_str_arg(args, 1, span)?;
                let store = self
                    .stores
                    .get(&id)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?
                    .clone();
                store.borrow_mut().entries.insert(key, value);
                Ok(Some(Value::Void))
            }
            "get" => {
                let key = self.eval_str_arg(args, 0, span)?;
                let store = self
                    .stores
                    .get(&id)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?
                    .clone();
                let s = store.borrow();
                match s.entries.get(&key) {
                    Some(val) => Ok(Some(Value::Opt(Some(Rc::new(Value::str_bytes(
                        val.clone(),
                    )))))),
                    None => Ok(Some(Value::Opt(None))),
                }
            }
            "contains" => {
                let key = self.eval_str_arg(args, 0, span)?;
                let store = self
                    .stores
                    .get(&id)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?
                    .clone();
                let has = store.borrow().entries.contains_key(&key);
                Ok(Some(Value::Bool(has)))
            }
            "remove" => {
                let key = self.eval_str_arg(args, 0, span)?;
                let store = self
                    .stores
                    .get(&id)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?
                    .clone();
                store.borrow_mut().entries.remove(&key);
                Ok(Some(Value::Void))
            }
            "len" => {
                let store = self
                    .stores
                    .get(&id)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?
                    .clone();
                let n = store.borrow().entries.len() as i128;
                Ok(Some(Value::Int(n)))
            }
            "close" => {
                if let Some(store) = self.stores.remove(&id) {
                    let s = store.borrow();
                    if let Err(e) = self.persist_store(&s) {
                        return Ok(Some(self.err_val(&self.io_error_name(&e))));
                    }
                }
                Ok(Some(Value::Void))
            }
            _ => Ok(None),
        }
    }

    // ---------- G4（E3.3 archive）LZ77 压缩 ----------

    /// io.archive.compress(data) !&[u8] / io.archive.decompress(data) !&[u8]——
    /// LZ77 压缩（compress::compress/decompress）。滑动窗口 4KB，反向引用 3..=258 字节；
    /// round-trip 对任意输入保真；非法压缩数据 → error.InvalidFormat。
    pub(crate) fn call_archive_method(
        &mut self,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        match field {
            "compress" => {
                let data = self.eval_str_arg(args, 0, span)?;
                Ok(Some(Value::str_bytes(hc::compress::compress(&data))))
            }
            "decompress" => {
                let data = self.eval_str_arg(args, 0, span)?;
                match hc::compress::decompress(&data) {
                    Ok(out) => Ok(Some(Value::str_bytes(out))),
                    Err(_) => Ok(Some(self.err_val("InvalidFormat"))),
                }
            }
            _ => Ok(None),
        }
    }

    // ---------- A6：标准库数据结构——Bitmap 位图 ----------

    /// io.bitmap.init(nbits) !Bitmap——创建 nbits 位的 Bitmap（全部清零）。
    pub(crate) fn call_bitmap_ns_method(
        &mut self,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        match field {
            "init" => {
                let nbits = self.eval_int_arg(args, 0, span)?;
                if nbits < 0 {
                    return Ok(Some(self.err_val("InvalidArgument")));
                }
                let nwords = (nbits as usize).div_ceil(64);
                let items: Vec<Rc<RefCell<Value>>> = (0..nwords)
                    .map(|_| Rc::new(RefCell::new(Value::Int(0))))
                    .collect();
                let words = Value::Arr(Rc::new(RefCell::new(items)));
                let mut fields = HashMap::new();
                fields.insert("words".into(), words);
                Ok(Some(Value::class("Bitmap", fields)))
            }
            _ => Ok(None),
        }
    }

    /// Bitmap 实例方法：set/get/clear/count/len。
    pub(crate) fn call_bitmap_method(
        &mut self,
        method: &str,
        v: &Value,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        let words_val = match self.deref_value(v.clone()) {
            Value::Class(c) => match c.borrow().fields.get("words") {
                Some(val) => val.clone(),
                None => return Ok(Some(self.err_val("NoField"))),
            },
            _ => return Ok(Some(self.err_val("TypeError"))),
        };
        let words_arr = match &words_val {
            Value::Arr(arr) => arr.clone(),
            _ => return Ok(Some(self.err_val("TypeError"))),
        };

        match method {
            "set" => {
                let idx = self.eval_int_arg(args, 0, span)?;
                if idx < 0 {
                    return Ok(Some(self.err_val("InvalidArgument")));
                }
                let word_idx = (idx as usize) >> 6;
                let bit = 1u64 << ((idx as usize) & 63);
                let arr = words_arr.borrow_mut();
                if let Some(cell) = arr.get(word_idx) {
                    let val = match *cell.borrow() {
                        Value::Int(v) => v as u64,
                        _ => 0,
                    };
                    *cell.borrow_mut() = Value::Int((val | bit) as i128);
                }
                Ok(Some(Value::Void))
            }
            "get" => {
                let idx = self.eval_int_arg(args, 0, span)?;
                if idx < 0 {
                    return Ok(Some(Value::Bool(false)));
                }
                let word_idx = (idx as usize) >> 6;
                let bit = 1u64 << ((idx as usize) & 63);
                let arr = words_arr.borrow();
                let result = arr
                    .get(word_idx)
                    .map_or(false, |cell| match *cell.borrow() {
                        Value::Int(v) => (v as u64) & bit != 0,
                        _ => false,
                    });
                Ok(Some(Value::Bool(result)))
            }
            "clear" => {
                let idx = self.eval_int_arg(args, 0, span)?;
                if idx < 0 {
                    return Ok(Some(self.err_val("InvalidArgument")));
                }
                let word_idx = (idx as usize) >> 6;
                let bit = 1u64 << ((idx as usize) & 63);
                let arr = words_arr.borrow_mut();
                if let Some(cell) = arr.get(word_idx) {
                    let val = match *cell.borrow() {
                        Value::Int(v) => v as u64,
                        _ => 0,
                    };
                    *cell.borrow_mut() = Value::Int((val & !bit) as i128);
                }
                Ok(Some(Value::Void))
            }
            "count" => {
                let arr = words_arr.borrow();
                let total: u64 = arr
                    .iter()
                    .map(|cell| match *cell.borrow() {
                        Value::Int(v) => (v as u64).count_ones() as u64,
                        _ => 0,
                    })
                    .sum();
                Ok(Some(Value::Int(total as i128)))
            }
            "len" => {
                let nwords = words_arr.borrow().len();
                Ok(Some(Value::Int((nwords * 64) as i128)))
            }
            _ => Ok(None),
        }
    }

    // ---------- A6：RingBuf 环形缓冲 ----------

    /// io.ringbuf.init(cap) !RingBuf——创建容量 cap 的 RingBuf。
    pub(crate) fn call_ringbuf_ns_method(
        &mut self,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        match field {
            "init" => {
                let cap = self.eval_int_arg(args, 0, span)?;
                if cap < 0 {
                    return Ok(Some(self.err_val("InvalidArgument")));
                }
                let cap = cap as usize;
                let mut fields = HashMap::new();
                fields.insert("buf".into(), Value::Arr(Rc::new(RefCell::new(Vec::new()))));
                fields.insert("head".into(), Value::Int(0));
                fields.insert("len".into(), Value::Int(0));
                fields.insert("cap".into(), Value::Int(cap as i128));
                Ok(Some(Value::class("RingBuf", fields)))
            }
            _ => Ok(None),
        }
    }

    /// RingBuf 实例方法：push/pop/len/capacity/is_full/is_empty/clear/peek。
    pub(crate) fn call_ringbuf_method(
        &mut self,
        method: &str,
        v: &Value,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        let class_data = match self.deref_value(v.clone()) {
            Value::Class(c) => c,
            _ => return Ok(Some(self.err_val("TypeError"))),
        };

        // 提取字段值（避免在同一个 match arm 中同时 borrow 和 borrow_mut）
        let get_i = |name: &str| -> Option<i128> {
            match class_data.borrow().fields.get(name)? {
                Value::Int(n) => Some(*n),
                _ => None,
            }
        };
        let get_arr = |name: &str| -> Option<Rc<RefCell<Vec<Rc<RefCell<Value>>>>>> {
            match class_data.borrow().fields.get(name)? {
                Value::Arr(a) => Some(a.clone()),
                _ => None,
            }
        };

        match method {
            "push" => {
                let val = self.eval(&args[0])?;
                let cap = get_i("cap").unwrap_or(0) as usize;
                let cur_len = get_i("len").unwrap_or(0) as usize;
                if cur_len >= cap {
                    return Ok(Some(Value::Bool(false)));
                }
                let head = get_i("head").unwrap_or(0) as usize;
                let idx = (head + cur_len) % cap;
                if let Some(buf) = get_arr("buf") {
                    let mut b = buf.borrow_mut();
                    if idx < b.len() {
                        b[idx] = Rc::new(RefCell::new(val));
                    } else {
                        b.push(Rc::new(RefCell::new(val)));
                    }
                }
                class_data
                    .borrow_mut()
                    .fields
                    .insert("len".into(), Value::Int((cur_len + 1) as i128));
                Ok(Some(Value::Bool(true)))
            }
            "pop" => {
                let cur_len = get_i("len").unwrap_or(0) as usize;
                if cur_len == 0 {
                    return Ok(Some(Value::Opt(None)));
                }
                let head = get_i("head").unwrap_or(0) as usize;
                let cap = get_i("cap").unwrap_or(0) as usize;
                let val = if let Some(buf) = get_arr("buf") {
                    let b = buf.borrow();
                    b.get(head)
                        .map(|c| c.borrow().clone())
                        .unwrap_or(Value::Void)
                } else {
                    Value::Void
                };
                class_data
                    .borrow_mut()
                    .fields
                    .insert("head".into(), Value::Int(((head + 1) % cap) as i128));
                class_data
                    .borrow_mut()
                    .fields
                    .insert("len".into(), Value::Int((cur_len - 1) as i128));
                Ok(Some(val))
            }
            "len" => Ok(Some(get_i("len").map(Value::Int).unwrap_or(Value::Int(0)))),
            "capacity" => Ok(Some(get_i("cap").map(Value::Int).unwrap_or(Value::Int(0)))),
            "is_full" => {
                let cur_len = get_i("len").unwrap_or(0) as usize;
                let cap = get_i("cap").unwrap_or(0) as usize;
                Ok(Some(Value::Bool(cur_len >= cap)))
            }
            "is_empty" => {
                let cur_len = get_i("len").unwrap_or(0);
                Ok(Some(Value::Bool(cur_len == 0)))
            }
            "clear" => {
                let mut f = class_data.borrow_mut();
                f.fields.insert("head".into(), Value::Int(0));
                f.fields.insert("len".into(), Value::Int(0));
                Ok(Some(Value::Void))
            }
            "peek" => {
                let idx = self.eval_int_arg(args, 0, span)?;
                if idx < 0 {
                    return Ok(Some(Value::Opt(None)));
                }
                let cur_len = get_i("len").unwrap_or(0) as usize;
                if (idx as usize) >= cur_len {
                    return Ok(Some(Value::Opt(None)));
                }
                let head = get_i("head").unwrap_or(0) as usize;
                let cap = get_i("cap").unwrap_or(0) as usize;
                let pos = (head + idx as usize) % cap;
                let val = if let Some(buf) = get_arr("buf") {
                    let b = buf.borrow();
                    b.get(pos)
                        .map(|c| c.borrow().clone())
                        .unwrap_or(Value::Void)
                } else {
                    Value::Void
                };
                Ok(Some(val))
            }
            _ => Ok(None),
        }
    }

    // ---------- A6：PageMem 页内存池 ----------

    /// io.pagemem.init(num_pages) !PageMem——创建 num_pages 页的 PageMem。
    pub(crate) fn call_pagemem_ns_method(
        &mut self,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        match field {
            "init" => {
                let num = self.eval_int_arg(args, 0, span)?;
                if num < 0 {
                    return Ok(Some(self.err_val("InvalidArgument")));
                }
                let mut fields = HashMap::new();
                // 空闲页栈：全部页索引（LIFO）
                let n = num as usize;
                let items: Vec<Rc<RefCell<Value>>> = (0..n)
                    .rev()
                    .map(|i| Rc::new(RefCell::new(Value::Int(i as i128))))
                    .collect();
                fields.insert("free".into(), Value::Arr(Rc::new(RefCell::new(items))));
                fields.insert("total".into(), Value::Int(num));
                Ok(Some(Value::class("PageMem", fields)))
            }
            _ => Ok(None),
        }
    }

    /// PageMem 实例方法：alloc/free/available/total。
    pub(crate) fn call_pagemem_method(
        &mut self,
        method: &str,
        v: &Value,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        let class_data = match self.deref_value(v.clone()) {
            Value::Class(c) => c,
            _ => return Ok(Some(self.err_val("TypeError"))),
        };

        let get_i = |name: &str| -> Option<i128> {
            match class_data.borrow().fields.get(name)? {
                Value::Int(n) => Some(*n),
                _ => None,
            }
        };
        let get_arr = |name: &str| -> Option<Rc<RefCell<Vec<Rc<RefCell<Value>>>>>> {
            match class_data.borrow().fields.get(name)? {
                Value::Arr(a) => Some(a.clone()),
                _ => None,
            }
        };

        match method {
            "alloc" => {
                if let Some(free) = get_arr("free") {
                    let mut f = free.borrow_mut();
                    if let Some(cell) = f.pop() {
                        let val = cell.borrow().clone();
                        return Ok(Some(val));
                    }
                }
                Ok(Some(Value::Opt(None)))
            }
            "free" => {
                let idx = self.eval_int_arg(args, 0, span)?;
                if idx < 0 {
                    return Ok(Some(self.err_val("InvalidArgument")));
                }
                let total = get_i("total").unwrap_or(0) as usize;
                if (idx as usize) >= total {
                    return Ok(Some(Value::Void));
                }
                // 检查是否已空闲（double-free 安全）
                if let Some(free) = get_arr("free") {
                    let f = free.borrow();
                    let already_free = f.iter().any(|cell| match *cell.borrow() {
                        Value::Int(v) => v == idx,
                        _ => false,
                    });
                    if !already_free {
                        drop(f);
                        free.borrow_mut()
                            .push(Rc::new(RefCell::new(Value::Int(idx))));
                    }
                }
                Ok(Some(Value::Void))
            }
            "available" => {
                let n = if let Some(free) = get_arr("free") {
                    free.borrow().len()
                } else {
                    0
                };
                Ok(Some(Value::Int(n as i128)))
            }
            "total" => Ok(Some(
                get_i("total").map(Value::Int).unwrap_or(Value::Int(0)),
            )),
            _ => Ok(None),
        }
    }

    // ---------- A6：IntrList 侵入式链表 ----------

    /// io.intrlist.init() !IntrList——创建空链表。
    pub(crate) fn call_intrlist_ns_method(
        &mut self,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        match field {
            "init" => {
                if !args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let mut fields = HashMap::new();
                fields.insert("prev".into(), Value::Arr(Rc::new(RefCell::new(Vec::new()))));
                fields.insert("next".into(), Value::Arr(Rc::new(RefCell::new(Vec::new()))));
                fields.insert("vals".into(), Value::Arr(Rc::new(RefCell::new(Vec::new()))));
                fields.insert("head".into(), Value::Int(-1));
                fields.insert("tail".into(), Value::Int(-1));
                fields.insert("len".into(), Value::Int(0));
                fields.insert("free".into(), Value::Arr(Rc::new(RefCell::new(Vec::new()))));
                Ok(Some(Value::class("IntrList", fields)))
            }
            _ => Ok(None),
        }
    }

    /// IntrList 实例方法：push_front/pop_front/push_back/pop_back/remove/len/is_empty/clear。
    pub(crate) fn call_intrlist_method(
        &mut self,
        method: &str,
        v: &Value,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        let class_data = match self.deref_value(v.clone()) {
            Value::Class(c) => c,
            _ => return Ok(Some(self.err_val("TypeError"))),
        };

        let get_i = |name: &str| -> Option<i128> {
            match class_data.borrow().fields.get(name)? {
                Value::Int(n) => Some(*n),
                _ => None,
            }
        };
        let get_arr = |name: &str| -> Option<Rc<RefCell<Vec<Rc<RefCell<Value>>>>>> {
            match class_data.borrow().fields.get(name)? {
                Value::Arr(a) => Some(a.clone()),
                _ => None,
            }
        };

        match method {
            "push_front" => {
                let val = self.eval(&args[0])?;
                // 分配节点索引
                let idx = if let Some(free) = get_arr("free") {
                    let mut f = free.borrow_mut();
                    f.pop().map(|cell| match *cell.borrow() {
                        Value::Int(n) => n as usize,
                        _ => unreachable!(),
                    })
                } else {
                    None
                };
                let idx = idx.unwrap_or_else(|| {
                    // 新分配：扩展所有数组
                    let n = if let Some(prev) = get_arr("prev") {
                        prev.borrow().len()
                    } else {
                        0
                    };
                    n
                });
                let head = get_i("head").unwrap_or(-1) as isize;
                // 设置 prev[idx] = -1, next[idx] = old_head, vals[idx] = val
                if let Some(prev) = get_arr("prev") {
                    let mut p = prev.borrow_mut();
                    if idx >= p.len() {
                        p.push(Rc::new(RefCell::new(Value::Int(-1))));
                    } else {
                        p[idx] = Rc::new(RefCell::new(Value::Int(-1)));
                    }
                }
                if let Some(next) = get_arr("next") {
                    let mut n = next.borrow_mut();
                    if idx >= n.len() {
                        n.push(Rc::new(RefCell::new(Value::Int(head as i128))));
                    } else {
                        n[idx] = Rc::new(RefCell::new(Value::Int(head as i128)));
                    }
                }
                if let Some(vals) = get_arr("vals") {
                    let mut v_arr = vals.borrow_mut();
                    if idx >= v_arr.len() {
                        v_arr.push(Rc::new(RefCell::new(val)));
                    } else {
                        v_arr[idx] = Rc::new(RefCell::new(val));
                    }
                }
                // 如果 old_head 存在，设置 prev[old_head] = idx
                if head >= 0 {
                    if let Some(prev) = get_arr("prev") {
                        let p = prev.borrow_mut();
                        if let Some(cell) = p.get(head as usize) {
                            *cell.borrow_mut() = Value::Int(idx as i128);
                        }
                    }
                } else {
                    // 空链表，tail = idx
                    class_data
                        .borrow_mut()
                        .fields
                        .insert("tail".into(), Value::Int(idx as i128));
                }
                class_data
                    .borrow_mut()
                    .fields
                    .insert("head".into(), Value::Int(idx as i128));
                let cur_len = get_i("len").unwrap_or(0);
                class_data
                    .borrow_mut()
                    .fields
                    .insert("len".into(), Value::Int(cur_len + 1));
                Ok(Some(Value::Int(idx as i128)))
            }
            "pop_front" => {
                let head = get_i("head").unwrap_or(-1);
                if head < 0 {
                    return Ok(Some(Value::Opt(None)));
                }
                let head = head as usize;
                // 读取值
                let val = if let Some(vals) = get_arr("vals") {
                    let v = vals.borrow();
                    v.get(head)
                        .map(|c| c.borrow().clone())
                        .unwrap_or(Value::Void)
                } else {
                    Value::Void
                };
                // 获取 next 索引
                let next_idx = if let Some(next) = get_arr("next") {
                    let n = next.borrow();
                    n.get(head)
                        .map(|c| match *c.borrow() {
                            Value::Int(v) => v,
                            _ => -1,
                        })
                        .unwrap_or(-1)
                } else {
                    -1
                };
                // 释放 head 索引
                if let Some(free) = get_arr("free") {
                    free.borrow_mut()
                        .push(Rc::new(RefCell::new(Value::Int(head as i128))));
                }
                // 更新 head
                if next_idx >= 0 {
                    if let Some(prev) = get_arr("prev") {
                        let p = prev.borrow_mut();
                        if let Some(cell) = p.get(next_idx as usize) {
                            *cell.borrow_mut() = Value::Int(-1);
                        }
                    }
                    class_data
                        .borrow_mut()
                        .fields
                        .insert("head".into(), Value::Int(next_idx));
                } else {
                    class_data
                        .borrow_mut()
                        .fields
                        .insert("head".into(), Value::Int(-1));
                    class_data
                        .borrow_mut()
                        .fields
                        .insert("tail".into(), Value::Int(-1));
                }
                let cur_len = get_i("len").unwrap_or(0);
                class_data
                    .borrow_mut()
                    .fields
                    .insert("len".into(), Value::Int(cur_len - 1));
                Ok(Some(val))
            }
            "push_back" => {
                let val = self.eval(&args[0])?;
                // 分配节点索引
                let idx = if let Some(free) = get_arr("free") {
                    let mut f = free.borrow_mut();
                    f.pop().map(|cell| match *cell.borrow() {
                        Value::Int(n) => n as usize,
                        _ => unreachable!(),
                    })
                } else {
                    None
                };
                let idx = idx.unwrap_or_else(|| {
                    if let Some(prev) = get_arr("prev") {
                        prev.borrow().len()
                    } else {
                        0
                    }
                });
                let tail = get_i("tail").unwrap_or(-1) as isize;
                // 设置 prev[idx] = old_tail, next[idx] = -1, vals[idx] = val
                if let Some(prev) = get_arr("prev") {
                    let mut p = prev.borrow_mut();
                    if idx >= p.len() {
                        p.push(Rc::new(RefCell::new(Value::Int(tail as i128))));
                    } else {
                        p[idx] = Rc::new(RefCell::new(Value::Int(tail as i128)));
                    }
                }
                if let Some(next) = get_arr("next") {
                    let mut n = next.borrow_mut();
                    if idx >= n.len() {
                        n.push(Rc::new(RefCell::new(Value::Int(-1))));
                    } else {
                        n[idx] = Rc::new(RefCell::new(Value::Int(-1)));
                    }
                }
                if let Some(vals) = get_arr("vals") {
                    let mut v_arr = vals.borrow_mut();
                    if idx >= v_arr.len() {
                        v_arr.push(Rc::new(RefCell::new(val)));
                    } else {
                        v_arr[idx] = Rc::new(RefCell::new(val));
                    }
                }
                // 如果 old_tail 存在，设置 next[old_tail] = idx
                if tail >= 0 {
                    if let Some(next) = get_arr("next") {
                        let n = next.borrow_mut();
                        if let Some(cell) = n.get(tail as usize) {
                            *cell.borrow_mut() = Value::Int(idx as i128);
                        }
                    }
                } else {
                    // 空链表，head = idx
                    class_data
                        .borrow_mut()
                        .fields
                        .insert("head".into(), Value::Int(idx as i128));
                }
                class_data
                    .borrow_mut()
                    .fields
                    .insert("tail".into(), Value::Int(idx as i128));
                let cur_len = get_i("len").unwrap_or(0);
                class_data
                    .borrow_mut()
                    .fields
                    .insert("len".into(), Value::Int(cur_len + 1));
                Ok(Some(Value::Int(idx as i128)))
            }
            "pop_back" => {
                let tail = get_i("tail").unwrap_or(-1);
                if tail < 0 {
                    return Ok(Some(Value::Opt(None)));
                }
                let tail = tail as usize;
                // 读取值
                let val = if let Some(vals) = get_arr("vals") {
                    let v = vals.borrow();
                    v.get(tail)
                        .map(|c| c.borrow().clone())
                        .unwrap_or(Value::Void)
                } else {
                    Value::Void
                };
                // 获取 prev 索引
                let prev_idx = if let Some(prev) = get_arr("prev") {
                    let p = prev.borrow();
                    p.get(tail)
                        .map(|c| match *c.borrow() {
                            Value::Int(v) => v,
                            _ => -1,
                        })
                        .unwrap_or(-1)
                } else {
                    -1
                };
                // 释放 tail 索引
                if let Some(free) = get_arr("free") {
                    free.borrow_mut()
                        .push(Rc::new(RefCell::new(Value::Int(tail as i128))));
                }
                // 更新 tail
                if prev_idx >= 0 {
                    if let Some(next) = get_arr("next") {
                        let n = next.borrow_mut();
                        if let Some(cell) = n.get(prev_idx as usize) {
                            *cell.borrow_mut() = Value::Int(-1);
                        }
                    }
                    class_data
                        .borrow_mut()
                        .fields
                        .insert("tail".into(), Value::Int(prev_idx));
                } else {
                    class_data
                        .borrow_mut()
                        .fields
                        .insert("head".into(), Value::Int(-1));
                    class_data
                        .borrow_mut()
                        .fields
                        .insert("tail".into(), Value::Int(-1));
                }
                let cur_len = get_i("len").unwrap_or(0);
                class_data
                    .borrow_mut()
                    .fields
                    .insert("len".into(), Value::Int(cur_len - 1));
                Ok(Some(val))
            }
            "remove" => {
                let idx = self.eval_int_arg(args, 0, span)?;
                if idx < 0 {
                    return Ok(Some(Value::Opt(None)));
                }
                let idx = idx as usize;
                // 检查节点是否存在（通过 vals 是否有值且节点未被移除）
                let node_exists = if let Some(vals) = get_arr("vals") {
                    let v = vals.borrow();
                    v.get(idx).is_some()
                } else {
                    false
                };
                if !node_exists {
                    return Ok(Some(Value::Opt(None)));
                }
                // 读取 prev 和 next
                let prev_idx = if let Some(prev) = get_arr("prev") {
                    let p = prev.borrow();
                    p.get(idx)
                        .map(|c| match *c.borrow() {
                            Value::Int(v) => v,
                            _ => -1,
                        })
                        .unwrap_or(-1)
                } else {
                    -1
                };
                let next_idx = if let Some(next) = get_arr("next") {
                    let n = next.borrow();
                    n.get(idx)
                        .map(|c| match *c.borrow() {
                            Value::Int(v) => v,
                            _ => -1,
                        })
                        .unwrap_or(-1)
                } else {
                    -1
                };
                // 读取值
                let val = if let Some(vals) = get_arr("vals") {
                    let v = vals.borrow();
                    v.get(idx)
                        .map(|c| c.borrow().clone())
                        .unwrap_or(Value::Void)
                } else {
                    Value::Void
                };
                // 更新前后节点的链接
                if prev_idx >= 0 {
                    if let Some(next) = get_arr("next") {
                        let n = next.borrow_mut();
                        if let Some(cell) = n.get(prev_idx as usize) {
                            *cell.borrow_mut() = Value::Int(next_idx);
                        }
                    }
                } else {
                    class_data
                        .borrow_mut()
                        .fields
                        .insert("head".into(), Value::Int(next_idx));
                }
                if next_idx >= 0 {
                    if let Some(prev) = get_arr("prev") {
                        let p = prev.borrow_mut();
                        if let Some(cell) = p.get(next_idx as usize) {
                            *cell.borrow_mut() = Value::Int(prev_idx);
                        }
                    }
                } else {
                    class_data
                        .borrow_mut()
                        .fields
                        .insert("tail".into(), Value::Int(prev_idx));
                }
                // 释放索引
                if let Some(free) = get_arr("free") {
                    free.borrow_mut()
                        .push(Rc::new(RefCell::new(Value::Int(idx as i128))));
                }
                let cur_len = get_i("len").unwrap_or(0);
                class_data
                    .borrow_mut()
                    .fields
                    .insert("len".into(), Value::Int(cur_len - 1));
                Ok(Some(val))
            }
            "len" => Ok(Some(get_i("len").map(Value::Int).unwrap_or(Value::Int(0)))),
            "is_empty" => {
                let cur_len = get_i("len").unwrap_or(0);
                Ok(Some(Value::Bool(cur_len == 0)))
            }
            "clear" => {
                let mut f = class_data.borrow_mut();
                f.fields
                    .insert("prev".into(), Value::Arr(Rc::new(RefCell::new(Vec::new()))));
                f.fields
                    .insert("next".into(), Value::Arr(Rc::new(RefCell::new(Vec::new()))));
                f.fields
                    .insert("vals".into(), Value::Arr(Rc::new(RefCell::new(Vec::new()))));
                f.fields.insert("head".into(), Value::Int(-1));
                f.fields.insert("tail".into(), Value::Int(-1));
                f.fields.insert("len".into(), Value::Int(0));
                f.fields
                    .insert("free".into(), Value::Arr(Rc::new(RefCell::new(Vec::new()))));
                Ok(Some(Value::Void))
            }
            _ => Ok(None),
        }
    }

    // ---------- A6：TreeMap 有序映射 ----------

    /// io.treemap.init() !TreeMap——创建空 TreeMap。
    pub(crate) fn call_treemap_ns_method(
        &mut self,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        match field {
            "init" => {
                if !args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let mut fields = HashMap::new();
                fields.insert("keys".into(), Value::Arr(Rc::new(RefCell::new(Vec::new()))));
                fields.insert("vals".into(), Value::Arr(Rc::new(RefCell::new(Vec::new()))));
                fields.insert("left".into(), Value::Arr(Rc::new(RefCell::new(Vec::new()))));
                fields.insert(
                    "right".into(),
                    Value::Arr(Rc::new(RefCell::new(Vec::new()))),
                );
                fields.insert("root".into(), Value::Int(-1));
                fields.insert("len".into(), Value::Int(0));
                fields.insert("free".into(), Value::Arr(Rc::new(RefCell::new(Vec::new()))));
                Ok(Some(Value::class("TreeMap", fields)))
            }
            _ => Ok(None),
        }
    }

    /// TreeMap 实例方法：insert/get/contains/remove/len/is_empty/clear。
    pub(crate) fn call_treemap_method(
        &mut self,
        method: &str,
        v: &Value,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        let class_data = match self.deref_value(v.clone()) {
            Value::Class(c) => c,
            _ => return Ok(Some(self.err_val("TypeError"))),
        };

        let get_i = |name: &str| -> Option<i128> {
            match class_data.borrow().fields.get(name)? {
                Value::Int(n) => Some(*n),
                _ => None,
            }
        };
        let get_arr = |name: &str| -> Option<Rc<RefCell<Vec<Rc<RefCell<Value>>>>>> {
            match class_data.borrow().fields.get(name)? {
                Value::Arr(a) => Some(a.clone()),
                _ => None,
            }
        };
        let set_i = |cd: &Rc<RefCell<ClassData>>, name: &str, val: i128| {
            cd.borrow_mut().fields.insert(name.into(), Value::Int(val));
        };

        match method {
            "insert" => {
                let key = self.eval_int_arg(args, 0, span)?;
                let val = self.eval(&args[1])?;
                // 如果 root 为空，直接创建根节点
                let root = get_i("root").unwrap_or(-1);
                if root < 0 {
                    let idx = 0;
                    // 扩展所有数组
                    if let Some(keys) = get_arr("keys") {
                        keys.borrow_mut()
                            .push(Rc::new(RefCell::new(Value::Int(key))));
                    }
                    if let Some(vals) = get_arr("vals") {
                        vals.borrow_mut().push(Rc::new(RefCell::new(val)));
                    }
                    if let Some(left) = get_arr("left") {
                        left.borrow_mut()
                            .push(Rc::new(RefCell::new(Value::Int(-1))));
                    }
                    if let Some(right) = get_arr("right") {
                        right
                            .borrow_mut()
                            .push(Rc::new(RefCell::new(Value::Int(-1))));
                    }
                    set_i(&class_data, "root", 0);
                    set_i(&class_data, "len", 1);
                    return Ok(Some(Value::Void));
                }

                // 非递归遍历查找插入位置
                let mut cur = root as usize;
                loop {
                    let cur_key = if let Some(keys) = get_arr("keys") {
                        let k = keys.borrow();
                        match k
                            .get(cur)
                            .map(|c| c.borrow().clone())
                            .unwrap_or(Value::Int(0))
                        {
                            Value::Int(n) => n,
                            _ => 0,
                        }
                    } else {
                        0
                    };
                    if key == cur_key {
                        // 键已存在，更新值
                        if let Some(vals) = get_arr("vals") {
                            let mut v = vals.borrow_mut();
                            if let Some(cell) = v.get(cur) {
                                *cell.borrow_mut() = val;
                            }
                        }
                        return Ok(Some(Value::Void));
                    }

                    let side_opt = if key < cur_key {
                        // 检查左子节点
                        if let Some(left) = get_arr("left") {
                            let l = left.borrow();
                            match l
                                .get(cur)
                                .map(|c| c.borrow().clone())
                                .unwrap_or(Value::Int(-1))
                            {
                                Value::Int(n) if n >= 0 => Some(n as usize),
                                _ => None,
                            }
                        } else {
                            None
                        }
                    } else {
                        // 检查右子节点
                        if let Some(right) = get_arr("right") {
                            let r = right.borrow();
                            match r
                                .get(cur)
                                .map(|c| c.borrow().clone())
                                .unwrap_or(Value::Int(-1))
                            {
                                Value::Int(n) if n >= 0 => Some(n as usize),
                                _ => None,
                            }
                        } else {
                            None
                        }
                    };

                    match side_opt {
                        Some(next) => cur = next,
                        None => {
                            // 分配新节点
                            let idx = if let Some(free) = get_arr("free") {
                                let mut f = free.borrow_mut();
                                f.pop().map(|cell| match *cell.borrow() {
                                    Value::Int(n) => n as usize,
                                    _ => unreachable!(),
                                })
                            } else {
                                None
                            };
                            let idx = idx.unwrap_or_else(|| {
                                if let Some(keys) = get_arr("keys") {
                                    keys.borrow().len()
                                } else {
                                    0
                                }
                            });
                            // 设置新节点
                            if let Some(keys) = get_arr("keys") {
                                let mut k = keys.borrow_mut();
                                if idx >= k.len() {
                                    k.push(Rc::new(RefCell::new(Value::Int(key))));
                                } else {
                                    k[idx] = Rc::new(RefCell::new(Value::Int(key)));
                                }
                            }
                            if let Some(vals) = get_arr("vals") {
                                let mut v = vals.borrow_mut();
                                if idx >= v.len() {
                                    v.push(Rc::new(RefCell::new(val)));
                                } else {
                                    v[idx] = Rc::new(RefCell::new(val));
                                }
                            }
                            if let Some(left) = get_arr("left") {
                                let mut l = left.borrow_mut();
                                if idx >= l.len() {
                                    l.push(Rc::new(RefCell::new(Value::Int(-1))));
                                } else {
                                    l[idx] = Rc::new(RefCell::new(Value::Int(-1)));
                                }
                            }
                            if let Some(right) = get_arr("right") {
                                let mut r = right.borrow_mut();
                                if idx >= r.len() {
                                    r.push(Rc::new(RefCell::new(Value::Int(-1))));
                                } else {
                                    r[idx] = Rc::new(RefCell::new(Value::Int(-1)));
                                }
                            }
                            // 更新父节点的子节点指针
                            if key < cur_key {
                                if let Some(left) = get_arr("left") {
                                    let mut l = left.borrow_mut();
                                    if let Some(cell) = l.get(cur) {
                                        *cell.borrow_mut() = Value::Int(idx as i128);
                                    }
                                }
                            } else {
                                if let Some(right) = get_arr("right") {
                                    let mut r = right.borrow_mut();
                                    if let Some(cell) = r.get(cur) {
                                        *cell.borrow_mut() = Value::Int(idx as i128);
                                    }
                                }
                            }
                            let cur_len = get_i("len").unwrap_or(0);
                            set_i(&class_data, "len", cur_len + 1);
                            return Ok(Some(Value::Void));
                        }
                    }
                }
            }
            "get" => {
                let key = self.eval_int_arg(args, 0, span)?;
                let mut cur = get_i("root").unwrap_or(-1);
                loop {
                    if cur < 0 {
                        return Ok(Some(Value::Opt(None)));
                    }
                    let cur_key = if let Some(keys) = get_arr("keys") {
                        let k = keys.borrow();
                        match k
                            .get(cur as usize)
                            .map(|c| c.borrow().clone())
                            .unwrap_or(Value::Int(0))
                        {
                            Value::Int(n) => n,
                            _ => 0,
                        }
                    } else {
                        0
                    };
                    if key == cur_key {
                        let val = if let Some(vals) = get_arr("vals") {
                            let v = vals.borrow();
                            v.get(cur as usize)
                                .map(|c| c.borrow().clone())
                                .unwrap_or(Value::Void)
                        } else {
                            Value::Void
                        };
                        return Ok(Some(val));
                    }
                    if key < cur_key {
                        if let Some(left) = get_arr("left") {
                            let l = left.borrow();
                            cur = match l
                                .get(cur as usize)
                                .map(|c| c.borrow().clone())
                                .unwrap_or(Value::Int(-1))
                            {
                                Value::Int(n) => n,
                                _ => -1,
                            };
                        } else {
                            cur = -1;
                        }
                    } else {
                        if let Some(right) = get_arr("right") {
                            let r = right.borrow();
                            cur = match r
                                .get(cur as usize)
                                .map(|c| c.borrow().clone())
                                .unwrap_or(Value::Int(-1))
                            {
                                Value::Int(n) => n,
                                _ => -1,
                            };
                        } else {
                            cur = -1;
                        }
                    }
                }
            }
            "contains" => {
                let key = self.eval_int_arg(args, 0, span)?;
                let mut cur = get_i("root").unwrap_or(-1);
                loop {
                    if cur < 0 {
                        return Ok(Some(Value::Bool(false)));
                    }
                    let cur_key = if let Some(keys) = get_arr("keys") {
                        let k = keys.borrow();
                        match k
                            .get(cur as usize)
                            .map(|c| c.borrow().clone())
                            .unwrap_or(Value::Int(0))
                        {
                            Value::Int(n) => n,
                            _ => 0,
                        }
                    } else {
                        0
                    };
                    if key == cur_key {
                        return Ok(Some(Value::Bool(true)));
                    }
                    if key < cur_key {
                        if let Some(left) = get_arr("left") {
                            let l = left.borrow();
                            cur = match l
                                .get(cur as usize)
                                .map(|c| c.borrow().clone())
                                .unwrap_or(Value::Int(-1))
                            {
                                Value::Int(n) => n,
                                _ => -1,
                            };
                        } else {
                            cur = -1;
                        }
                    } else {
                        if let Some(right) = get_arr("right") {
                            let r = right.borrow();
                            cur = match r
                                .get(cur as usize)
                                .map(|c| c.borrow().clone())
                                .unwrap_or(Value::Int(-1))
                            {
                                Value::Int(n) => n,
                                _ => -1,
                            };
                        } else {
                            cur = -1;
                        }
                    }
                }
            }
            "len" => Ok(Some(get_i("len").map(Value::Int).unwrap_or(Value::Int(0)))),
            "is_empty" => {
                let cur_len = get_i("len").unwrap_or(0);
                Ok(Some(Value::Bool(cur_len == 0)))
            }
            "clear" => {
                let mut f = class_data.borrow_mut();
                f.fields
                    .insert("keys".into(), Value::Arr(Rc::new(RefCell::new(Vec::new()))));
                f.fields
                    .insert("vals".into(), Value::Arr(Rc::new(RefCell::new(Vec::new()))));
                f.fields
                    .insert("left".into(), Value::Arr(Rc::new(RefCell::new(Vec::new()))));
                f.fields.insert(
                    "right".into(),
                    Value::Arr(Rc::new(RefCell::new(Vec::new()))),
                );
                f.fields.insert("root".into(), Value::Int(-1));
                f.fields.insert("len".into(), Value::Int(0));
                f.fields
                    .insert("free".into(), Value::Arr(Rc::new(RefCell::new(Vec::new()))));
                Ok(Some(Value::Void))
            }
            _ => Ok(None),
        }
    }

    // ---------- G5（E3.3 text）正则文本处理 ----------

    /// io.text.* —— `matches(pattern, text) bool`（是否含匹配；`^`/`$` 锚定控制
    /// 全串）/ `find(pattern, text) ?int`（首个匹配起点；无 → null）/
    /// `replace(pattern, text, repl) &[u8]`（替换全部非重叠匹配，每处取最长）/
    /// `split(pattern, text) Vec(&[u8])`（按匹配分割，含空段）。非法模式 →
    /// error.InvalidFormat。
    pub(crate) fn call_text_method(
        &mut self,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        match field {
            "matches" => {
                let pat = self.eval_str_arg(args, 0, span)?;
                let text = self.eval_str_arg(args, 1, span)?;
                let ast = match parse_regex(&pat) {
                    Some(a) => a,
                    None => return Ok(Some(self.err_val("InvalidFormat"))),
                };
                let mut m = RegexMatcher::new(&ast, &text);
                Ok(Some(Value::Bool(m.find_at(0).is_some())))
            }
            "find" => {
                let pat = self.eval_str_arg(args, 0, span)?;
                let text = self.eval_str_arg(args, 1, span)?;
                let ast = match parse_regex(&pat) {
                    Some(a) => a,
                    None => return Ok(Some(self.err_val("InvalidFormat"))),
                };
                let mut m = RegexMatcher::new(&ast, &text);
                match m.find_at(0) {
                    Some((s, _e)) => Ok(Some(Value::Opt(Some(Rc::new(Value::Int(s as i128)))))),
                    None => Ok(Some(Value::Opt(None))),
                }
            }
            "replace" => {
                let pat = self.eval_str_arg(args, 0, span)?;
                let text = self.eval_str_arg(args, 1, span)?;
                let repl = self.eval_str_arg(args, 2, span)?;
                let ast = match parse_regex(&pat) {
                    Some(a) => a,
                    None => return Ok(Some(self.err_val("InvalidFormat"))),
                };
                let mut m = RegexMatcher::new(&ast, &text);
                let mut out: Vec<u8> = Vec::new();
                let mut last = 0usize;
                let mut cur = 0usize;
                loop {
                    let mf = m.find_at(cur);
                    match mf {
                        Some((s, e)) => {
                            out.extend_from_slice(&text[last.min(text.len())..s]);
                            out.extend_from_slice(&repl);
                            if e > s {
                                last = e;
                                cur = e;
                                if e == text.len() {
                                    break;
                                }
                            } else {
                                // 空匹配：复制该位置字节后前进，避免死循环
                                last = s + 1;
                                cur = s + 1;
                                if s < text.len() {
                                    out.push(text[s]);
                                }
                                if cur > text.len() {
                                    break;
                                }
                            }
                        }
                        None => break,
                    }
                }
                out.extend_from_slice(&text[last.min(text.len())..]);
                Ok(Some(Value::str_bytes(out)))
            }
            "split" => {
                let pat = self.eval_str_arg(args, 0, span)?;
                let text = self.eval_str_arg(args, 1, span)?;
                let ast = match parse_regex(&pat) {
                    Some(a) => a,
                    None => return Ok(Some(self.err_val("InvalidFormat"))),
                };
                let mut m = RegexMatcher::new(&ast, &text);
                let mut parts: Vec<Value> = Vec::new();
                let mut start = 0usize;
                let mut cur = 0usize;
                loop {
                    match m.find_at(cur) {
                        Some((s, e)) => {
                            parts.push(Value::str_bytes(text[start..s].to_vec()));
                            if e > s {
                                start = e;
                                cur = e;
                                if e == text.len() {
                                    break; // 匹配到末尾：尾空段由最后 push 补
                                }
                            } else {
                                // 空匹配：不消耗字符（该位置字节归下一段），仅前进搜索游标
                                start = s;
                                cur = s + 1;
                                if cur > text.len() {
                                    break;
                                }
                            }
                        }
                        None => break,
                    }
                }
                parts.push(Value::str_bytes(text[start.min(text.len())..].to_vec()));
                Ok(Some(Value::vec(parts, Value::Alloc)))
            }
            _ => Ok(None),
        }
    }

    // ---------- G5（E3.3 rng）伪随机数 ----------

    /// io.rng.* —— `seed(v)`（重置状态；0 → 回退默认）/ `next() int`（下个原始
    /// 64 位）/ `int(n) int`（[0, n) 均匀，拒绝采样免模偏差）/ `float() f64`
    /// （[0, 1)，高 53 位均匀）。全局态在 Interp 实例（协作式单线程执行下安全）；
    /// 命名空间类名 RngNs 避开示例 84-rng 的用户类 Rng（内建先于用户方法分派）。
    pub(crate) fn call_rng_method(
        &mut self,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        match field {
            "seed" => {
                let v = self.eval_int_arg(args, 0, span)?;
                self.rng_state = if v == 0 {
                    0x9e37_79b9_7f4a_7c15
                } else {
                    v as u64
                };
                Ok(Some(Value::Void))
            }
            "next" => {
                let n = xorshift64(&mut self.rng_state);
                Ok(Some(Value::Int(n as i128)))
            }
            "int" => {
                let bound = self.eval_int_arg(args, 0, span)?;
                if bound <= 0 {
                    return Ok(Some(Value::Int(0)));
                }
                let b = bound as u64;
                let threshold = b.wrapping_neg() % b;
                let mut v = xorshift64(&mut self.rng_state);
                while v < threshold {
                    v = xorshift64(&mut self.rng_state);
                }
                Ok(Some(Value::Int((v % b) as i128)))
            }
            "float" => {
                let v = xorshift64(&mut self.rng_state) >> 11;
                let f = (v as f64) / ((1u64 << 53) as f64);
                Ok(Some(Value::Float(f)))
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn call_file_method(
        &mut self,
        field: &str,
        v: &Value,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        let fd = self.file_fd(v, span)?;
        match field {
            // f.close()：关闭并注销句柄
            "close" => {
                self.files.remove(&fd);
                Ok(Some(Value::Void))
            }
            // f.write_all(data)
            "write_all" => {
                let data = self.eval_str_arg(args, 0, span)?;
                let file = self
                    .files
                    .get_mut(&fd)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?;
                file.write_all(&data)
                    .map_err(|e| RtError::msg("Io", format!("write: {e}")))?;
                Ok(Some(Value::Void))
            }
            // f.read_all(alloc)（方法形态，等价 io.fs.read_all）
            "read_all" => {
                let file = self
                    .files
                    .get_mut(&fd)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?;
                file.seek(std::io::SeekFrom::Start(0))
                    .map_err(|e| RtError::msg("Io", format!("seek: {e}")))?;
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)
                    .map_err(|e| RtError::msg("Io", format!("read: {e}")))?;
                Ok(Some(Value::str_bytes(buf)))
            }
            // M5.4：f.seek(offset)——绝对定位
            "seek" => {
                let off = self.eval_int_arg(args, 0, span)?;
                let file = self
                    .files
                    .get_mut(&fd)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?;
                file.seek(std::io::SeekFrom::Start(off.max(0) as u64))
                    .map_err(|e| RtError::msg("Io", format!("seek: {e}")))?;
                Ok(Some(Value::Void))
            }
            // f.pos() !u64：当前位置
            "pos" => {
                let file = self
                    .files
                    .get_mut(&fd)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?;
                let pos = file
                    .stream_position()
                    .map_err(|e| RtError::msg("Io", format!("pos: {e}")))?;
                Ok(Some(Value::Int(pos as i128)))
            }
            // f.read_at(offset, len) &[u8]：指定位置读（不改变当前位置）
            "read_at" => {
                let off = self.eval_int_arg(args, 0, span)?;
                let len = self.eval_int_arg(args, 1, span)?.max(0) as usize;
                let file = self
                    .files
                    .get_mut(&fd)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?;
                let saved = file
                    .stream_position()
                    .map_err(|e| RtError::msg("Io", format!("pos: {e}")))?;
                file.seek(std::io::SeekFrom::Start(off.max(0) as u64))
                    .map_err(|e| RtError::msg("Io", format!("seek: {e}")))?;
                let mut buf = vec![0u8; len];
                let k = file
                    .read(&mut buf)
                    .map_err(|e| RtError::msg("Io", format!("read: {e}")))?;
                buf.truncate(k);
                let _ = file.seek(std::io::SeekFrom::Start(saved));
                Ok(Some(Value::str_bytes(buf)))
            }
            // f.write_at(offset, data)：指定位置写（不改变当前位置）
            "write_at" => {
                let off = self.eval_int_arg(args, 0, span)?;
                let data = self.eval_str_arg(args, 1, span)?;
                let file = self
                    .files
                    .get_mut(&fd)
                    .ok_or_else(|| RtError::new("BadFd", Some(span.clone())))?;
                let saved = file
                    .stream_position()
                    .map_err(|e| RtError::msg("Io", format!("pos: {e}")))?;
                file.seek(std::io::SeekFrom::Start(off.max(0) as u64))
                    .map_err(|e| RtError::msg("Io", format!("seek: {e}")))?;
                file.write_all(&data)
                    .map_err(|e| RtError::msg("Io", format!("write: {e}")))?;
                let _ = file.seek(std::io::SeekFrom::Start(saved));
                Ok(Some(Value::Void))
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn call_time_method(
        &mut self,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        match field {
            // io.time.now()：毫秒时间戳
            "now" => {
                let ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i128;
                Ok(Some(Value::Int(ms)))
            }
            // io.time.sleep(ms)
            "sleep" => {
                let ms = self.eval_int_arg(args, 0, span)?;
                std::thread::sleep(std::time::Duration::from_millis(ms.max(0) as u64));
                Ok(Some(Value::Void))
            }
            // G5（E3.3 time 完整）：单调测量——io.time.tick()（纳秒计数，epoch 基准）
            // / io.time.elapsed(tick)（自 tick 起毫秒数）。时区完整留 1.x（需 tz 库）。
            "tick" => {
                let ns = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as i128;
                Ok(Some(Value::Int(ns)))
            }
            "elapsed" => {
                let tick = self.eval_int_arg(args, 0, span)?;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as i128;
                Ok(Some(Value::Int((now - tick).max(0) / 1_000_000)))
            }
            // 时区完整（A4）：UTC 日历分量
            "components" => {
                let ts = self.eval_int_arg(args, 0, span)?;
                let (year, month, day, hour, min, sec, ms) = timestamp_to_components(ts);
                let mut fields = HashMap::new();
                fields.insert("year".to_string(), Value::Int(year as i128));
                fields.insert("month".to_string(), Value::Int(month as i128));
                fields.insert("day".to_string(), Value::Int(day as i128));
                fields.insert("hour".to_string(), Value::Int(hour as i128));
                fields.insert("min".to_string(), Value::Int(min as i128));
                fields.insert("sec".to_string(), Value::Int(sec as i128));
                fields.insert("ms".to_string(), Value::Int(ms as i128));
                Ok(Some(Value::class("TimeComponents", fields)))
            }
            // 本地 UTC 偏移（分钟）
            "local_offset" => {
                let offset_min = local_utc_offset_minutes();
                Ok(Some(Value::Int(offset_min as i128)))
            }
            // 格式化时间戳为 ISO 8601 字符串
            "format" => {
                let ts = self.eval_int_arg(args, 0, span)?;
                let (year, month, day, hour, min, sec, ms) = timestamp_to_components(ts);
                let s = format!(
                    "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
                    year, month, day, hour, min, sec, ms
                );
                Ok(Some(Value::str(&s)))
            }
            _ => Ok(None),
        }
    }

    /// `X.new(args, alloc)` 兼容构造（C1 审计后旧示例；tag1 仅支持 value 类构造）
    pub(crate) fn call_new_builtin(
        &mut self,
        ty: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Value> {
        let fields = match self.types.get(ty) {
            Some(TypeDef::Class { fields, .. }) => fields.clone(),
            _ => return Err(RtError::new("UnknownType", Some(span.clone()))),
        };
        let mut f = HashMap::new();
        // 两种形态：new(alloc, 字段值...) 或 new(字段值..., alloc)
        let (vals_start, vals_end) = if args.len() > 1 {
            let is_alloc_first = matches!(&args[0], Expr::Ident(n, _) if n == "alloc");
            let is_alloc_last = matches!(args.last(), Some(Expr::Ident(n, _)) if n == "alloc");
            if is_alloc_first {
                (1usize, args.len())
            } else if is_alloc_last && args.len() > 1 {
                (0usize, args.len() - 1)
            } else {
                (0usize, args.len())
            }
        } else {
            (0usize, args.len())
        };
        let mut ai = vals_start;
        for fd in fields {
            if ai < vals_end {
                let v = self.eval(&args[ai])?;
                f.insert(fd.name.clone(), v);
                ai += 1;
            } else if matches!(fd.ty.strip(), Type::Named(n, _) if n.starts_with("Vec")) {
                f.insert(fd.name.clone(), Value::arr(vec![]));
            } else {
                f.insert(fd.name.clone(), self.default_value(Some(&fd.ty))?);
            }
        }
        Ok(Value::class(ty, f))
    }

    /// 解析器辅助内建（71：peek/advance/expect/skip_space/is_digit/parse_number）
    pub(crate) fn call_parser_builtin(
        &mut self,
        name: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        // 先求值全部参数，避免闭包借用冲突
        let mut vals = Vec::new();
        for a in args {
            vals.push(self.eval(a)?);
        }
        let get_bytes = |ix: usize, vals: &[Value]| -> std::result::Result<Vec<u8>, RtError> {
            let v = &vals[ix];
            match v {
                Value::Str(s) => Ok(s.borrow().clone()),
                Value::Ptr(p) => match &*p.borrow() {
                    Value::Str(s) => Ok(s.borrow().clone()),
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                },
                _ => Err(RtError::new("TypeError", Some(span.clone()))),
            }
        };
        let get_pos =
            |ix: usize, vals: &[Value]| -> std::result::Result<Rc<RefCell<Value>>, RtError> {
                match &vals[ix] {
                    Value::Ptr(p) => Ok(p.clone()),
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            };
        match name {
            "skip_space" => {
                let data = get_bytes(0, &vals)?;
                let pos = get_pos(1, &vals)?;
                let mut i = match &*pos.borrow() {
                    Value::Int(i) => *i as usize,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                while i < data.len() && data[i].is_ascii_whitespace() {
                    i += 1;
                }
                *pos.borrow_mut() = Value::Int(i as i128);
                Ok(Some(Value::Void))
            }
            "peek" => {
                let data = get_bytes(0, &vals)?;
                let pos = get_pos(1, &vals)?;
                let i = match &*pos.borrow() {
                    Value::Int(i) => *i as usize,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                Ok(Some(if i < data.len() {
                    Value::Opt(Some(Rc::new(Value::Int(data[i] as i128))))
                } else {
                    Value::Opt(None)
                }))
            }
            "advance" => {
                let pos = get_pos(1, &vals)?;
                let i = match &*pos.borrow() {
                    Value::Int(i) => *i as i128,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                *pos.borrow_mut() = Value::Int(i + 1);
                Ok(Some(Value::Void))
            }
            "expect" => {
                let data = get_bytes(0, &vals)?;
                let pos = get_pos(1, &vals)?;
                let want_byte = match &vals[2] {
                    Value::Int(i) => *i as u8,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let i = match &*pos.borrow() {
                    Value::Int(i) => *i as usize,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                if i < data.len() && data[i] == want_byte {
                    *pos.borrow_mut() = Value::Int(i as i128 + 1);
                    Ok(Some(Value::Void))
                } else {
                    Err(RtError::new("UnexpectedToken", Some(span.clone())))
                }
            }
            "is_digit" => {
                let v = &vals[0];
                let v = match v {
                    Value::Ptr(p) => p.borrow().clone(),
                    other => other.clone(),
                };
                match v {
                    Value::Int(i) => Ok(Some(Value::Bool((i as u8 as char).is_ascii_digit()))),
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            "parse_number" => {
                let data = get_bytes(0, &vals)?;
                let pos = get_pos(1, &vals)?;
                let mut i = match &*pos.borrow() {
                    Value::Int(i) => *i as usize,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let start = i;
                while i < data.len() && data[i].is_ascii_digit() {
                    i += 1;
                }
                let n: i128 = String::from_utf8_lossy(&data[start..i])
                    .parse()
                    .unwrap_or(0);
                *pos.borrow_mut() = Value::Int(i as i128);
                Ok(Some(Value::Int(n)))
            }
            _ => Ok(None),
        }
    }

    /// serialize 命名空间（M5.3）：解析辅助组组织为库命名空间
    ///
    /// `serialize.parse_int/parse_float/parse_number/skip_space/peek/advance/is_digit/expect`
    /// 对齐对应自由内建（call_builtin）；`serialize.json.parse` / `serialize.csv.parse`
    /// 对齐虚拟根 json.parse / csv.parse。
    pub(crate) fn call_serialize_builtin(
        &mut self,
        name: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Value> {
        let helper = name.strip_prefix("serialize.").unwrap_or(name);
        match helper {
            "json.parse" => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                if let Value::Str(s) = v {
                    let text = String::from_utf8_lossy(&s.borrow()).to_string();
                    let obj = self.parse_json_obj(&text)?;
                    return Ok(Value::class("Map", obj));
                }
                Err(RtError::new("TypeError", Some(span.clone())))
            }
            "csv.parse" => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                if let Value::Str(s) = v {
                    let text = String::from_utf8_lossy(&s.borrow()).to_string();
                    let rows = text
                        .split('\n')
                        .map(|line| line.strip_suffix('\r').unwrap_or(line))
                        .filter(|line| !line.is_empty())
                        .map(|line| line.split(',').map(Value::str).collect::<Vec<_>>())
                        .map(Value::arr)
                        .collect::<Vec<_>>();
                    return Ok(Value::arr(rows));
                }
                Err(RtError::new("TypeError", Some(span.clone())))
            }
            // parse_int/parse_float/parse_number/skip_space/peek/advance/is_digit/expect
            _ => match self.call_builtin(helper, args, span)? {
                Some(v) => Ok(v),
                None => Err(RtError::new("NotBuiltin", Some(span.clone()))),
            },
        }
    }

    /// String 内建静态方法（String = 内建新类型，M3 定案；tag1：from/from_slice/concat）
    pub(crate) fn call_string_builtin(
        &mut self,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Value> {
        match field {
            "from" => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                match v {
                    Value::Str(s) => Ok(Value::Str(s)),
                    other => Ok(Value::str(&other.display())),
                }
            }
            "from_slice" => {
                // String.from_slice(&buf, arena)：字节切片/数组 → String（49-arena-pool）
                if args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                let bytes = match v {
                    Value::Str(s) => s.borrow().clone(),
                    Value::Arr(a) => a
                        .borrow()
                        .iter()
                        .map(|c| match &*c.borrow() {
                            Value::Int(i) => (i & 0xFF) as u8,
                            other => other.display().as_bytes().first().copied().unwrap_or(0),
                        })
                        .collect(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                Ok(Value::str_bytes(bytes))
            }
            "concat" => {
                let a = self.eval(&args[0])?;
                let b = self.eval(&args[1])?;
                let a = self.deref_value(a);
                let b = self.deref_value(b);
                match (a, b) {
                    (Value::Str(x), Value::Str(y)) => {
                        let mut bytes = x.borrow().clone();
                        bytes.extend_from_slice(&y.borrow());
                        Ok(Value::str_bytes(bytes))
                    }
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            "compare" => {
                let a = self.eval(&args[0])?;
                let b = self.eval(&args[1])?;
                let a = self.deref_value(a);
                let b = self.deref_value(b);
                let ord = match (&a, &b) {
                    (Value::Str(x), Value::Str(y)) => {
                        let (x, y) = (x.borrow().clone(), y.borrow().clone());
                        x.cmp(&y)
                    }
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let v = match ord {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                };
                Ok(Value::Int(v))
            }
            "join" => {
                let parts = self.eval(&args[0])?;
                let parts = self.deref_value(parts);
                let sep = self.eval(&args[1])?;
                let sep = self.deref_value(sep);
                let sep_bytes: Vec<u8> = match &sep {
                    Value::Str(s) => s.borrow().clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let items: Vec<Vec<u8>> = match &parts {
                    Value::Arr(a) => a
                        .borrow()
                        .iter()
                        .map(|c| match &*c.borrow() {
                            Value::Str(s) => s.borrow().clone(),
                            other => other.display().into_bytes(),
                        })
                        .collect(),
                    Value::Ptr(p) => match &*p.borrow() {
                        Value::Arr(a) => a
                            .borrow()
                            .iter()
                            .map(|c| match &*c.borrow() {
                                Value::Str(s) => s.borrow().clone(),
                                other => other.display().into_bytes(),
                            })
                            .collect(),
                        _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                    },
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let mut out = Vec::new();
                for (i, it) in items.iter().enumerate() {
                    if i > 0 {
                        out.extend_from_slice(&sep_bytes);
                    }
                    out.extend_from_slice(it);
                }
                Ok(Value::str_bytes(out))
            }
            _ => Err(RtError::new("NoMethod", Some(span.clone()))),
        }
    }

    pub(crate) fn call_math(
        &mut self,
        ns: &str,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        if ns != "math" {
            return Ok(None);
        }
        match field {
            "nan" => Ok(Some(Value::Float(f64::NAN))),
            "inf" => Ok(Some(Value::Float(f64::INFINITY))),
            "inf_neg" => Ok(Some(Value::Float(f64::NEG_INFINITY))),
            "sqrt" | "abs" | "pow" | "floor" | "ceil" | "round" => {
                if args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                let f = match v {
                    Value::Int(i) => i as f64,
                    Value::Float(f) => f,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let r = match field {
                    "sqrt" => f.sqrt(),
                    "abs" => f.abs(),
                    "pow" => f.powf(2.0),
                    "floor" => f.floor(),
                    "ceil" => f.ceil(),
                    "round" => f.round(),
                    _ => unreachable!(),
                };
                Ok(Some(Value::Float(r)))
            }
            _ => Ok(None),
        }
    }
}

// ---------- 时区辅助函数 ----------

/// 将毫秒时间戳转换为 UTC 日历分量（year, month, day, hour, min, sec, ms）
/// 算法：类 gmtime 分解，处理闰年、跨世纪闰年规则。
fn timestamp_to_components(ts: i128) -> (i32, u32, u32, u32, u32, u32, u32) {
    let ms = (ts.rem_euclid(1000)) as u32;
    let total_secs = ts.div_euclid(1000);
    let sec_of_day = total_secs.rem_euclid(86400);
    let hour = (sec_of_day / 3600) as u32;
    let min = ((sec_of_day / 60) % 60) as u32;
    let sec = (sec_of_day % 60) as u32;

    let days = total_secs.div_euclid(86400);

    // Howard Hinnant 算法：days since epoch → year/month/day
    const DAYS_PER_400Y: i128 = 146097;
    let z = days + 719468; // 从 0000-03-01 偏移
    let era = z.div_euclid(DAYS_PER_400Y);
    let doe = z.rem_euclid(DAYS_PER_400Y); // day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // year of era [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // month phase [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // day [1, 31]
    let (month, year) = if mp < 10 {
        ((mp + 3) as u32, y) // month 3..12
    } else {
        ((mp - 9) as u32, y + 1) // month 1..2, year+1
    };

    (year as i32, month, day, hour, min, sec, ms)
}

/// 获取本地 UTC 偏移（分钟）。
/// 当前实现返回 0（UTC）；后续可通过平台 API 或 TZ 环境变量扩展。
fn local_utc_offset_minutes() -> i32 {
    0
}
