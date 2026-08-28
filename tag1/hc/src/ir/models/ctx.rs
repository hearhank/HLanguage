//! IR 执行上下文（ADR-0028：自 ir/mod.rs 拆分；运行时堆与 io/线程/通道注册表）

use super::*;

/// 运行时堆：跨帧共享的 cell 池（指针可跨帧存活——如传入函数后写穿调用方槽）。
#[derive(Debug, Default)]
pub struct Ctx {
    pub cells: Vec<Cell>,
    /// 全局/常量名 → cell 索引（Phase 5）：cell 由 [`IrRuntime::init`] 预分配，
    /// `@__init__`（`StoreGlobal`）写入初值；`LoadGlobal`/`StoreGlobal` 读写写穿。
    pub globals: HashMap<String, usize>,
    /// io.print/printErr 输出缓冲（Phase 7）：`execute_ir` 运行后冲刷到 stdout。
    pub out: Vec<u8>,
    /// 程序参数（io.args()；由 `hc run`/`hc test` 注入，对齐 oracle `Interp.args`）
    pub args: Vec<Vec<u8>>,
    /// io.exit 请求的退出码（F2：对齐 oracle `Interp.exit_code`；`execute_ir` 遇
    /// ExitRequested 时读取并映射进程退出码）
    pub exit_code: Option<u8>,
    /// io.fs 真实文件句柄表（Phase 7）：File 值 = `Class{_fd}`，fd 索引本表。
    pub files: HashMap<i64, std::fs::File>,
    /// 下一文件描述符（自增分配）
    pub next_fd: i64,
    /// io.net TCP 连接表（fd → TcpStream）
    pub tcp_streams: HashMap<i64, std::net::TcpStream>,
    /// io.net TCP 监听器表（fd → TcpListener）
    pub tcp_listeners: HashMap<i64, std::net::TcpListener>,
    /// 下一网络描述符（自增分配）
    pub next_net_fd: i64,
    /// G5/§8.3 Debug 泄漏检测：全局 alloc 分配记录表（(size, line)；IR 无行号 → line 0）。
    /// IR 值无引用计数，分配登记不自动注销——`leaks()`/`leak_report()` 反映本 run 内
    /// 已分配数（对齐 oracle 语义的 Debug 簿记可观测面；tree-walking 侧用弱引用精确跟踪）。
    pub alloc_tracker: Vec<(usize, u32)>,
    /// 组 G（Q8）：当前线程子任务的每线程 alloc 覆盖。协作式单线程执行下，线程 fn
    /// 运行期间置 Some(每线程 Arena)，`alloc` 解析（LoadGlobal / implicit_env_value）
    /// 优先返回该值——对齐 oracle `Interp` 的 `push_scope` + `bind("alloc", 每线程 arena)`。
    pub current_alloc: Option<IrValue>,
    /// 组 G：当前执行深度（`exec_body` 每次进入时刷新）。线程 fn 以 `cur_depth + 1`
    /// 起步，对齐 oracle 共享 `call_depth` 的 StackOverflow 防护（非独立栈）。
    pub cur_depth: usize,
    /// G1（E3.1）：UDP socket 注册表（UdpSocket 值持 fd；对齐 oracle udp_sockets）
    pub udp_sockets: HashMap<i64, std::net::UdpSocket>,
    /// G2（io 差异项）：Dir 句柄注册表（fd → 目录路径；Dir 值持 `_fd`，list_dir 按路径重读）
    pub dirs: HashMap<i64, String>,
    /// G3（E3.2 ipc）：管道注册表（pid → 共享缓冲 + 写端开标志；PipeReader/PipeWriter
    /// 共享同一 pid，协作式模型下读写均不阻塞）
    pub pipes: HashMap<i64, PipeIr>,
    /// G3（E3.2 ipc）：共享内存注册表（id → 定长字节区；Shm 值持 `shm` id）
    pub shms: HashMap<i64, Vec<u8>>,
    /// 下一管道/共享内存/目录/存储描述符（对齐 oracle 计数器从 1 起步）
    pub next_pipe_fd: i64,
    pub next_shm_fd: i64,
    pub next_dir_fd: i64,
    pub next_store_fd: i64,
    /// G4（E3.3 storage）：键值存储注册表（id → (路径, 键值)；KvStore 值持 `store` id）
    pub stores: HashMap<i64, (String, HashMap<Vec<u8>, Vec<u8>>)>,
    /// G5（E3.3 rng）：全局伪随机数状态（xorshift64；`io.rng.seed` 重置——协作式
    /// 单线程执行下全局态安全；默认种子常量对齐 oracle）
    pub rng_state: u64,
    /// K4（ADR-0014）：`@intFromPtr` 登记的整数地址 → 原值（Ptr/Boxed，round-trip 重建用）。
    /// `@ptrFromInt` 依此重建原指针；未登记地址合成匿名槽（同地址幂等——对齐 interp
    /// 合成 cell 与原生 inttoptr 虚拟指针语义）。
    pub addr_registry: HashMap<i128, IrValue>,
    /// E4：OS 线程控制表（tid → ThreadStateIr）
    pub thread_handles: HashMap<i64, ThreadStateIr>,
    /// E4：下一线程 ID（自增分配）
    pub next_tid: i64,
    /// E4：通道注册表（通道 ID → 通道状态，Pipe 使用 mpsc）
    pub channels: HashMap<i64, ChannelStateIr>,
    /// E4：下一通道 ID（自增分配）
    pub next_channel_id: i64,
    /// E4：当前模块引用（供 spawn 新线程克隆以访问函数定义）
    pub module: Option<Arc<IrModule>>,
    /// 协程调度器（M:N 模型，IR 版本）
    pub scheduler: GoroutineSchedulerIr,
}

impl Ctx {
    pub(in crate::ir) fn alloc(&mut self, cell: Cell) -> usize {
        self.cells.push(cell);
        self.cells.len() - 1
    }
    /// 读槽值（槽 → cell → value；槽/元素/字段 cell 恒为 `Cell::Value`——不变量）
    pub(in crate::ir) fn get(&self, frame: &Frame, slot: usize) -> &IrValue {
        match &self.cells[frame.cells[slot]] {
            Cell::Value(v) => v,
            _ => unreachable!("slot cell is not a value cell"),
        }
    }
    /// 写槽值
    pub(in crate::ir) fn set(&mut self, frame: &Frame, slot: usize, v: IrValue) {
        self.cells[frame.cells[slot]] = Cell::Value(v);
    }
    /// 读 cell 值（指针目标/数组元素/类字段）
    pub(in crate::ir) fn cell_value(&self, cell: usize) -> &IrValue {
        match &self.cells[cell] {
            Cell::Value(v) => v,
            _ => unreachable!("cell is not a value cell"),
        }
    }
    /// 读 cell 为值：Value 直接克隆，非 Value cell（Class/Map/Arena/Boxed）还原为
    /// 对应句柄值（遍历产物如 Map 的 KV 条目即以 Class cell 承载，捕获/收集时用）
    pub(in crate::ir) fn read_cell(&self, cell: usize) -> IrValue {
        match &self.cells[cell] {
            Cell::Value(v) => v.clone(),
            Cell::Class { .. } => IrValue::Class(cell),
            Cell::Map { .. } => IrValue::Map(cell),
            Cell::Arena(_) => IrValue::Arena(cell),
            Cell::Boxed { .. } => IrValue::Boxed(cell),
            other => unreachable!("cell is not a value or handle cell: {other:?}"),
        }
    }
    /// 写 cell 值（写穿）
    pub(in crate::ir) fn set_cell(&mut self, cell: usize, v: IrValue) {
        self.cells[cell] = Cell::Value(v);
    }
    /// 数组底层长度（Phase 2）
    pub(in crate::ir) fn elems_len(&self, cell: usize) -> usize {
        match &self.cells[cell] {
            Cell::Elems(e) => e.len(),
            _ => 0,
        }
    }

    /// G5/§8.3 Debug 泄漏检测：分配清单文本（供程序退出报告 / 测试断言）
    pub fn leak_report(&self) -> String {
        let mut out = String::new();
        for (size, line) in &self.alloc_tracker {
            out.push_str(&format!("leak: line {line}: {size} bytes\n"));
        }
        out
    }

    /// G5/§8.3 Debug 泄漏检测：当前已登记分配数
    pub fn leak_count(&self) -> usize {
        self.alloc_tracker.len()
    }
}
