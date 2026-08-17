# 标准库四大支柱范围

> 标准库按「数据为中心」的四大支柱组织（`CONTEXT.md` 术语）。策略：Zig 式广度优先，四大支柱做扎实，其余按需。API 冻结进入 M10。

## 基础层（所有支柱共用）

| 模块 | 内容 | 参考 |
|---|---|---|
| `mem` | Allocator 抽象、全局回退分配器、Arena（完整设计见 [`08-mem-allocator-design.md`](08-mem-allocator-design.md)） | Zig `std.mem` |
| `io` | I/O + 并发统一接口（含 async/await 支持，2026-08-13 逆转） | Zig `std.Io` |
| `time` | 时间与时区 | Zig `std.time` |
| `debug` | 断言、日志、调试设施 | Zig `std.debug` |

## 支柱一：定义数据

| 模块 | 内容 |
|---|---|
| `types` | 运行时类型元数据、反射式信息（comptime 生成） |
| `serialize` / `deserialize` | **分层序列化**（2026-08-13 定案；2026-08-14 修订）：Continuous 类型 ↔ bytes 内建（to_bytes/from_bytes）；class（默认）↔ JSON 内建（to_json/from_json）+ 脚本生成可定制；集合（含 Table）→ 二进制 |
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
| `ffi` | C ABI 互操作（`hc cc` 配套）、外部符号绑定；`extern fn` + `@cImport`（Q-S4 定案，C 指针外置不参与登记） |
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

### io（标准库模块，import 引入，函数直接调用；入口不再注入——2026-08-17 定案，见 ADR-0010）

- `io.print(comptime 格式串, ...)` — 格式化输出（Q2 comptime 校验；Zig 式说明符）
- **程序环境（2026-08-14 定案；2026-08-17 修订为模块形态）**：`io.env(name) ?&[u8]`（环境变量）/ `io.stdin`、`io.stdout`、`io.stderr`（字节流：read_all/write_all）/ `io.exit(t: ExitType, code: u8) !void`（`enum ExitType { Exit, Error }` 默认枚举：Exit 正常静默、Error 错误退出打印标记；main error → Error/1、正常 → Exit/0）——形态：标准库模块函数 + 模块内环境状态；**命令行参数仅经入口 `main(args)` 注入，`io.args()` 取消（2026-08-17 定案）**
- `io.fs.open(path) !File` / `io.fs.open_dir(path) !Dir` / `defer f.close()`
- `io.fs.read_file(path, alloc) !&[u8]`（路径直读） / `io.fs.read_all(&f, alloc) !&[u8]`（句柄读）
- `io.fs.write_all(&f, data) !void` ≡ `f.write_all(data)`（双语，Q20）
- `io.fs.append(path, data) !void` ≡ `f.append(data)`；`io.fs.rename(a, b) !void`；`io.fs.remove(path) !void`
- `io.fs.read_int(path) !i64` / `io.fs.write_int(path, v) !void`
- **随机访问（2026-08-14 定案）**：`f.seek(offset: usize) !void`（绝对定位游标）/ `f.pos() usize`（当前偏移）/ `f.read_at(buf, offset) !usize` / `f.write_at(data, offset) !void`（定位读写，不改游标）
- `io.fs.list_dir(&dir, alloc) !Vec(DirEntry)`（DirEntry：name / is_dir）
- `io.net.connect(url) !Conn` / `io.net.get(url) !&[u8]` / `io.net.read_all(&conn, alloc)`
- `io.net.listen(port) !Server` / `io.net.accept(&server) !Conn`
- `io.net.read_frame(&conn, alloc) !&[u8]` / `io.net.write_frame(&conn, data) !void`（长度前缀帧）
- **UDP（2026-08-14 定案，进 1.0）**：`io.net.udp.bind(port) !UdpSocket` / `send_to(&sock, addr, data) !void` / `recv_from(&sock, alloc) !(Addr, &[u8])`（元组多值返回）/ `close()`
- `io.time.now() i64` / `io.time.sleep(ms) void`

### 集合与字符串（Q15 构造；String = u8[] 别名 Q3）

- `Vec(T).init(alloc)` — append / len / extend / to_bytes / from_bytes
- `Map(K, V).init(alloc)` — put / get ?V / contains / remove / len
- `String.from(&[u8], alloc)` — concat / split / join / find ?usize / substring / replace / to_upper / to_bytes / **as_slice（内容视图，无前缀，R-2）** / == 内容比较
- `String.from_slice(&buf, arena)`（arena 分配形态）
- 内建：`copy(&v)` / `box(v, alloc)`（Q12）
- `@` 内建（Q-S1/Q-S3 定案）：`@sizeOf` / `@alignOf` / `@offsetOf` / `@typeOf` / `@intCast` / `@ptrCast` / `@alignCast` / `@compileError` / `@atomicLoad` / `@atomicStore` / `@atomicRmw`（内存序 relaxed/acquire/release/acq_rel/seq_cst）

### 内存

- `alloc`（默认分配器，global；每线程独立实例，Q8）
- `Arena.init(alloc)` / `arena.alloc(n)` / `arena.alloc(T{...})` / `deinit`（无所有权，Q16）

### 算法与工具

- `sort(&mut arr)` / `sort(&mut vec, cmp 闭包)` / `binary_search(&arr, v) ?usize`
- `json.parse(data)` / `Order.from_json(data)`（class 序列化分层）
- `utf8.decode(data)`；`math.nan(f64)` / `math.inf(f32)` / `math.inf_neg(f64)`（类型参数 comptime 式）
- 待定归属：`fmt_int(i32) String`、`parse_int(&[u8]) ?i32`、`min(a, b)`、`sqrt(x)`、`read_u64_le(&[u8]) u64`（57 使用）
- `debug` 断言（Q-T1 定案，测试块内隐式可用）：`expect(cond)` / `expect_eq(a, b)` / `expect_neq(a, b)` / `expect_error(e, expr)` / `expect_eq_slices(a, b)`——均 `anyerror!void`
- `test_io`（Q-T4 定案）——**2026-08-17 取消**（ADR-0010）：测试直接调 `main()`；需要 io 的测试经 `import H.std.{io}` 使用环境

### 并发（12.21/12.24/Q14/Q20）

- `spawn(f, args...) o Thread(T)` — `join() !T` / `cancel() !void` / `is_done() bool` / `detach()`
- `async fn` → `Future(R)`（R 含 !）；`await f`
- 四模式类型：`init(alloc)` / `write(v)` / `read() T` / `try_read() ?T` / `close()` / **`send(v)` / `recv() T`（通道，Q-R12）**
- 原子原语（Q-S3）：`@atomicLoad` / `@atomicStore` / `@atomicRmw` + 内存序五值
- `Io.evented(alloc)` / `Io.threaded()`（运行时显式切换，Q35）

## 系统编程扩展（内核/驱动方向，2026-08-14 评估补充）

内核编程场景的标准库缺口——可自建，但应纳入 std；**1.x 候选，不阻塞 1.0 用户态系统编程**。前提：需先补齐 `05-open-questions-and-risks.md` 系统编程缺口 K1–K6（union/volatile/asm/int↔ptr/符号导出/裸机），否则以下模块在内核场景不可用。

| 模块 | 内容 | 内核场景 |
|---|---|---|
| `mem.bitmap` | 位图 | 物理页分配器、inode 位图 |
| `collections.intrusive` | 侵入式链表（节点内嵌对象，依赖指针自由） | TCB 链表、空闲块链表 |
| `io.ring` | 定容无锁环形缓冲（SPSC） | 驱动 DMA、串口、IPC |
| `collections.tree` | 红黑树/基数树 | 调度器就绪队列、地址空间 VMA |
| `mem.page` | 页对齐分配、buddy 构建块、内存屏障封装 | 内核堆、页表 |
