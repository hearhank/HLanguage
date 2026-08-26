//! 内建函数 IR 降级：@sizeOf/@intCast/@panic 等内建函数的 IR 指令生成

use super::*;

pub(crate) fn call_fs_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "open" => {
            let path = path_arg_ir(ctx, args, 0)?;
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
            {
                Ok(f) => Ok(Some(register_file_ir(ctx, f))),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "create" => {
            let path = path_arg_ir(ctx, args, 0)?;
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(&path)
            {
                Ok(f) => Ok(Some(register_file_ir(ctx, f))),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "read_file" => {
            let path = path_arg_ir(ctx, args, 0)?;
            match std::fs::read(&path) {
                Ok(b) => Ok(Some(str_bytes_val(b))),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "read_all" => {
            let f = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "read_all"))?;
            let fd = file_fd_ir(ctx, f)?;
            let file = ctx
                .files
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad file"))?;
            file.seek(std::io::SeekFrom::Start(0))
                .map_err(|e| IrError::msg("Io", format!("seek: {e}")))?;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)
                .map_err(|e| IrError::msg("Io", format!("read: {e}")))?;
            Ok(Some(str_bytes_val(buf)))
        }
        "write_all" => {
            let f = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "write_all"))?;
            let fd = file_fd_ir(ctx, f)?;
            let data = str_arg_ir(ctx, args, 1)?;
            let file = ctx
                .files
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad file"))?;
            file.write_all(&data)
                .map_err(|e| IrError::msg("Io", format!("write: {e}")))?;
            Ok(Some(IrValue::Void))
        }
        "append" => {
            let path = path_arg_ir(ctx, args, 0)?;
            let data = str_arg_ir(ctx, args, 1)?;
            match std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(&path)
            {
                Ok(mut f) => {
                    f.write_all(&data)
                        .map_err(|e| IrError::msg("Io", format!("append: {e}")))?;
                    Ok(Some(IrValue::Void))
                }
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "remove" => {
            let path = path_arg_ir(ctx, args, 0)?;
            match std::fs::remove_file(&path) {
                Ok(_) => Ok(Some(IrValue::Void)),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "rename" => {
            let from = path_arg_ir(ctx, args, 0)?;
            let to = path_arg_ir(ctx, args, 1)?;
            match std::fs::rename(&from, &to) {
                Ok(_) => Ok(Some(IrValue::Void)),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "read_int" => {
            let path = path_arg_ir(ctx, args, 0)?;
            match std::fs::read(&path) {
                Ok(b) => match String::from_utf8_lossy(&b).trim().parse::<i64>() {
                    Ok(n) => Ok(Some(IrValue::Int(n as i128))),
                    Err(_) => Ok(Some(err_val(module, "InvalidFormat"))),
                },
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "write_int" => {
            let path = path_arg_ir(ctx, args, 0)?;
            let v = int_arg_ir(ctx, args, 1)?;
            match std::fs::write(&path, v.to_string().as_bytes()) {
                Ok(_) => Ok(Some(IrValue::Void)),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "list_dir" => {
            // G2（io 差异项）：双形态——第一参为 Str（路径）或 Dir 值（句柄）；
            // 返回 Vec(DirEntry)，每条 {name, is_dir}（对齐 oracle call_fs_method）。
            let v0 = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "list_dir"))?;
            let v0d = deref_value(ctx, v0);
            match v0d {
                IrValue::Class(c) if class_name(ctx, *c) == "Dir" => {
                    let fd = dir_fd_ir(ctx, v0d)?;
                    let path = ctx
                        .dirs
                        .get(&fd)
                        .ok_or_else(|| IrError::msg("BadFd", "bad dir"))?
                        .clone();
                    let entries = list_dir_entries_ir(ctx, module, &path)?;
                    Ok(Some(entries))
                }
                IrValue::Str(s) => {
                    let path = String::from_utf8_lossy(s).into_owned();
                    let entries = list_dir_entries_ir(ctx, module, &path)?;
                    Ok(Some(entries))
                }
                _ => Err(IrError::msg("TypeError", "list_dir expects path or Dir")),
            }
        }
        // G2（io 差异项）：io.fs.open_dir(path) !Dir——目录句柄。
        // 读校验成功则注册 fd→path（供 dir.list_dir / dir.close），返回 Dir 值。
        "open_dir" => {
            let path = path_arg_ir(ctx, args, 0)?;
            match std::fs::read_dir(&path) {
                Ok(_) => {
                    let fd = ctx.next_dir_fd;
                    ctx.next_dir_fd += 1;
                    ctx.dirs.insert(fd, path);
                    let mut fields = HashMap::new();
                    fields.insert(
                        "_fd".into(),
                        ctx.alloc(Cell::Value(IrValue::Int(fd as i128))),
                    );
                    Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                        name: "Dir".into(),
                        fields,
                    }))))
                }
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        _ => Ok(None),
    }
}

pub(crate) fn call_file_method_ir(
    ctx: &mut Ctx,
    _module: &IrModule,
    self_v: &IrValue,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let fd = file_fd_ir(ctx, self_v)?;
    match field {
        "close" => {
            ctx.files.remove(&fd);
            Ok(Some(IrValue::Void))
        }
        "write_all" => {
            let data = str_arg_ir(ctx, args, 0)?;
            let file = ctx
                .files
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad file"))?;
            file.write_all(&data)
                .map_err(|e| IrError::msg("Io", format!("write: {e}")))?;
            Ok(Some(IrValue::Void))
        }
        "read_all" => {
            let file = ctx
                .files
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad file"))?;
            file.seek(std::io::SeekFrom::Start(0))
                .map_err(|e| IrError::msg("Io", format!("seek: {e}")))?;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)
                .map_err(|e| IrError::msg("Io", format!("read: {e}")))?;
            Ok(Some(str_bytes_val(buf)))
        }
        "seek" => {
            let off = int_arg_ir(ctx, args, 0)?;
            let file = ctx
                .files
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad file"))?;
            file.seek(std::io::SeekFrom::Start(off.max(0) as u64))
                .map_err(|e| IrError::msg("Io", format!("seek: {e}")))?;
            Ok(Some(IrValue::Void))
        }
        "pos" => {
            let file = ctx
                .files
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad file"))?;
            let pos = file
                .stream_position()
                .map_err(|e| IrError::msg("Io", format!("pos: {e}")))?;
            Ok(Some(IrValue::Int(pos as i128)))
        }
        "read_at" => {
            let off = int_arg_ir(ctx, args, 0)?;
            let len = int_arg_ir(ctx, args, 1)?.max(0) as usize;
            let file = ctx
                .files
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad file"))?;
            let saved = file
                .stream_position()
                .map_err(|e| IrError::msg("Io", format!("pos: {e}")))?;
            file.seek(std::io::SeekFrom::Start(off.max(0) as u64))
                .map_err(|e| IrError::msg("Io", format!("seek: {e}")))?;
            let mut buf = vec![0u8; len];
            let k = file
                .read(&mut buf)
                .map_err(|e| IrError::msg("Io", format!("read: {e}")))?;
            buf.truncate(k);
            let _ = file.seek(std::io::SeekFrom::Start(saved));
            Ok(Some(str_bytes_val(buf)))
        }
        "write_at" => {
            let off = int_arg_ir(ctx, args, 0)?;
            let data = str_arg_ir(ctx, args, 1)?;
            let file = ctx
                .files
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad file"))?;
            let saved = file
                .stream_position()
                .map_err(|e| IrError::msg("Io", format!("pos: {e}")))?;
            file.seek(std::io::SeekFrom::Start(off.max(0) as u64))
                .map_err(|e| IrError::msg("Io", format!("seek: {e}")))?;
            file.write_all(&data)
                .map_err(|e| IrError::msg("Io", format!("write: {e}")))?;
            let _ = file.seek(std::io::SeekFrom::Start(saved));
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

pub(crate) fn call_time_method_ir(
    ctx: &mut Ctx,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "now" => {
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i128;
            Ok(Some(IrValue::Int(ms)))
        }
        "sleep" => {
            let ms = int_arg_ir(ctx, args, 0)?;
            std::thread::sleep(std::time::Duration::from_millis(ms.max(0) as u64));
            Ok(Some(IrValue::Void))
        }
        // G5（E3.3 time 完整）：单调测量——tick()（纳秒计数，epoch 基准）/ elapsed(tick)
        //（自 tick 起毫秒数）。时区完整留 1.x（需 tz 库）。
        "tick" => {
            let ns = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as i128;
            Ok(Some(IrValue::Int(ns)))
        }
        "elapsed" => {
            let tick = int_arg_ir(ctx, args, 0)?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as i128;
            Ok(Some(IrValue::Int((now - tick).max(0) / 1_000_000)))
        }
        _ => Ok(None),
    }
}

pub(crate) fn call_net_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "connect" => {
            let host = str_arg_ir(ctx, args, 0)?;
            let port = int_arg_ir(ctx, args, 1)? as u16;
            let host = String::from_utf8_lossy(&host).to_string();
            match std::net::TcpStream::connect((host.as_str(), port)) {
                Ok(stream) => {
                    let fd = ctx.next_net_fd;
                    ctx.next_net_fd += 1;
                    let _ = stream.set_nodelay(true);
                    ctx.tcp_streams.insert(fd, stream);
                    let mut fields = HashMap::new();
                    fields.insert(
                        "fd".into(),
                        ctx.alloc(Cell::Value(IrValue::Int(fd as i128))),
                    );
                    Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                        name: "TcpConn".into(),
                        fields,
                    }))))
                }
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "listen" => {
            let host = str_arg_ir(ctx, args, 0)?;
            let port = int_arg_ir(ctx, args, 1)? as u16;
            let host = String::from_utf8_lossy(&host).to_string();
            let addr = format!("{host}:{port}");
            match std::net::TcpListener::bind(&addr) {
                Ok(listener) => {
                    let fd = ctx.next_net_fd;
                    ctx.next_net_fd += 1;
                    ctx.tcp_listeners.insert(fd, listener);
                    let mut fields = HashMap::new();
                    fields.insert(
                        "fd".into(),
                        ctx.alloc(Cell::Value(IrValue::Int(fd as i128))),
                    );
                    Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                        name: "TcpListener".into(),
                        fields,
                    }))))
                }
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        // G1（E3.1）：io.net.get(url) !&[u8]——HTTP GET 客户端，返回响应体字节
        "get" => {
            let url = str_arg_ir(ctx, args, 0)?;
            let url = String::from_utf8_lossy(&url).to_string();
            match http_get_ir(&url) {
                Ok(body) => Ok(Some(str_bytes_val(body))),
                Err(name) => Ok(Some(err_val(module, &name))),
            }
        }
        // Q20 双语：命名空间形式 io.net.read_all(&conn, alloc) ≡ conn.read_all(alloc)
        //（write/shutdown/close/local_port 同构；第一个实参解引用剥 Ptr → 实例方法）
        "read_all" | "write" | "shutdown" | "close" | "local_port" => {
            let conn = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", field))?;
            let conn = deref_value(ctx, conn).clone();
            call_conn_method_ir(ctx, module, &conn, field, &args[1..])
        }
        // io.net.accept(&server) !Conn ≡ server.accept()
        "accept" => {
            let srv = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "accept"))?;
            let srv = deref_value(ctx, srv).clone();
            call_listener_method_ir(ctx, module, &srv, "accept", &args[1..])
        }
        _ => Ok(None),
    }
}

// ---- G1（E3.1）UDP：io.net.udp 命名空间 + UdpSocket 实例 ----

/// io.net.udp 命名空间分派：`bind(port)` / `bind(host, port) !UdpSocket`；
/// send_to/recv_from/close 命名空间形式（第一实参为 socket）委托实例方法。
/// bind 首参为整型 → port 首参（bind(0, alloc) 亦归此，alloc 忽略）。
pub(crate) fn call_udp_ns_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "bind" => {
            let is_port_first = args
                .first()
                .map(|a| matches!(deref_value(ctx, a), IrValue::Int(_)))
                .unwrap_or(false);
            let (host, port_i) = if is_port_first {
                ("127.0.0.1".to_string(), 0)
            } else if args.len() >= 2 {
                let h = str_arg_ir(ctx, args, 0)?;
                (String::from_utf8_lossy(&h).to_string(), 1)
            } else {
                ("127.0.0.1".to_string(), 0)
            };
            let port = int_arg_ir(ctx, args, port_i)? as u16;
            Ok(Some(udp_bind_ir(ctx, module, &host, port)?))
        }
        "send_to" | "recv_from" | "close" => {
            let sock = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", field))?;
            let sock = deref_value(ctx, sock).clone();
            call_udp_socket_method_ir(ctx, module, &sock, field, &args[1..])
        }
        _ => Ok(None),
    }
}

/// UdpSocket 实例方法：send_to(addr, data) !void / recv_from(alloc) ![addr, data] /
/// local_port() !u16 / close() !void。recv_from 空队列（200ms 读超时）→ error.TimedOut。
pub(crate) fn call_udp_socket_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    self_v: &IrValue,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let fd = net_fd_ir(ctx, self_v)?;
    match field {
        "send_to" => {
            let addr = str_arg_ir(ctx, args, 0)?;
            let addr = String::from_utf8_lossy(&addr).to_string();
            let (host, port) = parse_udp_addr_ir(&addr).map_err(|e| IrError::msg(e, "udp addr"))?;
            let data = str_arg_ir(ctx, args, 1)?;
            let sock = ctx
                .udp_sockets
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad udp socket"))?;
            match sock.send_to(&data, (host.as_str(), port)) {
                Ok(_) => Ok(Some(IrValue::Void)),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "recv_from" => {
            let sock = ctx
                .udp_sockets
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad udp socket"))?;
            let mut buf = vec![0u8; 65536];
            match sock.recv_from(&mut buf) {
                Ok((n, peer)) => {
                    buf.truncate(n);
                    let addr = peer.to_string();
                    Ok(Some(make_arr(
                        ctx,
                        vec![str_val(&addr), str_bytes_val(buf)],
                    )))
                }
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "local_port" => {
            let sock = ctx
                .udp_sockets
                .get(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad udp socket"))?;
            match sock.local_addr() {
                Ok(a) => Ok(Some(IrValue::Int(a.port() as i128))),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "close" => {
            ctx.udp_sockets.remove(&fd);
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

// ---- G2（io 差异项）Dir：open_dir 返回的目录句柄 ----

/// Dir 类方法分派：`dir.list_dir(alloc) !Vec(DirEntry)`（重开枚举）/
/// `dir.close()`（注销句柄）。
pub(crate) fn call_dir_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    self_v: &IrValue,
    field: &str,
    _args: &[IrValue],
) -> R<Option<IrValue>> {
    let fd = dir_fd_ir(ctx, self_v)?;
    match field {
        "list_dir" => {
            let path = ctx
                .dirs
                .get(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad dir"))?
                .clone();
            let entries = list_dir_entries_ir(ctx, module, &path)?;
            Ok(Some(entries))
        }
        "close" => {
            ctx.dirs.remove(&fd);
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

// ---- G3（E3.2 ipc）：管道 + 共享内存 ----

/// io.ipc 命名空间分派：`pipe()`（匿名管道 → `[reader, writer]`）/ `shm(name, size) !Shm`
///（命名共享内存）。进程内 IPC 原语——注册表 + 类名分派承载（Q20 双语），协作式模型下
/// 读写均不阻塞。
pub(crate) fn call_ipc_method_ir(
    ctx: &mut Ctx,
    _module: &IrModule,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "pipe" => {
            let pid = ctx.next_pipe_fd;
            ctx.next_pipe_fd += 1;
            ctx.pipes.insert(
                pid,
                PipeIr {
                    buf: Vec::new(),
                    writer_open: true,
                },
            );
            let reader = {
                let mut fld = HashMap::new();
                fld.insert(
                    "pipe".into(),
                    ctx.alloc(Cell::Value(IrValue::Int(pid as i128))),
                );
                IrValue::Class(ctx.alloc(Cell::Class {
                    name: "PipeReader".into(),
                    fields: fld,
                }))
            };
            let writer = {
                let mut fld = HashMap::new();
                fld.insert(
                    "pipe".into(),
                    ctx.alloc(Cell::Value(IrValue::Int(pid as i128))),
                );
                IrValue::Class(ctx.alloc(Cell::Class {
                    name: "PipeWriter".into(),
                    fields: fld,
                }))
            };
            Ok(Some(make_arr(ctx, vec![reader, writer])))
        }
        "shm" => {
            // name 参数当前仅用于形态约束（命名共享内存的标识语义），区域本体按 id 注册
            let _name = path_arg_ir(ctx, args, 0)?;
            let size = int_arg_ir(ctx, args, 1)?.max(0) as usize;
            let id = ctx.next_shm_fd;
            ctx.next_shm_fd += 1;
            ctx.shms.insert(id, vec![0u8; size]);
            let mut fld = HashMap::new();
            fld.insert(
                "shm".into(),
                ctx.alloc(Cell::Value(IrValue::Int(id as i128))),
            );
            Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                name: "Shm".into(),
                fields: fld,
            }))))
        }
        _ => Ok(None),
    }
}

/// Pipe 类方法分派（is_reader 区分读写端）：写端 `write(data) !void` / `close() !void`；
/// 读端 `read(alloc) !&[u8]`（排空可读字节；空且写端开 → 空切片，不阻塞）/
/// `read_all(alloc) !&[u8]` / `is_closed() bool` / `close() !void`。
/// `close` 幂等：读端 close 注销注册表（管道随之拆除）、写端 close 仅置 writer_open=false；
/// 管道已注销后再 close 为 no-op（不报 BadFd）。
pub(crate) fn call_pipe_method_ir(
    ctx: &mut Ctx,
    _module: &IrModule,
    is_reader: bool,
    self_v: &IrValue,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let pid = pipe_id_ir(ctx, self_v)?;
    match field {
        "close" => {
            if is_reader {
                ctx.pipes.remove(&pid);
            } else if let Some(pipe) = ctx.pipes.get_mut(&pid) {
                pipe.writer_open = false;
            }
            Ok(Some(IrValue::Void))
        }
        "write" if !is_reader => {
            let data = str_arg_ir(ctx, args, 0)?;
            let pipe = ctx
                .pipes
                .get_mut(&pid)
                .ok_or_else(|| IrError::msg("BadFd", "bad pipe"))?;
            pipe.buf.extend_from_slice(&data);
            Ok(Some(IrValue::Void))
        }
        "read" | "read_all" if is_reader => {
            let pipe = ctx
                .pipes
                .get_mut(&pid)
                .ok_or_else(|| IrError::msg("BadFd", "bad pipe"))?;
            let out = std::mem::take(&mut pipe.buf);
            Ok(Some(str_bytes_val(out)))
        }
        "is_closed" if is_reader => {
            let pipe = ctx
                .pipes
                .get(&pid)
                .ok_or_else(|| IrError::msg("BadFd", "bad pipe"))?;
            Ok(Some(IrValue::Bool(!pipe.writer_open)))
        }
        _ => Ok(None),
    }
}

/// Shm 类方法分派：`write(data) !void`（覆盖内容，截断到 size）/ `read(alloc) !&[u8]`
/// / `close() !void`（注销句柄）。
pub(crate) fn call_shm_method_ir(
    ctx: &mut Ctx,
    _module: &IrModule,
    self_v: &IrValue,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let id = shm_id_ir(ctx, self_v)?;
    match field {
        "write" => {
            let data = str_arg_ir(ctx, args, 0)?;
            let shm = ctx
                .shms
                .get_mut(&id)
                .ok_or_else(|| IrError::msg("BadFd", "bad shm"))?;
            let cap = shm.capacity();
            let take = data.len().min(cap);
            shm.clear();
            shm.extend_from_slice(&data[..take]);
            Ok(Some(IrValue::Void))
        }
        "read" => {
            let shm = ctx
                .shms
                .get(&id)
                .ok_or_else(|| IrError::msg("BadFd", "bad shm"))?;
            Ok(Some(str_bytes_val(shm.clone())))
        }
        "close" => {
            ctx.shms.remove(&id);
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

// ---- G4（E3.3 storage）文件持久化键值存储 ----

/// io.storage.open(path) !KvStore——打开/创建文件持久化的键值存储。
/// 文件存在则装载既有条目（二进制格式：u32 键长 + 键 + u32 值长 + 值，小端）；
/// 缺文件视为空库（close 时创建）。KvStore 值持 `store` id → 注册表。
pub(crate) fn call_storage_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "open" => {
            let path = path_arg_ir(ctx, args, 0)?;
            let mut entries = HashMap::new();
            if let Ok(bytes) = std::fs::read(&path) {
                let mut i = 0usize;
                while i < bytes.len() {
                    // 格式：u32 键长 + 键 + u32 值长 + 值（vlen 不紧跟 klen——键在中间）
                    if i + 4 > bytes.len() {
                        return Ok(Some(err_val(module, "InvalidFormat")));
                    }
                    let klen = u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
                    i += 4;
                    if i + klen + 4 > bytes.len() {
                        return Ok(Some(err_val(module, "InvalidFormat")));
                    }
                    let key = bytes[i..i + klen].to_vec();
                    i += klen;
                    let vlen = u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
                    i += 4;
                    if i + vlen > bytes.len() {
                        return Ok(Some(err_val(module, "InvalidFormat")));
                    }
                    let val = bytes[i..i + vlen].to_vec();
                    entries.insert(key, val);
                    i += vlen;
                }
            }
            let id = ctx.next_store_fd;
            ctx.next_store_fd += 1;
            ctx.stores.insert(id, (path, entries));
            let mut fld = HashMap::new();
            fld.insert(
                "store".into(),
                ctx.alloc(Cell::Value(IrValue::Int(id as i128))),
            );
            Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                name: "KvStore".into(),
                fields: fld,
            }))))
        }
        _ => Ok(None),
    }
}

/// KvStore 实例方法：`put(key, value) !void` / `get(key) !?&[u8]`（缺失 → null）/
/// `contains(key) bool` / `remove(key) !void`（幂等）/ `len() usize` /
/// `close() !void`（落盘 + 注销注册表；已关闭再 close 为 no-op）。
pub(crate) fn call_store_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    self_v: &IrValue,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let id = store_id_ir(ctx, self_v)?;
    match field {
        "put" => {
            let key = str_arg_ir(ctx, args, 0)?;
            let value = str_arg_ir(ctx, args, 1)?;
            let store = ctx
                .stores
                .get_mut(&id)
                .ok_or_else(|| IrError::msg("BadFd", "bad store"))?;
            store.1.insert(key, value);
            Ok(Some(IrValue::Void))
        }
        "get" => {
            let key = str_arg_ir(ctx, args, 0)?;
            let store = ctx
                .stores
                .get(&id)
                .ok_or_else(|| IrError::msg("BadFd", "bad store"))?;
            match store.1.get(&key) {
                Some(val) => Ok(Some(opt_val(Some(str_bytes_val(val.clone()))))),
                None => Ok(Some(IrValue::Opt(None))),
            }
        }
        "contains" => {
            let key = str_arg_ir(ctx, args, 0)?;
            let store = ctx
                .stores
                .get(&id)
                .ok_or_else(|| IrError::msg("BadFd", "bad store"))?;
            Ok(Some(IrValue::Bool(store.1.contains_key(&key))))
        }
        "remove" => {
            let key = str_arg_ir(ctx, args, 0)?;
            let store = ctx
                .stores
                .get_mut(&id)
                .ok_or_else(|| IrError::msg("BadFd", "bad store"))?;
            store.1.remove(&key);
            Ok(Some(IrValue::Void))
        }
        "len" => {
            let store = ctx
                .stores
                .get(&id)
                .ok_or_else(|| IrError::msg("BadFd", "bad store"))?;
            Ok(Some(IrValue::Int(store.1.len() as i128)))
        }
        "close" => {
            if let Some((path, entries)) = ctx.stores.remove(&id) {
                // 落盘：二进制格式（u32 键长 + 键 + u32 值长 + 值，小端）写回 path
                let mut out = Vec::new();
                for (k, v) in &entries {
                    out.extend_from_slice(&(k.len() as u32).to_le_bytes());
                    out.extend_from_slice(k);
                    out.extend_from_slice(&(v.len() as u32).to_le_bytes());
                    out.extend_from_slice(v);
                }
                if let Err(e) = std::fs::write(&path, out) {
                    return Ok(Some(err_val(module, &io_error_name_ir(&e))));
                }
            }
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

// ---- G4（E3.3 archive）LZ77 压缩 ----

/// io.archive.compress(data) !&[u8] / io.archive.decompress(data) !&[u8]——
/// LZ77 压缩（compress::compress/decompress 共享层）。非法压缩数据 → error.InvalidFormat。
pub(crate) fn call_archive_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "compress" => {
            let data = str_arg_ir(ctx, args, 0)?;
            Ok(Some(str_bytes_val(crate::runtime::compress::compress(
                &data,
            ))))
        }
        "decompress" => {
            let data = str_arg_ir(ctx, args, 0)?;
            match crate::runtime::compress::decompress(&data) {
                Ok(out) => Ok(Some(str_bytes_val(out))),
                Err(_) => Ok(Some(err_val(module, "InvalidFormat"))),
            }
        }
        _ => Ok(None),
    }
}

// ---- A6：标准库数据结构——Bitmap 位图 ----

/// io.bitmap.init(nbits) !Bitmap——创建 Bitmap。
pub(crate) fn call_bitmap_ns_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "init" => {
            let nbits = int_arg_ir(ctx, args, 0)?;
            if nbits < 0 {
                return Ok(Some(err_val(module, "InvalidArgument")));
            }
            let nwords = (nbits as usize).div_ceil(64);
            let items: Vec<IrValue> = (0..nwords).map(|_| IrValue::Int(0)).collect();
            let arr = make_arr(ctx, items);
            let arr_cell = match arr {
                IrValue::Arr(c) => c,
                _ => unreachable!(),
            };
            let mut fields = HashMap::new();
            fields.insert("words".into(), arr_cell);
            Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                name: "Bitmap".into(),
                fields,
            }))))
        }
        _ => Ok(None),
    }
}

/// Bitmap 实例方法：set/get/clear/count/len。
pub(crate) fn call_bitmap_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    method: &str,
    self_v: &IrValue,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    // 提取 words 数组中指定 word_idx 对应的元素 cell 索引
    let words_elem_idx = |ctx: &Ctx, v: &IrValue, word_idx: usize| -> R<usize> {
        let class_cell = match v {
            IrValue::Class(c) => *c,
            _ => return Err(IrError::msg("TypeError", "expected Bitmap")),
        };
        let words_cell = match &ctx.cells[class_cell] {
            Cell::Class { fields, .. } => *fields
                .get("words")
                .ok_or_else(|| IrError::msg("NoField", "no words"))?,
            _ => return Err(IrError::msg("TypeError", "expected Bitmap")),
        };
        let elems = match &ctx.cells[words_cell] {
            Cell::Elems(e) => e.clone(),
            _ => return Err(IrError::msg("TypeError", "expected array")),
        };
        elems
            .get(word_idx)
            .copied()
            .ok_or_else(|| IrError::msg("IndexError", "bitmap index out of bounds"))
    };

    match method {
        "set" => {
            let idx = int_arg_ir(ctx, args, 0)?;
            if idx < 0 {
                return Ok(Some(err_val(module, "InvalidArgument")));
            }
            let word_idx = (idx as usize) >> 6;
            let bit = 1u64 << ((idx as usize) & 63);
            let elem_cell = words_elem_idx(ctx, self_v, word_idx)?;
            let val = match &ctx.cells[elem_cell] {
                Cell::Value(IrValue::Int(v)) => *v as u64,
                _ => 0,
            };
            ctx.cells[elem_cell] = Cell::Value(IrValue::Int((val | bit) as i128));
            Ok(Some(IrValue::Void))
        }
        "get" => {
            let idx = int_arg_ir(ctx, args, 0)?;
            if idx < 0 {
                return Ok(Some(IrValue::Bool(false)));
            }
            let word_idx = (idx as usize) >> 6;
            let bit = 1u64 << ((idx as usize) & 63);
            let elem_cell = words_elem_idx(ctx, self_v, word_idx)?;
            let result = match &ctx.cells[elem_cell] {
                Cell::Value(IrValue::Int(v)) => (*v as u64) & bit != 0,
                _ => false,
            };
            Ok(Some(IrValue::Bool(result)))
        }
        "clear" => {
            let idx = int_arg_ir(ctx, args, 0)?;
            if idx < 0 {
                return Ok(Some(err_val(module, "InvalidArgument")));
            }
            let word_idx = (idx as usize) >> 6;
            let bit = 1u64 << ((idx as usize) & 63);
            let elem_cell = words_elem_idx(ctx, self_v, word_idx)?;
            let val = match &ctx.cells[elem_cell] {
                Cell::Value(IrValue::Int(v)) => *v as u64,
                _ => 0,
            };
            ctx.cells[elem_cell] = Cell::Value(IrValue::Int((val & !bit) as i128));
            Ok(Some(IrValue::Void))
        }
        "count" => {
            let class_cell = match self_v {
                IrValue::Class(c) => *c,
                _ => return Err(IrError::msg("TypeError", "expected Bitmap")),
            };
            let words_cell = match &ctx.cells[class_cell] {
                Cell::Class { fields, .. } => *fields
                    .get("words")
                    .ok_or_else(|| IrError::msg("NoField", "no words"))?,
                _ => return Err(IrError::msg("TypeError", "expected Bitmap")),
            };
            let total: u64 = match &ctx.cells[words_cell] {
                Cell::Elems(elems) => elems
                    .iter()
                    .map(|ec| match &ctx.cells[*ec] {
                        Cell::Value(IrValue::Int(v)) => (*v as u64).count_ones() as u64,
                        _ => 0,
                    })
                    .sum(),
                _ => 0,
            };
            Ok(Some(IrValue::Int(total as i128)))
        }
        "len" => {
            let class_cell = match self_v {
                IrValue::Class(c) => *c,
                _ => return Err(IrError::msg("TypeError", "expected Bitmap")),
            };
            let words_cell = match &ctx.cells[class_cell] {
                Cell::Class { fields, .. } => *fields
                    .get("words")
                    .ok_or_else(|| IrError::msg("NoField", "no words"))?,
                _ => return Err(IrError::msg("TypeError", "expected Bitmap")),
            };
            let nwords = match &ctx.cells[words_cell] {
                Cell::Elems(elems) => elems.len(),
                _ => 0,
            };
            Ok(Some(IrValue::Int((nwords * 64) as i128)))
        }
        _ => Ok(None),
    }
}

/// io.ringbuf.init(cap) !RingBuf——创建 RingBuf。
pub(crate) fn call_ringbuf_ns_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "init" => {
            let cap = int_arg_ir(ctx, args, 0)?;
            if cap < 0 {
                return Ok(Some(err_val(module, "InvalidArgument")));
            }
            let cap = cap as usize;
            let mut fields = HashMap::new();
            fields.insert("buf".into(), ctx.alloc(Cell::Elems(Vec::new())));
            fields.insert("head".into(), ctx.alloc(Cell::Value(IrValue::Int(0))));
            fields.insert("len".into(), ctx.alloc(Cell::Value(IrValue::Int(0))));
            fields.insert(
                "cap".into(),
                ctx.alloc(Cell::Value(IrValue::Int(cap as i128))),
            );
            Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                name: "RingBuf".into(),
                fields,
            }))))
        }
        _ => Ok(None),
    }
}

/// RingBuf 实例方法：push/pop/len/capacity/is_full/is_empty/clear/peek。
pub(crate) fn call_ringbuf_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    method: &str,
    self_v: &IrValue,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    // 提取字段 cell 索引
    let class_cell = match self_v {
        IrValue::Class(c) => *c,
        _ => return Err(IrError::msg("TypeError", "expected RingBuf")),
    };
    let get_field = |ctx: &Ctx, name: &str| -> Option<usize> {
        match &ctx.cells[class_cell] {
            Cell::Class { fields, .. } => fields.get(name).copied(),
            _ => None,
        }
    };
    let get_int = |ctx: &Ctx, name: &str| -> Option<i128> {
        let cell = get_field(ctx, name)?;
        match &ctx.cells[cell] {
            Cell::Value(IrValue::Int(n)) => Some(*n),
            _ => None,
        }
    };
    let set_int = |ctx: &mut Ctx, name: &str, val: i128| {
        if let Some(cell) = get_field(ctx, name) {
            ctx.cells[cell] = Cell::Value(IrValue::Int(val));
        }
    };

    match method {
        "push" => {
            let val = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "push"))?;
            let cap = get_int(ctx, "cap").unwrap_or(0) as usize;
            let cur_len = get_int(ctx, "len").unwrap_or(0) as usize;
            if cur_len >= cap {
                return Ok(Some(IrValue::Bool(false)));
            }
            let head = get_int(ctx, "head").unwrap_or(0) as usize;
            let idx = (head + cur_len) % cap;
            // 向 buf 数组 idx 位置写入 val
            if let Some(buf_cell) = get_field(ctx, "buf") {
                let elems = match &ctx.cells[buf_cell] {
                    Cell::Elems(e) => e.clone(),
                    _ => Vec::new(),
                };
                if idx < elems.len() {
                    ctx.cells[elems[idx]] = Cell::Value(val.clone());
                } else {
                    // 扩展数组
                    let new_elem = ctx.alloc(Cell::Value(val.clone()));
                    let mut new_elems = elems;
                    new_elems.push(new_elem);
                    ctx.cells[buf_cell] = Cell::Elems(new_elems);
                }
            }
            set_int(ctx, "len", (cur_len + 1) as i128);
            Ok(Some(IrValue::Bool(true)))
        }
        "pop" => {
            let cur_len = get_int(ctx, "len").unwrap_or(0) as usize;
            if cur_len == 0 {
                return Ok(Some(IrValue::Opt(None)));
            }
            let head = get_int(ctx, "head").unwrap_or(0) as usize;
            let cap = get_int(ctx, "cap").unwrap_or(0) as usize;
            let val = if let Some(buf_cell) = get_field(ctx, "buf") {
                match &ctx.cells[buf_cell] {
                    Cell::Elems(elems) => {
                        if head < elems.len() {
                            ctx.cells[elems[head]].clone()
                        } else {
                            Cell::Value(IrValue::Void)
                        }
                    }
                    _ => Cell::Value(IrValue::Void),
                }
            } else {
                Cell::Value(IrValue::Void)
            };
            set_int(ctx, "head", ((head + 1) % cap) as i128);
            set_int(ctx, "len", (cur_len - 1) as i128);
            match val {
                Cell::Value(v) => Ok(Some(v)),
                _ => Ok(Some(IrValue::Void)),
            }
        }
        "len" => Ok(Some(IrValue::Int(get_int(ctx, "len").unwrap_or(0)))),
        "capacity" => Ok(Some(IrValue::Int(get_int(ctx, "cap").unwrap_or(0)))),
        "is_full" => {
            let cur_len = get_int(ctx, "len").unwrap_or(0) as usize;
            let cap = get_int(ctx, "cap").unwrap_or(0) as usize;
            Ok(Some(IrValue::Bool(cur_len >= cap)))
        }
        "is_empty" => {
            let cur_len = get_int(ctx, "len").unwrap_or(0);
            Ok(Some(IrValue::Bool(cur_len == 0)))
        }
        "clear" => {
            set_int(ctx, "head", 0);
            set_int(ctx, "len", 0);
            Ok(Some(IrValue::Void))
        }
        "peek" => {
            let idx = int_arg_ir(ctx, args, 0)?;
            if idx < 0 {
                return Ok(Some(IrValue::Opt(None)));
            }
            let cur_len = get_int(ctx, "len").unwrap_or(0) as usize;
            if (idx as usize) >= cur_len {
                return Ok(Some(IrValue::Opt(None)));
            }
            let head = get_int(ctx, "head").unwrap_or(0) as usize;
            let cap = get_int(ctx, "cap").unwrap_or(0) as usize;
            let pos = (head + idx as usize) % cap;
            let val = if let Some(buf_cell) = get_field(ctx, "buf") {
                match &ctx.cells[buf_cell] {
                    Cell::Elems(elems) => {
                        if pos < elems.len() {
                            match &ctx.cells[elems[pos]] {
                                Cell::Value(v) => v.clone(),
                                _ => IrValue::Void,
                            }
                        } else {
                            IrValue::Void
                        }
                    }
                    _ => IrValue::Void,
                }
            } else {
                IrValue::Void
            };
            Ok(Some(val))
        }
        _ => Ok(None),
    }
}

/// io.pagemem.init(num_pages) !PageMem——创建 PageMem。
pub(crate) fn call_pagemem_ns_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "init" => {
            let num = int_arg_ir(ctx, args, 0)?;
            if num < 0 {
                return Ok(Some(err_val(module, "InvalidArgument")));
            }
            let n = num as usize;
            let items: Vec<IrValue> = (0..n).rev().map(|i| IrValue::Int(i as i128)).collect();
            let arr = make_arr(ctx, items);
            let arr_cell = match arr {
                IrValue::Arr(c) => c,
                _ => unreachable!(),
            };
            let mut fields = HashMap::new();
            fields.insert("free".into(), arr_cell);
            fields.insert("total".into(), ctx.alloc(Cell::Value(IrValue::Int(num))));
            Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                name: "PageMem".into(),
                fields,
            }))))
        }
        _ => Ok(None),
    }
}

/// PageMem 实例方法：alloc/free/available/total。
pub(crate) fn call_pagemem_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    method: &str,
    self_v: &IrValue,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let class_cell = match self_v {
        IrValue::Class(c) => *c,
        _ => return Err(IrError::msg("TypeError", "expected PageMem")),
    };
    let get_field = |ctx: &Ctx, name: &str| -> Option<usize> {
        match &ctx.cells[class_cell] {
            Cell::Class { fields, .. } => fields.get(name).copied(),
            _ => None,
        }
    };
    let get_int = |ctx: &Ctx, name: &str| -> Option<i128> {
        let cell = get_field(ctx, name)?;
        match &ctx.cells[cell] {
            Cell::Value(IrValue::Int(n)) => Some(*n),
            _ => None,
        }
    };

    match method {
        "alloc" => {
            if let Some(free_cell) = get_field(ctx, "free") {
                let elems = match &ctx.cells[free_cell] {
                    Cell::Elems(e) => e.clone(),
                    _ => Vec::new(),
                };
                if let Some(last) = elems.last() {
                    let val = match &ctx.cells[*last] {
                        Cell::Value(v) => v.clone(),
                        _ => IrValue::Int(0),
                    };
                    // 移除最后一个元素
                    let mut new_elems = elems;
                    new_elems.pop();
                    ctx.cells[free_cell] = Cell::Elems(new_elems);
                    return Ok(Some(val));
                }
            }
            Ok(Some(IrValue::Opt(None)))
        }
        "free" => {
            let idx = int_arg_ir(ctx, args, 0)?;
            if idx < 0 {
                return Ok(Some(err_val(module, "InvalidArgument")));
            }
            let total = get_int(ctx, "total").unwrap_or(0) as usize;
            if (idx as usize) >= total {
                return Ok(Some(IrValue::Void));
            }
            if let Some(free_cell) = get_field(ctx, "free") {
                // 检查是否已空闲（double-free 安全）
                let elems = match &ctx.cells[free_cell] {
                    Cell::Elems(e) => e.clone(),
                    _ => Vec::new(),
                };
                let already_free = elems.iter().any(|&ec| match &ctx.cells[ec] {
                    Cell::Value(IrValue::Int(v)) => *v == idx,
                    _ => false,
                });
                if !already_free {
                    let new_elem = ctx.alloc(Cell::Value(IrValue::Int(idx)));
                    let mut new_elems = elems;
                    new_elems.push(new_elem);
                    ctx.cells[free_cell] = Cell::Elems(new_elems);
                }
            }
            Ok(Some(IrValue::Void))
        }
        "available" => {
            let n = if let Some(free_cell) = get_field(ctx, "free") {
                match &ctx.cells[free_cell] {
                    Cell::Elems(e) => e.len(),
                    _ => 0,
                }
            } else {
                0
            };
            Ok(Some(IrValue::Int(n as i128)))
        }
        "total" => Ok(Some(IrValue::Int(get_int(ctx, "total").unwrap_or(0)))),
        _ => Ok(None),
    }
}

// ---- A6：标准库数据结构——IntrList 侵入式链表 ----

/// io.intrlist.init() !IntrList——创建空链表。
pub(crate) fn call_intrlist_ns_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "init" => {
            if !args.is_empty() {
                return Err(IrError::msg("ArityMismatch", "init"));
            }
            let mut fields = HashMap::new();
            fields.insert("prev".into(), ctx.alloc(Cell::Elems(Vec::new())));
            fields.insert("next".into(), ctx.alloc(Cell::Elems(Vec::new())));
            fields.insert("vals".into(), ctx.alloc(Cell::Elems(Vec::new())));
            fields.insert("head".into(), ctx.alloc(Cell::Value(IrValue::Int(-1))));
            fields.insert("tail".into(), ctx.alloc(Cell::Value(IrValue::Int(-1))));
            fields.insert("len".into(), ctx.alloc(Cell::Value(IrValue::Int(0))));
            fields.insert("free".into(), ctx.alloc(Cell::Elems(Vec::new())));
            Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                name: "IntrList".into(),
                fields,
            }))))
        }
        _ => Ok(None),
    }
}

/// IntrList 实例方法：push_front/pop_front/push_back/pop_back/remove/len/is_empty/clear。
pub(crate) fn call_intrlist_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    method: &str,
    self_v: &IrValue,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let class_cell = match self_v {
        IrValue::Class(c) => *c,
        _ => return Err(IrError::msg("TypeError", "expected IntrList")),
    };
    let get_field = |ctx: &Ctx, name: &str| -> Option<usize> {
        match &ctx.cells[class_cell] {
            Cell::Class { fields, .. } => fields.get(name).copied(),
            _ => None,
        }
    };
    let get_int = |ctx: &Ctx, name: &str| -> Option<i128> {
        let cell = get_field(ctx, name)?;
        match &ctx.cells[cell] {
            Cell::Value(IrValue::Int(n)) => Some(*n),
            _ => None,
        }
    };
    let set_int = |ctx: &mut Ctx, name: &str, val: i128| {
        if let Some(cell) = get_field(ctx, name) {
            ctx.cells[cell] = Cell::Value(IrValue::Int(val));
        }
    };

    match method {
        "push_front" => {
            let val = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "push_front"))?;
            // 分配节点索引
            let idx = if let Some(free_cell) = get_field(ctx, "free") {
                let elems = match &ctx.cells[free_cell] {
                    Cell::Elems(e) => e.clone(),
                    _ => Vec::new(),
                };
                if let Some(last) = elems.last() {
                    let val = match &ctx.cells[*last] {
                        Cell::Value(v) => v.clone(),
                        _ => IrValue::Int(0),
                    };
                    let mut new_elems = elems;
                    new_elems.pop();
                    ctx.cells[free_cell] = Cell::Elems(new_elems);
                    match val {
                        IrValue::Int(n) => Some(n as usize),
                        _ => None,
                    }
                } else {
                    None
                }
            } else {
                None
            };
            let idx = idx.unwrap_or_else(|| {
                if let Some(prev_cell) = get_field(ctx, "prev") {
                    match &ctx.cells[prev_cell] {
                        Cell::Elems(e) => e.len(),
                        _ => 0,
                    }
                } else {
                    0
                }
            });
            let head = get_int(ctx, "head").unwrap_or(-1) as isize;

            // 设置 prev[idx] = -1
            if let Some(prev_cell) = get_field(ctx, "prev") {
                let elems = match &ctx.cells[prev_cell] {
                    Cell::Elems(e) => e.clone(),
                    _ => Vec::new(),
                };
                let new_elem = ctx.alloc(Cell::Value(IrValue::Int(-1)));
                let mut new_elems = elems;
                if idx < new_elems.len() {
                    new_elems[idx] = new_elem;
                } else {
                    new_elems.push(new_elem);
                }
                ctx.cells[prev_cell] = Cell::Elems(new_elems);
            }
            // 设置 next[idx] = old_head
            if let Some(next_cell) = get_field(ctx, "next") {
                let elems = match &ctx.cells[next_cell] {
                    Cell::Elems(e) => e.clone(),
                    _ => Vec::new(),
                };
                let new_elem = ctx.alloc(Cell::Value(IrValue::Int(head as i128)));
                let mut new_elems = elems;
                if idx < new_elems.len() {
                    new_elems[idx] = new_elem;
                } else {
                    new_elems.push(new_elem);
                }
                ctx.cells[next_cell] = Cell::Elems(new_elems);
            }
            // 设置 vals[idx] = val
            if let Some(vals_cell) = get_field(ctx, "vals") {
                let elems = match &ctx.cells[vals_cell] {
                    Cell::Elems(e) => e.clone(),
                    _ => Vec::new(),
                };
                let new_elem = ctx.alloc(Cell::Value(val.clone()));
                let mut new_elems = elems;
                if idx < new_elems.len() {
                    new_elems[idx] = new_elem;
                } else {
                    new_elems.push(new_elem);
                }
                ctx.cells[vals_cell] = Cell::Elems(new_elems);
            }
            // 如果 old_head 存在，设置 prev[old_head] = idx
            if head >= 0 {
                if let Some(prev_cell) = get_field(ctx, "prev") {
                    let elems = match &ctx.cells[prev_cell] {
                        Cell::Elems(e) => e.clone(),
                        _ => Vec::new(),
                    };
                    if let Some(&elem_cell) = elems.get(head as usize) {
                        ctx.cells[elem_cell] = Cell::Value(IrValue::Int(idx as i128));
                    }
                }
            } else {
                set_int(ctx, "tail", idx as i128);
            }
            set_int(ctx, "head", idx as i128);
            let cur_len = get_int(ctx, "len").unwrap_or(0);
            set_int(ctx, "len", cur_len + 1);
            Ok(Some(IrValue::Int(idx as i128)))
        }
        "pop_front" => {
            let head = get_int(ctx, "head").unwrap_or(-1);
            if head < 0 {
                return Ok(Some(IrValue::Opt(None)));
            }
            let head = head as usize;
            // 读取值
            let val = if let Some(vals_cell) = get_field(ctx, "vals") {
                match &ctx.cells[vals_cell] {
                    Cell::Elems(elems) => {
                        if head < elems.len() {
                            match &ctx.cells[elems[head]] {
                                Cell::Value(v) => v.clone(),
                                _ => IrValue::Void,
                            }
                        } else {
                            IrValue::Void
                        }
                    }
                    _ => IrValue::Void,
                }
            } else {
                IrValue::Void
            };
            // 获取 next 索引
            let next_idx = if let Some(next_cell) = get_field(ctx, "next") {
                match &ctx.cells[next_cell] {
                    Cell::Elems(elems) => {
                        if head < elems.len() {
                            match &ctx.cells[elems[head]] {
                                Cell::Value(IrValue::Int(v)) => *v,
                                _ => -1,
                            }
                        } else {
                            -1
                        }
                    }
                    _ => -1,
                }
            } else {
                -1
            };
            // 释放 head 索引
            if let Some(free_cell) = get_field(ctx, "free") {
                let elems = match &ctx.cells[free_cell] {
                    Cell::Elems(e) => e.clone(),
                    _ => Vec::new(),
                };
                let new_elem = ctx.alloc(Cell::Value(IrValue::Int(head as i128)));
                let mut new_elems = elems;
                new_elems.push(new_elem);
                ctx.cells[free_cell] = Cell::Elems(new_elems);
            }
            // 更新 head
            if next_idx >= 0 {
                if let Some(prev_cell) = get_field(ctx, "prev") {
                    let elems = match &ctx.cells[prev_cell] {
                        Cell::Elems(e) => e.clone(),
                        _ => Vec::new(),
                    };
                    if let Some(&elem_cell) = elems.get(next_idx as usize) {
                        ctx.cells[elem_cell] = Cell::Value(IrValue::Int(-1));
                    }
                }
                set_int(ctx, "head", next_idx);
            } else {
                set_int(ctx, "head", -1);
                set_int(ctx, "tail", -1);
            }
            let cur_len = get_int(ctx, "len").unwrap_or(0);
            set_int(ctx, "len", cur_len - 1);
            Ok(Some(val))
        }
        "push_back" => {
            let val = args
                .get(0)
                .ok_or_else(|| IrError::msg("ArityMismatch", "push_back"))?;
            // 分配节点索引
            let idx = if let Some(free_cell) = get_field(ctx, "free") {
                let elems = match &ctx.cells[free_cell] {
                    Cell::Elems(e) => e.clone(),
                    _ => Vec::new(),
                };
                if let Some(last) = elems.last() {
                    let val = match &ctx.cells[*last] {
                        Cell::Value(v) => v.clone(),
                        _ => IrValue::Int(0),
                    };
                    let mut new_elems = elems;
                    new_elems.pop();
                    ctx.cells[free_cell] = Cell::Elems(new_elems);
                    match val {
                        IrValue::Int(n) => Some(n as usize),
                        _ => None,
                    }
                } else {
                    None
                }
            } else {
                None
            };
            let idx = idx.unwrap_or_else(|| {
                if let Some(prev_cell) = get_field(ctx, "prev") {
                    match &ctx.cells[prev_cell] {
                        Cell::Elems(e) => e.len(),
                        _ => 0,
                    }
                } else {
                    0
                }
            });
            let tail = get_int(ctx, "tail").unwrap_or(-1) as isize;

            // 设置 prev[idx] = old_tail
            if let Some(prev_cell) = get_field(ctx, "prev") {
                let elems = match &ctx.cells[prev_cell] {
                    Cell::Elems(e) => e.clone(),
                    _ => Vec::new(),
                };
                let new_elem = ctx.alloc(Cell::Value(IrValue::Int(tail as i128)));
                let mut new_elems = elems;
                if idx < new_elems.len() {
                    new_elems[idx] = new_elem;
                } else {
                    new_elems.push(new_elem);
                }
                ctx.cells[prev_cell] = Cell::Elems(new_elems);
            }
            // 设置 next[idx] = -1
            if let Some(next_cell) = get_field(ctx, "next") {
                let elems = match &ctx.cells[next_cell] {
                    Cell::Elems(e) => e.clone(),
                    _ => Vec::new(),
                };
                let new_elem = ctx.alloc(Cell::Value(IrValue::Int(-1)));
                let mut new_elems = elems;
                if idx < new_elems.len() {
                    new_elems[idx] = new_elem;
                } else {
                    new_elems.push(new_elem);
                }
                ctx.cells[next_cell] = Cell::Elems(new_elems);
            }
            // 设置 vals[idx] = val
            if let Some(vals_cell) = get_field(ctx, "vals") {
                let elems = match &ctx.cells[vals_cell] {
                    Cell::Elems(e) => e.clone(),
                    _ => Vec::new(),
                };
                let new_elem = ctx.alloc(Cell::Value(val.clone()));
                let mut new_elems = elems;
                if idx < new_elems.len() {
                    new_elems[idx] = new_elem;
                } else {
                    new_elems.push(new_elem);
                }
                ctx.cells[vals_cell] = Cell::Elems(new_elems);
            }
            // 如果 old_tail 存在，设置 next[old_tail] = idx
            if tail >= 0 {
                if let Some(next_cell) = get_field(ctx, "next") {
                    let elems = match &ctx.cells[next_cell] {
                        Cell::Elems(e) => e.clone(),
                        _ => Vec::new(),
                    };
                    if let Some(&elem_cell) = elems.get(tail as usize) {
                        ctx.cells[elem_cell] = Cell::Value(IrValue::Int(idx as i128));
                    }
                }
            } else {
                set_int(ctx, "head", idx as i128);
            }
            set_int(ctx, "tail", idx as i128);
            let cur_len = get_int(ctx, "len").unwrap_or(0);
            set_int(ctx, "len", cur_len + 1);
            Ok(Some(IrValue::Int(idx as i128)))
        }
        "pop_back" => {
            let tail = get_int(ctx, "tail").unwrap_or(-1);
            if tail < 0 {
                return Ok(Some(IrValue::Opt(None)));
            }
            let tail = tail as usize;
            // 读取值
            let val = if let Some(vals_cell) = get_field(ctx, "vals") {
                match &ctx.cells[vals_cell] {
                    Cell::Elems(elems) => {
                        if tail < elems.len() {
                            match &ctx.cells[elems[tail]] {
                                Cell::Value(v) => v.clone(),
                                _ => IrValue::Void,
                            }
                        } else {
                            IrValue::Void
                        }
                    }
                    _ => IrValue::Void,
                }
            } else {
                IrValue::Void
            };
            // 获取 prev 索引
            let prev_idx = if let Some(prev_cell) = get_field(ctx, "prev") {
                match &ctx.cells[prev_cell] {
                    Cell::Elems(elems) => {
                        if tail < elems.len() {
                            match &ctx.cells[elems[tail]] {
                                Cell::Value(IrValue::Int(v)) => *v,
                                _ => -1,
                            }
                        } else {
                            -1
                        }
                    }
                    _ => -1,
                }
            } else {
                -1
            };
            // 释放 tail 索引
            if let Some(free_cell) = get_field(ctx, "free") {
                let elems = match &ctx.cells[free_cell] {
                    Cell::Elems(e) => e.clone(),
                    _ => Vec::new(),
                };
                let new_elem = ctx.alloc(Cell::Value(IrValue::Int(tail as i128)));
                let mut new_elems = elems;
                new_elems.push(new_elem);
                ctx.cells[free_cell] = Cell::Elems(new_elems);
            }
            // 更新 tail
            if prev_idx >= 0 {
                if let Some(next_cell) = get_field(ctx, "next") {
                    let elems = match &ctx.cells[next_cell] {
                        Cell::Elems(e) => e.clone(),
                        _ => Vec::new(),
                    };
                    if let Some(&elem_cell) = elems.get(prev_idx as usize) {
                        ctx.cells[elem_cell] = Cell::Value(IrValue::Int(-1));
                    }
                }
                set_int(ctx, "tail", prev_idx);
            } else {
                set_int(ctx, "head", -1);
                set_int(ctx, "tail", -1);
            }
            let cur_len = get_int(ctx, "len").unwrap_or(0);
            set_int(ctx, "len", cur_len - 1);
            Ok(Some(val))
        }
        "remove" => {
            let idx = int_arg_ir(ctx, args, 0)?;
            if idx < 0 {
                return Ok(Some(IrValue::Opt(None)));
            }
            let idx = idx as usize;
            // 检查节点是否存在
            let node_exists = if let Some(vals_cell) = get_field(ctx, "vals") {
                match &ctx.cells[vals_cell] {
                    Cell::Elems(elems) => idx < elems.len(),
                    _ => false,
                }
            } else {
                false
            };
            if !node_exists {
                return Ok(Some(IrValue::Opt(None)));
            }
            // 读取 prev 和 next
            let prev_idx = if let Some(prev_cell) = get_field(ctx, "prev") {
                match &ctx.cells[prev_cell] {
                    Cell::Elems(elems) => {
                        if idx < elems.len() {
                            match &ctx.cells[elems[idx]] {
                                Cell::Value(IrValue::Int(v)) => *v,
                                _ => -1,
                            }
                        } else {
                            -1
                        }
                    }
                    _ => -1,
                }
            } else {
                -1
            };
            let next_idx = if let Some(next_cell) = get_field(ctx, "next") {
                match &ctx.cells[next_cell] {
                    Cell::Elems(elems) => {
                        if idx < elems.len() {
                            match &ctx.cells[elems[idx]] {
                                Cell::Value(IrValue::Int(v)) => *v,
                                _ => -1,
                            }
                        } else {
                            -1
                        }
                    }
                    _ => -1,
                }
            } else {
                -1
            };
            // 读取值
            let val = if let Some(vals_cell) = get_field(ctx, "vals") {
                match &ctx.cells[vals_cell] {
                    Cell::Elems(elems) => {
                        if idx < elems.len() {
                            match &ctx.cells[elems[idx]] {
                                Cell::Value(v) => v.clone(),
                                _ => IrValue::Void,
                            }
                        } else {
                            IrValue::Void
                        }
                    }
                    _ => IrValue::Void,
                }
            } else {
                IrValue::Void
            };
            // 更新前后节点的链接
            if prev_idx >= 0 {
                if let Some(next_cell) = get_field(ctx, "next") {
                    let elems = match &ctx.cells[next_cell] {
                        Cell::Elems(e) => e.clone(),
                        _ => Vec::new(),
                    };
                    if let Some(&elem_cell) = elems.get(prev_idx as usize) {
                        ctx.cells[elem_cell] = Cell::Value(IrValue::Int(next_idx));
                    }
                }
            } else {
                set_int(ctx, "head", next_idx);
            }
            if next_idx >= 0 {
                if let Some(prev_cell) = get_field(ctx, "prev") {
                    let elems = match &ctx.cells[prev_cell] {
                        Cell::Elems(e) => e.clone(),
                        _ => Vec::new(),
                    };
                    if let Some(&elem_cell) = elems.get(next_idx as usize) {
                        ctx.cells[elem_cell] = Cell::Value(IrValue::Int(prev_idx));
                    }
                }
            } else {
                set_int(ctx, "tail", prev_idx);
            }
            // 释放索引
            if let Some(free_cell) = get_field(ctx, "free") {
                let elems = match &ctx.cells[free_cell] {
                    Cell::Elems(e) => e.clone(),
                    _ => Vec::new(),
                };
                let new_elem = ctx.alloc(Cell::Value(IrValue::Int(idx as i128)));
                let mut new_elems = elems;
                new_elems.push(new_elem);
                ctx.cells[free_cell] = Cell::Elems(new_elems);
            }
            let cur_len = get_int(ctx, "len").unwrap_or(0);
            set_int(ctx, "len", cur_len - 1);
            Ok(Some(val))
        }
        "len" => Ok(Some(IrValue::Int(get_int(ctx, "len").unwrap_or(0)))),
        "is_empty" => {
            let cur_len = get_int(ctx, "len").unwrap_or(0);
            Ok(Some(IrValue::Bool(cur_len == 0)))
        }
        "clear" => {
            if let Some(prev_cell) = get_field(ctx, "prev") {
                ctx.cells[prev_cell] = Cell::Elems(Vec::new());
            }
            if let Some(next_cell) = get_field(ctx, "next") {
                ctx.cells[next_cell] = Cell::Elems(Vec::new());
            }
            if let Some(vals_cell) = get_field(ctx, "vals") {
                ctx.cells[vals_cell] = Cell::Elems(Vec::new());
            }
            if let Some(free_cell) = get_field(ctx, "free") {
                ctx.cells[free_cell] = Cell::Elems(Vec::new());
            }
            set_int(ctx, "head", -1);
            set_int(ctx, "tail", -1);
            set_int(ctx, "len", 0);
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

// ---- A6：标准库数据结构——TreeMap 有序映射 ----

/// io.treemap.init() !TreeMap——创建空 TreeMap。
pub(crate) fn call_treemap_ns_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "init" => {
            if !args.is_empty() {
                return Err(IrError::msg("ArityMismatch", "init"));
            }
            let mut fields = HashMap::new();
            fields.insert("keys".into(), ctx.alloc(Cell::Elems(Vec::new())));
            fields.insert("vals".into(), ctx.alloc(Cell::Elems(Vec::new())));
            fields.insert("left".into(), ctx.alloc(Cell::Elems(Vec::new())));
            fields.insert("right".into(), ctx.alloc(Cell::Elems(Vec::new())));
            fields.insert("root".into(), ctx.alloc(Cell::Value(IrValue::Int(-1))));
            fields.insert("len".into(), ctx.alloc(Cell::Value(IrValue::Int(0))));
            fields.insert("free".into(), ctx.alloc(Cell::Elems(Vec::new())));
            Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                name: "TreeMap".into(),
                fields,
            }))))
        }
        _ => Ok(None),
    }
}

/// TreeMap 实例方法：insert/get/contains/len/is_empty/clear。
pub(crate) fn call_treemap_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    method: &str,
    self_v: &IrValue,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let class_cell = match self_v {
        IrValue::Class(c) => *c,
        _ => return Err(IrError::msg("TypeError", "expected TreeMap")),
    };
    let get_field = |ctx: &Ctx, name: &str| -> Option<usize> {
        match &ctx.cells[class_cell] {
            Cell::Class { fields, .. } => fields.get(name).copied(),
            _ => None,
        }
    };
    let get_int = |ctx: &Ctx, name: &str| -> Option<i128> {
        let cell = get_field(ctx, name)?;
        match &ctx.cells[cell] {
            Cell::Value(IrValue::Int(n)) => Some(*n),
            _ => None,
        }
    };
    let set_int = |ctx: &mut Ctx, name: &str, val: i128| {
        if let Some(cell) = get_field(ctx, name) {
            ctx.cells[cell] = Cell::Value(IrValue::Int(val));
        }
    };

    // 辅助：获取数组中指定索引的值
    let get_arr_elem = |ctx: &Ctx, arr_name: &str, idx: usize| -> Option<IrValue> {
        let cell = get_field(ctx, arr_name)?;
        match &ctx.cells[cell] {
            Cell::Elems(elems) => {
                if idx < elems.len() {
                    match &ctx.cells[elems[idx]] {
                        Cell::Value(v) => Some(v.clone()),
                        _ => None,
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    };
    // 辅助：设置数组中指定索引的值
    let set_arr_elem = |ctx: &mut Ctx, arr_name: &str, idx: usize, val: IrValue| {
        if let Some(cell) = get_field(ctx, arr_name) {
            let elems = match &ctx.cells[cell] {
                Cell::Elems(e) => e.clone(),
                _ => Vec::new(),
            };
            let new_elem = ctx.alloc(Cell::Value(val));
            let mut new_elems = elems;
            if idx < new_elems.len() {
                new_elems[idx] = new_elem;
            } else {
                new_elems.push(new_elem);
            }
            ctx.cells[cell] = Cell::Elems(new_elems);
        }
    };

    match method {
        "insert" => {
            let key = int_arg_ir(ctx, args, 0)?;
            let val = args
                .get(1)
                .ok_or_else(|| IrError::msg("ArityMismatch", "insert"))?;

            let root = get_int(ctx, "root").unwrap_or(-1);
            if root < 0 {
                // 创建根节点
                set_arr_elem(ctx, "keys", 0, IrValue::Int(key));
                set_arr_elem(ctx, "vals", 0, val.clone());
                set_arr_elem(ctx, "left", 0, IrValue::Int(-1));
                set_arr_elem(ctx, "right", 0, IrValue::Int(-1));
                set_int(ctx, "root", 0);
                set_int(ctx, "len", 1);
                return Ok(Some(IrValue::Void));
            }

            let mut cur = root as usize;
            loop {
                let cur_key = match get_arr_elem(ctx, "keys", cur) {
                    Some(IrValue::Int(n)) => n,
                    _ => 0,
                };
                if key == cur_key {
                    // 键已存在，更新值
                    set_arr_elem(ctx, "vals", cur, val.clone());
                    return Ok(Some(IrValue::Void));
                }

                let next_opt = if key < cur_key {
                    match get_arr_elem(ctx, "left", cur) {
                        Some(IrValue::Int(n)) if n >= 0 => Some(n as usize),
                        _ => None,
                    }
                } else {
                    match get_arr_elem(ctx, "right", cur) {
                        Some(IrValue::Int(n)) if n >= 0 => Some(n as usize),
                        _ => None,
                    }
                };

                match next_opt {
                    Some(next) => cur = next,
                    None => {
                        // 分配新节点
                        let idx = if let Some(free_cell) = get_field(ctx, "free") {
                            let elems = match &ctx.cells[free_cell] {
                                Cell::Elems(e) => e.clone(),
                                _ => Vec::new(),
                            };
                            if let Some(last) = elems.last() {
                                let val = match &ctx.cells[*last] {
                                    Cell::Value(v) => v.clone(),
                                    _ => IrValue::Int(0),
                                };
                                let mut new_elems = elems;
                                new_elems.pop();
                                ctx.cells[free_cell] = Cell::Elems(new_elems);
                                match val {
                                    IrValue::Int(n) => Some(n as usize),
                                    _ => None,
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        let idx = idx.unwrap_or_else(|| {
                            if let Some(keys_cell) = get_field(ctx, "keys") {
                                match &ctx.cells[keys_cell] {
                                    Cell::Elems(e) => e.len(),
                                    _ => 0,
                                }
                            } else {
                                0
                            }
                        });

                        set_arr_elem(ctx, "keys", idx, IrValue::Int(key));
                        set_arr_elem(ctx, "vals", idx, val.clone());
                        set_arr_elem(ctx, "left", idx, IrValue::Int(-1));
                        set_arr_elem(ctx, "right", idx, IrValue::Int(-1));

                        // 更新父节点的子节点指针
                        if key < cur_key {
                            set_arr_elem(ctx, "left", cur, IrValue::Int(idx as i128));
                        } else {
                            set_arr_elem(ctx, "right", cur, IrValue::Int(idx as i128));
                        }

                        let cur_len = get_int(ctx, "len").unwrap_or(0);
                        set_int(ctx, "len", cur_len + 1);
                        return Ok(Some(IrValue::Void));
                    }
                }
            }
        }
        "get" => {
            let key = int_arg_ir(ctx, args, 0)?;
            let mut cur = get_int(ctx, "root").unwrap_or(-1);
            loop {
                if cur < 0 {
                    return Ok(Some(IrValue::Opt(None)));
                }
                let cur_key = match get_arr_elem(ctx, "keys", cur as usize) {
                    Some(IrValue::Int(n)) => n,
                    _ => 0,
                };
                if key == cur_key {
                    let val = get_arr_elem(ctx, "vals", cur as usize).unwrap_or(IrValue::Void);
                    return Ok(Some(val));
                }
                if key < cur_key {
                    cur = match get_arr_elem(ctx, "left", cur as usize) {
                        Some(IrValue::Int(n)) => n,
                        _ => -1,
                    };
                } else {
                    cur = match get_arr_elem(ctx, "right", cur as usize) {
                        Some(IrValue::Int(n)) => n,
                        _ => -1,
                    };
                }
            }
        }
        "contains" => {
            let key = int_arg_ir(ctx, args, 0)?;
            let mut cur = get_int(ctx, "root").unwrap_or(-1);
            loop {
                if cur < 0 {
                    return Ok(Some(IrValue::Bool(false)));
                }
                let cur_key = match get_arr_elem(ctx, "keys", cur as usize) {
                    Some(IrValue::Int(n)) => n,
                    _ => 0,
                };
                if key == cur_key {
                    return Ok(Some(IrValue::Bool(true)));
                }
                if key < cur_key {
                    cur = match get_arr_elem(ctx, "left", cur as usize) {
                        Some(IrValue::Int(n)) => n,
                        _ => -1,
                    };
                } else {
                    cur = match get_arr_elem(ctx, "right", cur as usize) {
                        Some(IrValue::Int(n)) => n,
                        _ => -1,
                    };
                }
            }
        }
        "len" => Ok(Some(IrValue::Int(get_int(ctx, "len").unwrap_or(0)))),
        "is_empty" => {
            let cur_len = get_int(ctx, "len").unwrap_or(0);
            Ok(Some(IrValue::Bool(cur_len == 0)))
        }
        "clear" => {
            if let Some(cell) = get_field(ctx, "keys") {
                ctx.cells[cell] = Cell::Elems(Vec::new());
            }
            if let Some(cell) = get_field(ctx, "vals") {
                ctx.cells[cell] = Cell::Elems(Vec::new());
            }
            if let Some(cell) = get_field(ctx, "left") {
                ctx.cells[cell] = Cell::Elems(Vec::new());
            }
            if let Some(cell) = get_field(ctx, "right") {
                ctx.cells[cell] = Cell::Elems(Vec::new());
            }
            if let Some(cell) = get_field(ctx, "free") {
                ctx.cells[cell] = Cell::Elems(Vec::new());
            }
            set_int(ctx, "root", -1);
            set_int(ctx, "len", 0);
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

// ---- G5（E3.3 text）正则文本处理 ----

/// io.text.* —— `matches(pattern, text) bool`（是否含匹配；`^`/`$` 锚定控制
/// 全串）/ `find(pattern, text) ?int`（首个匹配起点；无 → null）/
/// `replace(pattern, text, repl) &[u8]`（替换全部非重叠匹配，每处取最长）/
/// `split(pattern, text) Vec(&[u8])`（按匹配分割，含空段）。非法模式 →
/// error.InvalidFormat。
pub(crate) fn call_text_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "matches" => {
            let pat = str_arg_ir(ctx, args, 0)?;
            let text = str_arg_ir(ctx, args, 1)?;
            let ast = match parse_regex(&pat) {
                Some(a) => a,
                None => return Ok(Some(err_val(module, "InvalidFormat"))),
            };
            let mut m = RegexMatcher::new(&ast, &text);
            Ok(Some(IrValue::Bool(m.find_at(0).is_some())))
        }
        "find" => {
            let pat = str_arg_ir(ctx, args, 0)?;
            let text = str_arg_ir(ctx, args, 1)?;
            let ast = match parse_regex(&pat) {
                Some(a) => a,
                None => return Ok(Some(err_val(module, "InvalidFormat"))),
            };
            let mut m = RegexMatcher::new(&ast, &text);
            match m.find_at(0) {
                Some((s, _e)) => Ok(Some(opt_val(Some(IrValue::Int(s as i128))))),
                None => Ok(Some(IrValue::Opt(None))),
            }
        }
        "replace" => {
            let pat = str_arg_ir(ctx, args, 0)?;
            let text = str_arg_ir(ctx, args, 1)?;
            let repl = str_arg_ir(ctx, args, 2)?;
            let ast = match parse_regex(&pat) {
                Some(a) => a,
                None => return Ok(Some(err_val(module, "InvalidFormat"))),
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
            Ok(Some(str_bytes_val(out)))
        }
        "split" => {
            let pat = str_arg_ir(ctx, args, 0)?;
            let text = str_arg_ir(ctx, args, 1)?;
            let ast = match parse_regex(&pat) {
                Some(a) => a,
                None => return Ok(Some(err_val(module, "InvalidFormat"))),
            };
            let mut m = RegexMatcher::new(&ast, &text);
            let mut parts: Vec<IrValue> = Vec::new();
            let mut start = 0usize;
            let mut cur = 0usize;
            loop {
                match m.find_at(cur) {
                    Some((s, e)) => {
                        parts.push(str_bytes_val(text[start..s].to_vec()));
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
            parts.push(str_bytes_val(text[start.min(text.len())..].to_vec()));
            let alloc = implicit_env_value(ctx, "alloc");
            Ok(Some(make_vec_with(ctx, parts, alloc)))
        }
        _ => Ok(None),
    }
}

// ---- G5（E3.3 rng）伪随机数 ----

/// io.rng.* —— `seed(v)`（重置状态；0 → 回退默认）/ `next() int`（下个原始
/// 64 位）/ `int(n) int`（[0, n) 均匀，拒绝采样免模偏差）/ `float() f64`
/// （[0, 1)，高 53 位均匀）。全局态在 Ctx（协作式单线程执行下安全）；
/// 命名空间类名 RngNs 避开示例 84-rng 的用户类 Rng。
pub(crate) fn call_rng_method_ir(
    ctx: &mut Ctx,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match field {
        "seed" => {
            let v = int_arg_ir(ctx, args, 0)?;
            ctx.rng_state = if v == 0 {
                0x9e37_79b9_7f4a_7c15
            } else {
                v as u64
            };
            Ok(Some(IrValue::Void))
        }
        "next" => {
            let n = xorshift64(&mut ctx.rng_state);
            Ok(Some(IrValue::Int(n as i128)))
        }
        "int" => {
            let bound = int_arg_ir(ctx, args, 0)?;
            if bound <= 0 {
                return Ok(Some(IrValue::Int(0)));
            }
            let b = bound as u64;
            let threshold = b.wrapping_neg() % b;
            let mut v = xorshift64(&mut ctx.rng_state);
            while v < threshold {
                v = xorshift64(&mut ctx.rng_state);
            }
            Ok(Some(IrValue::Int((v % b) as i128)))
        }
        "float" => {
            let v = xorshift64(&mut ctx.rng_state) >> 11;
            let f = (v as f64) / ((1u64 << 53) as f64);
            Ok(Some(IrValue::Float(f)))
        }
        _ => Ok(None),
    }
}

pub(crate) fn call_conn_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    self_v: &IrValue,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let fd = net_fd_ir(ctx, self_v)?;
    match field {
        "write" => {
            let data = str_arg_ir(ctx, args, 0)?;
            let stream = ctx
                .tcp_streams
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad conn"))?;
            match stream.write_all(&data) {
                Ok(_) => Ok(Some(IrValue::Void)),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "read" => {
            let n = int_arg_ir(ctx, args, 0)?.max(0) as usize;
            let stream = ctx
                .tcp_streams
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad conn"))?;
            let mut buf = vec![0u8; n];
            match stream.read(&mut buf) {
                Ok(0) => Ok(Some(str_bytes_val(vec![]))),
                Ok(k) => {
                    buf.truncate(k);
                    Ok(Some(str_bytes_val(buf)))
                }
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "read_all" => {
            let stream = ctx
                .tcp_streams
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad conn"))?;
            let mut buf = Vec::new();
            match stream.read_to_end(&mut buf) {
                Ok(_) => Ok(Some(str_bytes_val(buf))),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "write_u32_le" => {
            let n = int_arg_ir(ctx, args, 0)?;
            let stream = ctx
                .tcp_streams
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad conn"))?;
            match stream.write_all(&(n as u32).to_le_bytes()) {
                Ok(_) => Ok(Some(IrValue::Void)),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "read_u32_le" => {
            let stream = ctx
                .tcp_streams
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad conn"))?;
            let mut buf = [0u8; 4];
            match stream.read_exact(&mut buf) {
                Ok(_) => Ok(Some(IrValue::Int(u32::from_le_bytes(buf) as i128))),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "shutdown" => {
            let stream = ctx
                .tcp_streams
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad conn"))?;
            match stream.shutdown(std::net::Shutdown::Write) {
                Ok(_) => Ok(Some(IrValue::Void)),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "close" => {
            ctx.tcp_streams.remove(&fd);
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

pub(crate) fn call_listener_method_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    self_v: &IrValue,
    field: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let _ = args;
    let fd = net_fd_ir(ctx, self_v)?;
    match field {
        "local_port" => {
            let listener = ctx
                .tcp_listeners
                .get(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad listener"))?;
            match listener.local_addr() {
                Ok(addr) => Ok(Some(IrValue::Int(addr.port() as i128))),
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "accept" => {
            let listener = ctx
                .tcp_listeners
                .get_mut(&fd)
                .ok_or_else(|| IrError::msg("BadFd", "bad listener"))?;
            match listener.accept() {
                Ok((stream, _peer)) => {
                    let cfd = ctx.next_net_fd;
                    ctx.next_net_fd += 1;
                    let _ = stream.set_nodelay(true);
                    ctx.tcp_streams.insert(cfd, stream);
                    let mut fields = HashMap::new();
                    fields.insert(
                        "fd".into(),
                        ctx.alloc(Cell::Value(IrValue::Int(cfd as i128))),
                    );
                    Ok(Some(IrValue::Class(ctx.alloc(Cell::Class {
                        name: "TcpConn".into(),
                        fields,
                    }))))
                }
                Err(e) => Ok(Some(err_val(module, &io_error_name_ir(&e)))),
            }
        }
        "close" => {
            ctx.tcp_listeners.remove(&fd);
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

pub(crate) fn call_io_method_ir(
    ctx: &mut Ctx,
    _module: &IrModule,
    method: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    match method {
        "print" => {
            call_io_print_ir(ctx, args)?;
            Ok(Some(IrValue::Void))
        }
        // io.exit(ExitType, code)：正常退出信号（execute_ir 读 ctx.exit_code 映射退出码，
        // F2——与 oracle Interp.exit_code 对齐）
        "exit" => {
            if args.len() != 2 {
                return Err(IrError::msg("ArityMismatch", "io.exit expects 2 args"));
            }
            let t = deref_value(ctx, &args[0]);
            let code = match deref_value(ctx, &args[1]) {
                IrValue::Int(i) => (*i).clamp(0, 255) as u8,
                _ => return Err(IrError::msg("TypeError", "io.exit expects int code")),
            };
            let is_error = matches!(t, IrValue::Enum { variant, .. } if variant == "Error");
            if is_error {
                eprintln!("error: program exited with code {code}");
            }
            ctx.exit_code = Some(code);
            Err(IrError::msg("ExitRequested", format!("code {code}")))
        }
        "stdin" => {
            let mut line = String::new();
            match std::io::stdin().read_line(&mut line) {
                Ok(0) | Err(_) => Ok(Some(str_val(""))),
                Ok(_) => {
                    let trimmed = line.trim_end_matches(['\n', '\r']);
                    Ok(Some(str_val(trimmed)))
                }
            }
        }
        "args" => Ok(Some(make_arr(
            ctx,
            ctx.args.iter().map(|a| IrValue::Str(a.clone())).collect(),
        ))),
        "env" => {
            let name = str_arg_ir(ctx, args, 0)?;
            match std::env::var(String::from_utf8_lossy(&name).as_ref()) {
                Ok(v) => Ok(Some(opt_val(Some(str_val(&v))))),
                Err(_) => Ok(Some(IrValue::Opt(None))),
            }
        }
        _ => Ok(None),
    }
}

// ---- 组 G 线程生命周期（协作式延迟执行；对齐 oracle call_thread_method interp.rs:4610）----

/// Thread 类方法分派：`join() !T` / `cancel() !void` / `is_done() bool` / `detach()`。
/// 内建方法（对齐 oracle `call_builtin_method` interp.rs:3511-4027 全量面：标量/Str/Arr/
/// Map/Alloc/Arena/Io/Fs/Time/Net/TcpConn/TcpListener/File + iter/filter/map + 序列化）。
/// 返回 `Ok(None)` = 非内建方法（调用方回退到 `{Type}.{method}` 用户方法表）。
pub(crate) fn call_builtin_method(
    ctx: &mut Ctx,
    module: &IrModule,
    self_v: &IrValue,
    method: &str,
    args: &[IrValue],
) -> R<Option<IrValue>> {
    let self_v = deref_value(ctx, self_v).clone();
    // 标量方法（INumber/ICompare 族：a.add(b) ≡ a + b）
    if matches!(self_v, IrValue::Int(_) | IrValue::Float(_)) {
        if let Some(v) = call_scalar_method_ir(ctx, &self_v, method, args)? {
            return Ok(Some(v));
        }
    }
    match (&self_v, method) {
        (IrValue::Str(s), "concat") => {
            let other = args
                .first()
                .ok_or_else(|| IrError::msg("ArityMismatch", "concat"))?;
            match deref_value(ctx, other) {
                IrValue::Str(os) => {
                    let mut bytes = s.clone();
                    bytes.extend_from_slice(os);
                    Ok(Some(str_bytes_val(bytes)))
                }
                _ => Err(IrError::msg("TypeError", "concat expects &[u8]")),
            }
        }
        (IrValue::Str(s), "as_slice") => Ok(Some(IrValue::Str(s.clone()))),
        (IrValue::String(s), "as_slice") => Ok(Some(IrValue::Str(s.as_slice().to_vec()))),
        (IrValue::String(s), "into_array") => Ok(Some(IrValue::Str(s.as_slice().to_vec()))),
        (IrValue::Str(s), "split") => {
            let sep_v = deref_value(
                ctx,
                args.get(0)
                    .ok_or_else(|| IrError::msg("ArityMismatch", "split"))?,
            )
            .clone();
            let sep = match sep_v {
                IrValue::Int(i) => vec![i as u8],
                IrValue::Str(ss) => ss,
                _ => return Err(IrError::msg("TypeError", "split expects byte or bytes")),
            };
            let data = s.clone();
            let mut out = Vec::new();
            if sep.is_empty() {
                return Ok(Some(make_arr(ctx, vec![str_bytes_val(data)])));
            }
            let mut start = 0usize;
            let mut i = 0usize;
            while i + sep.len() <= data.len() {
                if &data[i..i + sep.len()] == sep.as_slice() {
                    out.push(str_bytes_val(data[start..i].to_vec()));
                    i += sep.len();
                    start = i;
                } else {
                    i += 1;
                }
            }
            out.push(str_bytes_val(data[start..].to_vec()));
            Ok(Some(make_arr(ctx, out)))
        }
        (IrValue::Str(s), "to_bytes") => {
            let mut out = (s.len() as u64).to_le_bytes().to_vec();
            out.extend_from_slice(s);
            Ok(Some(str_bytes_val(out)))
        }
        (IrValue::Str(s), "find") => {
            let needle_v = deref_value(
                ctx,
                args.get(0)
                    .ok_or_else(|| IrError::msg("ArityMismatch", "find"))?,
            )
            .clone();
            let needle_bytes: Vec<u8> = match needle_v {
                IrValue::Str(n) => n,
                IrValue::Int(i) => vec![i as u8],
                _ => return Err(IrError::msg("TypeError", "find expects byte or bytes")),
            };
            let data = s.clone();
            let pos = if needle_bytes.is_empty() {
                Some(0usize)
            } else {
                data.windows(needle_bytes.len())
                    .position(|w| w == needle_bytes.as_slice())
            };
            Ok(Some(match pos {
                Some(p) => IrValue::Opt(Some(Box::new(IrValue::Int(p as i128)))),
                None => IrValue::Opt(None),
            }))
        }
        (IrValue::Str(s), "substring") => {
            let lo = int_arg_ir(ctx, args, 0)?;
            let hi = int_arg_ir(ctx, args, 1)?;
            let (lo, hi) = (lo.max(0) as usize, hi.max(0) as usize);
            let hi = hi.min(s.len());
            let sub = s[lo.min(hi)..hi].to_vec();
            Ok(Some(str_bytes_val(sub)))
        }
        (IrValue::Str(s), "replace") => {
            let from_b = str_arg_ir(ctx, args, 0)?;
            let to_b = str_arg_ir(ctx, args, 1)?;
            let data = s.clone();
            let mut out = Vec::new();
            let mut i = 0usize;
            while i < data.len() {
                if from_b.is_empty() {
                    out.push(data[i]);
                    i += 1;
                } else if i + from_b.len() <= data.len()
                    && &data[i..i + from_b.len()] == from_b.as_slice()
                {
                    out.extend_from_slice(&to_b);
                    i += from_b.len();
                } else {
                    out.push(data[i]);
                    i += 1;
                }
            }
            Ok(Some(str_bytes_val(out)))
        }
        (IrValue::Str(_), "len") => Ok(Some(IrValue::Int(self_v.display(ctx).len() as i128))),
        (IrValue::String(s), "len") => Ok(Some(IrValue::Int(s.len() as i128))),
        // G2（io 差异项）：to_upper/to_lower——ASCII 大小写转换（非 ASCII 字节不变）
        (IrValue::Str(s), "to_upper") | (IrValue::Str(s), "to_lower") => {
            let upper = method == "to_upper";
            let out: Vec<u8> = s
                .iter()
                .map(|&b| {
                    if upper {
                        b.to_ascii_uppercase()
                    } else {
                        b.to_ascii_lowercase()
                    }
                })
                .collect();
            Ok(Some(str_bytes_val(out)))
        }
        (IrValue::Arr(c), "len") => Ok(Some(IrValue::Int(ctx.elems_len(*c) as i128))),
        (IrValue::Arr(c), "append") => {
            let v = args
                .first()
                .ok_or_else(|| IrError::msg("ArityMismatch", "append"))?
                .clone();
            let nc = ctx.alloc(Cell::Value(v));
            match &mut ctx.cells[*c] {
                Cell::Elems(e) => e.push(nc),
                _ => return Err(IrError::msg("TypeError", "append expects array")),
            }
            Ok(Some(IrValue::Void))
        }
        (IrValue::Arr(c), "push_back") => {
            let v = args
                .first()
                .ok_or_else(|| IrError::msg("ArityMismatch", "push_back"))?
                .clone();
            let nc = ctx.alloc(Cell::Value(v));
            match &mut ctx.cells[*c] {
                Cell::Elems(e) => e.push(nc),
                _ => return Err(IrError::msg("TypeError", "push_back expects array")),
            }
            Ok(Some(IrValue::Void))
        }
        (IrValue::Arr(c), "push_front") => {
            let v = args
                .first()
                .ok_or_else(|| IrError::msg("ArityMismatch", "push_front"))?
                .clone();
            let nc = ctx.alloc(Cell::Value(v));
            match &mut ctx.cells[*c] {
                Cell::Elems(e) => e.insert(0, nc),
                _ => return Err(IrError::msg("TypeError", "push_front expects array")),
            }
            Ok(Some(IrValue::Void))
        }
        (IrValue::Arr(c), "pop_back") => {
            let popped = match &mut ctx.cells[*c] {
                Cell::Elems(e) => e.pop(),
                _ => None,
            };
            let v = popped.map(|ec| ctx.cell_value(ec).clone());
            Ok(Some(opt_val(v)))
        }
        (IrValue::Arr(c), "pop_front") => {
            let popped = match &mut ctx.cells[*c] {
                Cell::Elems(e) => {
                    if e.is_empty() {
                        None
                    } else {
                        Some(e.remove(0))
                    }
                }
                _ => None,
            };
            let v = popped.map(|ec| ctx.cell_value(ec).clone());
            Ok(Some(opt_val(v)))
        }
        (IrValue::Arr(c), "front") => {
            let v = match &ctx.cells[*c] {
                Cell::Elems(e) => e.first().map(|ec| ctx.cell_value(*ec).clone()),
                _ => None,
            };
            Ok(Some(opt_val(v)))
        }
        (IrValue::Arr(c), "back") => {
            let v = match &ctx.cells[*c] {
                Cell::Elems(e) => e.last().map(|ec| ctx.cell_value(*ec).clone()),
                _ => None,
            };
            Ok(Some(opt_val(v)))
        }
        (IrValue::Arr(c), "get") => {
            let i = as_index(
                ctx,
                args.get(0)
                    .ok_or_else(|| IrError::msg("ArityMismatch", "get"))?,
            )?;
            let v = match &ctx.cells[*c] {
                Cell::Elems(e) => e.get(i).map(|ec| ctx.cell_value(*ec).clone()),
                _ => None,
            };
            Ok(Some(opt_val(v)))
        }
        (IrValue::Arr(c), "put") => {
            let i = as_index(
                ctx,
                args.get(0)
                    .ok_or_else(|| IrError::msg("ArityMismatch", "put"))?,
            )?;
            let v = args
                .get(1)
                .ok_or_else(|| IrError::msg("ArityMismatch", "put"))?
                .clone();
            let nc = ctx.alloc(Cell::Value(v));
            match &mut ctx.cells[*c] {
                Cell::Elems(e) => {
                    if i >= e.len() {
                        return Err(IrError::msg("IndexOutOfBounds", "index out of bounds"));
                    }
                    e[i] = nc;
                    Ok(Some(IrValue::Void))
                }
                _ => Err(IrError::msg("TypeError", "put expects array")),
            }
        }
        (IrValue::Arr(c), "remove") => {
            let i = as_index(
                ctx,
                args.get(0)
                    .ok_or_else(|| IrError::msg("ArityMismatch", "remove"))?,
            )?;
            let removed = match &mut ctx.cells[*c] {
                Cell::Elems(e) => {
                    if i >= e.len() {
                        return Err(IrError::msg("IndexOutOfBounds", "index out of bounds"));
                    }
                    Some(e.remove(i))
                }
                _ => None,
            };
            match removed {
                Some(ec) => Ok(Some(ctx.cell_value(ec).clone())),
                None => Err(IrError::msg("TypeError", "remove expects array")),
            }
        }
        (IrValue::Arr(c), "extend") => {
            let v = deref_value(
                ctx,
                args.first()
                    .ok_or_else(|| IrError::msg("ArityMismatch", "extend"))?,
            )
            .clone();
            match v {
                IrValue::Arr(src) => {
                    let src_elems = match &ctx.cells[src] {
                        Cell::Elems(e) => e.clone(),
                        _ => return Err(IrError::msg("TypeError", "extend expects array")),
                    };
                    match &mut ctx.cells[*c] {
                        Cell::Elems(e) => {
                            e.extend_from_slice(&src_elems);
                            Ok(Some(IrValue::Void))
                        }
                        _ => Err(IrError::msg("TypeError", "extend expects array")),
                    }
                }
                IrValue::Str(b) => {
                    let mut new_cells = Vec::new();
                    for byte in b {
                        new_cells.push(ctx.alloc(Cell::Value(IrValue::Int(byte as i128))));
                    }
                    match &mut ctx.cells[*c] {
                        Cell::Elems(e) => {
                            e.extend_from_slice(&new_cells);
                            Ok(Some(IrValue::Void))
                        }
                        _ => Err(IrError::msg("TypeError", "extend expects array")),
                    }
                }
                _ => Err(IrError::msg("TypeError", "extend expects array or bytes")),
            }
        }
        (IrValue::Arr(c), "append_u64") => {
            let n = match deref_value(
                ctx,
                args.first()
                    .ok_or_else(|| IrError::msg("ArityMismatch", "append_u64"))?,
            ) {
                IrValue::Int(i) => *i as u64,
                _ => return Err(IrError::msg("TypeError", "append_u64 expects int")),
            };
            let mut new_cells = Vec::new();
            for byte in n.to_le_bytes() {
                new_cells.push(ctx.alloc(Cell::Value(IrValue::Int(byte as i128))));
            }
            match &mut ctx.cells[*c] {
                Cell::Elems(e) => {
                    e.extend_from_slice(&new_cells);
                    Ok(Some(IrValue::Void))
                }
                _ => Err(IrError::msg("TypeError", "append_u64 expects array")),
            }
        }
        // G4：`Vec(T).init(alloc)`——集合空容器，捕获分配器引用（缺省回退全局 alloc）
        (IrValue::Arr(_), "init") => {
            let alloc_v = args
                .first()
                .cloned()
                .unwrap_or_else(|| implicit_env_value(ctx, "alloc"));
            let items = if args.len() >= 2 {
                match &args[1] {
                    IrValue::Arr(cell) => match &ctx.cells[*cell] {
                        Cell::Elems(elems) => elems
                            .iter()
                            .map(|e| match &ctx.cells[*e] {
                                Cell::Value(v) => v.clone(),
                                _ => IrValue::Int(0),
                            })
                            .collect(),
                        _ => Vec::new(),
                    },
                    _ => Vec::new(),
                }
            } else {
                Vec::new()
            };
            Ok(Some(make_vec_with(ctx, items, alloc_v)))
        }
        (IrValue::Arr(_), "from_bytes") => {
            let b = str_arg_ir(ctx, args, 0)?;
            if b.len() < 8 {
                return Err(IrError::msg("InvalidBytes", "truncated byte data"));
            }
            let n = u64::from_le_bytes(b[0..8].try_into().unwrap()) as usize;
            let mut items = Vec::new();
            let mut pos = 8usize;
            for _ in 0..n {
                let v = if b.len() >= pos + 4 {
                    let i = i32::from_le_bytes(b[pos..pos + 4].try_into().unwrap());
                    pos += 4;
                    IrValue::Int(i as i128)
                } else {
                    break;
                };
                items.push(v);
            }
            let alloc = implicit_env_value(ctx, "alloc");
            Ok(Some(make_vec_with(ctx, items, alloc)))
        }
        (IrValue::Arr(c), "to_bytes") => {
            // 集合 → 字节（u64 LE 元素数前缀 + 逐元素 value_to_bytes，对齐 oracle
            // interp.rs:3959-3969）。IR 的 value_to_bytes_ir 覆盖标量/字符串子集，
            // 聚合元素序列化为空（Phase 7 取舍）。
            let elems = match &ctx.cells[*c] {
                Cell::Elems(e) => e.clone(),
                _ => return Err(IrError::msg("TypeError", "to_bytes expects array")),
            };
            let mut out = (elems.len() as u64).to_le_bytes().to_vec();
            for ec in elems {
                let v = ctx.cell_value(ec).clone();
                out.extend(value_to_bytes_ir(ctx, &v));
            }
            Ok(Some(str_bytes_val(out)))
        }
        (IrValue::Slice { len, .. }, "len") => Ok(Some(IrValue::Int(*len as i128))),
        // G4：`Map(K,V).init(alloc)`——集合空容器，捕获分配器引用（缺省回退全局 alloc）
        (IrValue::Map(_), "init") => {
            let alloc_v = args
                .first()
                .cloned()
                .unwrap_or_else(|| implicit_env_value(ctx, "alloc"));
            Ok(Some(make_map_with(ctx, HashMap::new(), alloc_v)))
        }
        // 集合（G4）：Map 句柄方法与 Class("Map") 共用实现
        (IrValue::Map(_), m) => call_map_method_ir(ctx, &self_v, m, args),
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Map" => {
            call_map_method_ir(ctx, &self_v, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Alloc" => {
            call_alloc_method_ir(ctx, module, m, args)
        }
        (IrValue::Arena(c), m) => call_arena_method_ir(ctx, module, *c, m, args),
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Io" => {
            call_io_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Thread" => {
            call_thread_method_ir(ctx, module, &self_v, m, args)
        }
        // 组 F：四模式容器——`init(alloc[, cap])` 构造真实容器；其余方法 FIFO 分派。
        // 写者/读者数量为类型层契约，协作式单线程下四变体运行时行为相同
        // （对齐 oracle interp.rs call_builtin_method 的 `is_four_mode_type` 分派）。
        (IrValue::Class(c), "init") if is_four_mode_type_ir(&class_name(ctx, *c)) => {
            let name = class_name(ctx, *c);
            Ok(Some(make_four_mode_ir(ctx, &name, args)?))
        }
        (IrValue::Class(c), m) if is_four_mode_type_ir(&class_name(ctx, *c)) => {
            call_four_mode_method_ir(ctx, module, &self_v, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Fs" => {
            call_fs_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Time" => {
            call_time_method_ir(ctx, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Net" => {
            call_net_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "TcpConn" => {
            call_conn_method_ir(ctx, module, &self_v, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "TcpListener" => {
            call_listener_method_ir(ctx, module, &self_v, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "File" => {
            call_file_method_ir(ctx, module, &self_v, m, args)
        }
        // ---- G1-G5 模块分派（Q20 双语：interp/IR 同一套类名）----
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Udp" => {
            call_udp_ns_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "UdpSocket" => {
            call_udp_socket_method_ir(ctx, module, &self_v, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Dir" => {
            call_dir_method_ir(ctx, module, &self_v, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Ipc" => {
            call_ipc_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "PipeReader" => {
            call_pipe_method_ir(ctx, module, true, &self_v, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "PipeWriter" => {
            call_pipe_method_ir(ctx, module, false, &self_v, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Shm" => {
            call_shm_method_ir(ctx, module, &self_v, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Storage" => {
            call_storage_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "KvStore" => {
            call_store_method_ir(ctx, module, &self_v, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Archive" => {
            call_archive_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Text" => {
            call_text_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "RngNs" => {
            call_rng_method_ir(ctx, m, args)
        }
        // A6：标准库数据结构——Bitmap 位图
        (IrValue::Class(c), m) if class_name(ctx, *c) == "BitmapNs" => {
            call_bitmap_ns_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "Bitmap" => {
            call_bitmap_method_ir(ctx, module, m, &IrValue::Class(*c), args)
        }
        // A6：标准库数据结构——RingBuf 环形缓冲
        (IrValue::Class(c), m) if class_name(ctx, *c) == "RingBufNs" => {
            call_ringbuf_ns_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "RingBuf" => {
            call_ringbuf_method_ir(ctx, module, m, &IrValue::Class(*c), args)
        }
        // A6：标准库数据结构——PageMem 页内存池
        (IrValue::Class(c), m) if class_name(ctx, *c) == "PageMemNs" => {
            call_pagemem_ns_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "PageMem" => {
            call_pagemem_method_ir(ctx, module, m, &IrValue::Class(*c), args)
        }
        // A6：标准库数据结构——IntrList 侵入式链表
        (IrValue::Class(c), m) if class_name(ctx, *c) == "IntrListNs" => {
            call_intrlist_ns_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "IntrList" => {
            call_intrlist_method_ir(ctx, module, m, &IrValue::Class(*c), args)
        }
        // A6：标准库数据结构——TreeMap 有序映射
        (IrValue::Class(c), m) if class_name(ctx, *c) == "TreeMapNs" => {
            call_treemap_ns_method_ir(ctx, module, m, args)
        }
        (IrValue::Class(c), m) if class_name(ctx, *c) == "TreeMap" => {
            call_treemap_method_ir(ctx, module, m, &IrValue::Class(*c), args)
        }
        // Class to_bytes：无布局表（Phase 7 取舍——堆类型请用 to_json）
        (IrValue::Class(_), "to_bytes") => Err(IrError::msg(
            "Unsupported",
            "class to_bytes requires type layout (not in IR runtime)",
        )),
        (IrValue::Class(_), "to_json") => Ok(Some(str_val(&value_to_json_ir(ctx, &self_v)))),
        (_, "iter") => Ok(Some(iter_to_arr_ir(ctx, module, &self_v, 0)?)),
        (_, "filter") => {
            let f = deref_value(
                ctx,
                args.first()
                    .ok_or_else(|| IrError::msg("ArityMismatch", "filter"))?,
            )
            .clone();
            let src = iter_to_arr_ir(ctx, module, &self_v, 0)?;
            let mut out = Vec::new();
            for item in arr_items(ctx, &src)? {
                if call_closure_bool_ir(ctx, module, &f, &[item.clone()])? {
                    out.push(item);
                }
            }
            Ok(Some(make_arr(ctx, out)))
        }
        (_, "map") => {
            let f = deref_value(
                ctx,
                args.first()
                    .ok_or_else(|| IrError::msg("ArityMismatch", "map"))?,
            )
            .clone();
            let src = iter_to_arr_ir(ctx, module, &self_v, 0)?;
            let mut out = Vec::new();
            for item in arr_items(ctx, &src)? {
                let mapped = call_closure_value_ir(ctx, module, &f, &[item])?;
                out.push(mapped);
            }
            Ok(Some(make_arr(ctx, out)))
        }
        // E4：Mutex 方法 lock/try_lock
        (IrValue::Mutex(m), "lock") => match m.lock() {
            Ok(v) => Ok(Some(v.clone())),
            Err(_) => Err(IrError::msg("MutexPoisoned", "Mutex is poisoned")),
        },
        (IrValue::Mutex(m), "try_lock") => match m.try_lock() {
            Ok(v) => Ok(Some(IrValue::Opt(Some(Box::new(v.clone()))))),
            Err(_) => Ok(Some(IrValue::Opt(None))),
        },
        // E4：chan<T> 方法
        (IrValue::Chan(ch), "send") => {
            if args.len() != 1 {
                return Err(IrError::msg("ArityMismatch", "send"));
            }
            let mut inner = ch.inner.lock().unwrap();
            if inner.closed {
                return Ok(Some(err_val(module, "Closed")));
            }
            while inner.queue.len() >= ch.capacity && !inner.closed {
                inner = ch.send_cond.wait(inner).unwrap();
            }
            if inner.closed {
                return Ok(Some(err_val(module, "Closed")));
            }
            inner.queue.push_back(args[0].clone());
            ch.recv_cond.notify_one();
            Ok(Some(IrValue::Void))
        }
        (IrValue::Chan(ch), "recv") => {
            if !args.is_empty() {
                return Err(IrError::msg("ArityMismatch", "recv"));
            }
            let mut inner = ch.inner.lock().unwrap();
            while inner.queue.is_empty() && !inner.closed {
                inner = ch.recv_cond.wait(inner).unwrap();
            }
            if inner.queue.is_empty() && inner.closed {
                return Ok(Some(err_val(module, "Closed")));
            }
            let v = inner.queue.pop_front().unwrap();
            ch.send_cond.notify_one();
            Ok(Some(v))
        }
        (IrValue::Chan(ch), "try_send") => {
            if args.len() != 1 {
                return Err(IrError::msg("ArityMismatch", "try_send"));
            }
            let mut inner = ch.inner.lock().unwrap();
            if inner.closed || inner.queue.len() >= ch.capacity {
                return Ok(Some(IrValue::Bool(false)));
            }
            inner.queue.push_back(args[0].clone());
            ch.recv_cond.notify_one();
            Ok(Some(IrValue::Bool(true)))
        }
        (IrValue::Chan(ch), "try_recv") => {
            if !args.is_empty() {
                return Err(IrError::msg("ArityMismatch", "try_recv"));
            }
            let mut inner = ch.inner.lock().unwrap();
            if let Some(v) = inner.queue.pop_front() {
                ch.send_cond.notify_one();
                Ok(Some(opt_val(Some(v))))
            } else {
                Ok(Some(IrValue::Opt(None)))
            }
        }
        (IrValue::Chan(ch), "close") => {
            if !args.is_empty() {
                return Err(IrError::msg("ArityMismatch", "close"));
            }
            let mut inner = ch.inner.lock().unwrap();
            inner.closed = true;
            ch.send_cond.notify_all();
            ch.recv_cond.notify_all();
            Ok(Some(IrValue::Void))
        }
        _ => Ok(None),
    }
}

/// 隐式环境限定名调用（io.print / io.fs.open / alloc.init…）：
/// 根值 → 中段字段访问 → 末段方法分派（对齐 oracle eval_call 的隐式环境 + 方法分派）。
/// `json.parse`/`csv.parse`/`String.from` 为虚拟根静态内建（非值对象）。
pub(crate) fn call_dotted_implicit(
    ctx: &mut Ctx,
    module: &IrModule,
    name: &str,
    args: &[IrValue],
) -> R<IrValue> {
    // serialize 命名空间（M5.3）：解析辅助组——serialize.parse_int 等对齐自由内建，
    // serialize.json.parse/csv.parse 对齐虚拟根（与 interp call_serialize_builtin 对齐）
    if let Some(rest) = name.strip_prefix("serialize.") {
        return call_serialize_builtin_ir(ctx, module, rest, args);
    }
    match name {
        // Arena.init(alloc) 内建：真实 arena 句柄（对齐 oracle interp.rs:2559-2562
        // 特判——返回新建 arena，而非 Void）
        "Arena.init" => {
            return Ok(IrValue::Arena(ctx.alloc(Cell::Arena(ArenaStateIr::new()))));
        }
        // E4：Mutex.init(v) 内建：互斥锁（Arc<std::sync::Mutex>）
        "Mutex.init" => {
            if args.is_empty() {
                return Err(IrError::msg("ArityMismatch", "Mutex.init expects 1 arg"));
            }
            let v = deref_value(ctx, &args[0]).clone();
            return Ok(IrValue::Mutex(Arc::new(std::sync::Mutex::new(v))));
        }
        // E4：chan.init(alloc[, cap]) 内建：通道构造
        "chan.init" => {
            if args.is_empty() || args.len() > 2 {
                return Err(IrError::msg("ArityMismatch", "chan.init expects 1-2 args"));
            }
            let capacity = if args.len() == 2 {
                match deref_value(ctx, &args[1]) {
                    IrValue::Int(i) => (*i).max(0) as usize,
                    _ => return Err(IrError::msg("TypeError", "chan.init cap must be int")),
                }
            } else {
                0
            };
            return Ok(IrValue::Chan(Arc::new(ChanStateIr {
                inner: Mutex::new(ChanInnerIr {
                    queue: VecDeque::new(),
                    closed: false,
                }),
                send_cond: Condvar::new(),
                recv_cond: Condvar::new(),
                capacity,
            })));
        }
        // Table(T).init(rows, cols, init, alloc)（ADR-0027：分配器永远是最后一个参数）
        "Table.init" => {
            if args.len() < 4 {
                return Err(IrError::msg("ArityMismatch", "Table.init expects 4 args"));
            }
            let rows = match deref_value(ctx, &args[0]) {
                IrValue::Int(i) => (*i).max(0) as usize,
                _ => return Err(IrError::msg("TypeError", "Table.init rows must be int")),
            };
            let cols = match deref_value(ctx, &args[1]) {
                IrValue::Int(i) => (*i).max(0) as usize,
                _ => return Err(IrError::msg("TypeError", "Table.init cols must be int")),
            };
            let init_v = args[2].clone();
            let alloc_v = args[3].clone();
            let mut grid = Vec::new();
            for _ in 0..rows {
                let mut row = Vec::new();
                for _ in 0..cols {
                    row.push(init_v.clone());
                }
                grid.push(make_arr(ctx, row));
            }
            return Ok(make_vec_with(ctx, grid, alloc_v));
        }
        // math.nan/inf/inf_neg/sqrt/abs/pow/floor/ceil/round（对齐 oracle call_math
        // interp.rs:4922-4960：nan/inf/inf_neg 忽略类型名参数；数值函数取 arg[0]，
        // Int 强制 f64 后计算，返回 Float）
        "math.nan" => return Ok(IrValue::Float(f64::NAN)),
        "math.inf" => return Ok(IrValue::Float(f64::INFINITY)),
        "math.inf_neg" => return Ok(IrValue::Float(f64::NEG_INFINITY)),
        "math.sqrt" | "math.abs" | "math.pow" | "math.floor" | "math.ceil" | "math.round" => {
            let field = name.strip_prefix("math.").unwrap_or(name);
            let v = deref_value(
                ctx,
                args.first()
                    .ok_or_else(|| IrError::msg("ArityMismatch", format!("math.{field}")))?,
            );
            let f = match v {
                IrValue::Int(i) => *i as f64,
                IrValue::Float(f) => *f,
                _ => {
                    return Err(IrError::msg(
                        "TypeError",
                        format!("math.{field} expects a number"),
                    ))
                }
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
            return Ok(IrValue::Float(r));
        }
        "json.parse" => {
            let data = str_arg_ir(ctx, args, 0)?;
            let obj = parse_json_obj_ir(ctx, &String::from_utf8_lossy(&data))?;
            let mut fields = HashMap::new();
            for (k, v) in obj {
                fields.insert(k, ctx.alloc(Cell::Value(v)));
            }
            return Ok(IrValue::Class(ctx.alloc(Cell::Class {
                name: "Map".into(),
                fields,
            })));
        }
        "csv.parse" => {
            let data = str_arg_ir(ctx, args, 0)?;
            let text = String::from_utf8_lossy(&data).to_string();
            let rows: Vec<IrValue> = text
                .split('\n')
                .map(|line| line.strip_suffix('\r').unwrap_or(line))
                .filter(|line| !line.is_empty())
                .map(|line| line.split(',').map(str_val).collect::<Vec<_>>())
                .map(|cols| make_arr(ctx, cols))
                .collect();
            return Ok(make_arr(ctx, rows));
        }
        "String.from" => {
            let v = args
                .first()
                .ok_or_else(|| IrError::msg("ArityMismatch", "String.from"))?;
            let v = deref_value(ctx, v);
            let bytes = match v {
                IrValue::Str(s) => s.clone(),
                IrValue::String(s) => s.as_slice().to_vec(),
                other => other.display(ctx).as_bytes().to_vec(),
            };
            return Ok(IrValue::String(StringDataIr::from_slice(&bytes)));
        }
        "String.from_slice" => {
            let v = args
                .first()
                .ok_or_else(|| IrError::msg("ArityMismatch", "String.from_slice"))?;
            let v = deref_value(ctx, v);
            let bytes = match v {
                IrValue::Str(s) => s.clone(),
                IrValue::String(s) => s.as_slice().to_vec(),
                _ => {
                    return Err(IrError::msg(
                        "TypeError",
                        "String.from_slice expects &[u8] or String",
                    ))
                }
            };
            return Ok(IrValue::String(StringDataIr::from_slice(&bytes)));
        }
        "String.copy_from" => {
            let v = args
                .first()
                .ok_or_else(|| IrError::msg("ArityMismatch", "String.copy_from"))?;
            let v = deref_value(ctx, v);
            return match v {
                IrValue::String(s) => Ok(IrValue::String(s.clone())),
                _ => Err(IrError::msg("TypeError", "String.copy_from expects String")),
            };
        }
        "String.concat" => {
            if args.len() < 2 {
                return Err(IrError::msg(
                    "ArityMismatch",
                    "String.concat expects 2 args",
                ));
            }
            let a = deref_value(ctx, &args[0]);
            let b = deref_value(ctx, &args[1]);
            let bytes_a = match &a {
                IrValue::Str(s) => s.clone(),
                IrValue::String(s) => s.as_slice().to_vec(),
                _ => {
                    return Err(IrError::msg(
                        "TypeError",
                        "String.concat expects &[u8] or String",
                    ))
                }
            };
            let bytes_b = match &b {
                IrValue::Str(s) => s.clone(),
                IrValue::String(s) => s.as_slice().to_vec(),
                _ => {
                    return Err(IrError::msg(
                        "TypeError",
                        "String.concat expects &[u8] or String",
                    ))
                }
            };
            let mut out = bytes_a;
            out.extend_from_slice(&bytes_b);
            return Ok(IrValue::String(StringDataIr::from_slice(&out)));
        }
        "String.compare" => {
            if args.len() < 2 {
                return Err(IrError::msg(
                    "ArityMismatch",
                    "String.compare expects 2 args",
                ));
            }
            let a = deref_value(ctx, &args[0]);
            let b = deref_value(ctx, &args[1]);
            let bytes_a = match &a {
                IrValue::Str(s) => s.clone(),
                IrValue::String(s) => s.as_slice().to_vec(),
                _ => {
                    return Err(IrError::msg(
                        "TypeError",
                        "String.compare expects &[u8] or String",
                    ))
                }
            };
            let bytes_b = match &b {
                IrValue::Str(s) => s.clone(),
                IrValue::String(s) => s.as_slice().to_vec(),
                _ => {
                    return Err(IrError::msg(
                        "TypeError",
                        "String.compare expects &[u8] or String",
                    ))
                }
            };
            let ord = bytes_a.cmp(&bytes_b);
            let v = match ord {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            };
            return Ok(IrValue::Int(v));
        }
        "String.join" => {
            if args.len() < 2 {
                return Err(IrError::msg("ArityMismatch", "String.join expects 2 args"));
            }
            let parts_v = deref_value(ctx, &args[0]);
            let sep_v = deref_value(ctx, &args[1]);
            let sep_bytes = match &sep_v {
                IrValue::Str(s) => s.clone(),
                _ => {
                    return Err(IrError::msg(
                        "TypeError",
                        "String.join separator expects &[u8]",
                    ))
                }
            };
            let items: Vec<Vec<u8>> = match &parts_v {
                IrValue::Arr(c) => match &ctx.cells[*c] {
                    Cell::Elems(e) => e
                        .iter()
                        .map(|ec| {
                            let v = ctx.cell_value(*ec);
                            match v {
                                IrValue::Str(s) => s.clone(),
                                IrValue::String(s) => s.as_slice().to_vec(),
                                other => other.display(ctx).into_bytes(),
                            }
                        })
                        .collect(),
                    _ => return Err(IrError::msg("TypeError", "String.join expects array")),
                },
                _ => return Err(IrError::msg("TypeError", "String.join expects array")),
            };
            let mut out = Vec::new();
            for (i, it) in items.iter().enumerate() {
                if i > 0 {
                    out.extend_from_slice(&sep_bytes);
                }
                out.extend_from_slice(it);
            }
            return Ok(IrValue::String(StringDataIr::from_slice(&out)));
        }
        "String.as_slice" => {
            let v = args
                .first()
                .ok_or_else(|| IrError::msg("ArityMismatch", "String.as_slice"))?;
            let v = deref_value(ctx, v);
            return match v {
                IrValue::String(s) => Ok(IrValue::Str(s.as_slice().to_vec())),
                _ => Err(IrError::msg("TypeError", "String.as_slice expects String")),
            };
        }
        "String.into_array" => {
            let v = args
                .first()
                .ok_or_else(|| IrError::msg("ArityMismatch", "String.into_array"))?;
            let v = deref_value(ctx, v).clone();
            return match v {
                IrValue::String(s) => Ok(IrValue::Str(s.as_slice().to_vec())),
                _ => Err(IrError::msg(
                    "TypeError",
                    "String.into_array expects String",
                )),
            };
        }
        "String.len" => {
            let v = args
                .first()
                .ok_or_else(|| IrError::msg("ArityMismatch", "String.len"))?;
            let v = deref_value(ctx, v);
            return match v {
                IrValue::String(s) => Ok(IrValue::Int(s.len() as i128)),
                _ => Err(IrError::msg("TypeError", "String.len expects String")),
            };
        }
        _ => {}
    }
    let parts: Vec<&str> = name.split('.').collect();
    let root = parts[0];
    let mut self_v = implicit_env_value(ctx, root);
    for mid in &parts[1..parts.len() - 1] {
        self_v = field_value(ctx, &self_v, mid)?;
    }
    let method = parts[parts.len() - 1];
    let v = call_builtin_method(ctx, module, &self_v, method, args)?.ok_or_else(|| {
        IrError::msg(
            "NoMethod",
            format!("no method `{method}` on {}", ir_type_name(ctx, &self_v)),
        )
    })?;
    Ok(v)
}

/// serialize 命名空间（M5.3）：解析辅助组组织为库命名空间。
/// `rest` 为去掉 `serialize.` 前缀的辅助名；json/csv 虚拟根对齐 call_dotted_implicit 对应
/// 分支，其余对齐自由内建 call_builtin（parse_int/parse_float/parse_number/skip_space/
/// peek/advance/is_digit/expect）。
pub(crate) fn call_serialize_builtin_ir(
    ctx: &mut Ctx,
    module: &IrModule,
    rest: &str,
    args: &[IrValue],
) -> R<IrValue> {
    match rest {
        "json.parse" => {
            let data = str_arg_ir(ctx, args, 0)?;
            let obj = parse_json_obj_ir(ctx, &String::from_utf8_lossy(&data))?;
            let mut fields = HashMap::new();
            for (k, v) in obj {
                fields.insert(k, ctx.alloc(Cell::Value(v)));
            }
            Ok(IrValue::Class(ctx.alloc(Cell::Class {
                name: "Map".into(),
                fields,
            })))
        }
        "csv.parse" => {
            let data = str_arg_ir(ctx, args, 0)?;
            let text = String::from_utf8_lossy(&data).to_string();
            let rows: Vec<IrValue> = text
                .split('\n')
                .map(|line| line.strip_suffix('\r').unwrap_or(line))
                .filter(|line| !line.is_empty())
                .map(|line| line.split(',').map(str_val).collect::<Vec<_>>())
                .map(|cols| make_arr(ctx, cols))
                .collect();
            Ok(make_arr(ctx, rows))
        }
        _ => call_builtin(ctx, module, rest, args, &mut None),
    }
}

pub(crate) fn find_label(func: &IrFunc, id: usize) -> R<usize> {
    func.body
        .iter()
        .position(|i| matches!(i, IrInst::Label { id: l } if *l == id))
        .ok_or_else(|| {
            IrError::msg(
                "BadLabel",
                format!("label {id} not found in `{}`", func.name),
            )
        })
}

/// 断言内建（IR 参考语义：失败记 fail，返回时抛 AssertFailed）
/// 全量内建（对齐 oracle `call_builtin` interp.rs:2911-3404 全面：box/copy/@ 内建/
/// sqrt/min/max/read_u64_le/sort/binary_search/解析器/parse_int/parse_float/断言五件套）。
/// 断言失败经 `fail` 通道延迟到 `Return`（对齐 IR `AssertFailed` 通道）。
pub(crate) fn call_builtin(
    ctx: &mut Ctx,
    module: &IrModule,
    name: &str,
    args: &[IrValue],
    fail: &mut Option<String>,
) -> R<IrValue> {
    match name {
        "box" => {
            if args.is_empty() || args.len() > 2 {
                return Err(IrError::msg("ArityMismatch", "box expects 1-2 args"));
            }
            // G3：分配器引用——显式传入或回退全局 alloc（`box(v)` 单参形态）
            let alloc_v = if args.len() > 1 {
                args[1].clone()
            } else {
                implicit_env_value(ctx, "alloc")
            };
            let data = ctx.alloc(Cell::Value(args[0].clone()));
            let vtbl = ir_type_name(ctx, &args[0]);
            Ok(IrValue::Boxed(ctx.alloc(Cell::Boxed {
                data,
                vtbl,
                alloc: alloc_v,
            })))
        }
        "unbox" => {
            if args.len() != 1 {
                return Err(IrError::msg("ArityMismatch", "unbox expects 1 arg"));
            }
            // 从 Boxed 胖指针中取出值，消费 Box（所有权转移）。
            // 注意：不用 deref_value 因为后者会解引用 Boxed 到内部值。
            let v = &args[0];
            // 可能是 Ptr(Boxed) 或直接 Boxed
            let boxed_cell = match v {
                IrValue::Boxed(cell) => Some(*cell),
                IrValue::Ptr(cell) => match &ctx.cells[*cell] {
                    Cell::Value(IrValue::Boxed(bc)) => Some(*bc),
                    _ => None,
                },
                _ => None,
            };
            match boxed_cell {
                Some(cell) => {
                    let data = match &ctx.cells[cell] {
                        Cell::Boxed { data, .. } => *data,
                        _ => return Err(IrError::msg("TypeError", "unbox: corrupted boxed value")),
                    };
                    let inner = ctx.cell_value(data).clone();
                    // 清空 data cell 防止双重释放
                    ctx.cells[data] = Cell::Value(IrValue::Void);
                    Ok(inner)
                }
                None => {
                    let desc = type_descr(v);
                    Err(IrError::msg(
                        "TypeError",
                        format!("unbox expects a boxed value, got {desc}"),
                    ))
                }
            }
        }
        "copy" => {
            if args.is_empty() {
                return Err(IrError::msg("ArityMismatch", "copy"));
            }
            // copy(&x, .shallow)（L1：CopyMode 内建枚举，.shallow 推断）
            let shallow = if args.len() > 1 {
                matches!(
                    deref_value(ctx, &args[1]),
                    IrValue::Enum { variant, .. } if variant == "shallow"
                )
            } else {
                false
            };
            let v = args[0].clone();
            Ok(if shallow { v } else { deep_copy(ctx, v) })
        }
        // ---------- 组 G 线程（E4：真 OS 并行） ----------
        // spawn(f, args...) owned Thread(T)：立即使用 std::thread::spawn 启动 OS 线程
        // 执行 f(args...)，返回 Thread 句柄。join 等待线程结束返回结果。
        "spawn" => {
            if args.is_empty() {
                return Err(IrError::msg(
                    "ArityMismatch",
                    "spawn expects at least callee",
                ));
            }
            let callee = deref_value(ctx, &args[0]).clone();
            match &callee {
                IrValue::Fn(_) => {}
                IrValue::Closure { .. } => {
                    return Err(IrError::msg(
                        "NotSupported",
                        "spawn with closure not yet supported in IR backend",
                    ));
                }
                _ => return Err(IrError::msg("NotCallable", "spawn callee is not callable")),
            }
            let mut arg_vals = Vec::new();
            for a in &args[1..] {
                arg_vals.push(deref_value(ctx, a).clone());
            }
            // Q8：每线程独立 Arena 实例（在线程内创建，避免跨 Ctx 传递 cell 索引）

            // E4：创建 OS 线程共享状态
            let result = Arc::new(Mutex::new(None::<ThreadResultIr>));
            let cancel = Arc::new(AtomicBool::new(false));
            let done = Arc::new(AtomicBool::new(false));

            let tid = ctx.next_tid;
            ctx.next_tid += 1;

            let module = ctx
                .module
                .clone()
                .unwrap_or_else(|| Arc::new(IrModule::default()));

            let result_tx = result.clone();
            let cancel_tx = cancel.clone();
            let done_tx = done.clone();

            let callee_clone = callee.clone();
            let arg_vals_clone = arg_vals.clone();

            // 确保调度器已启动
            ctx.scheduler.start();

            // 创建协程任务闭包
            let task: Box<dyn FnOnce() + Send> = Box::new(move || {
                // 检查取消标志
                if cancel_tx.load(Ordering::SeqCst) {
                    let err_v = err_val(&*module, "Cancelled");
                    let mut r = result_tx.lock().unwrap();
                    *r = Some(ThreadResultIr::Ok(err_v));
                    done_tx.store(true, Ordering::SeqCst);
                    return;
                }

                let mut rt = IrRuntime::new();
                let init_r = rt.init(&*module);
                let r = match init_r {
                    Ok(()) => {
                        // 绑定每线程 alloc（在线程的 Ctx 内创建 Arena，避免 cell 索引跨 Ctx 传递）
                        let alloc_v =
                            IrValue::Arena(rt.ctx.alloc(Cell::Arena(ArenaStateIr::new())));
                        rt.ctx.current_alloc = Some(alloc_v);
                        match callee_clone {
                            IrValue::Fn(ref fname) => {
                                match pick_func(&rt.ctx, &*module, fname, &arg_vals_clone) {
                                    Some(idx) => {
                                        exec_func(&mut rt.ctx, &*module, idx, &arg_vals_clone, 0)
                                    }
                                    None => Err(IrError::msg(
                                        "NoFunction",
                                        format!("no function `{fname}`"),
                                    )),
                                }
                            }
                            _ => Err(IrError::msg("NotCallable", "spawn callee is not callable")),
                        }
                    }
                    Err(e) => Err(e),
                };

                let mut rv = result_tx.lock().unwrap();
                *rv = Some(match r {
                    Ok(v) => ThreadResultIr::Ok(v),
                    Err(e) => ThreadResultIr::Err(e),
                });
                done_tx.store(true, Ordering::SeqCst);
            });

            // 提交协程到调度器
            let _gid = ctx.scheduler.submit("spawn".to_string(), task);

            ctx.thread_handles.insert(
                tid,
                ThreadStateIr {
                    join_handle: None,
                    result,
                    cancel,
                    done,
                },
            );

            // 返回 Thread 类值，包含 _tid 和用户可读字段
            let mut fields = HashMap::new();
            fields.insert(
                "_tid".to_string(),
                ctx.alloc(Cell::Value(IrValue::Int(tid as i128))),
            );
            fields.insert(
                "done".to_string(),
                ctx.alloc(Cell::Value(IrValue::Bool(false))),
            );
            fields.insert(
                "cancel".to_string(),
                ctx.alloc(Cell::Value(IrValue::Bool(false))),
            );
            fields.insert(
                "detached".to_string(),
                ctx.alloc(Cell::Value(IrValue::Bool(false))),
            );
            Ok(IrValue::Class(ctx.alloc(Cell::Class {
                name: "Thread".into(),
                fields,
            })))
        }
        // Phase 3 移除：with_arena 已弃用，使用 Arena.init(alloc) 替代
        // ---------- 组 F：四模式类型实例化（Pipe<i32> → 空容器标记） ----------
        // 类型参数（TypeExpr）已降级为 Const 值、忽略；.init 在 call_method_ir 构造真实容器。
        "Pipe" | "Tee" | "Funnel" | "Hub" => {
            if name == "Pipe" {
                let (tx, rx) = std::sync::mpsc::channel();
                let cid = ctx.next_channel_id;
                ctx.next_channel_id += 1;
                ctx.channels.insert(
                    cid,
                    ChannelStateIr::Pipe {
                        sender: tx,
                        receiver: rx,
                    },
                );
                let mut fields = HashMap::new();
                fields.insert(
                    "_chan_id".to_string(),
                    ctx.alloc(Cell::Value(IrValue::Int(cid as i128))),
                );
                fields.insert(
                    "closed".to_string(),
                    ctx.alloc(Cell::Value(IrValue::Bool(false))),
                );
                Ok(IrValue::Class(ctx.alloc(Cell::Class {
                    name: name.into(),
                    fields,
                })))
            } else {
                // E4：Tee/Funnel/Hub 使用 Mutex+Condvar 队列
                let queue = Arc::new(Mutex::new(VecDeque::new()));
                let condvar = Arc::new(Condvar::new());
                let cid = ctx.next_channel_id;
                ctx.next_channel_id += 1;
                let state = match name {
                    "Tee" => ChannelStateIr::Tee {
                        queue: queue.clone(),
                        condvar: condvar.clone(),
                    },
                    "Funnel" => ChannelStateIr::Funnel {
                        queue: queue.clone(),
                        condvar: condvar.clone(),
                    },
                    "Hub" => ChannelStateIr::Hub {
                        queue: queue.clone(),
                        condvar: condvar.clone(),
                    },
                    _ => unreachable!(),
                };
                ctx.channels.insert(cid, state);
                let mut fields = HashMap::new();
                fields.insert(
                    "_chan_id".to_string(),
                    ctx.alloc(Cell::Value(IrValue::Int(cid as i128))),
                );
                fields.insert(
                    "closed".to_string(),
                    ctx.alloc(Cell::Value(IrValue::Bool(false))),
                );
                Ok(IrValue::Class(ctx.alloc(Cell::Class {
                    name: name.into(),
                    fields,
                })))
            }
        }
        // ---------- @ 内建 ----------
        "@intFromEnum" => {
            let v = deref_value(ctx, &args[0]);
            match v {
                IrValue::Enum { name, variant, .. } => {
                    // 内建枚举（L3）：ExitType = [Exit, Error]
                    let idx = if name == "ExitType" {
                        match variant.as_str() {
                            "Exit" => 0,
                            "Error" => 1,
                            _ => 0,
                        }
                    } else {
                        match module.enum_variants.get(name) {
                            Some(variants) => {
                                variants.iter().position(|v| v == variant).unwrap_or(0) as i128
                            }
                            None => 0,
                        }
                    };
                    Ok(IrValue::Int(idx))
                }
                _ => Err(IrError::msg("TypeError", "@intFromEnum expects enum")),
            }
        }
        "@enumFromInt" => {
            let ty = match deref_value(ctx, &args[0]) {
                IrValue::Str(s) => String::from_utf8_lossy(s).to_string(),
                _ => return Err(IrError::msg("TypeError", "@enumFromInt expects type name")),
            };
            let i = match deref_value(ctx, &args[1]) {
                IrValue::Int(i) => *i,
                _ => return Err(IrError::msg("TypeError", "@enumFromInt expects int")),
            };
            match module.enum_variants.get(&ty) {
                Some(variants) => match variants.get(i as usize) {
                    Some(v) => Ok(IrValue::Enum {
                        name: ty.clone(),
                        variant: v.clone(),
                        payload: None,
                    }),
                    None => Err(IrError::msg(
                        "IndexOutOfBounds",
                        "@enumFromInt: index out of bounds",
                    )),
                },
                None => Err(IrError::msg(
                    "UnknownType",
                    format!("@enumFromInt: unknown type `{ty}`"),
                )),
            }
        }
        "@panic" => {
            // Q-S2：@panic("消息", 位置) abort
            let msg = if args.is_empty() {
                "panic".to_string()
            } else {
                deref_value(ctx, &args[0]).display(ctx)
            };
            Err(IrError::msg("Panic", msg))
        }
        "@sizeOf" => {
            let ty = match deref_value(ctx, &args[0]) {
                IrValue::Str(s) => String::from_utf8_lossy(s).to_string(),
                _ => return Err(IrError::msg("TypeError", "@sizeOf expects type name")),
            };
            match scalar_size_ir(&ty) {
                Some(s) => Ok(IrValue::Int(s as i128)),
                None => Err(IrError::msg(
                    "UnknownType",
                    format!("@sizeOf: unknown type `{ty}`"),
                )),
            }
        }
        "@alignOf" => {
            let ty = match deref_value(ctx, &args[0]) {
                IrValue::Str(s) => String::from_utf8_lossy(s).to_string(),
                _ => return Err(IrError::msg("TypeError", "@alignOf expects type name")),
            };
            let align = match ty.as_str() {
                "i8" | "u8" | "bool" => 1,
                "i16" | "u16" | "f16" => 2,
                "i32" | "u32" | "f32" => 4,
                "i128" | "u128" | "f128" => 16,
                _ => scalar_size_ir(&ty).map(|s| s.min(8)).unwrap_or(8),
            };
            Ok(IrValue::Int(align as i128))
        }
        "@offsetOf" => Err(IrError::msg(
            "Unsupported",
            "@offsetOf requires type layout (not in IR runtime)",
        )),
        "@typeOf" => {
            let v = deref_value(ctx, &args[0]);
            Ok(str_val(&ir_type_name(ctx, v)))
        }
        "@intCast" => {
            let ty = match deref_value(ctx, &args[0]) {
                IrValue::Str(s) => String::from_utf8_lossy(s).to_string(),
                _ => return Err(IrError::msg("TypeError", "@intCast expects type name")),
            };
            let i = match deref_value(ctx, &args[1]) {
                IrValue::Int(i) => *i,
                _ => return Err(IrError::msg("TypeError", "@intCast expects int")),
            };
            if let Some((min, max)) = int_width_bounds_ir(&ty) {
                if i < min || i > max {
                    return Err(IrError::msg(
                        "IntCastOverflow",
                        format!("@intCast overflow to {ty}"),
                    ));
                }
            }
            Ok(IrValue::Int(i))
        }
        "@ptrCast" | "@alignCast" => {
            // tag1 指针无类型化——透传
            let v = args
                .last()
                .ok_or_else(|| IrError::msg("ArityMismatch", name))?;
            Ok(deref_value(ctx, v).clone())
        }
        "@volatileLoad" => {
            // K2：@volatileLoad(ptr)——读穿指针。IR 参考解释器无优化器，volatile
            // 透明 = deref_value（对齐 interp deref_checked）；原生 LLVM volatile 指令层体现。
            if args.len() != 1 {
                return Err(IrError::msg("ArityMismatch", "@volatileLoad"));
            }
            Ok(deref_value(ctx, &args[0]).clone())
        }
        "@volatileStore" => {
            // K2：@volatileStore(ptr, v)——写穿指针（对齐 StorePtr 写穿语义）
            if args.len() != 2 {
                return Err(IrError::msg("ArityMismatch", "@volatileStore"));
            }
            let t = args[0].clone();
            let v = args[1].clone();
            match t {
                IrValue::Ptr(cell) => ctx.set_cell(cell, v),
                IrValue::Boxed(cell) => {
                    let data = match &ctx.cells[cell] {
                        Cell::Boxed { data, .. } => Some(*data),
                        _ => None,
                    };
                    match data {
                        Some(d) => ctx.set_cell(d, v),
                        None => {
                            return Err(IrError::msg("BadAssign", "@volatileStore to non-pointer"))
                        }
                    }
                }
                _ => return Err(IrError::msg("BadAssign", "@volatileStore to non-pointer")),
            }
            Ok(IrValue::Void)
        }
        "@ptrFromInt" => {
            // K4：@ptrFromInt(addr)——整数地址 → 虚拟指针。登记过（@intFromPtr）→ 重建
            // 原指针（round-trip 保真，含 Ptr/Boxed 变体）；未登记 → 合成匿名槽（同地址
            // 幂等，对齐 interp 语义）。IR 指针 = cell 索引（永不回收，地址稳定）。
            if args.len() != 1 {
                return Err(IrError::msg("ArityMismatch", "@ptrFromInt"));
            }
            match deref_value(ctx, &args[0]).clone() {
                IrValue::Int(i) => {
                    if let Some(v) = ctx.addr_registry.get(&i) {
                        return Ok(v.clone());
                    }
                    let cell = ctx.alloc(Cell::Value(IrValue::Void));
                    ctx.addr_registry.insert(i, IrValue::Ptr(cell));
                    Ok(IrValue::Ptr(cell))
                }
                _ => Err(IrError::msg("TypeError", "@ptrFromInt expects an integer")),
            }
        }
        "@intFromPtr" => {
            // K4：@intFromPtr(p)——指针 → 整数地址。cell 索引即地址（对齐 interp Rc 堆地址
            // 的角色；登记原值进 addr_registry 供 @ptrFromInt 重建）。
            if args.len() != 1 {
                return Err(IrError::msg("ArityMismatch", "@intFromPtr"));
            }
            match &args[0] {
                IrValue::Ptr(cell) | IrValue::Boxed(cell) => {
                    let addr = *cell as i128;
                    ctx.addr_registry.insert(addr, args[0].clone());
                    Ok(IrValue::Int(addr))
                }
                _ => Err(IrError::msg("TypeError", "@intFromPtr expects a pointer")),
            }
        }
        "@atomicLoad" => {
            // Q-S3：@atomicLoad(T, p, order)——原子读。协作式单线程无并发竞争，atomic
            // 透明 = deref_value（对齐 @volatileLoad）；类型名/内存序参数已求值、忽略。
            if args.len() != 3 {
                return Err(IrError::msg("ArityMismatch", "@atomicLoad"));
            }
            Ok(deref_value(ctx, &args[1]).clone())
        }
        "@atomicStore" => {
            // Q-S3：@atomicStore(T, p, v, order)——原子写（对齐 @volatileStore 写穿）。
            if args.len() != 4 {
                return Err(IrError::msg("ArityMismatch", "@atomicStore"));
            }
            let t = args[1].clone();
            let v = args[2].clone();
            match t {
                IrValue::Ptr(cell) => ctx.set_cell(cell, v),
                IrValue::Boxed(cell) => {
                    let data = match &ctx.cells[cell] {
                        Cell::Boxed { data, .. } => Some(*data),
                        _ => None,
                    };
                    match data {
                        Some(d) => ctx.set_cell(d, v),
                        None => {
                            return Err(IrError::msg("BadAssign", "@atomicStore to non-pointer"))
                        }
                    }
                }
                _ => return Err(IrError::msg("BadAssign", "@atomicStore to non-pointer")),
            }
            Ok(IrValue::Void)
        }
        "@atomicRmw" => {
            // Q-S3：@atomicRmw(T, p, op, v, order)——读改写，返回旧值。op 为内建枚举
            // 变体（.add/.sub/.exchange/.cmpxchg；tag1 支持前三）。协作式下无竞争。
            if args.len() != 5 {
                return Err(IrError::msg("ArityMismatch", "@atomicRmw"));
            }
            let op_name = match deref_value(ctx, &args[2]) {
                IrValue::Enum { variant, .. } => variant.clone(),
                _ => return Err(IrError::msg("TypeError", "@atomicRmw expects enum op")),
            };
            let v = deref_value(ctx, &args[3]).clone();
            match args[1].clone() {
                IrValue::Ptr(cell) => {
                    let old = ctx.cell_value(cell).clone();
                    let new = match op_name.as_str() {
                        "add" | "sub" => match (&old, &v) {
                            (IrValue::Int(a), IrValue::Int(b)) => {
                                IrValue::Int(if op_name == "add" { a + b } else { a - b })
                            }
                            _ => {
                                return Err(IrError::msg(
                                    "TypeError",
                                    "@atomicRmw add/sub expects int",
                                ))
                            }
                        },
                        "exchange" => v.clone(),
                        _ => return Err(IrError::msg("TypeError", "@atomicRmw bad op")),
                    };
                    ctx.set_cell(cell, new);
                    Ok(old)
                }
                _ => Err(IrError::msg("BadAssign", "@atomicRmw expects pointer")),
            }
        }
        "@compileError" => {
            let msg = if args.is_empty() {
                "compileError".to_string()
            } else {
                deref_value(ctx, &args[0]).display(ctx)
            };
            Err(IrError::msg(
                "CompileError",
                format!("@compileError: {msg}"),
            ))
        }
        "@addWithOverflow" | "@subWithOverflow" | "@mulWithOverflow" => {
            // 返回 (T, bool) 元组；tag1 Int = i128 无溢出（标志恒 false）
            let a = match deref_value(ctx, &args[0]) {
                IrValue::Int(i) => *i,
                _ => return Err(IrError::msg("TypeError", "expected int")),
            };
            let b = match deref_value(ctx, &args[1]) {
                IrValue::Int(i) => *i,
                _ => return Err(IrError::msg("TypeError", "expected int")),
            };
            let r = match name {
                "@addWithOverflow" => a.wrapping_add(b),
                "@subWithOverflow" => a.wrapping_sub(b),
                _ => a.wrapping_mul(b),
            };
            Ok(make_arr(ctx, vec![IrValue::Int(r), IrValue::Bool(false)]))
        }
        // ---------- 数值工具 ----------
        "sqrt" => {
            let v = deref_value(ctx, &args[0]);
            match v {
                IrValue::Int(i) => Ok(IrValue::Float((*i as f64).sqrt())),
                IrValue::Float(f) => Ok(IrValue::Float(f.sqrt())),
                _ => Err(IrError::msg("TypeError", "sqrt expects number")),
            }
        }
        "min" | "max" => {
            if args.len() != 2 {
                return Err(IrError::msg("ArityMismatch", name));
            }
            let a = deref_value(ctx, &args[0]).clone();
            let b = deref_value(ctx, &args[1]).clone();
            let take_a = match (&a, &b) {
                (IrValue::Int(x), IrValue::Int(y)) => {
                    if name == "min" {
                        x <= y
                    } else {
                        x >= y
                    }
                }
                (IrValue::Float(x), IrValue::Float(y)) => {
                    if name == "min" {
                        x <= y
                    } else {
                        x >= y
                    }
                }
                (IrValue::Int(x), IrValue::Float(y)) => {
                    if name == "min" {
                        (*x as f64) <= *y
                    } else {
                        (*x as f64) >= *y
                    }
                }
                (IrValue::Float(x), IrValue::Int(y)) => {
                    if name == "min" {
                        *x <= (*y as f64)
                    } else {
                        *x >= (*y as f64)
                    }
                }
                _ => return Err(IrError::msg("TypeError", "min/max expects numbers")),
            };
            Ok(if take_a { a } else { b })
        }
        // ---------- 格式辅助（M5.3 serialize）：fmt_int/fmt_float → String ----------
        "fmt_int" => {
            let v = deref_value(ctx, &args[0]);
            match v {
                IrValue::Int(i) => Ok(str_val(&i.to_string())),
                _ => Err(IrError::msg("TypeError", "fmt_int expects integer")),
            }
        }
        "fmt_float" => {
            let v = deref_value(ctx, &args[0]);
            match v {
                IrValue::Float(f) => Ok(str_val(&IrValue::Float(*f).display(ctx))),
                IrValue::Int(i) => Ok(str_val(&IrValue::Float(*i as f64).display(ctx))),
                _ => Err(IrError::msg("TypeError", "fmt_float expects float")),
            }
        }
        // ---------- 字节/算法 ----------
        "read_u64_le" => {
            let v = deref_value(ctx, &args[0]);
            let b = value_bytes_ir(ctx, v)
                .ok_or_else(|| IrError::msg("TypeError", "read_u64_le expects bytes"))?;
            if b.len() < 8 {
                return Err(IrError::msg("IndexOutOfBounds", "read_u64_le: truncated"));
            }
            let n = u64::from_le_bytes(b[0..8].try_into().unwrap());
            Ok(IrValue::Int(n as i128))
        }
        "sort" => {
            let v = deref_value(ctx, &args[0]).clone();
            // 对齐 oracle interp.rs:3195-3205：第二参若提供必须是比较器闭包，
            // 否则 TypeError（避免静默不排序）
            let cmp_f = match args.get(1) {
                Some(a) => {
                    let f = deref_value(ctx, a).clone();
                    match &f {
                        IrValue::Closure { .. } | IrValue::Fn(_) => Some(f),
                        _ => {
                            return Err(IrError::msg(
                                "TypeError",
                                "sort comparator must be a closure",
                            ))
                        }
                    }
                }
                None => None,
            };
            match v {
                IrValue::Arr(c) => {
                    let elems = match &ctx.cells[c] {
                        Cell::Elems(e) => e.clone(),
                        _ => return Err(IrError::msg("TypeError", "sort expects array")),
                    };
                    let mut items: Vec<(usize, IrValue)> = elems
                        .iter()
                        .map(|ec| (*ec, ctx.cell_value(*ec).clone()))
                        .collect();
                    items.sort_by(|x, y| match &cmp_f {
                        Some(f) => {
                            let r =
                                call_closure_value_ir(ctx, module, f, &[x.1.clone(), y.1.clone()]);
                            match r {
                                Ok(IrValue::Int(i)) if i < 0 => std::cmp::Ordering::Less,
                                Ok(IrValue::Int(i)) if i > 0 => std::cmp::Ordering::Greater,
                                Ok(IrValue::Float(ff)) if ff < 0.0 => std::cmp::Ordering::Less,
                                Ok(IrValue::Float(ff)) if ff > 0.0 => std::cmp::Ordering::Greater,
                                _ => std::cmp::Ordering::Equal,
                            }
                        }
                        None => {
                            if value_lt(&x.1, &y.1) {
                                std::cmp::Ordering::Less
                            } else if x.1.value_eq(ctx, &y.1) {
                                std::cmp::Ordering::Equal
                            } else {
                                std::cmp::Ordering::Greater
                            }
                        }
                    });
                    let new_elems: Vec<usize> = items.iter().map(|(c, _)| *c).collect();
                    ctx.cells[c] = Cell::Elems(new_elems);
                    Ok(IrValue::Void)
                }
                _ => Err(IrError::msg("TypeError", "sort expects array")),
            }
        }
        "binary_search" => {
            let v = deref_value(ctx, &args[0]).clone();
            let target = deref_value(ctx, &args[1]).clone();
            let items: Vec<IrValue> = match &v {
                IrValue::Arr(c) => match &ctx.cells[*c] {
                    Cell::Elems(e) => e.iter().map(|ec| ctx.cell_value(*ec).clone()).collect(),
                    _ => return Err(IrError::msg("TypeError", "binary_search expects array")),
                },
                IrValue::Slice { data, start, len } => match &ctx.cells[*data] {
                    Cell::Elems(e) => e[*start..*start + *len]
                        .iter()
                        .map(|ec| ctx.cell_value(*ec).clone())
                        .collect(),
                    _ => return Err(IrError::msg("TypeError", "binary_search expects slice")),
                },
                _ => {
                    return Err(IrError::msg(
                        "TypeError",
                        "binary_search expects array or slice",
                    ))
                }
            };
            let mut lo = 0usize;
            let mut hi = items.len();
            while lo < hi {
                let mid = (lo + hi) / 2;
                if value_lt(&items[mid], &target) {
                    lo = mid + 1;
                } else if items[mid].value_eq(ctx, &target) {
                    return Ok(IrValue::Opt(Some(Box::new(IrValue::Int(mid as i128)))));
                } else {
                    hi = mid;
                }
            }
            Ok(IrValue::Opt(None))
        }
        // ---------- 解析器辅助（71-recursive-parser；操作 &[u8] 与 *usize）----------
        "skip_space" | "peek" | "advance" | "is_digit" | "parse_number" => {
            let r = call_parser_builtin_ir(ctx, module, name, args)?
                .ok_or_else(|| IrError::msg("NoMethod", name))?;
            Ok(r)
        }
        "parse_int" => {
            let s = str_arg_ir(ctx, args, 0)?;
            let text = String::from_utf8_lossy(&s).trim().to_string();
            let parsed = if text.is_empty() {
                None
            } else {
                text.parse::<i128>().ok()
            };
            Ok(match parsed {
                Some(n) => IrValue::Opt(Some(Box::new(IrValue::Int(n)))),
                None => IrValue::Opt(None),
            })
        }
        "parse_float" => {
            let s = str_arg_ir(ctx, args, 0)?;
            let text = String::from_utf8_lossy(&s).trim().to_string();
            let parsed = if text.is_empty() {
                None
            } else {
                text.parse::<f64>().ok()
            };
            Ok(match parsed {
                Some(n) => IrValue::Opt(Some(Box::new(IrValue::Float(n)))),
                None => IrValue::Opt(None),
            })
        }
        // ---------- 断言五件套（Q-T1）：测试函数内隐式可用；3 参 expect = 解析器 ----------
        "expect" => {
            if args.len() == 3 {
                let r = call_parser_builtin_ir(ctx, module, "expect", args)?
                    .ok_or_else(|| IrError::msg("NoMethod", "expect parser"))?;
                return Ok(r);
            }
            if args.first().map_or(false, |v| v.as_bool()) {
                Ok(IrValue::Void)
            } else {
                *fail = Some("expect failed".into());
                Ok(IrValue::Void)
            }
        }
        "expect_eq" | "expect_neq" => {
            if args.len() != 2 {
                return Err(IrError::msg("ArityMismatch", name));
            }
            let a = deref_value(ctx, &args[0]);
            let b = deref_value(ctx, &args[1]);
            let eq = a.value_eq(ctx, b);
            let want_eq = name == "expect_eq";
            if eq != want_eq {
                *fail = Some(format!(
                    "{} failed: expected {} {}, got {}",
                    name,
                    if want_eq { "=" } else { "!=" },
                    b.display(ctx),
                    a.display(ctx)
                ));
            }
            Ok(IrValue::Void)
        }
        "expect_error" => {
            if args.len() != 2 {
                return Err(IrError::msg("ArityMismatch", "expect_error"));
            }
            let want = deref_value(ctx, &args[0]);
            let got = deref_value(ctx, &args[1]);
            match (want, got) {
                // M4.2：错误码比较（码全局唯一）
                (IrValue::Err { name: w, .. }, IrValue::Err { name: g, .. }) if w == g => {
                    Ok(IrValue::Void)
                }
                (IrValue::Err { name: w, .. }, IrValue::Err { name: g, .. }) => {
                    *fail = Some(format!(
                        "expect_error failed: expected error.{w}, got error.{g}"
                    ));
                    Ok(IrValue::Void)
                }
                (_, g) => {
                    *fail = Some(format!(
                        "expect_error failed: expected error, got {}",
                        ir_type_name(ctx, g)
                    ));
                    Ok(IrValue::Void)
                }
            }
        }
        "expect_eq_slices" => {
            if args.len() != 2 {
                return Err(IrError::msg("ArityMismatch", "expect_eq_slices"));
            }
            let a = deref_value(ctx, &args[0]);
            let b = deref_value(ctx, &args[1]);
            if a.value_eq(ctx, b) {
                Ok(IrValue::Void)
            } else {
                *fail = Some(format!(
                    "expect_eq_slices failed: {} != {}",
                    a.display(ctx),
                    b.display(ctx)
                ));
                Ok(IrValue::Void)
            }
        }
        _ => Ok(IrValue::Void),
    }
}
