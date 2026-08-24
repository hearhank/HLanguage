//! Tree-walking 解释器（M3.2 脚本模式 `hc run`——tag1 子集）
//!
//! tag1 采用作用域链环境 + 引用计数槽。字节码 VM（M3.2 完整）与 LLVM 原生
//! 后端（M3.3）留后续里程碑；本模块保证双模式承诺的「脚本模式」先行可用。

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Seek, Write};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use hc::ast::*;
use hc::comptime::{self, Instantiated};
use hc::token::Span;

use crate::value::{
    AllocBlock, AllocErr, AllocatorImpl, ArenaAllocErr, ArenaState, BoxedData, ClassData,
    ClosureData, LazyIterData, LazyOp, LeakRecord, MapData, PoolState, Value, VecData,
};

/// 线程运行结果（跨线程传递）
enum ThreadResult {
    Ok(Value),
    Err(RtError),
}

/// OS 线程控制块
struct ThreadState {
    join_handle: Option<thread::JoinHandle<()>>,
    result: Arc<Mutex<Option<ThreadResult>>>,
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
}

// ---------- 子模块 ----------
mod call;
mod eval;
mod expr;
mod io;
mod layout;
mod loader;

pub(crate) use self::call::*;
pub(crate) use self::eval::*;
pub(crate) use self::expr::*;
pub(crate) use self::io::*;
pub(crate) use self::layout::*;

pub const MAX_CALL_DEPTH: usize = 1000;

/// 分配 n 字节零初始化内存；n ≤ 0 → 空切片（保留旧行为）；n 超出可表示容量 /
/// 底层分配失败 → None（调用方转 `error.OutOfMemory`——`vec![0u8; n]` 对超大 n
/// 会直接中止进程，而 H 的分配失败应是可 catch 的 error union 值，非 panic）
pub(crate) fn alloc_zeroed_bytes(n: i128) -> Option<Vec<u8>> {
    if n <= 0 {
        return Some(Vec::new());
    }
    if n as u128 > usize::MAX as u128 {
        return None;
    }
    let mut v = Vec::new();
    v.try_reserve_exact(n as usize).ok()?;
    v.resize(n as usize, 0u8);
    Some(v)
}

/// 组 F：四模式共享容器类型名（OneToOne/OneToMany/ManyToOne/ManyToMany——内建泛型
/// 共享容器，写者数量由类型名保证；协作式单线程下四变体运行时行为相同）
pub(crate) fn is_four_mode_type(name: &str) -> bool {
    matches!(name, "OneToOne" | "OneToMany" | "ManyToOne" | "ManyToMany")
}

/// 渲染类型为源码串（E1 `types.fields` 元数据 + 诊断用）——对齐 06-02 类型语法
#[allow(dead_code)]
pub(crate) fn fmt_type_str(t: &Type) -> String {
    match t.strip() {
        Type::ComptimeInt(v) => format!("{v}"),
        Type::Named(n, args) => {
            if args.is_empty() {
                n.clone()
            } else {
                let inner: Vec<String> = args.iter().map(fmt_type_str).collect();
                format!("{n}({})", inner.join(", "))
            }
        }
        Type::Ptr(inner, mut_) => {
            if *mut_ {
                format!("*mut {}", fmt_type_str(inner))
            } else {
                format!("*{}", fmt_type_str(inner))
            }
        }
        Type::Slice(inner, mut_) => {
            if *mut_ {
                format!("&mut [{}]", fmt_type_str(inner))
            } else {
                format!("&[{}]", fmt_type_str(inner))
            }
        }
        Type::Optional(inner) => format!("?{}", fmt_type_str(inner)),
        Type::ErrorUnion(err, ok) => match err {
            Some(e) => format!("{}!{}", fmt_type_str(e), fmt_type_str(ok)),
            None => format!("!{}", fmt_type_str(ok)),
        },
        Type::Tuple(items) => {
            let inner: Vec<String> = items.iter().map(fmt_type_str).collect();
            format!("({})", inner.join(", "))
        }
        Type::Array(n, inner) => format!("[{n}]{}", fmt_type_str(inner)),
        Type::Infer => "_".to_string(),
        Type::Owned(inner) => fmt_type_str(inner),
    }
}

#[derive(Debug, Clone)]
pub struct RtError {
    pub name: String,
    pub span: Option<Span>,
    pub message: String,
    /// M4.2：错误码（M2.6 表「包 ID + 包内码」；根作用域报告输出）
    pub code: Option<u32>,
    /// 内部控制流信号（跨 eval 边界传播 return/break/continue——
    /// `catch return x` / `orelse continue` / switch 臂内 return 等）
    /// 仅模块内使用（不暴露于公共 API，故非 `pub`）
    signal: Option<Flow>,
}

impl RtError {
    pub fn new(name: &str, span: Option<Span>) -> Self {
        Self {
            name: name.to_string(),
            span,
            message: String::new(),
            code: None,
            signal: None,
        }
    }
    pub fn msg(name: &str, message: impl Into<String>) -> Self {
        Self {
            name: name.to_string(),
            span: None,
            message: message.into(),
            code: None,
            signal: None,
        }
    }
    /// 内部控制流信号（非用户可见错误）：携带 return/break/continue 流
    fn signal(flow: Flow) -> Self {
        Self {
            name: "__ctrl_flow__".into(),
            span: None,
            message: String::new(),
            code: None,
            signal: Some(flow),
        }
    }
    fn is_signal(&self) -> bool {
        self.signal.is_some()
    }
    /// M4.2：附加错误码（根作用域报告用）
    pub fn with_code(mut self, code: u32) -> Self {
        self.code = Some(code);
        self
    }
    pub fn render(&self, source: &str) -> String {
        let _ = source;
        let code_str = self
            .code
            .map(|c| format!(" (0x{c:08X})"))
            .unwrap_or_default();
        match &self.span {
            // M2.6：错误报告以原始错误位置为前提（错误名 + 源码行列，不输出调用链）
            Some(s) => {
                if self.message.is_empty() {
                    format!("error.{}{} at {}:{}", self.name, code_str, s.line, s.col)
                } else {
                    format!(
                        "error.{}{} at {}:{}: {}",
                        self.name, code_str, s.line, s.col, self.message
                    )
                }
            }
            None => {
                if self.message.is_empty() {
                    format!("error.{}{}", self.name, code_str)
                } else {
                    format!("error.{}{}: {}", self.name, code_str, self.message)
                }
            }
        }
    }
}

type Result<T> = std::result::Result<T, RtError>;

/// 控制流信号
#[derive(Debug, Clone)]
pub(crate) enum Flow {
    None,
    Return(Value),
    /// 表达式值（switch 表达式臂 / 闭包单表达式体）——与 `Return`（语句 return）区分：
    /// 语句 return 必须向上传播到函数边界，表达式值就地消费
    Value(Value),
    /// 带标签 break/continue（`:label`）：`None` = 无标签（最内层循环）
    Break(Option<String>),
    Continue(Option<String>),
}

/// 作用域
pub(crate) struct Scope {
    vars: HashMap<String, Rc<RefCell<Value>>>,
    defers: Vec<DeferEntry>,
}

pub(crate) struct DeferEntry {
    expr: Expr,
    errdefer: bool,
}

impl Scope {
    fn new() -> Self {
        Self {
            vars: HashMap::new(),
            defers: Vec::new(),
        }
    }
}

/// 函数定义（含重载登记：同名单参数数不同）
#[derive(Clone)]
pub(crate) struct FnDef {
    name: String,
    params: Vec<Param>,
    #[allow(dead_code)] // tag1：返回类型参与重载选择归 M2 期望类型传播
    ret: Option<Type>,
    body: Block,
    is_test: bool,
    /// `[test("名称")]`：测试显示名（省略时显示函数名）
    test_name: Option<String>,
    /// D1：`[test(async)]` / `[test(thread)]` 测试模式
    test_mode: TestMode,
    /// D1：`[test(timeout=5)]` 测试超时（秒）
    test_timeout: Option<u64>,
    #[allow(dead_code)] // 类型方法标记（tag1：方法经注入 self 路径调用）
    method_of: Option<String>,
    /// 组 E E2：`async fn` 标记——调用点返回 `Future(R)`（延迟执行），await 运行体
    is_async: bool,
    /// A1（ADR-0020）：`extern fn`——纯声明（无 body，链接期解析外部 C 符号）。
    /// 解释器拒绝调用。
    is_extern: bool,
    span: Span,
}

/// 类型定义
#[derive(Clone)]
#[allow(dead_code)] // tag1：接口/枚举元数据留待 M2 类型检查完整实现
enum TypeDef {
    Class {
        ifaces: Vec<Type>,
        traits: Vec<Trait>,
        fields: Vec<FieldDecl>,
        methods: Vec<Method>,
        /// struct（连续内存值类型）= true；class（可能非连续）= false
        is_struct: bool,
    },
    Enum {
        variants: Vec<EnumVariant>,
    },
    /// K1（ADR-0014）：无标签 union——字段内存重叠；运行时值以
    /// `Value::Class` + `@union` 标记表示，写字段同步重解释所有字段
    Union {
        fields: Vec<FieldDecl>,
    },
    Interface {
        supers: Vec<Type>,
    },
}

/// G3（E3.2 ipc）：匿名管道——读写两端共享同一缓冲（写端写追加、读端读排空；
/// 协作式模型下 read 不阻塞，空缓冲返回空切片；writer_open 标记写端关闭）
struct Pipe {
    buf: Vec<u8>,
    writer_open: bool,
}

/// G3（E3.2 ipc）：命名共享内存——进程内注册表形态（定长字节区，id 定位；
/// write 覆盖内容截断到 size，read 取当前内容）
struct Shm {
    data: Vec<u8>,
}

/// G4（E3.3 storage）：文件持久化的键值存储（KvStore 值持 store id → 注册表）。
/// 内存中持有 entries；`close` 时以二进制格式落盘（u32 键长 + 键 + u32 值长 + 值，
/// 均小端）——缺文件视为空库，close 即建。
pub(crate) struct KvStore {
    path: String,
    entries: HashMap<Vec<u8>, Vec<u8>>,
}

// G4/G5 纯函数共享层（ADR-0004 语义唯一源）：正则引擎 / RLE / xorshift64 移入 hc crate
// （hc::regex / hc::rng），interp 与 IR 后端共用同一实现，消除重复。
use hc::regex::{parse_regex, RegexMatcher};
use hc::rng::xorshift64;

pub struct Interp {
    pub source: String,
    funcs: HashMap<String, Vec<FnDef>>,
    types: HashMap<String, TypeDef>,
    globals: HashMap<String, Rc<RefCell<Value>>>,
    scopes: Vec<Scope>,
    call_depth: usize,
    /// 测试运行输出
    pub test_out: Vec<String>,
    /// 断言失败信息
    fail_info: Option<String>,
    /// 期望返回类型（M2 期望类型传播：调用点目标类型已知时参与重载选择）
    expected_ret: Option<String>,
    /// 当前函数声明的返回类型（期望类型传播：return 上下文参与重载选择）
    current_ret: Option<Type>,
    /// 当前正在执行的 main（供入口错误报告）
    in_main: bool,
    /// io.exit 请求的退出码（M4.2：ExitType + code 映射）
    pub exit_code: Option<u8>,
    /// 临时字段槽（tag1：&p.x 简化为值快照）
    tmp_field_cells: Vec<Rc<RefCell<Value>>>,
    /// 文件句柄注册表（M5.4 真实 IO：File 值持 fd → 真实 std::fs::File）
    files: HashMap<i64, std::fs::File>,
    next_fd: i64,
    /// M5.4 io.net：TCP 流/监听器注册表（TcpConn/TcpListener 值持 fd）
    tcp_streams: HashMap<i64, std::net::TcpStream>,
    tcp_listeners: HashMap<i64, std::net::TcpListener>,
    /// G1（E3.1）：UDP socket 注册表（UdpSocket 值持 fd）
    udp_sockets: HashMap<i64, std::net::UdpSocket>,
    /// G2（io 差异项）：Dir 句柄注册表（fd → 目录路径；list_dir 时按路径重读）
    dirs: HashMap<i64, String>,
    /// G3（E3.2 ipc）：管道/共享内存注册表（Pipe 两端持同一 pipe_id；Shm 命名区域）
    pipes: HashMap<i64, Rc<RefCell<Pipe>>>,
    shms: HashMap<i64, Rc<RefCell<Shm>>>,
    next_pipe_fd: i64,
    next_shm_fd: i64,
    next_dir_fd: i64,
    next_net_fd: i64,
    /// G4（E3.3 storage）：键值存储注册表（KvStore 值持 store id）
    stores: HashMap<i64, Rc<RefCell<KvStore>>>,
    next_store_fd: i64,
    /// G5（E3.3 rng）：全局伪随机数状态（xorshift64；`io.rng.seed` 重置——协作式
    /// 单线程 Interp，全局态安全；命名空间类名 RngNs 避开示例 84-rng 的用户类 Rng）
    rng_state: u64,
    /// 程序参数（M5.4：io.args()；由 CLI 注入，默认取进程参数）
    pub args: Vec<String>,
    /// 错误名 → 首次出现位置（M2.6 错误码表；根作用域未处理错误报告定位用）
    error_locs: HashMap<String, Span>,
    /// M2.5/M4.7 Debug 悬垂标记：被取过地址的目标 cell 地址集合
    tracked: std::collections::HashSet<usize>,
    /// K4（ADR-0014）：`@intFromPtr` 登记的整数地址 → 可重建值（Ptr/Boxed 的 Rc）。
    /// `@ptrFromInt` 依此重建原指针（round-trip 保真）；未登记地址合成匿名槽（同地址
    /// 幂等——对齐原生 inttoptr 虚拟指针语义；interp 无真实物理内存，MMIO 地址 = 匿名槽）。
    addr_registry: std::collections::HashMap<usize, Value>,
    /// Debug 悬垂标记开关（Debug 默认开；Release 裸读，用户负责）
    debug_dangling: bool,
    /// G5/§8.3 Debug 泄漏检测：全局 alloc 分配记录表（`alloc.alloc(n)` 登记，
    /// 值销毁后弱引用失效自动视为释放；退出时仍存活者 = 泄漏）
    alloc_tracker: Rc<RefCell<Vec<LeakRecord>>>,
    /// M2.7 只读捕获强制（Phase 8）：当前执行闭包体内**只读**的捕获 cell 地址集合。
    /// 非 `mut` 闭包调用时压入其环境 cell 地址；写入这些 cell → ReadonlyCapture。
    /// 栈式（嵌套闭包叠压；仅直接重绑定被捕获变量受限——经指针/字段/索引写穿放行）。
    readonly_caps: Vec<usize>,
    /// M4.2 错误码运行时表示：错误名 → 码（编译期表 + 运行时动态扩展）
    error_codes: HashMap<String, u32>,
    /// 码（包内序）→ 错误名（反向表）
    error_names: Vec<String>,
    /// M1.4：同包兄弟文件（外部符号——跨文件语义检查用）
    extern_programs: Vec<Program>,
    /// M7.2：依赖包（包名 + 程序；跨包语义检查 + pub 过滤装载用）
    dep_programs: Vec<(String, Program)>,
    /// ADR-0010：import 环境别名（bound → io 族环境键；`import H.std.{io as my}` → my → io）
    import_env: HashMap<String, String>,
    /// E2.2 根回收队列：未 join/未 detach 的 Thread 在作用域退出时提升到根作用域，
    /// 程序（main / 全部测试）结束时运行到完成（副作用发生；无隐式阻塞）
    root_threads: Vec<Value>,
    /// E1（ADR-0013）：受限脚本模式——`script { }` 块装载期求值用。置位后
    /// io/alloc/stdout/stderr/argv/网络不可用（受限 H 核心子集），仅注入 `types`
    /// 元数据对象（Q23）。
    script_mode: bool,
    /// E1.2 组 D D3：具体化登记期进行中的具体化名集合（`Pair<@i32>` 键）。
    /// 自/互递归类型函数（`LinkedList(T) { next: ?LinkedList(T) }`）在登记期重入时
    /// 命中即返回键本身（叶），防止无限实例化。
    instantiating: Vec<String>,
    /// E1.2 组 D D4c：comptime 值函数调用深度（自递归守卫——`fn f(T: type)` 体再调
    /// `f<i32>` 无限编译期求值会栈溢出，超限报编译错误）。
    comptime_value_depth: usize,
    /// D1-4：装载的程序快照，供线程模式测试 fork 新 Interp
    program: Option<Arc<Program>>,
    /// E4 true-OMP：OS 线程注册表（线程 ID → 控制块）
    thread_handles: HashMap<i64, ThreadState>,
    /// E4：下一线程 ID（自增分配）
    next_tid: i64,
}

impl Flow {
    #[allow(dead_code)]
    fn as_value(self) -> Value {
        match self {
            Flow::Return(v) => v,
            Flow::Value(v) => v,
            Flow::None => Value::Void,
            Flow::Break(_) | Flow::Continue(_) => Value::Void,
        }
    }
}

/// 值是否为错误值（errdefer 错误路径判定）
pub(crate) fn value_is_err(v: &Value) -> bool {
    matches!(v, Value::Err { .. })
}

/// 类型参数是否含泛型占位（大写类型名 T/U 等，tag1 启发式）
/// 展平限定名链（io.net.double → "io.net.double"；根非 Ident → None——
/// 对象方法链如 conn.read 的根是实例名，不在函数表）
pub(crate) fn qualified_flat_name(base: &Expr, field: &str) -> Option<String> {
    let mut parts = vec![field.to_string()];
    let mut e = base;
    loop {
        match e {
            Expr::Dot {
                base: b, field: f, ..
            }
            | Expr::Field {
                base: b, field: f, ..
            } => {
                parts.push(f.clone());
                e = b.as_ref();
            }
            Expr::Ident(n, _) => {
                parts.push(n.clone());
                parts.reverse();
                return Some(parts.join("."));
            }
            _ => return None,
        }
    }
}

pub(crate) fn type_has_generic(t: &Type) -> bool {
    match t.strip() {
        Type::Named(n, _) => {
            n.chars().next().map_or(false, |c| c.is_uppercase())
                && !n.starts_with("String")
                && !n.starts_with("Vec")
                && !n.starts_with("Map")
                && !n.starts_with("Deque")
        }
        Type::Ptr(inner, _)
        | Type::Slice(inner, _)
        | Type::Optional(inner)
        | Type::Owned(inner) => type_has_generic(inner),
        Type::Tuple(items) => items.iter().any(type_has_generic),
        _ => false,
    }
}

/// 解析整数字面量（含进制、_ 分隔、宽度后缀）→ (值, 宽度名)
pub fn parse_int_text(text: &str) -> std::result::Result<(i128, String), RtError> {
    let t = text.trim();
    let (radix, digits): (u32, &str) = if let Some(rest) = t.strip_prefix("0x") {
        (16, rest)
    } else if let Some(rest) = t.strip_prefix("0X") {
        (16, rest)
    } else if let Some(rest) = t.strip_prefix("0b") {
        (2, rest)
    } else if let Some(rest) = t.strip_prefix("0B") {
        (2, rest)
    } else if let Some(rest) = t.strip_prefix("0o") {
        (8, rest)
    } else {
        (10, t)
    };
    // 分离后缀（已知宽度名：i8..i128/isize/u8..u128/usize/f16..f128）
    const SUFFIXES: [&str; 14] = [
        "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
        "f32", "f64",
    ];
    let mut split = digits.len();
    for sfx in SUFFIXES {
        if digits.len() > sfx.len() && digits.ends_with(sfx) {
            let cand = &digits[..digits.len() - sfx.len()];
            if cand.ends_with(|c: char| c.is_ascii_digit() || c == '_' || c.is_alphabetic()) {
                // 仅当前缀是纯数字（含进制前缀）时视为后缀
                if cand.chars().all(|c| c.is_ascii_digit() || c == '_') {
                    split = digits.len() - sfx.len();
                    break;
                }
            }
        }
    }
    let (digits, suffix) = digits.split_at(split);
    let cleaned: String = digits.replace('_', "");
    let n = i128::from_str_radix(&cleaned, radix)
        .map_err(|_| RtError::msg("BadInt", format!("invalid integer literal `{text}`")))?;
    Ok((n, suffix.to_string()))
}

/// 测试辅助：断言函数定义存在
pub fn count_fns(program: &Program) -> usize {
    fn count(decls: &[Decl]) -> usize {
        decls
            .iter()
            .map(|d| match d {
                Decl::Fn { .. } => 1,
                Decl::Namespace { decls, .. } => count(decls),
                _ => 0,
            })
            .sum()
    }
    count(&program.decls)
}
