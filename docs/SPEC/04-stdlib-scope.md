# 标准库四大支柱范围

> 标准库按「数据为中心」的四大支柱组织（`CONTEXT.md` 术语）。策略：Zig 式广度优先，四大支柱做扎实，其余按需。API 冻结进入 M10。

## 基础层（所有支柱共用）

| 模块 | 内容 | 参考 |
|---|---|---|
| `mem` | Allocator 抽象、全局回退分配器、Arena | Zig `std.mem` |
| `io` | I/O + 并发统一接口（含 async/await 支持，2026-08-13 逆转） | Zig `std.Io` |
| `time` | 时间与时区 | Zig `std.time` |
| `debug` | 断言、日志、调试设施 | Zig `std.debug` |

## 支柱一：定义数据

| 模块 | 内容 |
|---|---|
| `types` | 运行时类型元数据、反射式信息（comptime 生成） |
| `serialize` / `deserialize` | **分层序列化**（2026-08-13 定案）：struct ↔ bytes 内建（to_bytes/from_bytes）；class ↔ JSON 内建（to_json/from_json）+ 脚本生成可定制；集合 → 二进制 |
| `validate` | 数据约束校验（定义数据时声明的约束的运行时检查） |

## 支柱二：修改数据

| 模块 | 内容 |
|---|---|
| `collections` | `Vec`、`Map`、`Set`、`String`、`Deque` 等容器（数据栈对象规则适用） |
| `transform` | 迭代、映射、过滤、归约等数据变换 |
| `sort` | 排序与搜索 |
| `text` | 文本处理（正则等） |

## 支柱三：传输数据

| 模块 | 内容 |
|---|---|
| `net` | TCP/UDP、HTTP 客户端/服务端 |
| `ipc` | 进程间通信、管道、共享内存 |
| `ffi` | C ABI 互操作（`hc cc` 配套）、外部符号绑定 |
| `channel` | 线程间数据传递（配合 `io` 接口） |

## 支柱四：保存数据

| 模块 | 内容 |
|---|---|
| `fs` | 文件系统、路径、流式读写 |
| `storage` | 键值存储接口、数据库连接抽象 |
| `archive` | 归档与压缩 |

## 端到端验收基准

M7 结束前必须存在一个**同时使用四大支柱的示例程序**（如：一个网络服务，接收数据 → 校验/变换 → 序列化 → 落盘），且能在双模式下运行并结果一致。此示例是「1.0 可用性」的事实检验。

## 示例 API 清单（语法规格基线，2026-08-13 Q27 定案）

> 以下为 85 个示例中出现的标准库 API 形态——示例即规格，M7 细化实现时以此为准绳展开。标记「待定」的为示例中使用但归属未定（内建 or 标准库，M0 定）。

### io（接口约束参数 `io: *T where T: Io`；入口编译器注入句柄，Q22c）

- `io.print(comptime 格式串, ...)` — 格式化输出（Q2 comptime 校验；Zig 式说明符）
- `io.fs.open(path) !File` / `io.fs.open_dir(path) !Dir` / `defer f.close()`
- `io.fs.read_file(path, alloc) !&[u8]`（路径直读） / `io.fs.read_all(&f, alloc) !&[u8]`（句柄读）
- `io.fs.write_all(&f, data) !void` ≡ `f.write_all(data)`（双语，Q20）
- `io.fs.append(path, data) !void` ≡ `f.append(data)`；`io.fs.rename(a, b) !void`；`io.fs.remove(path) !void`
- `io.fs.read_int(path) !i64` / `io.fs.write_int(path, v) !void`
- `io.fs.list_dir(&dir, alloc) !Vec(DirEntry)`（DirEntry：name / is_dir）
- `io.net.connect(url) !Conn` / `io.net.get(url) !&[u8]` / `io.net.read_all(&conn, alloc)`
- `io.net.listen(port) !Server` / `io.net.accept(&server) !Conn`
- `io.net.read_frame(&conn, alloc) !&[u8]` / `io.net.write_frame(&conn, data) !void`（长度前缀帧）
- `io.time.now() i64` / `io.time.sleep(ms) void`

### 集合与字符串（Q15 构造；String = u8[] 别名 Q3）

- `Vec(T).init(alloc)` — append / len / extend / to_bytes / from_bytes
- `Map(K, V).init(alloc)` — put / get ?V / contains / remove / len
- `String.from(&[u8], alloc)` — concat / split / join / find ?usize / substring / replace / to_upper / to_bytes / == 内容比较
- `String.from_slice(&buf, arena)`（arena 分配形态）
- 内建：`copy(&v)` / `box(v, alloc)`（Q12）

### 内存

- `alloc`（默认分配器，global；每线程独立实例，Q8）
- `Arena.init(alloc)` / `arena.alloc(n)` / `arena.alloc(T{...})` / `deinit`（无所有权，Q16）

### 算法与工具

- `sort(&mut arr)` / `sort(&mut vec, cmp 闭包)` / `binary_search(&arr, v) ?usize`
- `json.parse(data)` / `Order.from_json(data)`（class 序列化分层）
- `utf8.decode(data)`；`math.nan(f64)` / `math.inf(f32)` / `math.inf_neg(f64)`（类型参数 comptime 式）
- 待定归属：`fmt_int(i32) String`、`parse_int(&[u8]) ?i32`、`min(a, b)`、`sqrt(x)`、`read_u64_le(&[u8]) u64`（57 使用）

### 并发（12.21/12.24/Q14/Q20）

- `spawn(f, args...) o Thread(T)` — `join() !T` / `cancel() !void` / `is_done() bool` / `detach()`
- `async fn` → `Future(R)`（R 含 !）；`await f`
- 四模式类型：`init(alloc)` / `write(v)` / `read() T` / `try_read() ?T` / `close()`
- `Io.evented(alloc)` / `Io.threaded()`（运行时显式切换，Q35）
