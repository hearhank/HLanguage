# H 标准库文档

H.std 标准库为编译器内建（Rust 实现，无 .hc 源）；本页为目录化摘要（覆盖 tag1 已落地子集，非完整 API）。`import H.std.{io}` 显式引入对应模块。

## io（I/O）

- `io.print(fmt: String, args...) !void` — 格式化输出到 stdout
- `io.env(name: String) ?&[u8]` — 环境变量
- `io.stdin / io.stdout / io.stderr` — 标准流（可读/写）
- `io.stdout.write_all(data) / io.stderr.write_all(data)` — 写真实标准输出/错误流（G2，独立字节流）
- `io.exit(t: ExitType, code: u8) !void` — 退出：Exit 静默 / Error 错误退出打印标记

## io.fs（文件系统）

- `io.fs.open(path) !File` — 打开文件（f.read_all(alloc) / f.write_all(data) / f.close()）
- `io.fs.read_file(path, alloc) !&[u8]` — 路径直读
- `io.fs.write_all(&f, data) !void ≡ f.write_all(data)` — 句柄写（Q20 双语）
- `io.fs.append(path, data) !void / io.fs.rename(a, b) !void / io.fs.remove(path) !void` — 文件增删改
- `io.fs.read_int(path) !i64 / io.fs.write_int(path, v) !void` — 整数读写
- `io.fs.open_dir(path) !Dir` — 目录句柄（G2：dir.list_dir(alloc) / dir.close()）
- `io.fs.list_dir(path) !Vec(DirEntry)` — 目录枚举（G2：{name, is_dir}）
- `f.seek(offset) / f.pos() / f.read_at(buf, offset) / f.write_at(data, offset)` — 文件随机访问

## io.net（网络）

- `io.net.get(url) !&[u8]` — HTTP GET 客户端（G1；仅 http://，非 200 → error.Http{code}）
- `io.net.connect(host, port, alloc) !TcpConn` — TCP 客户端
- `io.net.listen(host, port, alloc) !TcpListener` — TCP 服务端（accept/read_all/write/shutdown/close/local_port，Q20 双语）
- `io.net.udp.bind(port) !UdpSocket` — UDP（G1；bind(host, port) 亦支持）
- `sock.send_to(addr, data) / sock.recv_from(alloc) ![addr, data] / sock.close()` — UDP 收发（空队列 200ms → error.TimedOut）

## io.time（时间）

- `io.time.now() i64` — 毫秒时间戳
- `io.time.sleep(ms) void` — 休眠
- `io.time.tick() i64 / io.time.elapsed(tick) i64` — 单调测量（G5；时区完整留 1.x）

## io.text（文本正则，G5）

- `io.text.matches(pattern, text) bool` — 是否含匹配（^/$ 锚定控制全串）
- `io.text.find(pattern, text) ?int` — 首个匹配起点；无 → null
- `io.text.replace(pattern, text, repl) &[u8]` — 替换全部非重叠匹配（每处最长）
- `io.text.split(pattern, text) Vec(&[u8])` — 按匹配分割（含空段）
- `子集：字面量 / `.` / `[...]` 范围取反 / `\d` `\w` `\s` / 分组 / `*` `+` `?` `{n,m}` / `|` / `^` `$` / `\xNN` 转义` — 非法模式 → error.InvalidFormat

## io.rng（伪随机数，G5）

- `io.rng.seed(v)` — 设定种子（0 → 回退默认）
- `io.rng.next() u64` — xorshift64* 原始 64 位
- `io.rng.int(n) int` — [0, n) 均匀（拒绝采样免模偏差）
- `io.rng.float() f64` — [0, 1) 高 53 位

## io.ipc（进程内通信，G3）

- `io.ipc.pipe() ![PipeReader, PipeWriter]` — 匿名管道（2 元素数组，同 UDP recv_from 约定）
- `reader.read(alloc)/read_all(alloc)/is_closed()/close()；writer.write(data)/close()` — 管道读写（空且写端开 → 空切片，不阻塞——协作式）
- `io.ipc.shm(name, size) !Shm` — 命名共享内存定长字节区（write/read/close）

## io.storage / io.archive（持久化/压缩，G4）

- `io.storage.open(path) !KvStore` — 文件持久化键值存储
- `kv.put(key, value) / kv.get(key) !?&[u8]（缺失 → null）/ kv.contains(key) / kv.remove(key) / kv.len() / kv.close()` — 键值方法（close 落盘+注销幂等）
- `io.archive.compress(data) !&[u8] / decompress(data) !&[u8]` — RLE 压缩/解压（非法数据 → error.InvalidFormat）

## alloc / mem（内存分配）

- `alloc.init(T) T` — 类型名/字面量构造实例（带参 `alloc.init(T{...})`）
- `alloc.alloc(size: usize) *u8` — 原始分配
- `alloc.free(ptr) !void` — 释放
- `mem.Arena.init(alloc) Arena` — Arena 分配器（typed 构造 arena.init(T)）
- `mem.Allocator` — 分配器抽象

## collections（集合）

- `Vec(T).init(alloc) Vec(T)` — 动态数组
- `Vec(T).append(v: T)` — 追加
- `String.from(bytes: []const u8, alloc) String` — 字节 → 字符串
- `String 方法：concat / split / join / find ?usize / substring / replace / to_upper / to_lower / as_slice / to_bytes / == 内容比较` — G2：to_upper/to_lower 为 ASCII 大小写转换（非 ASCII 字节不变）
- `Map(K,V).init(alloc) Map` — 哈希表
- `Deque(T).init(alloc) Deque` — 双端队列

## serialize（序列化）

- `serialize.parse_int(s: String) !i64` — 十进制 → 整数
- `serialize.parse_float(s: String) !f64` — 十进制 → 浮点
- `serialize.json.parse(s: String) !Value` — JSON 解析
- `serialize.csv.parse(s: String) !Vec` — CSV 解析
- `to_bytes(v) []u8 / from_bytes(T, bytes) T` — 字节序列化（箱）
- `to_json(v) String / from_json(T, s) T` — JSON 序列化（箱）

## scalar 接口族（标量接口）

- `interface ICompare` — 比较：lt/le/eq/ne/gt/ge
- `interface INumber: ICompare` — 数值运算：add/sub/mul/div/rem/neg/abs
- `interface IInt: INumber` — 整数：位运算/移位
- `interface IUint: IInt` — 无符号整数
- `interface IFloat: INumber` — 浮点：sqrt/pow/floor/ceil/round
- `interface IIterable` — 迭代三态（iter/next/done）

## @ 内建

- `@sizeOf(T) usize` — 类型字节大小
- `@alignOf(T) usize` — 类型对齐
- `@offsetOf(T, field) usize` — 字段偏移
- `@typeOf(v) type` — 值类型
- `@intCast(T, x) T` — 整数类型转换
- `@ptrCast(T, p) T` — 指针类型转换
- `@volatileLoad(p) T` — 防优化掉读穿（MMIO）
- `@volatileStore(p, v)` — 防优化掉写穿（MMIO）
- `@compileError(msg)` — 编译期错误
- `@addWithOverflow(a, b) (T, bool)` — 溢出检测加法
- `@panic(msg)` — 运行时中止
- `box(v) / copy(v)` — 装箱/复制

## 线程（组 G 生命周期）

- `spawn(f, args...) o Thread(T)` — 协作式延迟执行：立即返回句柄，join 时运行
- `thread.join() !T` — 运行到完成并取结果
- `thread.cancel() !void` — 协作取消（未运行 → join 返回 error.Cancelled）
- `thread.is_done() bool` — 完成查询
- `thread.detach()` — 立即运行到完成并丢弃结果

