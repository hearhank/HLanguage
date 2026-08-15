//! Tree-walking 解释器（M3.2 脚本模式 `hc run`——tag1 子集）
//!
//! tag1 采用作用域链环境 + 引用计数槽。字节码 VM（M3.2 完整）与 LLVM 原生
//! 后端（M3.3）留后续里程碑；本模块保证双模式承诺的「脚本模式」先行可用。

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Seek, Write};
use std::rc::Rc;

use hc::ast::*;
use hc::token::Span;

use crate::value::{ClosureData, Value};

pub const MAX_CALL_DEPTH: usize = 1000;

/// 运行时错误（tag1：错误名 + 位置；错误码表 M2.6 后续）
#[derive(Debug, Clone)]
pub struct RtError {
    pub name: String,
    pub span: Option<Span>,
    pub message: String,
    /// M4.2：错误码（M2.6 表「包 ID + 包内码」；根作用域报告输出）
    pub code: Option<u32>,
    /// 内部控制流信号（跨 eval 边界传播 return/break/continue——
    /// `catch return x` / `orelse continue` / switch 臂内 return 等）
    pub signal: Option<Flow>,
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
enum Flow {
    None,
    Return(Value),
    /// 表达式值（switch 表达式臂 / 闭包单表达式体）——与 `Return`（语句 return）区分：
    /// 语句 return 必须向上传播到函数边界，表达式值就地消费
    Value(Value),
    Break,
    Continue,
}

/// 作用域
struct Scope {
    vars: HashMap<String, Rc<RefCell<Value>>>,
    defers: Vec<DeferEntry>,
}

struct DeferEntry {
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
struct FnDef {
    name: String,
    params: Vec<Param>,
    #[allow(dead_code)] // tag1：返回类型参与重载选择归 M2 期望类型传播
    ret: Option<Type>,
    body: Block,
    is_test: bool,
    #[allow(dead_code)] // 类型方法标记（tag1：方法经注入 self 路径调用）
    method_of: Option<String>,
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
    },
    Enum {
        variants: Vec<EnumVariant>,
    },
    Interface {
        supers: Vec<Type>,
    },
}

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
    /// 程序参数（M5.4：io.args()；由 CLI 注入，默认取进程参数）
    pub args: Vec<String>,
    /// 错误名 → 首次出现位置（M2.6 错误码表；根作用域未处理错误报告定位用）
    error_locs: HashMap<String, Span>,
    /// M2.5/M4.7 Debug 悬垂标记：被取过地址的目标 cell 地址集合
    tracked: std::collections::HashSet<usize>,
    /// Debug 悬垂标记开关（Debug 默认开；Release 裸读，用户负责）
    debug_dangling: bool,
    /// M4.2 错误码运行时表示：错误名 → 码（编译期表 + 运行时动态扩展）
    error_codes: HashMap<String, u32>,
    /// 码（包内序）→ 错误名（反向表）
    error_names: Vec<String>,
}

impl Interp {
    pub fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
            funcs: HashMap::new(),
            types: HashMap::new(),
            globals: HashMap::new(),
            scopes: vec![Scope::new()],
            call_depth: 0,
            test_out: Vec::new(),
            fail_info: None,
            expected_ret: None,
            current_ret: None,
            in_main: false,
            exit_code: None,
            tmp_field_cells: Vec::new(),
            files: HashMap::new(),
            next_fd: 1,
            args: std::env::args().skip(1).collect(),
            error_locs: HashMap::new(),
            tracked: std::collections::HashSet::new(),
            debug_dangling: true,
            error_codes: HashMap::new(),
            error_names: Vec::new(),
        }
    }

    /// M2.5/M4.7：Debug 悬垂标记开关（Debug 默认开；Release 裸读，用户负责）
    pub fn set_debug_dangling(&mut self, on: bool) -> &mut Self {
        self.debug_dangling = on;
        self
    }

    /// M2.6/M4.2：从编译期错误码表记录——错误名 → 首次出现位置（同名保留首个）
    /// + 错误名 ↔ 码映射（运行时错误值携带码；未登记错误名动态分配）
    fn record_error_locs(&mut self, program: &Program) {
        let table = hc::error_code_table(program);
        for entry in table.entries() {
            self.error_locs
                .entry(entry.name.clone())
                .or_insert_with(|| entry.span.clone());
            self.error_codes
                .entry(entry.name.clone())
                .or_insert(entry.code);
        }
        // 反向表：码 → 名（按包内序对齐编译期表）
        for (name, code) in &self.error_codes {
            let idx = hc::ErrorCodeTable::index_of(*code) as usize;
            while self.error_names.len() <= idx {
                self.error_names.push(String::new());
            }
            self.error_names[idx] = name.clone();
        }
    }

    /// M4.2：错误名 → 错误值（码 = 编译期表；运行时未登记错误名动态分配，
    /// 沿用当前包 ID 高位——anyerror 任意码）
    fn err_val(&mut self, name: &str) -> Value {
        let code = match self.error_codes.get(name) {
            Some(c) => *c,
            None => {
                let pkg = hc::ErrorCodeTable::package_of(
                    self.error_codes.values().next().copied().unwrap_or(0),
                );
                let idx = self.error_names.len() as u16;
                let code = hc::ErrorCodeTable::encode(pkg, idx);
                self.error_codes.insert(name.to_string(), code);
                self.error_names.push(name.to_string());
                code
            }
        };
        Value::Err {
            name: name.to_string(),
            code,
        }
    }

    // ---------- 程序装载 ----------

    pub fn load(&mut self, program: &Program) -> Result<()> {
        // 第零遍：语义检查（M2 静态 pass——宽度/引用赋值/类型错误编译期报错）
        let diags = hc::check_semantics(program);
        if let Some(d) = diags.iter().find(|d| d.is_error()) {
            return Err(RtError::msg(
                "CompileError",
                format!("{}:{}: {}", d.span.line, d.span.col, d.message),
            ));
        }
        self.record_error_locs(program);
        // 第一遍：登记类型
        for d in &program.decls {
            self.register_type_decl(d)?;
        }
        // 内建枚举（M4.2 L3）：ExitType{ Exit, Error }
        self.types
            .entry("ExitType".to_string())
            .or_insert(TypeDef::Enum {
                variants: vec![
                    EnumVariant {
                        name: "Exit".into(),
                        payload: None,
                        span: Span::new(0, 0, 0, 0),
                    },
                    EnumVariant {
                        name: "Error".into(),
                        payload: None,
                        span: Span::new(0, 0, 0, 0),
                    },
                ],
            });
        // 第二遍：登记函数（含类型方法）
        for d in &program.decls {
            self.register_fn_decl(d)?;
        }
        // 第三遍：global / const 初始化 + 执行 namespace 内声明（tag1：扁平化）
        for d in &program.decls {
            self.exec_decl_top(d)?;
        }
        // 第四遍：`using NS;` 别名解析（M1.4/Q21）——限定名 → 扁平名导入
        self.apply_usings(program);
        Ok(())
    }

    /// M1.4：`using NS;` 导入命名空间函数为扁平名（文件自身定义优先；
    /// 同包跨命名空间 using 即达；`using NS as M` 等价重命名前缀）
    fn apply_usings(&mut self, program: &Program) {
        for d in &program.decls {
            self.collect_using(d);
        }
    }

    fn collect_using(&mut self, d: &Decl) {
        match d {
            Decl::Using { path, alias, .. } => {
                let prefix = path.join(".");
                let qp = format!("{prefix}.");
                let keys: Vec<String> = self
                    .funcs
                    .keys()
                    .filter(|k| k.starts_with(&qp))
                    .cloned()
                    .collect();
                for k in keys {
                    let member = k[qp.len()..].to_string();
                    let flat = match alias {
                        Some(a) => format!("{a}.{member}"),
                        None => member,
                    };
                    // 文件自身定义优先：扁平名已存在则不覆盖
                    if !self.funcs.contains_key(&flat) {
                        let defs = self.funcs.get(&k).cloned().unwrap_or_default();
                        if !defs.is_empty() {
                            self.funcs.entry(flat).or_default().extend(defs);
                        }
                    }
                }
            }
            Decl::Namespace { decls, .. } => {
                for inner in decls {
                    self.collect_using(inner);
                }
            }
            _ => {}
        }
    }

    /// M1.4：加载同包兄弟文件声明（符号登记），跳过其 test 与 main（入口/测试归属目标文件）
    pub fn load_siblings(&mut self, programs: &[&Program]) -> Result<()> {
        for p in programs {
            self.record_error_locs(p);
            for d in &p.decls {
                self.register_type_decl(d)?;
            }
        }
        for p in programs {
            for d in &p.decls {
                self.register_fn_decl_skip_entry(d)?;
            }
        }
        for p in programs {
            for d in &p.decls {
                self.exec_decl_top(d)?;
            }
        }
        Ok(())
    }

    fn register_type_decl(&mut self, d: &Decl) -> Result<()> {
        self.register_type_decl_prefixed(d, "")
    }

    /// 类型登记（Q21 命名空间）：扁平名 + 限定名双注册。
    /// 扁平名（`Line`）供包内直接引用；限定名（`Orders.Line`）供
    /// `Vec(Orders.Line)` / `Orders.Line{...}` 限定访问（M1.4）。
    fn register_type_decl_prefixed(&mut self, d: &Decl, prefix: &str) -> Result<()> {
        match d {
            Decl::Class {
                name,
                ifaces,
                traits,
                fields,
                methods,
                ..
            } => {
                let qname = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                let type_def = TypeDef::Class {
                    ifaces: ifaces.clone(),
                    traits: traits.clone(),
                    fields: fields.clone(),
                    methods: methods.clone(),
                };
                self.types.insert(name.clone(), type_def.clone());
                if !prefix.is_empty() {
                    self.types.insert(qname.clone(), type_def);
                }
                // 类型方法登记：Type.method / Orders.Type.method —— 首参 self 由调用点注入
                for m in methods {
                    let fname = format!("{qname}.{}", m.name);
                    self.funcs.entry(fname.clone()).or_default().push(FnDef {
                        name: fname,
                        params: m.params.clone(),
                        ret: m.ret.clone(),
                        body: m.body.clone(),
                        is_test: false,
                        method_of: Some(qname.clone()),
                        span: m.span.clone(),
                    });
                }
            }
            Decl::Enum { name, variants, .. } => {
                let type_def = TypeDef::Enum {
                    variants: variants.clone(),
                };
                self.types.insert(name.clone(), type_def.clone());
                if !prefix.is_empty() {
                    self.types.insert(format!("{prefix}.{name}"), type_def);
                }
            }
            Decl::Interface { name, supers, .. } => {
                let type_def = TypeDef::Interface {
                    supers: supers.clone(),
                };
                self.types.insert(name.clone(), type_def.clone());
                if !prefix.is_empty() {
                    self.types.insert(format!("{prefix}.{name}"), type_def);
                }
            }
            Decl::Namespace { name, decls, .. } => {
                let new_prefix = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                for inner in decls {
                    self.register_type_decl_prefixed(inner, &new_prefix)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn register_fn_decl(&mut self, d: &Decl) -> Result<()> {
        self.register_fn_decl_prefixed(d, "")
    }

    /// 兄弟文件函数注册：跳过 test fn 与 main（M1.4 包加载）
    fn register_fn_decl_skip_entry(&mut self, d: &Decl) -> Result<()> {
        self.register_fn_decl_prefixed_filter(d, "", true)
    }

    /// 函数注册（Q21 命名空间）：扁平名 + 限定名双注册。
    /// 扁平名（`square`）供 `using Math;` 后直接调用；限定名（`Math.square`）
    /// 供 `Math.square(5)` 静态调用（eval_call Dot 分支经 funcs 命中）。
    fn register_fn_decl_prefixed(&mut self, d: &Decl, prefix: &str) -> Result<()> {
        self.register_fn_decl_prefixed_filter(d, prefix, false)
    }

    fn register_fn_decl_prefixed_filter(
        &mut self,
        d: &Decl,
        prefix: &str,
        skip_entry: bool,
    ) -> Result<()> {
        match d {
            Decl::Fn {
                name,
                params,
                ret,
                body,
                is_test,
                span,
                ..
            } => {
                // 兄弟文件：不登记 test fn（测试归属目标文件）与 main（入口归属目标文件）
                if skip_entry && (*is_test || name == "main") {
                    return Ok(());
                }
                // 兄弟文件（skip_entry）：顶层函数不注册（文件私有，避免跨文件污染
                // 同名重载池，如 64/74 各自 describe）；命名空间函数只注册限定名
                // （扁平名由目标文件 `using NS;` 导入）。自身文件：扁平 + 限定双注册。
                if skip_entry && prefix.is_empty() {
                    return Ok(());
                }
                let fdef = FnDef {
                    name: name.clone(),
                    params: params.clone(),
                    ret: ret.clone(),
                    body: body.clone(),
                    is_test: *is_test,
                    method_of: None,
                    span: span.clone(),
                };
                if !skip_entry {
                    self.funcs
                        .entry(name.clone())
                        .or_default()
                        .push(fdef.clone());
                }
                if !prefix.is_empty() {
                    let qname = format!("{prefix}.{name}");
                    self.funcs.entry(qname).or_default().push(fdef);
                }
            }
            Decl::Namespace { name, decls, .. } => {
                let new_prefix = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                for inner in decls {
                    self.register_fn_decl_prefixed_filter(inner, &new_prefix, skip_entry)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn exec_decl_top(&mut self, d: &Decl) -> Result<()> {
        match d {
            Decl::Global { name, init, .. } => {
                let v = match init {
                    Some(e) => self.eval(e)?,
                    None => Value::Void,
                };
                self.globals.insert(name.clone(), Rc::new(RefCell::new(v)));
            }
            Decl::Const { name, init, ty, .. } => {
                // 错误集类型别名：注册为“错误集”类型占位
                if let Some(Type::Named(tn, _)) = ty {
                    if tn.starts_with("error_set:") {
                        self.types
                            .insert(name.clone(), TypeDef::Interface { supers: vec![] });
                        return Ok(());
                    }
                }
                let v = self.eval(init)?;
                self.globals.insert(name.clone(), Rc::new(RefCell::new(v)));
            }
            Decl::Namespace { decls, .. } => {
                for inner in decls {
                    self.exec_decl_top(inner)?;
                }
            }
            Decl::Using { path, .. } => {
                // tag1：using 无操作（模块扁平化；跨包解析归 M1.4/M7.2）
                let _ = path;
            }
            Decl::Script { .. } => {
                // E1：第三块实现；tag1 不执行
            }
            _ => {}
        }
        Ok(())
    }

    // ---------- 作用域 ----------

    fn push_scope(&mut self) {
        self.scopes.push(Scope::new());
    }

    fn pop_scope(&mut self) -> Result<()> {
        let scope = self.scopes.pop().expect("scope stack underflow");
        // M2.5/M4.7 Debug 悬垂标记：作用域退出 = 目标销毁（LIFO）→ 把被取过地址的
        // 目标 cell 内容标记为 Dangling（有指针持有的 cell 不释放、地址唯一——
        // 无地址碰撞误判；Release 关闭时不标记）
        if self.debug_dangling {
            for (name, cell) in &scope.vars {
                let _ = name;
                let addr = Rc::as_ptr(cell) as usize;
                if self.tracked.remove(&addr) {
                    *cell.borrow_mut() = Value::Dangling;
                }
            }
        }
        self.run_defers(scope, false)
    }

    fn run_defers(&mut self, scope: Scope, err_path: bool) -> Result<()> {
        // LIFO（Q21：后声明先执行）
        let mut err = None;
        for entry in scope.defers.iter().rev() {
            if entry.errdefer && !err_path {
                continue;
            }
            if let Err(e) = self.eval(&entry.expr) {
                err = Some(e);
            }
        }
        match err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    fn bind(&mut self, name: &str, v: Value) -> Rc<RefCell<Value>> {
        let cell = Rc::new(RefCell::new(v));
        self.scopes
            .last_mut()
            .unwrap()
            .vars
            .insert(name.to_string(), cell.clone());
        cell
    }

    fn lookup(&self, name: &str) -> Option<Rc<RefCell<Value>>> {
        for s in self.scopes.iter().rev() {
            if let Some(v) = s.vars.get(name) {
                return Some(v.clone());
            }
        }
        self.globals.get(name).cloned()
    }

    // ---------- 语句 ----------

    pub fn exec_fn_body(&mut self, body: &Block, params: &[(String, Value)]) -> Result<Value> {
        if self.call_depth >= MAX_CALL_DEPTH {
            return Err(RtError::msg("StackOverflow", "maximum call depth exceeded"));
        }
        self.call_depth += 1;
        self.push_scope();
        for (name, v) in params {
            self.bind(name, v.clone());
        }
        let result = self.exec_block_inner(body);
        let _ = self.pop_scope();
        self.call_depth -= 1;
        match result {
            Ok(Flow::Return(v)) => Ok(v),
            Ok(Flow::Value(v)) => Ok(v),
            Ok(Flow::None) => Ok(Value::Void),
            Ok(Flow::Break) | Ok(Flow::Continue) => Err(RtError::msg(
                "InvalidControlFlow",
                "break/continue outside loop",
            )),
            Err(e) if e.is_signal() => match e.signal {
                Some(Flow::Return(v)) => Ok(v),
                Some(Flow::Value(v)) => Ok(v),
                Some(Flow::Break) | Some(Flow::Continue) => Err(RtError::msg(
                    "InvalidControlFlow",
                    "break/continue outside loop",
                )),
                _ => Err(e),
            },
            Err(e) => Err(e),
        }
    }

    fn exec_block(&mut self, b: &Block) -> Result<Flow> {
        self.push_scope();
        let r = self.exec_block_inner(b);
        let _ = self.pop_scope();
        r
    }

    fn exec_block_inner(&mut self, b: &Block) -> Result<Flow> {
        for stmt in &b.stmts {
            let f = self.exec_stmt(stmt)?;
            if !matches!(f, Flow::None) {
                return Ok(f);
            }
        }
        Ok(Flow::None)
    }

    fn exec_stmt(&mut self, s: &Stmt) -> Result<Flow> {
        match s {
            Stmt::Empty => Ok(Flow::None),
            Stmt::Block(b) => self.exec_block(b),
            Stmt::VarDecl {
                name,
                mut_,
                ty,
                init,
                span: _,
            } => {
                // 期望类型传播（M2 定案）：目标类型已知时优先返回类型匹配的重载
                let prev_expected = self.expected_ret.clone();
                if let Some(t) = ty {
                    if let Type::Named(tn, _) = t.strip() {
                        self.expected_ret = Some(tn.clone());
                    }
                }
                let mut v = match init {
                    Some(e) => self.eval(e)?,
                    None => self.default_value(ty.as_ref())?,
                };
                self.expected_ret = prev_expected;
                let _ = mut_;
                // [continuous] 值语义：目标类型连续时赋值即复制（显式标注或源类型可查）
                let continuous = match ty {
                    Some(t) => match t.strip() {
                        Type::Named(tn, _) => self.type_is_continuous(tn),
                        _ => false,
                    },
                    None => match init {
                        // var p2 = p1（p1 为连续类型值）
                        Some(Expr::Ident(src, _)) => match self.lookup(src) {
                            Some(cell) => match &*cell.borrow() {
                                Value::Class(c) => {
                                    let cname = c.borrow().name.clone();
                                    self.type_is_continuous(&cname)
                                }
                                _ => false,
                            },
                            None => false,
                        },
                        _ => false,
                    },
                };
                if continuous {
                    v = self.deep_copy(v);
                }
                self.bind(name, v);
                Ok(Flow::None)
            }
            Stmt::ConstDecl { name, init, .. } => {
                let v = self.eval(init)?;
                self.bind(name, v);
                Ok(Flow::None)
            }
            Stmt::Expr(e) => {
                self.eval(e)?;
                Ok(Flow::None)
            }
            Stmt::If(ifs) => self.exec_if(ifs),
            Stmt::While(w) => self.exec_while(w),
            Stmt::For(f) => self.exec_for(f),
            Stmt::Switch(sw) => match self.exec_switch(sw)? {
                // 语句级 switch：表达式臂值丢弃；语句 return/break/continue 原样传播
                Flow::Value(_) => Ok(Flow::None),
                other => Ok(other),
            },
            Stmt::Return(e, _) => {
                // 期望类型传播：return 上下文用当前函数返回类型参与重载选择
                let prev_expected = self.expected_ret.clone();
                if self.expected_ret.is_none() {
                    if let Some(rt) = &self.current_ret {
                        match rt.strip() {
                            Type::ErrorUnion(_, inner) => match inner.strip() {
                                Type::Named(n, _) => self.expected_ret = Some(n.clone()),
                                _ => {}
                            },
                            Type::Named(n, _) => self.expected_ret = Some(n.clone()),
                            _ => {}
                        }
                    }
                }
                let v = match e {
                    Some(e) => self.eval(e)?,
                    None => Value::Void,
                };
                self.expected_ret = prev_expected;
                Ok(Flow::Return(v))
            }
            Stmt::Break(_, _) => Ok(Flow::Break),
            Stmt::Continue(_, _) => Ok(Flow::Continue),
            Stmt::Defer(e, _) => {
                self.scopes.last_mut().unwrap().defers.push(DeferEntry {
                    expr: e.clone(),
                    errdefer: false,
                });
                Ok(Flow::None)
            }
            Stmt::Errdefer(e, _) => {
                self.scopes.last_mut().unwrap().defers.push(DeferEntry {
                    expr: e.clone(),
                    errdefer: true,
                });
                Ok(Flow::None)
            }
        }
    }

    /// 类型是否为 [continuous] 连续内存
    fn type_is_continuous(&self, tn: &str) -> bool {
        match self.types.get(tn) {
            Some(TypeDef::Class { traits, .. }) => {
                traits.iter().any(|tr| matches!(tr, Trait::Continuous))
            }
            _ => false,
        }
    }

    /// M5.4：Io 实例（含 fs/time 子模块；fs = 路径式文件 API，time = 毫秒时钟）
    fn io_value(&self) -> Value {
        let mut f = HashMap::new();
        f.insert("fs".into(), Value::class("Fs", HashMap::new()));
        f.insert("time".into(), Value::class("Time", HashMap::new()));
        Value::class("Io", f)
    }

    fn default_value(&self, ty: Option<&Type>) -> Result<Value> {
        match ty {
            None => Ok(Value::Void),
            Some(t) => match t.strip() {
                Type::Named(n, _) => match n.as_str() {
                    "i8" | "i16" | "i32" | "i64" | "i128" | "isize" => Ok(Value::Int(0)),
                    "u8" | "u16" | "u32" | "u64" | "u128" | "usize" => Ok(Value::Int(0)),
                    "f32" | "f64" | "f16" | "f128" => Ok(Value::Float(0.0)),
                    "bool" => Ok(Value::Bool(false)),
                    "void" => Ok(Value::Void),
                    "String" | "&[u8]" => Ok(Value::str("")),
                    "Vec" | "Deque" => Ok(Value::arr(vec![])),
                    "Map" => Ok(Value::class("Map", HashMap::new())),
                    _ => {
                        // Vec(T) / Map 集合类型
                        if n == "Vec" {
                            return Ok(Value::arr(vec![]));
                        }
                        if n == "Map" {
                            return Ok(Value::class("Map", HashMap::new()));
                        }
                        // 命名类型：class / enum 空实例
                        match self.types.get(n) {
                            Some(TypeDef::Class { fields, .. }) => {
                                let mut f = HashMap::new();
                                for fd in fields {
                                    f.insert(fd.name.clone(), self.default_value(Some(&fd.ty))?);
                                }
                                Ok(Value::class(n, f))
                            }
                            Some(TypeDef::Enum { .. }) => Ok(Value::Enum {
                                name: n.clone(),
                                variant: "__none__".into(),
                                payload: None,
                            }),
                            _ => Err(RtError::msg("UnknownType", format!("unknown type `{n}`"))),
                        }
                    }
                },
                Type::Optional(_) => Ok(Value::Opt(None)),
                Type::Ptr(_, _) => Ok(Value::Void),
                Type::Slice(_, _) => Ok(Value::str("")),
                Type::Infer | Type::Owned(_) => Ok(Value::Void),
                _ => Ok(Value::Void),
            },
        }
    }

    fn exec_if(&mut self, ifs: &IfStmt) -> Result<Flow> {
        let cond = self.eval(&ifs.cond)?;
        // optional 捕获：if (maybe) |v| { ... }——Some 绑定 v，None 走 else
        if let Some((_, name)) = &ifs.capture {
            match self.deref_value(cond) {
                Value::Opt(Some(v)) => {
                    self.push_scope();
                    self.bind(name, (*v).clone());
                    let r = self.exec_block(&ifs.then_b);
                    let _ = self.pop_scope();
                    return r;
                }
                Value::Opt(None) => {
                    if let Some(else_b) = &ifs.else_b {
                        return self.exec_stmt(else_b);
                    }
                    return Ok(Flow::None);
                }
                other => {
                    self.push_scope();
                    self.bind(name, other);
                    let r = self.exec_block(&ifs.then_b);
                    let _ = self.pop_scope();
                    return r;
                }
            }
        }
        if cond.as_bool() {
            self.exec_block(&ifs.then_b)
        } else if let Some(else_b) = &ifs.else_b {
            self.exec_stmt(else_b)
        } else {
            Ok(Flow::None)
        }
    }

    fn exec_while(&mut self, w: &WhileStmt) -> Result<Flow> {
        loop {
            let cond = self.eval(&w.cond)?;
            if !cond.as_bool() {
                return Ok(Flow::None);
            }
            self.push_scope();
            let r = self.exec_block_inner(&w.body);
            let _ = self.pop_scope();
            match r {
                Ok(Flow::Break) => return Ok(Flow::None),
                Ok(Flow::Continue) => {}
                Ok(Flow::Return(v)) => return Ok(Flow::Return(v)),
                Ok(Flow::Value(_)) => {}
                Ok(Flow::None) => {}
                // `orelse continue` / `catch break` 等表达式内信号 → 恢复为流
                Err(e) if e.is_signal() => match e.signal {
                    Some(Flow::Break) => return Ok(Flow::None),
                    Some(Flow::Continue) => {}
                    Some(Flow::Return(v)) => return Ok(Flow::Return(v)),
                    _ => return Err(e),
                },
                Err(e) => return Err(e),
            }
            if let Some(step) = &w.step {
                self.eval(step)?;
            }
        }
    }

    fn exec_for(&mut self, f: &ForStmt) -> Result<Flow> {
        let iter = self.eval(&f.iter)?;
        // 展开可迭代对象
        let items: Vec<(Rc<RefCell<Value>>, bool)> = self.iter_items(&iter)?;
        for (cell, is_ref) in items {
            self.push_scope();
            match f.capture {
                CaptureMode::Read => {
                    if is_ref {
                        // 只读捕获：绑定值副本（不写回）
                        let v = cell.borrow().clone();
                        self.bind(&f.capture_name, v);
                    } else {
                        self.bind(&f.capture_name, cell.borrow().clone());
                    }
                }
                CaptureMode::Mut | CaptureMode::Move => {
                    // 可写捕获：绑定共享槽（写回原数组）
                    self.scopes
                        .last_mut()
                        .unwrap()
                        .vars
                        .insert(f.capture_name.clone(), cell);
                }
            }
            let r = self.exec_block_inner(&f.body);
            let _ = self.pop_scope();
            match r {
                Ok(Flow::Break) => return Ok(Flow::None),
                Ok(Flow::Continue) => continue,
                Ok(Flow::Return(v)) => return Ok(Flow::Return(v)),
                Ok(Flow::Value(_)) => {}
                Ok(Flow::None) => {}
                // `orelse continue` 等表达式内信号 → 恢复为流
                Err(e) if e.is_signal() => match e.signal {
                    Some(Flow::Break) => return Ok(Flow::None),
                    Some(Flow::Continue) => continue,
                    Some(Flow::Return(v)) => return Ok(Flow::Return(v)),
                    _ => return Err(e),
                },
                Err(e) => return Err(e),
            }
        }
        Ok(Flow::None)
    }

    /// 返回迭代项列表：(共享槽, 是否源容器引用)
    fn iter_items(&mut self, v: &Value) -> Result<Vec<(Rc<RefCell<Value>>, bool)>> {
        let deref = self.deref_value(v.clone());
        match &deref {
            Value::Arr(a) => Ok(a.borrow().iter().map(|c| (c.clone(), true)).collect()),
            Value::Slice { data, start, len } => {
                let d = data.borrow();
                Ok((0..*len).map(|i| (d[*start + i].clone(), true)).collect())
            }
            Value::Class(c) if c.borrow().name == "Map" => {
                // Map 遍历：键值对捕获（|kv| → kv.key / kv.value）
                let d = c.borrow();
                let items: Vec<Value> = d
                    .fields
                    .iter()
                    .map(|(k, v)| {
                        let mut f = HashMap::new();
                        f.insert("key".to_string(), Value::str(k));
                        f.insert("value".to_string(), v.clone());
                        Value::class("KV", f)
                    })
                    .collect();
                Ok(items
                    .into_iter()
                    .map(|v| (Rc::new(RefCell::new(v)), false))
                    .collect())
            }
            Value::Class(_c) => {
                // 用户类型迭代（IIterable 契约）：循环调用 next(self) 直到 null
                let mut items = Vec::new();
                loop {
                    let next_v = self.eval_next_method(&deref)?;
                    match next_v {
                        Value::Opt(Some(v)) => items.push((*v).clone()),
                        Value::Opt(None) => break,
                        Value::Void => break,
                        other => items.push(other),
                    }
                }
                Ok(items
                    .into_iter()
                    .map(|v| (Rc::new(RefCell::new(v)), false))
                    .collect())
            }
            Value::Str(s) => {
                let bytes: Vec<u8> = s.borrow().clone();
                Ok(bytes
                    .into_iter()
                    .map(|b| (Rc::new(RefCell::new(Value::Int(b as i128))), false))
                    .collect())
            }
            _ => Err(RtError::msg(
                "NotIterable",
                format!("value of type `{}` is not iterable", deref.type_name()),
            )),
        }
    }

    /// 调用用户类型迭代器的 next(self) 方法（IIterable 契约，tag1：next → ?T）
    fn eval_next_method(&mut self, v: &Value) -> Result<Value> {
        let type_name = v.type_name();
        let fname = format!("{type_name}.next");
        if !self.funcs.contains_key(&fname) {
            return Err(RtError::msg(
                "NotIterable",
                format!("type `{type_name}` has no `next` method (IIterable)"),
            ));
        }
        let self_v = v.clone();
        let vals = vec![self_v];
        let fdef = self.pick_fn(&fname, &vals)?;
        self.call_fn(&fdef, &vals, &Span::new(0, 0, 0, 0))
    }

    fn exec_switch(&mut self, sw: &SwitchStmt) -> Result<Flow> {
        let subject = self.eval(&sw.subject)?;
        let subject = self.deref_value(subject);
        for arm in &sw.arms {
            for pat in &arm.patterns {
                if self.match_pattern(&subject, pat)? {
                    return self.exec_switch_arm(arm, subject.clone());
                }
            }
        }
        if sw.has_else {
            for arm in &sw.arms {
                if arm
                    .patterns
                    .iter()
                    .any(|p| matches!(p, SwitchPattern::Else))
                {
                    return self.exec_switch_arm(arm, subject.clone());
                }
            }
        }
        Ok(Flow::None)
    }

    /// 执行 switch 臂；单表达式臂体（`int => |i| i`）作为 switch 表达式值返回
    fn exec_switch_arm(&mut self, arm: &SwitchArm, subject: Value) -> Result<Flow> {
        self.push_scope();
        if let Some((_, name)) = &arm.capture {
            // 枚举负载捕获：`int => |i| i` 中 i = 负载值
            let cap = match &subject {
                Value::Enum {
                    payload: Some(p), ..
                } => (**p).clone(),
                _ => subject.clone(),
            };
            self.bind(name, cap);
        }
        // 单表达式臂体：返回值（switch 表达式语义）——Flow::Value 区别于语句 return
        if arm.body.stmts.len() == 1 {
            if let Stmt::Expr(e) = &arm.body.stmts[0] {
                let v = self.eval(e);
                let _ = self.pop_scope();
                return match v {
                    Ok(val) => Ok(Flow::Value(val)),
                    Err(err) => Err(err),
                };
            }
        }
        let r = self.exec_block_inner(&arm.body);
        let _ = self.pop_scope();
        r
    }

    fn match_pattern(&self, subject: &Value, pat: &SwitchPattern) -> Result<bool> {
        match (subject, pat) {
            (Value::Enum { variant, .. }, SwitchPattern::Ident(s)) => Ok(variant == s),
            (Value::Int(i), SwitchPattern::Int(s)) => {
                let (n, _) = parse_int_text(s)?;
                Ok(*i == n)
            }
            (Value::Float(f), SwitchPattern::Float(s)) => {
                Ok(*f == s.replace('_', "").parse::<f64>().unwrap_or(f64::NAN))
            }
            (Value::Str(st), SwitchPattern::Str(s)) => Ok(*st.borrow() == s.as_bytes()),
            (Value::Int(c), SwitchPattern::Char(pc)) => Ok(*c == *pc as i128),
            (Value::Err { name, .. }, SwitchPattern::Error(pe)) => Ok(name == pe),
            (Value::Bool(b), SwitchPattern::Ident(s)) => {
                Ok((*b && s == "true") || (!*b && s == "false"))
            }
            (Value::Opt(None), SwitchPattern::Ident(s)) => Ok(s == "null"),
            _ => Ok(false),
        }
    }

    // ---------- 表达式 ----------

    /// 任意字节容器 → 字节（Str / Arr(Int) / Slice 视图；57-protocol-parse 长度前缀帧）
    fn value_bytes(&self, v: &Value) -> Option<Vec<u8>> {
        match v {
            Value::Str(s) => Some(s.borrow().clone()),
            Value::Arr(a) => Some(
                a.borrow()
                    .iter()
                    .map(|c| match c.borrow().clone() {
                        Value::Int(i) => i as u8,
                        _ => 0,
                    })
                    .collect(),
            ),
            Value::Slice { data, start, len } => {
                let d = data.borrow();
                let mut out = Vec::with_capacity(*len);
                for i in 0..*len {
                    match d[*start + i].borrow().clone() {
                        Value::Int(n) => out.push(n as u8),
                        _ => return None,
                    }
                }
                Some(out)
            }
            _ => None,
        }
    }

    fn deref_value(&self, v: Value) -> Value {
        match v {
            Value::Ptr(c) => c.borrow().clone(),
            other => other,
        }
    }

    /// M2.5/M4.7：仅检查悬垂（不解引用）——指针指向已销毁目标 → 抛错带位置
    fn check_dangling(&self, v: &Value, span: &Span) -> Result<()> {
        if self.debug_dangling {
            if let Value::Ptr(c) = v {
                if matches!(&*c.borrow(), Value::Dangling) {
                    return Err(RtError::new("DanglingPointer", Some(span.clone())));
                }
            }
        }
        Ok(())
    }

    /// M2.5/M4.7：解引用访问（带悬垂检查）——Debug 下访问已销毁目标
    /// 的指针 → 抛错带位置；Release（debug_dangling=false）裸读（用户负责）
    fn deref_checked(&self, v: Value, span: &Span) -> Result<Value> {
        self.check_dangling(&v, span)?;
        Ok(self.deref_value(v))
    }

    /// 捕获当前作用域链（闭包环境快照）
    fn capture_env(&self) -> Vec<std::collections::HashMap<String, Rc<RefCell<Value>>>> {
        self.scopes
            .iter()
            .map(|s| s.vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .collect()
    }

    /// 调用闭包返回 bool（filter 谓词）
    fn call_closure_bool(&mut self, c: &ClosureData, args: &[Value], span: &Span) -> Result<bool> {
        let v = self.call_closure(c, args, span)?;
        Ok(v.as_bool())
    }

    /// 调用闭包返回值（map 变换）
    fn call_closure_value(
        &mut self,
        c: &ClosureData,
        args: &[Value],
        span: &Span,
    ) -> Result<Value> {
        self.call_closure(c, args, span)
    }

    /// 调用闭包：绑定参数到捕获环境之上
    fn call_closure(&mut self, c: &ClosureData, arg_vals: &[Value], span: &Span) -> Result<Value> {
        if c.params.len() != arg_vals.len() {
            return Err(RtError::new("ArityMismatch", Some(span.clone())));
        }
        let saved = std::mem::take(&mut self.scopes);
        // 闭包无声明返回类型：隔离期望类型（避免借用外层函数返回类型）
        let saved_ret = self.current_ret.take();
        let mut scopes: Vec<Scope> = c
            .env
            .iter()
            .map(|m| Scope {
                vars: m.clone(),
                defers: Vec::new(),
            })
            .collect();
        scopes.push(Scope::new());
        self.scopes = scopes;
        for (p, v) in c.params.iter().zip(arg_vals.iter()) {
            self.bind(p, v.clone());
        }
        // 单表达式闭包体（|v| v + a）：求值作为返回值
        let r: Result<Flow> = if c.body.stmts.len() == 1 {
            if let Stmt::Expr(e) = &c.body.stmts[0] {
                self.eval(e).map(|v| Flow::Value(v))
            } else {
                self.exec_block_inner(&c.body)
            }
        } else {
            self.exec_block_inner(&c.body)
        };
        let _ = self.pop_scope();
        self.scopes = saved;
        self.current_ret = saved_ret;
        match r {
            Ok(Flow::Return(v)) => Ok(v),
            Ok(Flow::Value(v)) => Ok(v),
            Ok(Flow::None) => Ok(Value::Void),
            Ok(Flow::Break) | Ok(Flow::Continue) => {
                Err(RtError::new("InvalidControlFlow", Some(span.clone())))
            }
            Err(e) if e.is_signal() => match e.signal {
                Some(Flow::Return(v)) => Ok(v),
                Some(Flow::Value(v)) => Ok(v),
                Some(Flow::Break) | Some(Flow::Continue) => {
                    Err(RtError::new("InvalidControlFlow", Some(span.clone())))
                }
                _ => Err(e),
            },
            Err(e) => Err(e),
        }
    }

    pub fn eval(&mut self, e: &Expr) -> Result<Value> {
        match e {
            Expr::IntLit { text, .. } => {
                let (n, _) = parse_int_text(text)?;
                Ok(Value::Int(n))
            }
            Expr::FloatLit { text, .. } => {
                let t = text.trim_end_matches(|c: char| c.is_alphabetic());
                let f: f64 = t.replace('_', "").parse().map_err(|_| {
                    RtError::msg("BadFloat", format!("invalid float literal `{text}`"))
                })?;
                Ok(Value::Float(f))
            }
            Expr::StrLit { value, .. } => Ok(Value::str(value)),
            Expr::CharLit(c, _) => Ok(Value::Int(*c as i128)),
            Expr::BoolLit(b, _) => Ok(Value::Bool(*b)),
            Expr::NullLit(_) => Ok(Value::Opt(None)),
            Expr::VoidLit(_) => Ok(Value::Void),
            Expr::ErrorLit(name, _) => Ok(self.err_val(name)),
            Expr::Ident(name, span) => {
                // 隐式环境注入
                match name.as_str() {
                    "alloc" => return Ok(Value::Alloc),
                    "test_io" | "io" => return Ok(self.io_value()),
                    "stdout" | "stderr" => return Ok(self.io_value()),
                    "pi" => return Ok(Value::Float(std::f64::consts::PI)),
                    "Vec" | "Deque" => return Ok(Value::arr(vec![])),
                    "Map" => return Ok(Value::class("Map", HashMap::new())),
                    "Table" => return Ok(Value::arr(vec![])),
                    _ => {}
                }
                match self.lookup(name) {
                    Some(cell) => Ok(cell.borrow().clone()),
                    None => {
                        // 函数名作为值（FnRef：apply(square, 5) / var f = square）
                        if self.funcs.contains_key(name) {
                            Ok(Value::Fn(name.clone()))
                        } else {
                            Err(RtError::new("UndefinedName", Some(span.clone())))
                        }
                    }
                }
            }
            Expr::ArrayLit(items, _) => {
                let mut vals = Vec::new();
                for it in items {
                    vals.push(self.eval(it)?);
                }
                Ok(Value::arr(vals))
            }
            Expr::TupleLit(items, _) => Ok(Value::arr(
                items.iter().map(|e| self.eval(e)).collect::<Result<_>>()?,
            )),
            Expr::NamedLit { ty, fields, .. } => {
                // class 字面量构造 / enum 带负载字面量
                match self.types.get(ty) {
                    Some(TypeDef::Class { .. }) => {
                        let mut f = HashMap::new();
                        for (k, v) in fields {
                            f.insert(k.clone(), self.eval(v)?);
                        }
                        Ok(Value::class(ty, f))
                    }
                    Some(TypeDef::Enum { .. }) => {
                        // enum 变体字面量：Type{variant = payload}——单字段
                        if fields.len() == 1 {
                            let (variant, payload) = &fields[0];
                            let pv = self.eval(payload)?;
                            Ok(Value::Enum {
                                name: ty.clone(),
                                variant: variant.clone(),
                                payload: Some(Rc::new(pv)),
                            })
                        } else {
                            Err(RtError::msg(
                                "BadEnumLiteral",
                                "enum literal takes exactly one variant",
                            ))
                        }
                    }
                    _ => Err(RtError::msg("UnknownType", format!("unknown type `{ty}`"))),
                }
            }
            Expr::Dot { base, field, .. } => {
                // ExitType 内建枚举（L3/M4.2）：ExitType.Exit / ExitType.Error
                if let Expr::Ident(bname, _) = base.as_ref() {
                    if bname == "ExitType" {
                        return Ok(Value::Enum {
                            name: "ExitType".into(),
                            variant: field.clone(),
                            payload: None,
                        });
                    }
                }
                // 枚举常量 Type.name（base 为类型名）
                if let Expr::Ident(bname, _) = base.as_ref() {
                    if self.types.contains_key(bname) {
                        return Ok(Value::Enum {
                            name: bname.clone(),
                            variant: field.clone(),
                            payload: None,
                        });
                    }
                }
                // 推断枚举值字面量 .name（L1）：类型未知——用兜底枚举名
                if matches!(base.as_ref(), Expr::VoidLit(_)) {
                    return Ok(Value::Enum {
                        name: "__inferred__".into(),
                        variant: field.clone(),
                        payload: None,
                    });
                }
                let b = self.eval(base)?;
                self.eval_dot(b, field)
            }
            Expr::Field { base, field, span } => {
                let b = self.eval(base)?;
                self.check_dangling(&b, span)?;
                self.eval_field(b, field, span)
            }
            Expr::Index {
                base,
                indices,
                span,
            } => {
                let b = self.eval(base)?;
                self.check_dangling(&b, span)?;
                let b = self.deref_value(b);
                // 切片取段 &arr[1..3] / "abc"[0..2]：索引为 Range 表达式
                if indices.len() == 1 {
                    if let Expr::Binary(BinOp::Range, lo, hi, _) = &indices[0] {
                        let lo_v = self.eval(lo)?;
                        let lo_i = self.as_index(&lo_v, span)?;
                        let (hi_i, open_end) = match hi.as_ref() {
                            Expr::IntLit { text, .. } if text == "__end__" => (0usize, true),
                            other => {
                                let hv = self.eval(other)?;
                                (self.as_index(&hv, span)?, false)
                            }
                        };
                        if let Value::Arr(a) = &b {
                            let total = a.borrow().len();
                            let hi_i = if open_end { total } else { hi_i };
                            let len = hi_i.saturating_sub(lo_i);
                            if hi_i > total || lo_i > total {
                                return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                            }
                            return Ok(Value::Slice {
                                data: a.clone(),
                                start: lo_i,
                                len,
                            });
                        }
                        if let Value::Str(s) = &b {
                            let bytes = s.borrow().clone();
                            let hi_i = if open_end { bytes.len() } else { hi_i };
                            if hi_i > bytes.len() || lo_i > bytes.len() {
                                return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                            }
                            return Ok(Value::str_bytes(bytes[lo_i..hi_i].to_vec()));
                        }
                        // 切片再切片（57-protocol-parse：data[0..8]——data 是 &[u8] 参数）
                        if let Value::Slice { data, start, len } = &b {
                            let total = *len;
                            let hi_i = if open_end { total } else { hi_i };
                            if hi_i > total || lo_i > total {
                                return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                            }
                            return Ok(Value::Slice {
                                data: data.clone(),
                                start: *start + lo_i,
                                len: hi_i.saturating_sub(lo_i),
                            });
                        }
                    }
                }
                // 普通索引；多参索引 t[i, j] 仅 Table（嵌套 Arr）合法（M8 定案）
                match &b {
                    Value::Arr(a) => {
                        // 多参索引：行 → 列（Table 语义）
                        if indices.len() >= 2 {
                            let r = self.eval(&indices[0])?;
                            let c = self.eval(&indices[1])?;
                            let ri = self.as_index(&r, span)?;
                            let ci = self.as_index(&c, span)?;
                            let arr = a.borrow();
                            if ri >= arr.len() {
                                return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                            }
                            let row_v = arr[ri].borrow().clone();
                            drop(arr);
                            let row_v = self.deref_value(row_v);
                            if let Value::Arr(row) = row_v {
                                let row = row.borrow();
                                if ci >= row.len() {
                                    return Err(RtError::new(
                                        "IndexOutOfBounds",
                                        Some(span.clone()),
                                    ));
                                }
                                return Ok(row[ci].borrow().clone());
                            }
                            return Err(RtError::new("BadIndex", Some(span.clone())));
                        }
                        if indices.len() != 1 {
                            return Err(RtError::new("BadIndex", Some(span.clone())));
                        }
                        let idx = self.eval(&indices[0])?;
                        let i = self.as_index(&idx, span)?;
                        let arr = a.borrow();
                        if i >= arr.len() {
                            return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                        }
                        let v = arr[i].borrow().clone();
                        drop(arr);
                        Ok(v)
                    }
                    Value::Str(s) => {
                        let idx = self.eval(&indices[0])?;
                        let i = self.as_index(&idx, span)?;
                        let bytes = s.borrow();
                        if i >= bytes.len() {
                            return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                        }
                        Ok(Value::Int(bytes[i] as i128))
                    }
                    Value::Slice { data, start, len } => {
                        let idx = self.eval(&indices[0])?;
                        let i = self.as_index(&idx, span)?;
                        if i >= *len {
                            return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                        }
                        let d = data.borrow();
                        let v = d[*start + i].borrow().clone();
                        drop(d);
                        Ok(v)
                    }
                    _ => Err(RtError::new("NotIndexable", Some(span.clone()))),
                }
            }
            Expr::Deref(e, span) => {
                let v = self.eval(e)?;
                self.deref_checked(v, span)
            }
            Expr::AddrOf(e, _, span) => {
                // &x / &mut x：产生共享槽指针
                match e.as_ref() {
                    Expr::Ident(name, _) => match self.lookup(name) {
                        Some(cell) => {
                            // M2.5 Debug 悬垂标记：登记目标——目标销毁时标记指针
                            if self.debug_dangling {
                                self.tracked.insert(Rc::as_ptr(&cell) as usize);
                            }
                            Ok(Value::Ptr(cell))
                        }
                        None => Err(RtError::msg("UndefinedName", format!("undefined `{name}`"))),
                    },
                    Expr::Field { base, field, .. } => {
                        let b = self.eval(base)?;
                        self.check_dangling(&b, span)?;
                        let b = self.deref_value(b);
                        match b {
                            Value::Class(c) => {
                                let cell = Rc::new(RefCell::new(
                                    c.borrow().fields.get(field).cloned().unwrap_or(Value::Void),
                                ));
                                // 写回需要字段级共享——tag1：修改经 Assign 的 field 路径处理
                                self.tmp_field_cells.push(cell.clone());
                                Ok(Value::Ptr(cell))
                            }
                            _ => Err(RtError::msg("BadAddrOf", "cannot take address")),
                        }
                    }
                    _ => {
                        let v = self.eval(e)?;
                        Ok(Value::Ptr(Rc::new(RefCell::new(v))))
                    }
                }
            }
            Expr::Unary(op, inner, span) => {
                let v = self.eval(inner)?;
                let v = self.deref_value(v);
                match op {
                    UnaryOp::Neg => match v {
                        Value::Int(i) => Ok(Value::Int(-i)),
                        Value::Float(f) => Ok(Value::Float(-f)),
                        _ => Err(RtError::new("TypeError", Some(span.clone()))),
                    },
                    UnaryOp::Not => Ok(Value::Bool(!v.as_bool())),
                    UnaryOp::BitNot => match v {
                        Value::Int(i) => Ok(Value::Int(!i)),
                        _ => Err(RtError::new("TypeError", Some(span.clone()))),
                    },
                }
            }
            Expr::Binary(op, l, r, span) => self.eval_binary(*op, l, r, span),
            Expr::Orelse(l, r, _) => {
                let v = self.eval(l)?;
                let v = self.deref_value(v);
                match v {
                    Value::Opt(None) => {
                        // orelse return/continue/break（控制流兜底）：向函数/循环边界传播
                        if let Expr::Block(b, _) = r.as_ref() {
                            let flow = self.exec_block_inner(b)?;
                            match flow {
                                Flow::None => Ok(Value::Void),
                                Flow::Value(v) => Ok(v),
                                Flow::Return(v) => Err(RtError::signal(Flow::Return(v))),
                                Flow::Break => Err(RtError::signal(Flow::Break)),
                                Flow::Continue => Err(RtError::signal(Flow::Continue)),
                            }
                        } else {
                            self.eval(r)
                        }
                    }
                    Value::Opt(Some(inner)) => Ok((*inner).clone()),
                    other => Ok(other),
                }
            }
            Expr::Unwrap(e, span) => {
                let v = self.eval(e)?;
                let v = self.deref_value(v);
                match v {
                    Value::Opt(Some(inner)) => Ok((*inner).clone()),
                    Value::Opt(None) => Err(RtError::new("NullUnwrap", Some(span.clone()))),
                    other => Ok(other),
                }
            }
            Expr::Try(e, _) => {
                let v = self.eval(e)?;
                match v {
                    // M2.6：错误沿**值通道**从当前函数返回（signal → 函数边界转
                    // Ok(Value::Err)），调用方 catch/try 可拦截；不转 RtError（抛错
                    // 通道会绕过 catch——错误传播必须经 try/catch 处理）
                    Value::Err { .. } => Err(RtError::signal(Flow::Return(v))),
                    other => Ok(other),
                }
            }
            Expr::Catch(e, kind, _) => {
                let v = self.eval(e)?;
                match &v {
                    Value::Err { .. } => match kind.as_ref() {
                        CatchKind::Default(d) => self.eval(d),
                        CatchKind::Bind { name: bname, body } => {
                            self.push_scope();
                            // 捕获绑定携带完整错误值（名 + 码）
                            let err_clone = v.clone();
                            self.bind(bname, err_clone);
                            let r = self.exec_block_inner(body);
                            let _ = self.pop_scope();
                            match r? {
                                Flow::None => Ok(Value::Void),
                                Flow::Value(v) => Ok(v),
                                // 语句 return/break/continue：向函数/循环边界传播（与块表达式一致）
                                Flow::Return(v) => Err(RtError::signal(Flow::Return(v))),
                                Flow::Break => Err(RtError::signal(Flow::Break)),
                                Flow::Continue => Err(RtError::signal(Flow::Continue)),
                            }
                        }
                    },
                    _ => Ok(v),
                }
            }
            Expr::Call { callee, args, span } => self.eval_call(callee, args, span),
            Expr::IfExpr {
                cond,
                capture,
                then_e,
                else_e,
                ..
            } => {
                let c = self.eval(cond)?;
                // optional 捕获表达式：if (maybe) |v| v else 0
                if let Some((_, name)) = capture {
                    match self.deref_value(c) {
                        Value::Opt(Some(v)) => {
                            self.push_scope();
                            self.bind(name, (*v).clone());
                            let r = self.eval(then_e);
                            let _ = self.pop_scope();
                            return r;
                        }
                        Value::Opt(None) => return self.eval(else_e),
                        other => {
                            self.push_scope();
                            self.bind(name, other);
                            let r = self.eval(then_e);
                            let _ = self.pop_scope();
                            return r;
                        }
                    }
                }
                if c.as_bool() {
                    self.eval(then_e)
                } else {
                    self.eval(else_e)
                }
            }
            Expr::SwitchExpr { subject, arms, .. } => {
                let sw = SwitchStmt {
                    subject: (**subject).clone(),
                    arms: arms.clone(),
                    has_else: arms
                        .iter()
                        .any(|a| a.patterns.iter().any(|p| matches!(p, SwitchPattern::Else))),
                    span: Span::new(0, 0, 0, 0),
                };
                match self.exec_switch(&sw)? {
                    Flow::None | Flow::Break | Flow::Continue => Ok(Value::Void),
                    // 表达式臂值：switch 表达式结果
                    Flow::Value(v) => Ok(v),
                    // 语句 return（`=> return x`）：向函数边界传播
                    Flow::Return(v) => Err(RtError::signal(Flow::Return(v))),
                }
            }
            Expr::Block(b, _) => {
                self.push_scope();
                let r = self.exec_block_inner(b);
                let _ = self.pop_scope();
                match r? {
                    Flow::None => Ok(Value::Void),
                    Flow::Value(v) => Ok(v),
                    // 语句 return/break/continue：向函数/循环边界传播
                    Flow::Return(v) => Err(RtError::signal(Flow::Return(v))),
                    Flow::Break => Err(RtError::signal(Flow::Break)),
                    Flow::Continue => Err(RtError::signal(Flow::Continue)),
                }
            }
            Expr::Assign {
                target,
                op,
                value,
                span,
            } => self.eval_assign(target, *op, value, span),
            Expr::FnRef(name, _) => Ok(Value::Fn(name.clone())),
            Expr::Closure {
                params,
                body,
                is_mut,
                ..
            } => {
                // 捕获环境：当前作用域链中所有共享槽（tag1：捕获整个链，语义 = 捕获所有外部变量）
                let env = self.capture_env();
                Ok(Value::Closure(ClosureData {
                    params: params.clone(),
                    body: body.clone(),
                    is_mut: *is_mut,
                    env,
                }))
            }
            Expr::TupleDestructure(names, e, _) => {
                let v = self.eval(e)?;
                let v = self.deref_value(v);
                if let Value::Arr(items) = v {
                    let items = items.borrow().clone();
                    if items.len() != names.len() {
                        return Err(RtError::msg("TupleArity", "destructure arity mismatch"));
                    }
                    for (n, it) in names.iter().zip(items.iter()) {
                        if n != "_" {
                            self.bind(n, it.borrow().clone());
                        }
                    }
                    Ok(Value::Void)
                } else {
                    Err(RtError::msg("TupleArity", "expected tuple in destructure"))
                }
            }
            // M2.4：move 运行时等同内层（所有权转移语义由作用域销毁体现；
            // 合法性检查在语义层）
            Expr::Move(inner, _) => self.eval(inner),
        }
    }

    // ---------- 表达式求值结束 ----------

    fn eval_dot(&mut self, b: Value, field: &str) -> Result<Value> {
        // math 命名空间特判（Fn 引用形式）
        if let Value::Fn(fname) = &b {
            return Ok(Value::Fn(format!("{fname}.{field}")));
        }
        match &b {
            Value::Enum { name, .. } => {
                // Type.variant 枚举常量
                return Ok(Value::Enum {
                    name: name.clone(),
                    variant: field.to_string(),
                    payload: None,
                });
            }
            _ => {
                // 字段访问（Str.len / Class.field）
                let span = Span::new(0, 0, 0, 0);
                self.eval_field(b, field, &span)
            }
        }
    }

    fn eval_field(&mut self, b: Value, field: &str, span: &Span) -> Result<Value> {
        let b = self.deref_value(b);
        match &b {
            Value::Class(c) => {
                let d = c.borrow();
                // Io 内建字段：io.alloc（M5.4 程序环境——默认分配器）
                if d.name == "Io" && field == "alloc" {
                    return Ok(Value::Alloc);
                }
                // Map 内建字段：len
                if d.name == "Map" && field == "len" {
                    return Ok(Value::Int(d.fields.len() as i128));
                }
                match d.fields.get(field) {
                    Some(v) => Ok(v.clone()),
                    None => Err(RtError::new("NoField", Some(span.clone()))),
                }
            }
            Value::Str(s) => match field {
                "len" => Ok(Value::Int(s.borrow().len() as i128)),
                _ => Err(RtError::new("NoField", Some(span.clone()))),
            },
            Value::Arr(a) => match field {
                "len" => Ok(Value::Int(a.borrow().len() as i128)),
                _ => Err(RtError::new("NoField", Some(span.clone()))),
            },
            Value::Slice { len, .. } => match field {
                "len" => Ok(Value::Int(*len as i128)),
                _ => Err(RtError::new("NoField", Some(span.clone()))),
            },
            _ => Err(RtError::new("NoField", Some(span.clone()))),
        }
    }

    fn as_index(&self, v: &Value, span: &Span) -> Result<usize> {
        match self.deref_value(v.clone()) {
            Value::Int(i) if i >= 0 => Ok(i as usize),
            _ => Err(RtError::new("BadIndex", Some(span.clone()))),
        }
    }

    fn eval_binary(&mut self, op: BinOp, l: &Expr, r: &Expr, span: &Span) -> Result<Value> {
        // 短路
        match op {
            BinOp::And => {
                let lv = self.eval(l)?;
                if !lv.as_bool() {
                    return Ok(Value::Bool(false));
                }
                let rv = self.eval(r)?;
                return Ok(Value::Bool(rv.as_bool()));
            }
            BinOp::Or => {
                let lv = self.eval(l)?;
                if lv.as_bool() {
                    return Ok(Value::Bool(true));
                }
                let rv = self.eval(r)?;
                return Ok(Value::Bool(rv.as_bool()));
            }
            _ => {}
        }
        let lv = self.eval(l)?;
        let rv = self.eval(r)?;
        self.binop_values(op, &lv, &rv, span)
    }

    fn binop_values(&self, op: BinOp, l: &Value, r: &Value, span: &Span) -> Result<Value> {
        let l = self.deref_value(l.clone());
        let r = self.deref_value(r.clone());
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::EucMod => {
                self.arith(op, &l, &r, span)
            }
            BinOp::Eq => Ok(Value::Bool(l.value_eq(&r))),
            BinOp::Ne => Ok(Value::Bool(!l.value_eq(&r))),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let lt = l.value_lt(&r);
                match lt {
                    Some(lt) => {
                        let v = match op {
                            BinOp::Lt => lt,
                            BinOp::Le => lt || l.value_eq(&r),
                            BinOp::Gt => !lt && !l.value_eq(&r),
                            BinOp::Ge => !lt || l.value_eq(&r),
                            _ => unreachable!(),
                        };
                        Ok(Value::Bool(v))
                    }
                    None => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                match (&l, &r) {
                    (Value::Int(a), Value::Int(b)) => {
                        let v = match op {
                            BinOp::BitAnd => Value::Int(a & b),
                            BinOp::BitOr => Value::Int(a | b),
                            BinOp::BitXor => Value::Int(a ^ b),
                            BinOp::Shl => {
                                // u64 语义：源值 ≤ u64::MAX 时按 64 位截断（xorshift 等）
                                if *a >= 0 && *a <= u64::MAX as i128 && *b < 64 {
                                    let v = (*a as u64).wrapping_shl(*b as u32);
                                    Value::Int(v as i128)
                                } else {
                                    Value::Int(a << b)
                                }
                            }
                            BinOp::Shr => {
                                if *a >= 0 && *a <= u64::MAX as i128 && *b < 64 {
                                    let v = (*a as u64).wrapping_shr(*b as u32);
                                    Value::Int(v as i128)
                                } else {
                                    Value::Int(a >> b)
                                }
                            }
                            _ => unreachable!(),
                        };
                        Ok(v)
                    }
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            BinOp::Range => {
                // 区间糖（Q29）：[lo, hi) 展开为数组
                match (&l, &r) {
                    (Value::Int(a), Value::Int(b)) => {
                        let mut items = Vec::new();
                        let mut i = *a;
                        while i < *b {
                            items.push(Value::Int(i));
                            i += 1;
                        }
                        Ok(Value::arr(items))
                    }
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            _ => Err(RtError::new("TypeError", Some(span.clone()))),
        }
    }

    fn arith(&self, op: BinOp, l: &Value, r: &Value, span: &Span) -> Result<Value> {
        match (l, r) {
            (Value::Int(a), Value::Int(b)) => {
                let v = match op {
                    BinOp::Add => a.checked_add(*b),
                    BinOp::Sub => a.checked_sub(*b),
                    BinOp::Mul => a.checked_mul(*b),
                    BinOp::Div => {
                        if *b == 0 {
                            return Err(RtError::new("DivisionByZero", Some(span.clone())));
                        }
                        Some(a / b)
                    }
                    BinOp::Mod => {
                        if *b == 0 {
                            return Err(RtError::new("DivisionByZero", Some(span.clone())));
                        }
                        Some(a % b)
                    }
                    BinOp::EucMod => {
                        if *b == 0 {
                            return Err(RtError::new("DivisionByZero", Some(span.clone())));
                        }
                        Some(a.rem_euclid(*b))
                    }
                    _ => None,
                };
                match v {
                    Some(v) => Ok(Value::Int(v)),
                    None => Err(RtError::new("Overflow", Some(span.clone()))),
                }
            }
            (Value::Float(a), Value::Float(b)) => {
                let v = match op {
                    BinOp::Add => a + b,
                    BinOp::Sub => a - b,
                    BinOp::Mul => a * b,
                    BinOp::Div => a / b,
                    BinOp::Mod | BinOp::EucMod => a % b,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                Ok(Value::Float(v))
            }
            (Value::Int(a), Value::Float(_b)) => self.arith(op, &Value::Float(*a as f64), r, span),
            (Value::Float(_a), Value::Int(b)) => self.arith(op, l, &Value::Float(*b as f64), span),
            _ => Err(RtError::new("TypeError", Some(span.clone()))),
        }
    }

    fn eval_assign(
        &mut self,
        target: &Expr,
        op: AssignOp,
        value: &Expr,
        span: &Span,
    ) -> Result<Value> {
        let new_v = match op {
            AssignOp::Set => self.eval(value)?,
            _ => {
                let cur = self.eval(target)?;
                let rhs = self.eval(value)?;
                let bop = match op {
                    AssignOp::Add => BinOp::Add,
                    AssignOp::Sub => BinOp::Sub,
                    AssignOp::Mul => BinOp::Mul,
                    AssignOp::Div => BinOp::Div,
                    AssignOp::BitOr => BinOp::BitOr,
                    AssignOp::BitAnd => BinOp::BitAnd,
                    AssignOp::BitXor => BinOp::BitXor,
                    AssignOp::Set => unreachable!(),
                };
                self.binop_values(bop, &cur, &rhs, span)?
            }
        };
        // 写入目标
        match target {
            Expr::Ident(name, _) => {
                let cell = self
                    .lookup(name)
                    .ok_or_else(|| RtError::new("UndefinedName", Some(span.clone())))?;
                *cell.borrow_mut() = new_v;
            }
            Expr::Deref(inner, _) => {
                // p.* = v：写入指针指向的槽
                let p = self.eval(inner)?;
                self.check_dangling(&p, span)?;
                match p {
                    Value::Ptr(cell) => {
                        *cell.borrow_mut() = new_v;
                    }
                    _ => return Err(RtError::new("BadAssign", Some(span.clone()))),
                }
            }
            Expr::Field { base, field, .. } => {
                let b = self.eval(base)?;
                self.check_dangling(&b, span)?;
                let b = self.deref_value(b);
                if let Value::Class(c) = b {
                    c.borrow_mut().fields.insert(field.clone(), new_v);
                } else {
                    return Err(RtError::new("TypeError", Some(span.clone())));
                }
            }
            Expr::Dot { base, field, .. } => {
                // 实例字段赋值（hp.x = v）；base 为类型名时非赋值目标
                if let Expr::Ident(bname, _) = base.as_ref() {
                    if self.types.contains_key(bname) {
                        return Err(RtError::new("BadAssign", Some(span.clone())));
                    }
                }
                let b = self.eval(base)?;
                self.check_dangling(&b, span)?;
                let b = self.deref_value(b);
                if let Value::Class(c) = b {
                    c.borrow_mut().fields.insert(field.clone(), new_v);
                } else {
                    return Err(RtError::new("TypeError", Some(span.clone())));
                }
            }
            Expr::Index { base, indices, .. } => {
                let b = self.eval(base)?;
                self.check_dangling(&b, span)?;
                let b = self.deref_value(b);
                // 可写切片 &mut arr[0..2]：索引为 Range
                if indices.len() == 1 {
                    if let Expr::Binary(BinOp::Range, lo, hi, _) = &indices[0] {
                        let lo_v = self.eval(lo)?;
                        let hi_v = self.eval(hi)?;
                        let lo_i = self.as_index(&lo_v, span)?;
                        let hi_i = self.as_index(&hi_v, span)?;
                        if let Value::Arr(a) = &b {
                            let total = a.borrow().len();
                            let _len = hi_i.saturating_sub(lo_i);
                            if hi_i > total || lo_i > total {
                                return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                            }
                            let new_v = match op {
                                AssignOp::Set => self.eval(value)?,
                                _ => {
                                    return Err(RtError::new("BadAssign", Some(span.clone())));
                                }
                            };
                            // 写回切片元素
                            if let Value::Arr(src) = new_v {
                                let src_items = src.borrow().clone();
                                let arr = a.borrow_mut();
                                for (k, item) in src_items.iter().enumerate() {
                                    if lo_i + k < arr.len() {
                                        *arr[lo_i + k].borrow_mut() = item.borrow().clone();
                                    }
                                }
                            }
                            return Ok(Value::Void);
                        }
                    }
                }
                if let Value::Arr(a) = b {
                    let idx = self.eval(&indices[0])?;
                    let i = self.as_index(&idx, span)?;
                    let new_v = match op {
                        AssignOp::Set => self.eval(value)?,
                        _ => {
                            let arr = a.borrow();
                            if i >= arr.len() {
                                return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                            }
                            let cur = arr[i].borrow().clone();
                            drop(arr);
                            let rhs = self.eval(value)?;
                            let bop = match op {
                                AssignOp::Add => BinOp::Add,
                                AssignOp::Sub => BinOp::Sub,
                                AssignOp::Mul => BinOp::Mul,
                                AssignOp::Div => BinOp::Div,
                                AssignOp::BitOr => BinOp::BitOr,
                                AssignOp::BitAnd => BinOp::BitAnd,
                                AssignOp::BitXor => BinOp::BitXor,
                                AssignOp::Set => unreachable!(),
                            };
                            self.binop_values(bop, &cur, &rhs, span)?
                        }
                    };
                    let arr = a.borrow_mut();
                    if i >= arr.len() {
                        return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                    }
                    *arr[i].borrow_mut() = new_v;
                } else {
                    return Err(RtError::new("TypeError", Some(span.clone())));
                }
            }
            _ => return Err(RtError::new("BadAssign", Some(span.clone()))),
        }
        Ok(Value::Void)
    }

    fn eval_call(&mut self, callee: &Expr, args: &[Expr], span: &Span) -> Result<Value> {
        // 方法调用 p.dist(q)：注入 self
        if let Expr::Field { base, field, .. } = callee {
            // Type.new(...) 构造（base 为类型名）
            if let Expr::Ident(bname, _) = base.as_ref() {
                if field == "new" && self.types.contains_key(bname) {
                    return self.call_new_builtin(bname, args, span);
                }
                // 集合类型 Vec(&[u8]).init(alloc)（此处 base 为类型名时）
                if matches!(bname.as_str(), "Vec" | "Map" | "Deque") && field == "init" {
                    let _ = args;
                    if bname == "Map" {
                        return Ok(Value::class("Map", HashMap::new()));
                    }
                    return Ok(Value::arr(vec![]));
                }
                // Table(T).init(alloc, rows, cols, init)（M8）
                if bname == "Table" && field == "init" {
                    if args.len() < 4 {
                        return Err(RtError::new("ArityMismatch", Some(span.clone())));
                    }
                    let rows = self.eval(&args[1])?;
                    let cols = self.eval(&args[2])?;
                    let init_v = self.eval(&args[3])?;
                    let rows = match self.deref_value(rows) {
                        Value::Int(i) => i.max(0) as usize,
                        _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                    };
                    let cols = match self.deref_value(cols) {
                        Value::Int(i) => i.max(0) as usize,
                        _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                    };
                    let mut grid = Vec::new();
                    for _ in 0..rows {
                        let mut row = Vec::new();
                        for _ in 0..cols {
                            row.push(init_v.clone());
                        }
                        grid.push(Value::arr(row));
                    }
                    return Ok(Value::arr(grid));
                }
            }
            let self_v = self.eval(base)?;
            let self_v = self.deref_value(self_v);
            // 内建方法（Str / Arr / Class 上的 len、concat 等）
            if let Some(v) = self.call_builtin_method(&self_v, field, args, span)? {
                return Ok(v);
            }
            let type_name = self_v.type_name();
            // 注入 self 为首参
            let mut all_args = vec![Expr::VoidLit(span.clone())]; // 占位，用运行时值
            let _ = &mut all_args;
            let mut vals = vec![self_v.clone()];
            for a in args {
                vals.push(self.eval(a)?);
            }
            let fname = format!("{type_name}.{field}");
            let fdef = self.pick_fn(&fname, &vals)?;
            return self.call_fn(&fdef, &vals, span);
        }
        // Dot 形式：Type.method 静态调用 / io.print 等实例方法 / math 命名空间
        match callee {
            Expr::Dot { base, field, .. } => {
                if let Expr::Ident(bname, _) = base.as_ref() {
                    // math.sqrt / math.nan
                    if let Some(v) = self.call_math(bname, field, args, span)? {
                        return Ok(v);
                    }
                    // Arena.init(alloc) 内建
                    if bname == "Arena" && field == "init" {
                        return Ok(Value::class("Arena", HashMap::new()));
                    }
                    // String.from / String.concat 内建（String = 内建新类型，M3 定案）
                    if bname == "String" {
                        return self.call_string_builtin(field, args, span);
                    }
                    // X.new(...) 旧样板构造（审计 C1 取消后示例未迁移；tag1 兼容）
                    if field == "new" && self.types.contains_key(bname) {
                        return self.call_new_builtin(bname, args, span);
                    }
                    // Vec.init(alloc) / Map.init(alloc) 集合构造
                    if matches!(bname.as_str(), "Vec" | "Map" | "Deque") && field == "init" {
                        let _ = args;
                        if bname == "Map" {
                            return Ok(Value::class("Map", HashMap::new()));
                        }
                        return Ok(Value::arr(vec![]));
                    }
                    // Table(T).init(alloc, rows, cols, init)：二维表（M8 定案）
                    if bname == "Table" && field == "init" {
                        if args.len() < 4 {
                            return Err(RtError::new("ArityMismatch", Some(span.clone())));
                        }
                        let rows = self.eval(&args[1])?;
                        let cols = self.eval(&args[2])?;
                        let init_v = self.eval(&args[3])?;
                        let rows = match self.deref_value(rows) {
                            Value::Int(i) => i.max(0) as usize,
                            _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                        };
                        let cols = match self.deref_value(cols) {
                            Value::Int(i) => i.max(0) as usize,
                            _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                        };
                        let mut grid = Vec::new();
                        for _ in 0..rows {
                            let mut row = Vec::new();
                            for _ in 0..cols {
                                row.push(init_v.clone());
                            }
                            grid.push(Value::arr(row));
                        }
                        return Ok(Value::arr(grid));
                    }
                    // Vec(i32).from_bytes：集合反序列化（u64 长度前缀 + 元素）
                    if matches!(bname.as_str(), "Vec" | "Deque") && field == "from_bytes" {
                        let bytes = self.eval(&args[0])?;
                        let bytes = self.deref_value(bytes);
                        let b = match self.value_bytes(&bytes) {
                            Some(b) => b,
                            None => return Err(RtError::new("TypeError", Some(span.clone()))),
                        };
                        if b.len() < 8 {
                            return Err(RtError::new("InvalidBytes", Some(span.clone())));
                        }
                        let n = u64::from_le_bytes(b[0..8].try_into().unwrap()) as usize;
                        let mut items = Vec::new();
                        let mut pos = 8usize;
                        for _ in 0..n {
                            // tag1：按 i32 元素 4 字节解析
                            let v = if b.len() >= pos + 4 {
                                let i = i32::from_le_bytes(b[pos..pos + 4].try_into().unwrap());
                                pos += 4;
                                Value::Int(i as i128)
                            } else {
                                break;
                            };
                            items.push(v);
                        }
                        return Ok(Value::arr(items));
                    }
                    // String.from(s, alloc) 内建
                    if bname == "String" && field == "from" {
                        let v = self.eval(&args[0])?;
                        let v = self.deref_value(v);
                        if let Value::Str(s) = v {
                            return Ok(Value::Str(s));
                        }
                        return Ok(Value::str(&v.display()));
                    }
                    // json.parse(data)（M5.3 序列化辅助）：JSON 对象 → Map
                    if bname == "json" && field == "parse" {
                        let v = self.eval(&args[0])?;
                        let v = self.deref_value(v);
                        if let Value::Str(s) = v {
                            let text = String::from_utf8_lossy(&s.borrow()).to_string();
                            let obj = self.parse_json_obj(&text)?;
                            return Ok(Value::class("Map", obj));
                        }
                        return Err(RtError::new("TypeError", Some(span.clone())));
                    }
                    // Type.method 静态调用：注入 self 为第一个实参
                    if self.types.contains_key(bname)
                        || self.funcs.contains_key(&format!("{bname}.{field}"))
                    {
                        // 序列化静态入口：Type.from_bytes / Type.from_json
                        if field == "from_bytes" && self.types.contains_key(bname) {
                            let bytes = self.eval(&args[0])?;
                            let bytes = self.deref_value(bytes);
                            let v = match self.value_bytes(&bytes) {
                                Some(b) => b,
                                None => return Err(RtError::new("TypeError", Some(span.clone()))),
                            };
                            return self.class_from_bytes(bname, &v);
                        }
                        if field == "from_json" && self.types.contains_key(bname) {
                            let json = self.eval(&args[0])?;
                            let json = self.deref_value(json);
                            let s = match json {
                                Value::Str(s) => s.borrow().clone(),
                                _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                            };
                            let obj = self.parse_json_obj(&String::from_utf8_lossy(&s))?;
                            return self.class_from_json(bname, &obj);
                        }
                        let mut vals = Vec::new();
                        for a in args {
                            vals.push(self.eval(a)?);
                        }
                        let fname = format!("{bname}.{field}");
                        let fdef = self.pick_fn(&fname, &vals)?;
                        return self.call_fn(&fdef, &vals, span);
                    }
                    // 实例方法：io.print(...) / arena.alloc(...)
                    let self_v = self.eval(base)?;
                    let self_v = self.deref_value(self_v);
                    if let Some(v) = self.call_builtin_method(&self_v, field, args, span)? {
                        return Ok(v);
                    }
                    let type_name = self_v.type_name();
                    let mut vals = vec![self_v];
                    for a in args {
                        vals.push(self.eval(a)?);
                    }
                    let fname = format!("{type_name}.{field}");
                    let fdef = self.pick_fn(&fname, &vals)?;
                    return self.call_fn(&fdef, &vals, span);
                }
                Err(RtError::new("NoMethod", Some(span.clone())))
            }
            Expr::Ident(name, _) => {
                // 集合类型实例化 Vec(i32)/Map(...)（类型表达式上下文 → 空容器）
                if matches!(name.as_str(), "Vec" | "Deque") {
                    return Ok(Value::arr(vec![]));
                }
                if name == "Map" {
                    return Ok(Value::class("Map", HashMap::new()));
                }
                if name == "Table" {
                    // Table(i32) 类型实例化：空二维容器（init 填充）
                    return Ok(Value::arr(vec![]));
                }
                // 用户函数优先于内建（同名冲突时，如 parse_int）
                if self.funcs.contains_key(name) {
                    let mut vals = Vec::new();
                    for a in args {
                        vals.push(self.eval(a)?);
                    }
                    let fdef = self.pick_fn(name, &vals)?;
                    return self.call_fn(&fdef, &vals, span);
                }
                // 内建函数
                if let Some(v) = self.call_builtin(name, args, span)? {
                    return Ok(v);
                }
                // 函数指针调用（apply(square, ...) → square 已是 Fn 值）
                if let Some(cell) = self.lookup(name) {
                    let v = cell.borrow().clone();
                    if let Value::Fn(fname) = v {
                        let mut vals = Vec::new();
                        for a in args {
                            vals.push(self.eval(a)?);
                        }
                        let fdef = self.pick_fn(&fname, &vals)?;
                        return self.call_fn(&fdef, &vals, span);
                    }
                    if let Value::Closure(closure) = v {
                        let mut vals = Vec::new();
                        for a in args {
                            vals.push(self.eval(a)?);
                        }
                        return self.call_closure(&closure, &vals, span);
                    }
                }
                let mut vals = Vec::new();
                for a in args {
                    vals.push(self.eval(a)?);
                }
                let fdef = self.pick_fn(name, &vals)?;
                self.call_fn(&fdef, &vals, span)
            }
            _ => {
                // 任意表达式求值后调用（Fn 值 / 闭包）
                let c = self.eval(callee)?;
                let c = self.deref_value(c);
                if let Value::Fn(fname) = c {
                    let mut vals = Vec::new();
                    for a in args {
                        vals.push(self.eval(a)?);
                    }
                    let fdef = self.pick_fn(&fname, &vals)?;
                    return self.call_fn(&fdef, &vals, span);
                }
                if let Value::Closure(closure) = c {
                    let mut vals = Vec::new();
                    for a in args {
                        vals.push(self.eval(a)?);
                    }
                    return self.call_closure(&closure, &vals, span);
                }
                Err(RtError::new("NotCallable", Some(span.clone())))
            }
        }
    }

    fn pick_fn(&self, name: &str, arg_vals: &[Value]) -> Result<FnDef> {
        let candidates = self
            .funcs
            .get(name)
            .ok_or_else(|| RtError::msg("NoFunction", format!("no function `{name}`")))?;
        // 1) 精确参数数量匹配
        let exact: Vec<&FnDef> = candidates
            .iter()
            .filter(|f| f.params.len() == arg_vals.len())
            .collect();
        let pool: Vec<&FnDef> = if exact.is_empty() {
            candidates.iter().collect()
        } else {
            exact
        };
        if pool.len() == 1 {
            return Ok(pool[0].clone());
        }
        // 2) 按实参值类型匹配（具体优先于泛型）
        let mut best: Option<&FnDef> = None;
        for f in &pool {
            let mut ok = true;
            let mut is_generic = false;
            for (p, a) in f.params.iter().zip(arg_vals.iter()) {
                let pt = p.ty.strip();
                // 指针实参解引用后匹配
                let a = match a {
                    Value::Ptr(cell) => &*cell.borrow(),
                    other => other,
                };
                match pt {
                    Type::Named(n, _) => {
                        let want_float = matches!(n.as_str(), "f32" | "f64" | "f16" | "f128");
                        let want_int = matches!(
                            n.as_str(),
                            "i8" | "i16"
                                | "i32"
                                | "i64"
                                | "i128"
                                | "isize"
                                | "u8"
                                | "u16"
                                | "u32"
                                | "u64"
                                | "u128"
                                | "usize"
                        );
                        let want_bool = n == "bool";
                        match a {
                            Value::Int(_) if want_float => ok = false,
                            Value::Float(_) if want_int => ok = false,
                            Value::Str(_) if want_int || want_float || want_bool => ok = false,
                            Value::Bool(_) if !want_bool => ok = false,
                            Value::Class(c) if n != "String" && c.borrow().name != *n => ok = false,
                            // 泛型 T（where T: INumber 等）：不排除（编译时验证归 M2）
                            _ if n.chars().next().map_or(false, |c| c.is_uppercase())
                                && !n.starts_with("String")
                                && !n.starts_with("Vec")
                                && !n.starts_with("Map") =>
                            {
                                is_generic = true;
                            }
                            _ => {}
                        }
                    }
                    Type::Slice(inner, _) => {
                        // &[u8] / &[T]：Str 或数组；泛型元素 T 标记为泛型
                        match a {
                            Value::Str(_) => {}
                            Value::Arr(_) | Value::Slice { .. } => {}
                            _ => ok = false,
                        }
                        if let Type::Named(n, _) = inner.strip() {
                            if n.chars().next().map_or(false, |c| c.is_uppercase())
                                && !n.starts_with("String")
                                && !n.starts_with("Vec")
                                && !n.starts_with("Map")
                            {
                                is_generic = true;
                            }
                        }
                    }
                    Type::Infer => {}
                    _ => {}
                }
            }
            if ok {
                // 具体优先于泛型；同级时优先返回类型匹配期望类型（M2.3/M2.7
                // 期望类型传播：var f: f64 = parse(...) / return parse(...)）；再同级保留首个
                match &best {
                    None => best = Some(f),
                    Some(b) => {
                        let b_generic = b.params.iter().any(|p| type_has_generic(&p.ty));
                        let f_ret = self.ret_matches_expected(f.ret.as_ref());
                        let b_ret = self.ret_matches_expected(b.ret.as_ref());
                        if !is_generic && b_generic {
                            // 具体优先于泛型
                            best = Some(f);
                        } else if is_generic && !b_generic {
                            // 保留 best（泛型不替换具体）
                        } else if f_ret && !b_ret {
                            // 同具体度：返回类型匹配期望 → 替换
                            best = Some(f);
                        }
                        // 同具体度同期望匹配：保留 best（首个注册，稳定）
                    }
                }
            }
        }
        if let Some(b) = best {
            return Ok(b.clone());
        }
        // 3) 带默认参数的回退（参数数 <= 声明数且尾部默认）
        for f in candidates {
            if f.params.len() > arg_vals.len() {
                let missing = f.params.len() - arg_vals.len();
                let tail_has_default = f.params[f.params.len() - missing..]
                    .iter()
                    .all(|p| p.default.is_some());
                if tail_has_default {
                    return Ok(f.clone());
                }
            }
        }
        Err(RtError::msg(
            "AmbiguousCall",
            format!(
                "no matching overload of `{name}` ({} arg(s))",
                arg_vals.len()
            ),
        ))
    }

    /// 期望类型传播（M2.3/M2.7）：函数返回类型是否匹配当前期望类型
    /// （`!T` 错误联合拆内层；`void` 为 Named("void")；无返回类型或泛型返回不匹配）
    fn ret_matches_expected(&self, ret: Option<&Type>) -> bool {
        let Some(exp) = &self.expected_ret else {
            return false;
        };
        let Some(ret) = ret else {
            return false;
        };
        let inner = match ret.strip() {
            Type::ErrorUnion(_, inner) => inner.strip(),
            other => other,
        };
        match inner {
            Type::Named(n, _) => n == exp,
            _ => false,
        }
    }

    fn call_fn(&mut self, fdef: &FnDef, arg_vals: &[Value], span: &Span) -> Result<Value> {
        if fdef.params.len() < arg_vals.len() {
            return Err(RtError::new("ArityMismatch", Some(span.clone())));
        }
        let mut bound: Vec<(String, Value)> = Vec::new();
        for (i, p) in fdef.params.iter().enumerate() {
            if i < arg_vals.len() {
                bound.push((p.name.clone(), arg_vals[i].clone()));
            } else if let Some(d) = &p.default {
                let v = self.eval(d)?;
                bound.push((p.name.clone(), v));
            } else {
                return Err(RtError::new("ArityMismatch", Some(span.clone())));
            }
        }
        let prev_ret = self.current_ret.clone();
        self.current_ret = fdef.ret.clone();
        let r = self.exec_fn_body(&fdef.body, &bound);
        self.current_ret = prev_ret;
        r
    }

    // ---------- 内建 ----------

    fn call_builtin(&mut self, name: &str, args: &[Expr], span: &Span) -> Result<Option<Value>> {
        match name {
            "box" => {
                if args.len() != 2 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let v = self.eval(&args[0])?;
                Ok(Some(Value::Ptr(Rc::new(RefCell::new(v)))))
            }
            "copy" => {
                if args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let v = self.eval(&args[0])?;
                // copy(&x, .shallow)（L1：CopyMode 内建枚举，.shallow 推断）
                let shallow = if args.len() > 1 {
                    let mode = self.eval(&args[1])?;
                    match self.deref_value(mode) {
                        Value::Enum { variant, .. } => variant == "shallow",
                        _ => false,
                    }
                } else {
                    false
                };
                Ok(Some(if shallow {
                    self.shallow_copy(v)
                } else {
                    self.deep_copy(v)
                }))
            }
            // @ 内建（M4.3 子集）：@intFromEnum / @enumFromInt
            "@intFromEnum" => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                match v {
                    Value::Enum { name, variant, .. } => {
                        // 内建枚举（L3）：ExitType = [Exit, Error]
                        let idx = if name == "ExitType" {
                            match variant.as_str() {
                                "Exit" => 0,
                                "Error" => 1,
                                _ => 0,
                            }
                        } else {
                            match self.types.get(&name) {
                                Some(TypeDef::Enum { variants }) => {
                                    variants.iter().position(|v| v.name == variant).unwrap_or(0)
                                        as i128
                                }
                                _ => 0,
                            }
                        };
                        Ok(Some(Value::Int(idx)))
                    }
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            "@enumFromInt" => {
                if args.len() != 2 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let ty = match &args[0] {
                    Expr::Ident(n, _) => n.clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let i = self.eval(&args[1])?;
                let i = match self.deref_value(i) {
                    Value::Int(i) => i,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                match self.types.get(&ty) {
                    Some(TypeDef::Enum { variants }) => {
                        let variant = variants.get(i as usize).map(|v| v.name.clone());
                        match variant {
                            Some(v) => Ok(Some(Value::Enum {
                                name: ty.clone(),
                                variant: v,
                                payload: None,
                            })),
                            None => Err(RtError::new("IndexOutOfBounds", Some(span.clone()))),
                        }
                    }
                    _ => Err(RtError::new("UnknownType", Some(span.clone()))),
                }
            }
            "@panic" => {
                // Q-S2：@panic("消息", 位置) abort
                let msg = if args.is_empty() {
                    "panic".to_string()
                } else {
                    let v = self.eval(&args[0])?;
                    self.deref_value(v).display()
                };
                Err(RtError::msg("Panic", msg))
            }
            // ---------- M4.3 @ 内建基础集 ----------
            "@sizeOf" => {
                // @sizeOf(T)：类型字节大小（连续类型与 to_bytes 布局一致）
                let ty = match &args[0] {
                    Expr::Ident(n, _) => n.clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                match self.type_size_of(&ty) {
                    Some(s) => Ok(Some(Value::Int(s as i128))),
                    None => Err(RtError::msg(
                        "UnknownType",
                        format!("@sizeOf: unknown type `{ty}`"),
                    )),
                }
            }
            "@alignOf" => {
                // @alignOf(T)：自然对齐（标量 = 宽度；连续 class = 最大字段对齐；其余 8）
                let ty = match &args[0] {
                    Expr::Ident(n, _) => n.clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let align = match ty.as_str() {
                    "i8" | "u8" | "bool" => 1,
                    "i16" | "u16" | "f16" => 2,
                    "i32" | "u32" | "f32" => 4,
                    "i128" | "u128" | "f128" => 16,
                    _ => {
                        // 连续 class：最大字段对齐
                        let mut max_a = 1usize;
                        if let Some(TypeDef::Class { fields, traits, .. }) = self.types.get(&ty) {
                            if traits.iter().any(|t| matches!(t, Trait::Continuous)) {
                                for fd in fields {
                                    if let Some(s) = self.field_serialized_size(&fd.ty) {
                                        let a = if matches!(
                                            fd.ty.strip(),
                                            Type::Named(n, _)
                                                if Self::scalar_size(n) == s
                                                    && !self.is_nested_continuous(n)
                                        ) {
                                            s
                                        } else {
                                            1
                                        };
                                        max_a = max_a.max(a);
                                    }
                                }
                                max_a
                            } else {
                                8
                            }
                        } else {
                            8
                        }
                    }
                };
                Ok(Some(Value::Int(align as i128)))
            }
            "@offsetOf" => {
                // @offsetOf(T, field)：连续 class 字段偏移（与 to_bytes 填充一致）
                let ty = match &args[0] {
                    Expr::Ident(n, _) => n.clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let field = match &args[1] {
                    Expr::Ident(f, _) => f.clone(),
                    Expr::StrLit { value, .. } => value.clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                match self.continuous_layout(&ty) {
                    Some((layout, _)) => match layout.iter().find(|(n, _, _)| *n == field) {
                        Some((_, off, _)) => Ok(Some(Value::Int(*off as i128))),
                        None => Err(RtError::msg(
                            "UnknownField",
                            format!("@offsetOf: `{ty}` has no field `{field}`"),
                        )),
                    },
                    None => Err(RtError::msg(
                        "NotContinuous",
                        format!("@offsetOf: `{ty}` is not a continuous type"),
                    )),
                }
            }
            "@typeOf" => {
                // @typeOf(expr)：表达式运行时类型名（tag1 简化：type_name）
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                Ok(Some(Value::str(&v.type_name())))
            }
            "@intCast" => {
                // @intCast(T, x)：整数转换（Debug 范围检查，溢出抛错带位置）
                let ty = match &args[0] {
                    Expr::Ident(n, _) => n.clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let v = self.eval(&args[1])?;
                let v = self.deref_value(v);
                let i = match v {
                    Value::Int(i) => i,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                if let Some((min, max)) = Self::int_width_bounds(&ty) {
                    if i < min || i > max {
                        return Err(RtError::new("IntCastOverflow", Some(span.clone())));
                    }
                }
                Ok(Some(Value::Int(i)))
            }
            "@ptrCast" | "@alignCast" => {
                // @ptrCast(T, p) / @alignCast(T, p)：tag1 指针无类型化——透传
                let v = self.eval(
                    args.last()
                        .ok_or_else(|| RtError::new("ArityMismatch", Some(span.clone())))?,
                )?;
                Ok(Some(v))
            }
            "@compileError" => {
                // 语义层应已拦截（编译期错误）；运行时到达 = 未拦截路径
                let msg = if args.is_empty() {
                    "compileError".to_string()
                } else {
                    let v = self.eval(&args[0])?;
                    self.deref_value(v).display()
                };
                Err(RtError::msg(
                    "CompileError",
                    format!("@compileError: {msg}"),
                ))
            }
            "@addWithOverflow" | "@subWithOverflow" | "@mulWithOverflow" => {
                // 返回 (T, bool) 元组；tag1 Int = i128 无溢出（标志恒 false）
                let a = self.eval(&args[0])?;
                let b = self.eval(&args[1])?;
                let a = match self.deref_value(a) {
                    Value::Int(i) => i,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let b = match self.deref_value(b) {
                    Value::Int(i) => i,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let r = match name {
                    "@addWithOverflow" => a.wrapping_add(b),
                    "@subWithOverflow" => a.wrapping_sub(b),
                    _ => a.wrapping_mul(b),
                };
                Ok(Some(Value::arr(vec![Value::Int(r), Value::Bool(false)])))
            }
            "sqrt" => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                match v {
                    Value::Int(i) => Ok(Some(Value::Float((i as f64).sqrt()))),
                    Value::Float(f) => Ok(Some(Value::Float(f.sqrt()))),
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            // min/max（M5.5 工具：i32/f64 数值比较，73-rate-limit 令牌桶）
            "min" | "max" => {
                if args.len() != 2 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let a0 = self.eval(&args[0])?;
                let a = self.deref_value(a0);
                let b0 = self.eval(&args[1])?;
                let b = self.deref_value(b0);
                let take_a = match (&a, &b) {
                    (Value::Int(x), Value::Int(y)) => {
                        if name == "min" {
                            x <= y
                        } else {
                            x >= y
                        }
                    }
                    (Value::Float(x), Value::Float(y)) => {
                        if name == "min" {
                            x <= y
                        } else {
                            x >= y
                        }
                    }
                    (Value::Int(x), Value::Float(y)) => {
                        if name == "min" {
                            (*x as f64) <= *y
                        } else {
                            (*x as f64) >= *y
                        }
                    }
                    (Value::Float(x), Value::Int(y)) => {
                        if name == "min" {
                            *x <= (*y as f64)
                        } else {
                            *x >= (*y as f64)
                        }
                    }
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                Ok(Some(if take_a { a } else { b }))
            }
            // read_u64_le(slice)：8 字节小端 → i64（57-protocol-parse 长度前缀帧）
            "read_u64_le" => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                let b = match self.value_bytes(&v) {
                    Some(b) => b,
                    None => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                if b.len() < 8 {
                    return Err(RtError::new("IndexOutOfBounds", Some(span.clone())));
                }
                let n = u64::from_le_bytes(b[0..8].try_into().unwrap());
                Ok(Some(Value::Int(n as i128)))
            }
            // std 算法（M5.2 最小集）：sort / binary_search
            "sort" => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                let has_cmp = args.len() > 1;
                let cmp_f = if has_cmp {
                    let f = self.eval(&args[1])?;
                    let f = self.deref_value(f);
                    match f {
                        Value::Closure(c) => Some(c),
                        _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                    }
                } else {
                    None
                };
                match v {
                    Value::Arr(a) => {
                        let mut items: Vec<Value> =
                            a.borrow().iter().map(|c| c.borrow().clone()).collect();
                        items.sort_by(|x, y| {
                            match &cmp_f {
                                Some(c) => {
                                    // 比较器闭包返回序（负数/零/正数）
                                    let r = self.call_closure(c, &[x.clone(), y.clone()], span);
                                    match r {
                                        Ok(Value::Int(i)) if i < 0 => std::cmp::Ordering::Less,
                                        Ok(Value::Int(i)) if i > 0 => std::cmp::Ordering::Greater,
                                        Ok(Value::Float(f)) if f < 0.0 => std::cmp::Ordering::Less,
                                        Ok(Value::Float(f)) if f > 0.0 => {
                                            std::cmp::Ordering::Greater
                                        }
                                        _ => std::cmp::Ordering::Equal,
                                    }
                                }
                                None => x.value_lt(y).map_or(std::cmp::Ordering::Equal, |lt| {
                                    if lt {
                                        std::cmp::Ordering::Less
                                    } else if x.value_eq(y) {
                                        std::cmp::Ordering::Equal
                                    } else {
                                        std::cmp::Ordering::Greater
                                    }
                                }),
                            }
                        });
                        for (c, v) in a.borrow().iter().zip(items.iter()) {
                            *c.borrow_mut() = v.clone();
                        }
                        Ok(Some(Value::Void))
                    }
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            // 解析器辅助（71-recursive-parser；操作 &[u8] 与 *usize）
            "skip_space" | "peek" | "advance" | "is_digit" | "parse_number" => {
                return self.call_parser_builtin(name, args, span);
            }
            "parse_int" => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                let s = match v {
                    Value::Str(s) => s.borrow().clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let text = String::from_utf8_lossy(&s).trim().to_string();
                let parsed = if text.is_empty() {
                    None
                } else {
                    text.parse::<i128>().ok()
                };
                Ok(Some(match parsed {
                    Some(n) => Value::Opt(Some(Rc::new(Value::Int(n)))),
                    None => Value::Opt(None),
                }))
            }
            "parse_float" => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                let s = match v {
                    Value::Str(s) => s.borrow().clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let text = String::from_utf8_lossy(&s).trim().to_string();
                let parsed = if text.is_empty() {
                    None
                } else {
                    text.parse::<f64>().ok()
                };
                Ok(Some(match parsed {
                    Some(n) => Value::Opt(Some(Rc::new(Value::Float(n)))),
                    None => Value::Opt(None),
                }))
            }
            "binary_search" => {
                let v = self.eval(&args[0])?;
                let target = self.eval(&args[1])?;
                let v = self.deref_value(v);
                let target = self.deref_value(target);
                let items: Vec<Value> = match &v {
                    Value::Arr(a) => a.borrow().iter().map(|c| c.borrow().clone()).collect(),
                    Value::Slice { data, start, len } => data
                        .borrow()
                        .iter()
                        .skip(*start)
                        .take(*len)
                        .map(|c| c.borrow().clone())
                        .collect(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let mut lo = 0usize;
                let mut hi = items.len();
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    let cmp = items[mid].value_lt(&target);
                    match cmp {
                        Some(true) => lo = mid + 1,
                        Some(false) if items[mid].value_eq(&target) => {
                            return Ok(Some(Value::Opt(Some(Rc::new(Value::Int(mid as i128))))))
                        }
                        _ => hi = mid,
                    }
                }
                Ok(Some(Value::Opt(None)))
            }
            // 断言五件套（Q-T1）：测试函数内隐式可用；3 参形式 = 解析器 expect（71）
            "expect" => {
                if args.len() == 3 {
                    return self.call_parser_builtin(name, args, span);
                }
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                if !v.as_bool() {
                    self.fail_info = Some(format!("expect failed at {}:{}", span.line, span.col));
                    return Err(RtError::new("AssertFailed", Some(span.clone())));
                }
                Ok(Some(Value::Void))
            }
            "expect_eq" | "expect_neq" => {
                if args.len() != 2 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let a = self.eval(&args[0])?;
                let b = self.eval(&args[1])?;
                let a = self.deref_value(a);
                let b = self.deref_value(b);
                let eq = a.value_eq(&b);
                let want_eq = name == "expect_eq";
                if eq != want_eq {
                    self.fail_info = Some(format!(
                        "{} failed at {}:{}: expected {} {}, got {}",
                        name,
                        span.line,
                        span.col,
                        if want_eq { "=" } else { "!=" },
                        b.display(),
                        a.display()
                    ));
                    return Err(RtError::new("AssertFailed", Some(span.clone())));
                }
                Ok(Some(Value::Void))
            }
            "expect_error" => {
                if args.len() != 2 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let want = self.eval(&args[0])?;
                let got = self.eval(&args[1])?;
                let want = self.deref_value(want);
                let got = self.deref_value(got);
                match (want, got) {
                    // M4.2：错误码比较（码全局唯一）
                    (Value::Err { name: w, .. }, Value::Err { name: g, .. }) if w == g => {
                        Ok(Some(Value::Void))
                    }
                    (Value::Err { name: w, .. }, Value::Err { name: g, .. }) => {
                        self.fail_info = Some(format!(
                            "expect_error failed at {}:{}: expected error.{w}, got error.{g}",
                            span.line, span.col
                        ));
                        Err(RtError::new("AssertFailed", Some(span.clone())))
                    }
                    (_, g) => {
                        self.fail_info = Some(format!(
                            "expect_error failed at {}:{}: expected error, got {}",
                            span.line,
                            span.col,
                            g.type_name()
                        ));
                        Err(RtError::new("AssertFailed", Some(span.clone())))
                    }
                }
            }
            "expect_eq_slices" => {
                if args.len() != 2 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let a = self.eval(&args[0])?;
                let b = self.eval(&args[1])?;
                let a = self.deref_value(a);
                let b = self.deref_value(b);
                if !a.value_eq(&b) {
                    self.fail_info = Some(format!(
                        "expect_eq_slices failed at {}:{}: {} != {}",
                        span.line,
                        span.col,
                        a.display(),
                        b.display()
                    ));
                    return Err(RtError::new("AssertFailed", Some(span.clone())));
                }
                Ok(Some(Value::Void))
            }
            _ => Ok(None),
        }
    }

    /// 标量方法（ICompare/INumber 族内建：add/sub/mul/div/neg/mod/abs/eq/lt）
    fn call_scalar_method(
        &mut self,
        self_v: &Value,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        // 整数操作保持整数语义（div 截断、mod 取余）
        if args.len() == 1 {
            let raw = self.eval(&args[0])?;
            let arg_v = self.deref_value(raw);
            if let (Value::Int(a), Value::Int(b)) = (self_v, &arg_v) {
                let i_op = match field {
                    "add" => {
                        Some(Value::Int(a.checked_add(*b).ok_or_else(|| {
                            RtError::new("Overflow", Some(span.clone()))
                        })?))
                    }
                    "sub" => {
                        Some(Value::Int(a.checked_sub(*b).ok_or_else(|| {
                            RtError::new("Overflow", Some(span.clone()))
                        })?))
                    }
                    "mul" => {
                        Some(Value::Int(a.checked_mul(*b).ok_or_else(|| {
                            RtError::new("Overflow", Some(span.clone()))
                        })?))
                    }
                    "div" => {
                        if *b == 0 {
                            return Err(RtError::new("DivisionByZero", Some(span.clone())));
                        }
                        Some(Value::Int(a / b))
                    }
                    "mod" => {
                        if *b == 0 {
                            return Err(RtError::new("DivisionByZero", Some(span.clone())));
                        }
                        Some(Value::Int(a % b))
                    }
                    "eq" => Some(Value::Bool(a == b)),
                    "lt" => Some(Value::Bool(a < b)),
                    _ => None,
                };
                if let Some(v) = i_op {
                    return Ok(Some(v));
                }
            }
        }
        // 一元整数操作
        if args.is_empty() {
            if let Value::Int(a) = self_v {
                let i_op = match field {
                    "neg" => Some(Value::Int(-*a)),
                    "abs" => Some(Value::Int(a.abs())),
                    _ => None,
                };
                if let Some(v) = i_op {
                    return Ok(Some(v));
                }
            }
        }
        let v = match self_v {
            Value::Int(i) => *i as f64,
            Value::Float(f) => *f,
            _ => return Ok(None),
        };
        let mut one_arg = |ix: &[Expr]| -> std::result::Result<f64, RtError> {
            let a = self.eval(&ix[0])?;
            let a = self.deref_value(a);
            match a {
                Value::Int(i) => Ok(i as f64),
                Value::Float(f) => Ok(f),
                _ => Err(RtError::new("TypeError", Some(span.clone()))),
            }
        };
        let r = match field {
            "add" => v + one_arg(args)?,
            "sub" => v - one_arg(args)?,
            "mul" => v * one_arg(args)?,
            "div" => v / one_arg(args)?,
            "mod" => v % one_arg(args)?,
            "neg" => -v,
            "abs" => v.abs(),
            "pow" => v.powf(one_arg(args)?),
            "eq" | "lt" => {
                let other = one_arg(args)?;
                let b = match field {
                    "eq" => v == other,
                    _ => v < other,
                };
                return Ok(Some(Value::Bool(b)));
            }
            _ => return Ok(None),
        };
        // 整数保持整数（无小数部分时）
        if r.fract() == 0.0 && r.is_finite() && r.abs() < 9e18 {
            Ok(Some(Value::Int(r as i128)))
        } else {
            Ok(Some(Value::Float(r)))
        }
    }

    fn call_builtin_method(
        &mut self,
        self_v: &Value,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        // 标量方法（INumber/ICompare 族：a.add(b) ≡ a + b）
        if matches!(self_v, Value::Int(_) | Value::Float(_)) {
            if let Some(v) = self.call_scalar_method(self_v, field, args, span)? {
                return Ok(Some(v));
            }
        }
        match (self_v, field) {
            (Value::Str(s), "concat") => {
                let other = self.eval(&args[0])?;
                let other = self.deref_value(other);
                if let Value::Str(os) = other {
                    let mut bytes = s.borrow().clone();
                    bytes.extend_from_slice(&os.borrow());
                    return Ok(Some(Value::str_bytes(bytes)));
                }
                Err(RtError::new("TypeError", Some(span.clone())))
            }
            (Value::Str(s), "as_slice") => Ok(Some(Value::Str(s.clone()))),
            (Value::Str(s), "split") => {
                // 按分隔符切分（返回 Vec of String）
                let sep_v = self.eval(&args[0])?;
                let sep_v = self.deref_value(sep_v);
                let sep = match sep_v {
                    Value::Int(i) => vec![i as u8],
                    Value::Str(ss) => ss.borrow().clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let data = s.borrow().clone();
                let mut out = Vec::new();
                if sep.is_empty() {
                    return Ok(Some(Value::arr(vec![Value::str_bytes(data)])));
                }
                let mut start = 0usize;
                let mut i = 0usize;
                while i + sep.len() <= data.len() {
                    if &data[i..i + sep.len()] == sep.as_slice() {
                        out.push(Value::str_bytes(data[start..i].to_vec()));
                        i += sep.len();
                        start = i;
                    } else {
                        i += 1;
                    }
                }
                out.push(Value::str_bytes(data[start..].to_vec()));
                Ok(Some(Value::arr(out)))
            }
            (Value::Str(s), "to_bytes") => {
                // 序列化格式：[u64 LE 长度][utf8]
                let b = s.borrow();
                let mut out = (b.len() as u64).to_le_bytes().to_vec();
                out.extend_from_slice(&b);
                Ok(Some(Value::str_bytes(out)))
            }
            (Value::Str(s), "find") => {
                let needle = self.eval(&args[0])?;
                let needle = self.deref_value(needle);
                let needle_bytes: Vec<u8> = match &needle {
                    Value::Str(n) => n.borrow().clone(),
                    Value::Int(i) => vec![*i as u8],
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let data = s.borrow().clone();
                let pos = if needle_bytes.is_empty() {
                    Some(0usize)
                } else {
                    data.windows(needle_bytes.len())
                        .position(|w| w == needle_bytes.as_slice())
                };
                Ok(Some(match pos {
                    Some(p) => Value::Opt(Some(Rc::new(Value::Int(p as i128)))),
                    None => Value::Opt(None),
                }))
            }
            (Value::Str(s), "substring") => {
                let lo = self.eval(&args[0])?;
                let hi = self.eval(&args[1])?;
                let lo = self.deref_value(lo);
                let hi = self.deref_value(hi);
                let (lo, hi) = match (lo, hi) {
                    (Value::Int(a), Value::Int(b)) => (a.max(0) as usize, b.max(0) as usize),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let data = s.borrow();
                let hi = hi.min(data.len());
                let sub = data[lo.min(hi)..hi].to_vec();
                Ok(Some(Value::str_bytes(sub)))
            }
            (Value::Str(s), "replace") => {
                let from = self.eval(&args[0])?;
                let to = self.eval(&args[1])?;
                let from = self.deref_value(from);
                let to = self.deref_value(to);
                let (from_b, to_b) = match (&from, &to) {
                    (Value::Str(a), Value::Str(b)) => (a.borrow().clone(), b.borrow().clone()),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let data = s.borrow().clone();
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
                Ok(Some(Value::str_bytes(out)))
            }
            (Value::Str(_), "len") => Ok(Some(Value::Int(self_v.display().len() as i128))),
            (Value::Arr(_a), "len") => {
                if let Value::Arr(a) = self_v {
                    Ok(Some(Value::Int(a.borrow().len() as i128)))
                } else {
                    unreachable!()
                }
            }
            (Value::Arr(a), "append") => {
                let v = self.eval(&args[0])?;
                a.borrow_mut().push(Rc::new(RefCell::new(v)));
                Ok(Some(Value::Void))
            }
            // extend(other)：追加另一集合/字节串的全部元素（57-protocol-parse 帧拼接）
            (Value::Arr(a), "extend") => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                match v {
                    Value::Arr(src) => {
                        for c in src.borrow().iter() {
                            a.borrow_mut().push(c.clone());
                        }
                        Ok(Some(Value::Void))
                    }
                    Value::Str(b) => {
                        for byte in b.borrow().iter() {
                            a.borrow_mut()
                                .push(Rc::new(RefCell::new(Value::Int(*byte as i128))));
                        }
                        Ok(Some(Value::Void))
                    }
                    _ => Err(RtError::new("TypeError", Some(span.clone()))),
                }
            }
            // append_u64(v)：u64 LE 8 字节追加为元素（57-protocol-parse 长度前缀）
            (Value::Arr(a), "append_u64") => {
                let v = self.eval(&args[0])?;
                let v = self.deref_value(v);
                let n = match v {
                    Value::Int(i) => i as u64,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                for byte in n.to_le_bytes() {
                    a.borrow_mut()
                        .push(Rc::new(RefCell::new(Value::Int(byte as i128))));
                }
                Ok(Some(Value::Void))
            }
            // Vec(i32).init(alloc)：集合空容器
            (Value::Arr(_), "init") => Ok(Some(Value::arr(vec![]))),
            // Vec(i32).from_bytes 集合反序列化（u64 长度前缀 + i32 元素）
            (Value::Arr(_), "from_bytes") => {
                let bytes = self.eval(&args[0])?;
                let bytes = self.deref_value(bytes);
                let b = match bytes {
                    Value::Str(s) => s.borrow().clone(),
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                if b.len() < 8 {
                    return Err(RtError::new("InvalidBytes", Some(span.clone())));
                }
                let n = u64::from_le_bytes(b[0..8].try_into().unwrap()) as usize;
                let mut items = Vec::new();
                let mut pos = 8usize;
                for _ in 0..n {
                    let v = if b.len() >= pos + 4 {
                        let i = i32::from_le_bytes(b[pos..pos + 4].try_into().unwrap());
                        pos += 4;
                        Value::Int(i as i128)
                    } else {
                        break;
                    };
                    items.push(v);
                }
                Ok(Some(Value::arr(items)))
            }
            // 迭代器链（12.8：立即求值变换，产生新数据对象）
            (Value::Arr(a), "iter") => Ok(Some(Value::Arr(a.clone()))),
            (Value::Slice { data, start, len }, "iter") => {
                let d = data.borrow();
                let items: Vec<Value> = d[*start..*start + *len]
                    .iter()
                    .map(|c| c.borrow().clone())
                    .collect();
                drop(d);
                Ok(Some(Value::arr(items)))
            }
            (Value::Arr(a), "filter") => {
                let f = self.eval(&args[0])?;
                let f = self.deref_value(f);
                if let Value::Closure(closure) = f {
                    let src = a.borrow().clone();
                    let mut out = Vec::new();
                    for cell in &src {
                        let item = cell.borrow().clone();
                        let keep = self.call_closure_bool(&closure, &[item.clone()], span)?;
                        if keep {
                            out.push(item);
                        }
                    }
                    Ok(Some(Value::arr(out)))
                } else {
                    Err(RtError::new("TypeError", Some(span.clone())))
                }
            }
            (Value::Arr(a), "map") => {
                let f = self.eval(&args[0])?;
                let f = self.deref_value(f);
                if let Value::Closure(closure) = f {
                    let src = a.borrow().clone();
                    let mut out = Vec::new();
                    for cell in src {
                        let item = cell.borrow().clone();
                        let mapped = self.call_closure_value(&closure, &[item], span)?;
                        out.push(mapped);
                    }
                    Ok(Some(Value::arr(out)))
                } else {
                    Err(RtError::new("TypeError", Some(span.clone())))
                }
            }
            // Map 方法（Map = class 实例，字段即键值）
            (Value::Class(c), "put") if c.borrow().name == "Map" => {
                let k = self.eval(&args[0])?;
                let v = self.eval(&args[1])?;
                let key = k.display();
                c.borrow_mut().fields.insert(key, v);
                Ok(Some(Value::Void))
            }
            (Value::Class(c), "get") if c.borrow().name == "Map" => {
                let k = self.eval(&args[0])?;
                let key = k.display();
                let v = c.borrow().fields.get(&key).cloned();
                Ok(Some(match v {
                    Some(x) => Value::Opt(Some(Rc::new(x))),
                    None => Value::Opt(None),
                }))
            }
            (Value::Class(c), "contains") if c.borrow().name == "Map" => {
                let k = self.eval(&args[0])?;
                let key = k.display();
                Ok(Some(Value::Bool(c.borrow().fields.contains_key(&key))))
            }
            (Value::Class(c), "remove") if c.borrow().name == "Map" => {
                let k = self.eval(&args[0])?;
                let key = k.display();
                c.borrow_mut().fields.remove(&key);
                Ok(Some(Value::Void))
            }
            (Value::Class(c), "len") if c.borrow().name == "Map" => {
                Ok(Some(Value::Int(c.borrow().fields.len() as i128)))
            }
            (
                Value::Slice {
                    data: _,
                    start: _,
                    len,
                },
                "len",
            ) => Ok(Some(Value::Int(*len as i128))),
            // 分配器方法
            (Value::Alloc, "init") => {
                // alloc.init(T) / alloc.init(T{...})
                if args.len() != 1 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                // 无参构造 alloc.init(T)：按类型创建空实例（字段逐赋值，definite assignment M2.5）
                if let Expr::Ident(tname, _) = &args[0] {
                    if let Some(TypeDef::Class { fields, .. }) = self.types.get(tname) {
                        let mut f = HashMap::new();
                        for fd in fields {
                            f.insert(fd.name.clone(), self.default_value(Some(&fd.ty))?);
                        }
                        return Ok(Some(Value::class(tname, f)));
                    }
                    if self.types.contains_key(tname) {
                        // 枚举等：空变体
                        return Ok(Some(Value::Enum {
                            name: tname.clone(),
                            variant: "__none__".into(),
                            payload: None,
                        }));
                    }
                }
                // 带参构造 alloc.init(T{...})：字面量求值即实例
                let v = self.eval(&args[0])?;
                Ok(Some(v))
            }
            (Value::Alloc, "alloc") => {
                let n = self.eval(&args[0])?;
                let n = self.deref_value(n);
                if let Value::Int(i) = n {
                    Ok(Some(Value::str_bytes(vec![0u8; i.max(0) as usize])))
                } else {
                    Err(RtError::new("TypeError", Some(span.clone())))
                }
            }
            (Value::Alloc, "deinit") => Ok(Some(Value::Void)),
            // Arena 方法
            (Value::Class(c), "alloc") if c.borrow().name == "Arena" => {
                // arena.alloc(n) 字节 / arena.alloc(Node{...}) 类型字面量
                if args.len() == 1 {
                    if let Expr::IntLit { .. } = &args[0] {
                        let n = self.eval(&args[0])?;
                        let n = self.deref_value(n);
                        if let Value::Int(i) = n {
                            return Ok(Some(Value::str_bytes(vec![0u8; i.max(0) as usize])));
                        }
                    }
                    let v = self.eval(&args[0])?;
                    return Ok(Some(v));
                }
                Err(RtError::new("ArityMismatch", Some(span.clone())))
            }
            (Value::Class(c), "init") if c.borrow().name == "Arena" => {
                // arena.init(T)：类型构造（tag1：仅返回空实例）
                let _ = args;
                Ok(Some(Value::Void))
            }
            // Io 方法
            (Value::Class(c), "print") if c.borrow().name == "Io" => {
                self.call_io_print(args, span)?;
                Ok(Some(Value::Void))
            }
            // io.exit(ExitType, code)（M4.2：Exit 静默正常 / Error 错误退出打印）
            (Value::Class(c), "exit") if c.borrow().name == "Io" => {
                if args.len() != 2 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let t = self.eval(&args[0])?;
                let code = self.eval(&args[1])?;
                let t = self.deref_value(t);
                let code = match self.deref_value(code) {
                    Value::Int(i) => i.clamp(0, 255) as u8,
                    _ => return Err(RtError::new("TypeError", Some(span.clone()))),
                };
                let is_error = matches!(t, Value::Enum { variant, .. } if variant == "Error");
                if is_error {
                    eprintln!("error: program exited with code {code}");
                }
                self.exit_code = Some(code);
                // 中止执行（正常退出信号）
                Err(RtError::msg("ExitRequested", format!("code {code}")))
            }
            // 序列化内建（M4.4；所有数据类型天生可序列化）
            (Value::Class(c), "to_bytes") => {
                let d = c.borrow();
                let bytes = self.class_to_bytes(&d.name, &d.fields)?;
                Ok(Some(Value::str_bytes(bytes)))
            }
            (Value::Class(c), "to_json") => {
                let _d = c.borrow();
                let json = self.value_to_json(&Value::Class(c.clone()));
                Ok(Some(Value::str(&json)))
            }
            (Value::Arr(a), "to_bytes") => {
                // 集合 → 字节（u64 LE 前缀 + 元素，M4.4）
                let items = a.borrow().clone();
                let mut out = Vec::new();
                out.extend_from_slice(&(items.len() as u64).to_le_bytes());
                for cell in items {
                    let v = cell.borrow().clone();
                    out.extend(self.value_to_bytes(&v));
                }
                Ok(Some(Value::str_bytes(out)))
            }
            (Value::Class(c), "from_json") if c.borrow().name == "Map" => {
                let json = self.eval(&args[0])?;
                let json = self.deref_value(json);
                if let Value::Str(s) = json {
                    let s = s.borrow().clone();
                    let obj = self.parse_json_obj(&String::from_utf8_lossy(&s))?;
                    let mut f = HashMap::new();
                    for (k, v) in obj {
                        f.insert(k, v);
                    }
                    Ok(Some(Value::class("Map", f)))
                } else {
                    Err(RtError::new("TypeError", Some(span.clone())))
                }
            }
            // M5.4 真实 IO：io.fs 模块函数 / io.time / File 句柄方法
            (Value::Class(c), m) if c.borrow().name == "Fs" => self.call_fs_method(m, args, span),
            (Value::Class(c), m) if c.borrow().name == "Time" => {
                self.call_time_method(m, args, span)
            }
            (Value::Class(c), m) if c.borrow().name == "File" => {
                let v = Value::Class(c.clone());
                self.call_file_method(m, &v, args, span)
            }
            // M5.4 程序环境：io.args() / io.env(name)
            (Value::Class(c), "args") if c.borrow().name == "Io" => Ok(Some(Value::arr(
                self.args.iter().map(|a| Value::str(a)).collect(),
            ))),
            (Value::Class(c), "env") if c.borrow().name == "Io" => {
                let name = self.eval_str_arg(args, 0, span)?;
                match std::env::var(String::from_utf8_lossy(&name).as_ref()) {
                    Ok(v) => Ok(Some(Value::Opt(Some(Rc::new(Value::str(&v)))))),
                    Err(_) => Ok(Some(Value::Opt(None))),
                }
            }
            _ => Ok(None),
        }
    }

    fn call_io_print(&mut self, args: &[Expr], span: &Span) -> Result<()> {
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
            if fmt[i] == b'{' && i + 1 < fmt.len() && fmt[i + 1] == b'}' {
                if argi < args.len() {
                    let v = self.eval(&args[argi])?;
                    let v = self.deref_value(v);
                    out.extend_from_slice(v.display().as_bytes());
                    argi += 1;
                }
                i += 2;
            } else if fmt[i] == b'{'
                && i + 2 < fmt.len()
                && fmt[i + 2] == b'}'
                && (fmt[i + 1] == b'x' || fmt[i + 1] == b'b' || fmt[i + 1] == b's')
            {
                // {x} {b} {s} 说明符
                let spec = fmt[i + 1];
                if argi < args.len() {
                    let v = self.eval(&args[argi])?;
                    let v = self.deref_value(v);
                    match spec {
                        b'x' => match &v {
                            Value::Int(n) => out.extend_from_slice(format!("{n:x}").as_bytes()),
                            _ => out.extend_from_slice(v.display().as_bytes()),
                        },
                        b'b' => match &v {
                            Value::Int(n) => out.extend_from_slice(format!("{n:b}").as_bytes()),
                            _ => out.extend_from_slice(v.display().as_bytes()),
                        },
                        _ => out.extend_from_slice(v.display().as_bytes()),
                    }
                    argi += 1;
                }
                i += 3;
            } else {
                out.push(fmt[i]);
                i += 1;
            }
        }
        let line = String::from_utf8_lossy(&out).to_string();
        if self.in_main {
            print!("{line}");
        } else {
            self.test_out.push(line);
        }
        Ok(())
    }

    // ---------- M5.4 真实 IO：io.fs / io.time / File 句柄 ----------

    /// 求值第 i 个参数并解引用为字节串（数据参数）
    fn eval_str_arg(&mut self, args: &[Expr], i: usize, span: &Span) -> Result<Vec<u8>> {
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
    fn eval_path_arg(&mut self, args: &[Expr], i: usize, span: &Span) -> Result<String> {
        let b = self.eval_str_arg(args, i, span)?;
        Ok(String::from_utf8_lossy(&b).into_owned())
    }

    /// 求值第 i 个参数为整数
    fn eval_int_arg(&mut self, args: &[Expr], i: usize, span: &Span) -> Result<i128> {
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
    fn file_fd(&self, v: &Value, span: &Span) -> Result<i64> {
        match self.deref_value(v.clone()) {
            Value::Class(c) if c.borrow().name == "File" => match c.borrow().fields.get("_fd") {
                Some(Value::Int(fd)) => Ok(*fd as i64),
                _ => Err(RtError::new("BadFd", Some(span.clone()))),
            },
            _ => Err(RtError::new("TypeError", Some(span.clone()))),
        }
    }

    /// std::io::Error → H 错误名（20-errors 错误集：NotFound/PermissionDenied/其它 Io）
    fn io_error_name(&self, e: &std::io::Error) -> String {
        match e.kind() {
            std::io::ErrorKind::NotFound => "NotFound".into(),
            std::io::ErrorKind::PermissionDenied => "PermissionDenied".into(),
            _ => "Io".into(),
        }
    }

    /// 注册真实文件句柄 → File 值（内部 `_fd` 字段）
    fn register_file(&mut self, f: std::fs::File) -> Value {
        let fd = self.next_fd;
        self.next_fd += 1;
        self.files.insert(fd, f);
        let mut fields = HashMap::new();
        fields.insert("_fd".into(), Value::Int(fd as i128));
        Value::class("File", fields)
    }

    fn call_fs_method(&mut self, field: &str, args: &[Expr], span: &Span) -> Result<Option<Value>> {
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
            // io.fs.create(path)：创建/截断供写入
            "create" => {
                let path = self.eval_path_arg(args, 0, span)?;
                match std::fs::OpenOptions::new()
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
            // io.fs.list_dir(path)：目录条目名
            "list_dir" => {
                let path = self.eval_path_arg(args, 0, span)?;
                match std::fs::read_dir(&path) {
                    Ok(rd) => {
                        let names: Vec<Value> = rd
                            .flatten()
                            .map(|e| Value::str(&e.file_name().to_string_lossy()))
                            .collect();
                        Ok(Some(Value::arr(names)))
                    }
                    Err(e) => Ok(Some(self.err_val(&self.io_error_name(&e)))),
                }
            }
            _ => Ok(None),
        }
    }

    fn call_file_method(
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
            _ => Ok(None),
        }
    }

    fn call_time_method(
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
            _ => Ok(None),
        }
    }

    /// `X.new(args, alloc)` 兼容构造（C1 审计后旧示例；tag1 仅支持 value 类构造）
    fn call_new_builtin(&mut self, ty: &str, args: &[Expr], span: &Span) -> Result<Value> {
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
    fn call_parser_builtin(
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

    /// String 内建静态方法（String = 内建新类型，M3 定案；tag1：from/from_slice/concat）
    fn call_string_builtin(&mut self, field: &str, args: &[Expr], span: &Span) -> Result<Value> {
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

    fn call_math(
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

    // ---------- 序列化内建辅助（M4.4；连续类型 byte 化、class json 化） ----------

    /// 标量声明宽度字节数（自然对齐 = 宽度）
    fn scalar_size(n: &str) -> usize {
        match n {
            "i8" | "u8" | "bool" => 1,
            "i16" | "u16" => 2,
            "i32" | "u32" | "f32" => 4,
            _ => 8, // i64/u64/isize/usize/f64/f16/f128
        }
    }

    /// M4.3：连续 class 布局——字段 (名, 偏移, 大小) 列表 + 总大小
    /// （与 to_bytes 直映射一致：自然对齐 + 字段间填充 + 尾部圆整；嵌套连续视为对齐 1）
    fn continuous_layout(&self, ty: &str) -> Option<(Vec<(String, usize, usize)>, usize)> {
        let (fdecls, traits) = match self.types.get(ty) {
            Some(TypeDef::Class { fields, traits, .. }) => (fields, traits),
            _ => return None,
        };
        if !traits.iter().any(|t| matches!(t, Trait::Continuous)) {
            return None;
        }
        let mut layout: Vec<(String, usize, usize)> = Vec::new();
        let mut offset = 0usize;
        let mut max_align = 1usize;
        for fd in fdecls {
            let size = match self.field_serialized_size(&fd.ty) {
                Some(s) => s,
                None => continue, // 非标量字段不占字节（与 class_to_bytes 一致）
            };
            let align = if matches!(fd.ty.strip(), Type::Named(n, _) if Self::scalar_size(n) == size && !self.is_nested_continuous(n))
            {
                size
            } else {
                1 // 嵌套连续（对齐 1，与 class_to_bytes 一致）
            };
            max_align = max_align.max(align);
            while offset % align != 0 {
                offset += 1;
            }
            layout.push((fd.name.clone(), offset, size));
            offset += size;
        }
        while offset % max_align != 0 {
            offset += 1;
        }
        Some((layout, offset))
    }

    fn is_nested_continuous(&self, n: &str) -> bool {
        matches!(
            self.types.get(n),
            Some(TypeDef::Class { traits, .. })
                if traits.iter().any(|t| matches!(t, Trait::Continuous))
        )
    }

    /// 字段序列化字节大小（连续布局用；标量 / 嵌套连续 / 元组）
    fn field_serialized_size(&self, t: &Type) -> Option<usize> {
        match t.strip() {
            Type::Named(n, _) => {
                if Self::is_scalar_name(n) {
                    Some(Self::scalar_size(n))
                } else if self.is_nested_continuous(n) {
                    self.continuous_layout(n).map(|(_, size)| size)
                } else {
                    None
                }
            }
            Type::Tuple(ts) => {
                let mut s = 0usize;
                for x in ts {
                    s += self.field_serialized_size(x)?;
                }
                Some(s)
            }
            _ => None,
        }
    }

    fn is_scalar_name(n: &str) -> bool {
        matches!(
            n,
            "i8" | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "isize"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "usize"
                | "f16"
                | "f32"
                | "f64"
                | "f128"
                | "bool"
        )
    }

    /// M4.3：@intCast 目标宽度范围（Debug 溢出检查）
    fn int_width_bounds(n: &str) -> Option<(i128, i128)> {
        match n {
            "i8" => Some((i8::MIN as i128, i8::MAX as i128)),
            "i16" => Some((i16::MIN as i128, i16::MAX as i128)),
            "i32" => Some((i32::MIN as i128, i32::MAX as i128)),
            "i64" => Some((i64::MIN as i128, i64::MAX as i128)),
            "i128" => Some((i128::MIN, i128::MAX)),
            "isize" => Some((isize::MIN as i128, isize::MAX as i128)),
            "u8" => Some((0, u8::MAX as i128)),
            "u16" => Some((0, u16::MAX as i128)),
            "u32" => Some((0, u32::MAX as i128)),
            "u64" => Some((0, u64::MAX as i128)),
            "u128" => Some((0, u128::MAX as i128)),
            "usize" => Some((0, usize::MAX as i128)),
            _ => None,
        }
    }

    /// M4.3：@sizeOf(T) 类型字节大小
    fn type_size_of(&self, ty: &str) -> Option<usize> {
        match ty {
            "i8" | "u8" | "bool" => Some(1),
            "i16" | "u16" | "f16" => Some(2),
            "i32" | "u32" | "f32" => Some(4),
            "i128" | "u128" | "f128" => Some(16),
            "i64" | "u64" | "isize" | "usize" | "f64" => Some(8),
            // 引用类型（String/集合/Table/堆上 class/指针/切片）= 指针宽
            "String" | "Vec" | "Map" | "Deque" | "Table" | "Allocator" => Some(8),
            _ => match self.types.get(ty) {
                Some(TypeDef::Class { traits, .. })
                    if traits.iter().any(|t| matches!(t, Trait::Continuous)) =>
                {
                    self.continuous_layout(ty).map(|(_, size)| size)
                }
                Some(TypeDef::Class { .. }) => Some(8), // 堆上 = 指针
                Some(TypeDef::Enum { variants }) => {
                    // 纯常量枚举 1 字节；带负载 = 最大负载大小（简化）
                    if variants.iter().all(|v| v.payload.is_none()) {
                        Some(1)
                    } else {
                        let mut max_s = 1usize;
                        for v in variants {
                            if let Some(p) = &v.payload {
                                if let Some(s) = self.field_serialized_size(p) {
                                    max_s = max_s.max(s);
                                }
                            }
                        }
                        Some(max_s)
                    }
                }
                Some(TypeDef::Interface { .. }) => Some(8),
                _ => None,
            },
        }
    }

    /// class 实例 → 字节（字段按声明类型打包 + C 结构体自然对齐，M4.4 直映射）
    fn class_to_bytes(&self, ty: &str, fields: &HashMap<String, Value>) -> Result<Vec<u8>> {
        let Some(TypeDef::Class { fields: fdecls, .. }) = self.types.get(ty) else {
            return Err(RtError::msg("UnknownType", format!("unknown type `{ty}`")));
        };
        let mut out = Vec::new();
        let mut offset = 0usize;
        let mut max_align = 1usize;
        for fd in fdecls {
            let v = fields.get(&fd.name).unwrap_or(&Value::Void);
            let v = self.deref_value(v.clone());
            let n = match fd.ty.strip() {
                Type::Named(n, _) => n.clone(),
                _ => continue,
            };
            // 嵌套 class：递归序列化（内部自含对齐），视为对齐 1
            if let Value::Class(c) = &v {
                if c.borrow().name == n {
                    let cd = c.borrow();
                    out.extend(self.class_to_bytes(&n, &cd.fields)?);
                    offset += out.len();
                    continue;
                }
            }
            if matches!(v, Value::Void) {
                continue;
            }
            let size = Self::scalar_size(&n);
            let align = size; // 自然对齐
            max_align = max_align.max(align);
            while offset % align != 0 {
                out.push(0);
                offset += 1;
            }
            match (&n[..], &v) {
                ("i8" | "u8", Value::Int(i)) => out.push(*i as u8),
                ("i16" | "u16", Value::Int(i)) => out.extend_from_slice(&(*i as i16).to_le_bytes()),
                ("i32" | "u32", Value::Int(i)) => out.extend_from_slice(&(*i as i32).to_le_bytes()),
                ("i64" | "u64" | "isize" | "usize", Value::Int(i)) => {
                    out.extend_from_slice(&(*i as i64).to_le_bytes())
                }
                ("f32", Value::Float(f)) => out.extend_from_slice(&(*f as f32).to_le_bytes()),
                ("f64" | "f16" | "f128", Value::Float(f)) => {
                    out.extend_from_slice(&f.to_le_bytes())
                }
                ("bool", Value::Bool(b)) => out.push(if *b { 1 } else { 0 }),
                _ => {}
            }
            offset += size;
        }
        // 结构体尾部对齐：总大小圆整到最大字段对齐
        while offset % max_align != 0 {
            out.push(0);
            offset += 1;
        }
        Ok(out)
    }

    /// bytes → class 实例（按字段声明类型解析 + 自然对齐跳填充）
    fn class_from_bytes(&self, ty: &str, bytes: &[u8]) -> Result<Value> {
        let Some(TypeDef::Class { fields: fdecls, .. }) = self.types.get(ty) else {
            return Err(RtError::msg("UnknownType", format!("unknown type `{ty}`")));
        };
        let mut pos = 0usize;
        let mut f = HashMap::new();
        for fd in fdecls {
            let n = match fd.ty.strip() {
                Type::Named(n, _) => n.clone(),
                _ => {
                    f.insert(fd.name.clone(), Value::Void);
                    continue;
                }
            };
            let size = Self::scalar_size(&n);
            let align = size; // 自然对齐
            while pos % align != 0 {
                pos += 1; // 跳过填充字节
            }
            let v = match (&n[..], pos) {
                ("i8" | "u8", _) => {
                    let v = bytes.get(pos).copied().unwrap_or(0);
                    pos += 1;
                    Value::Int(v as i128)
                }
                ("i16" | "u16", _) => {
                    let b = bytes
                        .get(pos..pos + 2)
                        .ok_or_else(|| RtError::msg("InvalidBytes", "truncated byte data"))?;
                    let v = i16::from_le_bytes(b.try_into().unwrap());
                    pos += 2;
                    Value::Int(v as i128)
                }
                ("i32" | "u32", _) => {
                    let b = bytes
                        .get(pos..pos + 4)
                        .ok_or_else(|| RtError::msg("InvalidBytes", "truncated byte data"))?;
                    let v = i32::from_le_bytes(b.try_into().unwrap());
                    pos += 4;
                    Value::Int(v as i128)
                }
                ("i64" | "u64" | "isize" | "usize", _) => {
                    let b = bytes
                        .get(pos..pos + 8)
                        .ok_or_else(|| RtError::msg("InvalidBytes", "truncated byte data"))?;
                    let v = i64::from_le_bytes(b.try_into().unwrap());
                    pos += 8;
                    Value::Int(v as i128)
                }
                ("f32", _) => {
                    let b = bytes
                        .get(pos..pos + 4)
                        .ok_or_else(|| RtError::msg("InvalidBytes", "truncated byte data"))?;
                    let v = f32::from_le_bytes(b.try_into().unwrap()) as f64;
                    pos += 4;
                    Value::Float(v)
                }
                ("f64" | "f16" | "f128", _) => {
                    let b = bytes
                        .get(pos..pos + 8)
                        .ok_or_else(|| RtError::msg("InvalidBytes", "truncated byte data"))?;
                    let v = f64::from_le_bytes(b.try_into().unwrap());
                    pos += 8;
                    Value::Float(v)
                }
                ("bool", _) => {
                    let v = bytes.get(pos).copied().unwrap_or(0);
                    pos += 1;
                    Value::Bool(v != 0)
                }
                _ => {
                    // 嵌套 class：递归解析（长度未知——tag1 不支持连续类型嵌套反序列化边界，跳过）
                    Value::Void
                }
            };
            f.insert(fd.name.clone(), v);
        }
        Ok(Value::class(ty, f))
    }

    /// 任意值 → 字节（标量/嵌套；Int 在 i32 范围用 4 字节——i32 元素集合 12 字节）
    fn value_to_bytes(&self, v: &Value) -> Vec<u8> {
        match v {
            Value::Int(i) => {
                if *i >= i32::MIN as i128 && *i <= i32::MAX as i128 {
                    (*i as i32).to_le_bytes().to_vec()
                } else {
                    (*i as i64).to_le_bytes().to_vec()
                }
            }
            Value::Float(f) => f.to_le_bytes().to_vec(),
            Value::Bool(b) => vec![if *b { 1 } else { 0 }],
            Value::Str(s) => {
                let b = s.borrow();
                let mut out = (b.len() as u64).to_le_bytes().to_vec();
                out.extend_from_slice(&b);
                out
            }
            Value::Class(c) => {
                let d = c.borrow();
                self.class_to_bytes(&d.name, &d.fields).unwrap_or_default()
            }
            Value::Ptr(p) => self.value_to_bytes(&p.borrow()),
            _ => vec![],
        }
    }

    /// 任意值 → JSON 字符串
    fn value_to_json(&self, v: &Value) -> String {
        match v {
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Str(s) => {
                let s = String::from_utf8_lossy(&s.borrow()).to_string();
                format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
            }
            Value::Arr(a) => {
                let items: Vec<String> = a
                    .borrow()
                    .iter()
                    .map(|c| self.value_to_json(&c.borrow()))
                    .collect();
                format!("[{}]", items.join(","))
            }
            Value::Class(c) => {
                let d = c.borrow();
                if d.name == "Map" {
                    let items: Vec<String> = d
                        .fields
                        .iter()
                        .map(|(k, v)| format!("\"{k}\":{}", self.value_to_json(v)))
                        .collect();
                    format!("{{{}}}", items.join(","))
                } else {
                    let items: Vec<String> = d
                        .fields
                        .iter()
                        .map(|(k, v)| format!("\"{k}\":{}", self.value_to_json(v)))
                        .collect();
                    format!("{{{}}}", items.join(","))
                }
            }
            Value::Opt(Some(v)) => self.value_to_json(v),
            Value::Opt(None) => "null".to_string(),
            Value::Ptr(p) => self.value_to_json(&p.borrow()),
            _ => "null".to_string(),
        }
    }

    /// JSON 对象字符串 → (key, value) 表（tag1：仅标量字段）
    fn parse_json_obj(&self, s: &str) -> Result<HashMap<String, Value>> {
        let mut out = HashMap::new();
        let s = s.trim();
        let inner = s
            .strip_prefix('{')
            .and_then(|r| r.strip_suffix('}'))
            .unwrap_or(s)
            .trim();
        if inner.is_empty() {
            return Ok(out);
        }
        for part in inner.split(',') {
            let part = part.trim();
            let Some(eq) = part.find(':') else { continue };
            let key = part[..eq].trim().trim_matches('"').to_string();
            let val = part[eq + 1..].trim();
            let v = if val == "true" {
                Value::Bool(true)
            } else if val == "false" {
                Value::Bool(false)
            } else if val.starts_with('"') {
                Value::str(val.trim_matches('"'))
            } else if let Ok(i) = val.parse::<i128>() {
                Value::Int(i)
            } else if let Ok(f) = val.parse::<f64>() {
                Value::Float(f)
            } else {
                continue;
            };
            out.insert(key, v);
        }
        Ok(out)
    }

    /// JSON 对象 → class 实例（匹配字段名）
    fn class_from_json(&self, ty: &str, obj: &HashMap<String, Value>) -> Result<Value> {
        let Some(TypeDef::Class { fields: fdecls, .. }) = self.types.get(ty) else {
            return Err(RtError::msg("UnknownType", format!("unknown type `{ty}`")));
        };
        let mut f = HashMap::new();
        for fd in fdecls {
            let v = obj
                .get(&fd.name)
                .cloned()
                .unwrap_or_else(|| self.default_value(Some(&fd.ty)).unwrap_or(Value::Void));
            f.insert(fd.name.clone(), v);
        }
        Ok(Value::class(ty, f))
    }

    fn deep_copy(&self, v: Value) -> Value {
        match v {
            Value::Arr(a) => {
                let items: Vec<Value> = a
                    .borrow()
                    .iter()
                    .map(|c| self.deep_copy(c.borrow().clone()))
                    .collect();
                Value::arr(items)
            }
            Value::Class(c) => {
                let d = c.borrow();
                let fields: HashMap<String, Value> = d
                    .fields
                    .iter()
                    .map(|(k, v)| (k.clone(), self.deep_copy(v.clone())))
                    .collect();
                Value::class(&d.name, fields)
            }
            Value::Ptr(p) => Value::Ptr(Rc::new(RefCell::new(self.deep_copy(p.borrow().clone())))),
            Value::Opt(Some(v)) => Value::Opt(Some(Rc::new(self.deep_copy((*v).clone())))),
            other => other,
        }
    }

    /// 浅复制（CopyMode.shallow，L1）：顶层容器新建，元素共享槽（内存问题用户负责）
    fn shallow_copy(&self, v: Value) -> Value {
        match v {
            Value::Arr(a) => {
                let items = a.borrow().clone();
                Value::Arr(Rc::new(RefCell::new(items)))
            }
            Value::Class(c) => {
                let d = c.borrow();
                Value::class(&d.name, d.fields.clone())
            }
            Value::Ptr(p) => Value::Ptr(p),
            other => other,
        }
    }

    // ---------- 入口 ----------

    /// 运行 main；错误返回 RtError（入口错误由运行时统一报告，06-language-spec）
    pub fn run_main(&mut self) -> Result<()> {
        self.in_main = true;
        let io = self.io_value();
        self.bind("io", io.clone());
        self.bind("alloc", Value::Alloc);
        let has_main = self.funcs.contains_key("main");
        if !has_main {
            self.in_main = false;
            return Err(RtError::msg("NoMain", "no `main` entry point"));
        }
        // main(io: Io) !void——单参数 io 版本或零参版本
        let (main_def, main_args): (FnDef, Vec<Value>) = match self.pick_fn("main", &[io.clone()]) {
            Ok(f) => (f, vec![io.clone()]),
            Err(_) => (self.pick_fn("main", &[])?, vec![]),
        };
        let r = self.call_fn(&main_def, &main_args, &Span::new(0, 0, 0, 0));
        self.in_main = false;
        match r {
            // 未处理错误到达根作用域（值通道）：记录错误名位置后 panic 式中止
            Ok(Value::Err { name, code }) => {
                let e = RtError::new(&name, self.error_locs.get(&name).cloned());
                let e = e.with_code(code);
                Err(e)
            }
            Ok(_) => Ok(()),
            // io.exit：正常退出信号（exit_code 已记录）
            Err(e) if e.name == "ExitRequested" => Ok(()),
            Err(e) => {
                // M2.6：未处理错误到达根作用域 → 记录错误名位置（原始错误定位），
                // panic 式中止（无恢复/不输出调用链；hc-tools 打印后非零退出）
                if e.span.is_none() && !e.is_signal() {
                    if let Some(sp) = self.error_locs.get(&e.name).cloned() {
                        let mut e2 = e.clone();
                        e2.span = Some(sp);
                        return Err(e2);
                    }
                }
                Err(e)
            }
        }
    }

    /// 运行全部测试；返回 (passed, failed, skipped)
    pub fn run_tests(&mut self) -> (usize, usize, usize) {
        let mut tests: Vec<FnDef> = Vec::new();
        for fns in self.funcs.values() {
            for f in fns {
                if f.is_test {
                    tests.push(f.clone());
                }
            }
        }
        // 声明序
        tests.sort_by(|a, b| a.span.line.cmp(&b.span.line));
        let (mut passed, mut failed, mut skipped) = (0, 0, 0);
        for t in tests {
            self.push_scope();
            self.bind("test_io", self.io_value());
            self.bind("alloc", Value::Alloc);
            self.fail_info = None;
            let r = self.exec_fn_body(&t.body, &[]);
            let _ = self.pop_scope();
            match r {
                Ok(Value::Err { name, .. }) => {
                    // M2.6：未处理错误到达测试根（值通道）→ 记 FAIL（不中止其它测试，Q-T2）
                    self.test_out
                        .push(format!("[FAIL] {} (error.{})", t.name, name));
                    failed += 1;
                }
                Ok(_) => {
                    self.test_out.push(format!("[PASS] {}", t.name));
                    passed += 1;
                }
                Err(e) if e.name == "SkipTest" => {
                    self.test_out.push(format!("[SKIP] {}", t.name));
                    skipped += 1;
                }
                Err(e) => {
                    let extra = self.fail_info.clone().unwrap_or_default();
                    self.test_out.push(format!(
                        "[FAIL] {} (error.{}{})",
                        t.name,
                        e.name,
                        if extra.is_empty() {
                            "".into()
                        } else {
                            format!(": {extra}")
                        }
                    ));
                    failed += 1;
                }
            }
        }
        (passed, failed, skipped)
    }

    pub fn collect_tests(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for fns in self.funcs.values() {
            for f in fns {
                if f.is_test {
                    names.push(f.name.clone());
                }
            }
        }
        names.sort();
        names
    }
}

impl Flow {
    #[allow(dead_code)]
    fn as_value(self) -> Value {
        match self {
            Flow::Return(v) => v,
            Flow::Value(v) => v,
            Flow::None => Value::Void,
            Flow::Break | Flow::Continue => Value::Void,
        }
    }
}

/// 类型参数是否含泛型占位（大写类型名 T/U 等，tag1 启发式）
fn type_has_generic(t: &Type) -> bool {
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
