//! M3.1 共享 IR（唯一语义源，ADR-0004）
//!
//! 线性指令 + 标签形态——字节码 VM（M3.2）与 LLVM 原生后端（M3.3）共用，
//! 禁止各后端私语义。覆盖：标量运算 / 控制流 / 函数调用 / **错误值通道**
//! （M2.6 传播模型：错误是值，`try`/`catch` 降级为错误值检查 + 分支）。
//!
//! 垂直切片范围（tag1）：标量 + bool + 字符串 + 函数/参数/局部变量 +
//! if（语句/表达式/else-if/optional 捕获）+ while（含续步）+ return +
//! error 字面量 + try/catch + orelse + 全局函数调用（含多级限定名）+
//! 断言内建。
//! **不做**（记录扩展）：defer/errdefer、for/switch、break/continue、
//! 闭包、集合/class 方法（原子内建调用）、指针操作。复杂库操作 = `CallBuiltin` 原子指令。

use crate::ast::*;
use std::collections::HashMap;

// ---------- IR 结构 ----------

#[derive(Debug, Clone, Default)]
pub struct IrModule {
    pub funcs: Vec<IrFunc>,
    /// 函数名（扁平 + 限定）→ 索引
    pub func_index: HashMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct IrFunc {
    pub name: String,
    /// 参数槽号（声明序）
    pub params: Vec<usize>,
    /// 槽总数（参数 + 局部变量 + 临时）
    pub n_slots: usize,
    pub body: Vec<IrInst>,
    pub is_test: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IrConst {
    Int(i128),
    Float(f64),
    Bool(bool),
    Str(String),
    Void,
    Null,
    /// error.Name（错误值 = 普通值，走值通道）
    Err(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    EucMod,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrUnOp {
    Neg,
    Not,
    BitNot,
}

#[derive(Debug, Clone)]
pub enum IrInst {
    /// temp = 常量
    Const {
        temp: usize,
        val: IrConst,
    },
    /// temp = slot
    Load {
        temp: usize,
        slot: usize,
    },
    /// slot = temp
    Store {
        slot: usize,
        temp: usize,
    },
    /// temp = a op b
    Bin {
        op: IrBinOp,
        temp: usize,
        a: usize,
        b: usize,
    },
    /// temp = op a
    Un {
        op: IrUnOp,
        temp: usize,
        a: usize,
    },
    Jump {
        label: usize,
    },
    JumpIf {
        temp: usize,
        label: usize,
    },
    JumpIfNot {
        temp: usize,
        label: usize,
    },
    /// temp 是 null → 跳转（orelse / optional 捕获降级）
    JumpIfNull {
        temp: usize,
        label: usize,
    },
    Label {
        id: usize,
    },
    /// temp = call name(args...)（错误值经值通道返回）
    Call {
        name: String,
        args: Vec<usize>,
        temp: usize,
    },
    /// temp = builtin(args...)（断言 / @ 内建）
    CallBuiltin {
        name: String,
        args: Vec<usize>,
        temp: usize,
    },
    /// temp 是错误值 → 跳转（try/catch 降级）
    JumpIfErr {
        temp: usize,
        label: usize,
    },
    Return {
        temp: usize,
    },
    ReturnVoid,
}

// ---------- AST → IR 降级 ----------

pub fn lower(program: &Program) -> IrModule {
    let mut module = IrModule::default();
    for d in &program.decls {
        lower_decl(d, &mut module);
    }
    module
}

fn lower_decl(d: &Decl, module: &mut IrModule) {
    match d {
        Decl::Fn {
            name,
            params,
            body,
            is_test,
            ..
        } => {
            let func = lower_func(name, params, body, *is_test);
            register_func(module, name, func);
        }
        Decl::Namespace { name, decls, .. } => {
            // namespace 内函数：扁平名 + 限定名双注册（与运行时/语义一致）；
            // 多级 namespace（io.net.connect）注册全限定名
            let mut inner: Vec<(String, String, IrFunc)> = Vec::new();
            collect_ns_funcs(decls, &[name.clone()], &mut inner);
            for (flat, qn, func) in inner {
                let idx = module.funcs.len();
                module.funcs.push(func);
                // 扁平名（using 导入后直接调用）：先到先得
                module.func_index.entry(flat).or_insert(idx);
                // 限定名（Math.square / io.net.connect）
                module.func_index.insert(qn, idx);
            }
        }
        _ => {}
    }
}

/// 递归收集 namespace 内非测试函数：(扁平名, 全限定名, IR 函数)
fn collect_ns_funcs(decls: &[Decl], path: &[String], out: &mut Vec<(String, String, IrFunc)>) {
    for d in decls {
        match d {
            Decl::Fn {
                name,
                params,
                body,
                is_test,
                ..
            } if !*is_test => {
                let mut qn = path.to_vec();
                qn.push(name.clone());
                let func = lower_func(name, params, body, false);
                out.push((name.clone(), qn.join("."), func));
            }
            Decl::Namespace {
                name,
                decls: nested,
                ..
            } => {
                let mut p = path.to_vec();
                p.push(name.clone());
                collect_ns_funcs(nested, &p, out);
            }
            _ => {}
        }
    }
}

fn register_func(module: &mut IrModule, name: &str, func: IrFunc) {
    let idx = module.funcs.len();
    module.funcs.push(func);
    module.func_index.insert(name.to_string(), idx);
}

fn lower_func(name: &str, params: &[Param], body: &Block, is_test: bool) -> IrFunc {
    let mut ctx = LowerCtx::default();
    ctx.push_scope();
    // 参数槽（声明序，从 0 开始）
    let param_slots: Vec<usize> = params.iter().map(|_| ctx.alloc_slot()).collect();
    // 局部变量槽（变量名 → 槽）
    for (p, slot) in params.iter().zip(param_slots.iter()) {
        ctx.bind(&p.name, *slot);
    }
    for stmt in &body.stmts {
        ctx.lower_stmt(stmt);
    }
    ctx.pop_scope();
    // 隐式末尾 return void
    ctx.insts.push(IrInst::ReturnVoid);
    let n_slots = ctx.next_slot;
    IrFunc {
        name: name.to_string(),
        params: param_slots,
        n_slots,
        body: ctx.insts,
        is_test,
    }
}

#[derive(Default)]
struct LowerCtx {
    /// 作用域栈：名字 → 槽（词法作用域，块退出恢复外层绑定——对齐解释器作用域）
    scopes: Vec<HashMap<String, usize>>,
    next_slot: usize,
    insts: Vec<IrInst>,
    next_label: usize,
}

impl LowerCtx {
    fn alloc_slot(&mut self) -> usize {
        let s = self.next_slot;
        self.next_slot += 1;
        s
    }
    fn new_label(&mut self) -> usize {
        let l = self.next_label;
        self.next_label += 1;
        l
    }
    fn push(&mut self, inst: IrInst) {
        self.insts.push(inst);
    }
    fn label(&mut self, id: usize) {
        self.insts.push(IrInst::Label { id });
    }
    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }
    fn pop_scope(&mut self) {
        self.scopes.pop();
    }
    /// 当前作用域绑定（遮蔽时分配新槽，旧绑定保留在外层）
    fn bind(&mut self, name: &str, slot: usize) {
        self.scopes
            .last_mut()
            .expect("bind outside any scope")
            .insert(name.to_string(), slot);
    }
    fn resolve(&self, name: &str) -> Option<usize> {
        self.scopes.iter().rev().find_map(|m| m.get(name).copied())
    }
    /// 块语句序列（推/弹作用域）；空块安全
    fn lower_block(&mut self, b: &Block) {
        self.push_scope();
        for stmt in &b.stmts {
            self.lower_stmt(stmt);
        }
        self.pop_scope();
    }

    /// 表达式 → 临时槽号
    fn lower_expr(&mut self, e: &Expr) -> usize {
        let t = self.alloc_slot();
        match e {
            Expr::IntLit { text, .. } => {
                let v = parse_int_lit(text);
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Int(v),
                });
            }
            Expr::FloatLit { text, .. } => {
                let v: f64 = text.parse().unwrap_or(0.0);
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Float(v),
                });
            }
            Expr::BoolLit(b, _) => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Bool(*b),
                });
            }
            Expr::StrLit { value, .. } => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Str(value.clone()),
                });
            }
            Expr::CharLit(c, _) => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Int(*c as i128),
                });
            }
            Expr::NullLit(_) => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Null,
                });
            }
            Expr::VoidLit(_) => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Void,
                });
            }
            Expr::ErrorLit(name, _) => {
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Err(name.clone()),
                });
            }
            Expr::Ident(name, _) => match self.resolve(name) {
                Some(slot) => self.push(IrInst::Load { temp: t, slot }),
                // 全局变量等不在 IR 范围内：void 占位（正常流程语义检查已拦截）
                None => self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Void,
                }),
            },
            Expr::Binary(op, l, r, _) => {
                let a = self.lower_expr(l);
                match op {
                    // 短路 and/or（与运行时 eval_binary 一致）
                    BinOp::And => {
                        let l_false = self.new_label();
                        let done = self.new_label();
                        self.push(IrInst::JumpIfNot {
                            temp: a,
                            label: l_false,
                        });
                        let b = self.lower_expr(r);
                        self.push(IrInst::Load { temp: t, slot: b });
                        self.push(IrInst::Jump { label: done });
                        self.label(l_false);
                        self.push(IrInst::Const {
                            temp: t,
                            val: IrConst::Bool(false),
                        });
                        self.label(done);
                    }
                    BinOp::Or => {
                        let l_true = self.new_label();
                        let done = self.new_label();
                        self.push(IrInst::JumpIf {
                            temp: a,
                            label: l_true,
                        });
                        let b = self.lower_expr(r);
                        self.push(IrInst::Load { temp: t, slot: b });
                        self.push(IrInst::Jump { label: done });
                        self.label(l_true);
                        self.push(IrInst::Const {
                            temp: t,
                            val: IrConst::Bool(true),
                        });
                        self.label(done);
                    }
                    // 区间糖（[lo,hi) 数组/切片）不在 IR 标量范围：void 占位
                    // （与集合同类，见文件头「不做」清单）
                    BinOp::Range => self.push(IrInst::Const {
                        temp: t,
                        val: IrConst::Void,
                    }),
                    _ => {
                        let b = self.lower_expr(r);
                        self.push(IrInst::Bin {
                            op: to_ir_binop(*op),
                            temp: t,
                            a,
                            b,
                        });
                    }
                }
            }
            Expr::Unary(op, inner, _) => {
                let a = self.lower_expr(inner);
                let un = match op {
                    UnaryOp::Neg => IrUnOp::Neg,
                    UnaryOp::Not => IrUnOp::Not,
                    UnaryOp::BitNot => IrUnOp::BitNot,
                };
                self.push(IrInst::Un { op: un, temp: t, a });
            }
            Expr::Try(inner, _) => {
                // try：错误值 → 从当前函数返回（值通道）
                let a = self.lower_expr(inner);
                let l_ret = self.new_label();
                let done = self.new_label();
                self.push(IrInst::JumpIfErr {
                    temp: a,
                    label: l_ret,
                });
                self.push(IrInst::Load { temp: t, slot: a });
                self.push(IrInst::Jump { label: done });
                self.label(l_ret);
                self.push(IrInst::Return { temp: a });
                self.label(done);
            }
            Expr::Catch(inner, kind, _) => {
                // catch：错误值 → 处理分支；结果统一到目标槽
                let a = self.lower_expr(inner);
                let l_catch = self.new_label();
                let done = self.new_label();
                let res_slot = self.alloc_slot();
                self.push(IrInst::JumpIfErr {
                    temp: a,
                    label: l_catch,
                });
                self.push(IrInst::Store {
                    slot: res_slot,
                    temp: a,
                });
                self.push(IrInst::Jump { label: done });
                self.label(l_catch);
                match kind.as_ref() {
                    CatchKind::Default(d) => {
                        let h = self.lower_expr(d);
                        self.push(IrInst::Store {
                            slot: res_slot,
                            temp: h,
                        });
                    }
                    CatchKind::Bind { name: bname, body } => {
                        let err_slot = self.alloc_slot();
                        self.push(IrInst::Store {
                            slot: err_slot,
                            temp: a,
                        });
                        self.push_scope();
                        self.bind(bname, err_slot);
                        // 块值：最后语句为表达式时取其值（只求值一次——对齐解释器 exec_block_inner）；
                        // 其余（赋值/return/块等作值）→ void 占位
                        let last_is_value = matches!(body.stmts.last(), Some(Stmt::Expr(_)));
                        let n = body.stmts.len() - usize::from(last_is_value);
                        for stmt in &body.stmts[..n] {
                            self.lower_stmt(stmt);
                        }
                        if last_is_value {
                            if let Some(Stmt::Expr(last)) = body.stmts.last() {
                                let h = self.lower_expr(last);
                                self.push(IrInst::Store {
                                    slot: res_slot,
                                    temp: h,
                                });
                            }
                        } else {
                            let h = self.alloc_slot();
                            self.push(IrInst::Const {
                                temp: h,
                                val: IrConst::Void,
                            });
                            self.push(IrInst::Store {
                                slot: res_slot,
                                temp: h,
                            });
                        }
                        self.pop_scope();
                    }
                }
                self.label(done);
                self.push(IrInst::Load {
                    temp: t,
                    slot: res_slot,
                });
            }
            Expr::Call { callee, args, .. } => {
                let arg_ts: Vec<usize> = args.iter().map(|a| self.lower_expr(a)).collect();
                match callee.as_ref() {
                    Expr::Ident(name, _) => {
                        if name.starts_with('@') || is_assert_builtin(name) {
                            self.push(IrInst::CallBuiltin {
                                name: name.clone(),
                                args: arg_ts,
                                temp: t,
                            });
                        } else {
                            self.push(IrInst::Call {
                                name: name.clone(),
                                args: arg_ts,
                                temp: t,
                            });
                        }
                    }
                    Expr::Dot { base, field, .. } | Expr::Field { base, field, .. } => {
                        // 展平限定名链：io.net.double → "io.net.double"
                        // （多级限定名经后缀二次处理后外层为 Field 形态）
                        let mut parts = vec![field.clone()];
                        let mut b = base.as_ref();
                        while let Expr::Dot {
                            base: b2,
                            field: f2,
                            ..
                        }
                        | Expr::Field {
                            base: b2,
                            field: f2,
                            ..
                        } = b
                        {
                            parts.push(f2.clone());
                            b = b2.as_ref();
                        }
                        if let Expr::Ident(ns, _) = b {
                            parts.push(ns.clone());
                            parts.reverse();
                            self.push(IrInst::Call {
                                name: parts.join("."),
                                args: arg_ts,
                                temp: t,
                            });
                        }
                        // 方法/实例调用（非命名空间限定）：记录扩展——不注册则运行时 NoFunction
                    }
                    _ => {
                        // 方法/内建调用：子集不支持（返回 void 占位）
                        self.push(IrInst::Const {
                            temp: t,
                            val: IrConst::Void,
                        });
                    }
                }
            }
            Expr::IfExpr {
                cond,
                capture,
                then_e,
                else_e,
                ..
            } => {
                // if 表达式：两分支结果统一到 res_slot（对齐解释器 IfExpr）
                let c = self.lower_expr(cond);
                let l_else = self.new_label();
                let l_done = self.new_label();
                let res_slot = self.alloc_slot();
                match capture.as_ref() {
                    Some((_, name)) => {
                        // optional 捕获：null → else；否则绑定 cond 值
                        self.push(IrInst::JumpIfNull {
                            temp: c,
                            label: l_else,
                        });
                        self.push_scope();
                        self.bind(name, c);
                        let tv = self.lower_expr(then_e);
                        self.pop_scope();
                        self.push(IrInst::Store {
                            slot: res_slot,
                            temp: tv,
                        });
                    }
                    None => {
                        self.push(IrInst::JumpIfNot {
                            temp: c,
                            label: l_else,
                        });
                        let tv = self.lower_expr(then_e);
                        self.push(IrInst::Store {
                            slot: res_slot,
                            temp: tv,
                        });
                    }
                }
                self.push(IrInst::Jump { label: l_done });
                self.label(l_else);
                let ev = self.lower_expr(else_e);
                self.push(IrInst::Store {
                    slot: res_slot,
                    temp: ev,
                });
                self.label(l_done);
                self.push(IrInst::Load {
                    temp: t,
                    slot: res_slot,
                });
            }
            Expr::Orelse(l, r, _) => {
                // orelse：null → 默认值
                let a = self.lower_expr(l);
                let l_null = self.new_label();
                let done = self.new_label();
                let res_slot = self.alloc_slot();
                self.push(IrInst::JumpIfNull {
                    temp: a,
                    label: l_null,
                });
                self.push(IrInst::Store {
                    slot: res_slot,
                    temp: a,
                });
                self.push(IrInst::Jump { label: done });
                self.label(l_null);
                let d = self.lower_expr(r);
                self.push(IrInst::Store {
                    slot: res_slot,
                    temp: d,
                });
                self.label(done);
                self.push(IrInst::Load {
                    temp: t,
                    slot: res_slot,
                });
            }
            Expr::Assign {
                target, op, value, ..
            } => match self.lower_assign(*op, target, value) {
                // 赋值表达式（while 续步 i += 1 等）：值 = 新值（对齐 eval_assign）
                Some(stored) => self.push(IrInst::Load {
                    temp: t,
                    slot: stored,
                }),
                None => self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Void,
                }),
            },
            _ => {
                // 集合/闭包/字段/索引等：子集不支持（void 占位）
                self.push(IrInst::Const {
                    temp: t,
                    val: IrConst::Void,
                });
            }
        }
        t
    }

    /// 赋值：返回写入目标槽的新值临时槽（目标不在 IR 范围 → None）
    /// 复合赋值 x op= v → x = x op v（对齐解释器 eval_assign）
    fn lower_assign(&mut self, op: AssignOp, target: &Expr, value: &Expr) -> Option<usize> {
        if let Expr::Ident(name, _) = target {
            if let Some(slot) = self.resolve(name) {
                let v = self.lower_expr(value);
                return Some(match op {
                    AssignOp::Set => {
                        self.push(IrInst::Store { slot, temp: v });
                        v
                    }
                    _ => {
                        let cur = self.alloc_slot();
                        self.push(IrInst::Load { temp: cur, slot });
                        let r = self.alloc_slot();
                        self.push(IrInst::Bin {
                            op: to_assign_binop(op),
                            temp: r,
                            a: cur,
                            b: v,
                        });
                        self.push(IrInst::Store { slot, temp: r });
                        r
                    }
                });
            }
        }
        None
    }

    fn lower_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::VarDecl { name, init, .. } => {
                // 遮蔽时分配新槽（词法作用域，块退出恢复外层绑定）
                let slot = self.alloc_slot();
                self.bind(name, slot);
                let t = match init {
                    Some(e) => self.lower_expr(e),
                    None => {
                        let t = self.alloc_slot();
                        self.push(IrInst::Const {
                            temp: t,
                            val: IrConst::Void,
                        });
                        t
                    }
                };
                self.push(IrInst::Store { slot, temp: t });
            }
            Stmt::ConstDecl { name, init, .. } => {
                let slot = self.alloc_slot();
                self.bind(name, slot);
                let t = self.lower_expr(init);
                self.push(IrInst::Store { slot, temp: t });
            }
            Stmt::Expr(Expr::Assign {
                target, op, value, ..
            }) => {
                // 语句级赋值：副作用即可（目标/字段/索引等不在 IR 范围 → 忽略）
                let _ = self.lower_assign(*op, target, value);
            }
            Stmt::Expr(e) => {
                let _ = self.lower_expr(e);
            }
            Stmt::If(ifs) => {
                let c = self.lower_expr(&ifs.cond);
                let l_else = self.new_label();
                let l_end = self.new_label();
                match &ifs.capture {
                    // optional 捕获：null → else；否则绑定 cond 值到捕获名（对齐解释器 exec_if）
                    Some((_, name)) => {
                        self.push(IrInst::JumpIfNull {
                            temp: c,
                            label: l_else,
                        });
                        self.push_scope();
                        self.bind(name, c);
                        for stmt in &ifs.then_b.stmts {
                            self.lower_stmt(stmt);
                        }
                        self.pop_scope();
                    }
                    None => {
                        self.push(IrInst::JumpIfNot {
                            temp: c,
                            label: l_else,
                        });
                        for stmt in &ifs.then_b.stmts {
                            self.lower_stmt(stmt);
                        }
                    }
                }
                match &ifs.else_b {
                    Some(else_b) => {
                        self.push(IrInst::Jump { label: l_end });
                        self.label(l_else);
                        self.lower_stmt(else_b);
                    }
                    None => {
                        self.label(l_else);
                    }
                }
                self.label(l_end);
            }
            Stmt::While(w) => {
                let l_top = self.new_label();
                let l_end = self.new_label();
                self.label(l_top);
                let c = self.lower_expr(&w.cond);
                self.push(IrInst::JumpIfNot {
                    temp: c,
                    label: l_end,
                });
                self.lower_block(&w.body);
                if let Some(step) = &w.step {
                    let _ = self.lower_expr(step);
                }
                self.push(IrInst::Jump { label: l_top });
                self.label(l_end);
            }
            Stmt::Return(e, _) => match e {
                Some(e) => {
                    let t = self.lower_expr(e);
                    self.push(IrInst::Return { temp: t });
                }
                None => self.push(IrInst::ReturnVoid),
            },
            Stmt::Block(b) => self.lower_block(b),
            // for/switch/break/continue/defer/errdefer：不在 IR 范围（记录扩展，见文件头）
            _ => {}
        }
    }
}

fn to_assign_binop(op: AssignOp) -> IrBinOp {
    match op {
        AssignOp::Add => IrBinOp::Add,
        AssignOp::Sub => IrBinOp::Sub,
        AssignOp::Mul => IrBinOp::Mul,
        AssignOp::Div => IrBinOp::Div,
        AssignOp::BitOr => IrBinOp::BitOr,
        AssignOp::BitAnd => IrBinOp::BitAnd,
        AssignOp::BitXor => IrBinOp::BitXor,
        AssignOp::Set => unreachable!("Set 单独处理"),
    }
}

fn to_ir_binop(op: BinOp) -> IrBinOp {
    match op {
        BinOp::Add => IrBinOp::Add,
        BinOp::Sub => IrBinOp::Sub,
        BinOp::Mul => IrBinOp::Mul,
        BinOp::Div => IrBinOp::Div,
        BinOp::Mod => IrBinOp::Mod,
        BinOp::EucMod => IrBinOp::EucMod,
        BinOp::BitAnd => IrBinOp::BitAnd,
        BinOp::BitOr => IrBinOp::BitOr,
        BinOp::BitXor => IrBinOp::BitXor,
        BinOp::Shl => IrBinOp::Shl,
        BinOp::Shr => IrBinOp::Shr,
        BinOp::Eq => IrBinOp::Eq,
        BinOp::Ne => IrBinOp::Ne,
        BinOp::Lt => IrBinOp::Lt,
        BinOp::Le => IrBinOp::Le,
        BinOp::Gt => IrBinOp::Gt,
        BinOp::Ge => IrBinOp::Ge,
        // 短路/区间在 lower_expr 单独处理，此处为不可达兜底
        BinOp::And | BinOp::Or | BinOp::Range => IrBinOp::Eq,
    }
}

/// 断言内建（IR 参考解释器实现）
fn is_assert_builtin(name: &str) -> bool {
    matches!(
        name,
        "expect" | "expect_eq" | "expect_neq" | "expect_error" | "expect_eq_slices"
    )
}

/// 整数/浮点字面量解析（后缀、下划线、进制）
fn parse_int_lit(text: &str) -> i128 {
    let cleaned: String = text
        .chars()
        .take_while(|c| {
            c.is_ascii_digit()
                || matches!(c, 'x' | 'X' | 'b' | 'B' | 'o' | 'O' | 'a'..='f' | 'A'..='F' | '_')
        })
        .collect();
    let cleaned = cleaned.replace('_', "");
    let (radix, digits) = if let Some(r) = cleaned.strip_prefix("0x").or(cleaned.strip_prefix("0X"))
    {
        (16u32, r)
    } else if let Some(r) = cleaned.strip_prefix("0b").or(cleaned.strip_prefix("0B")) {
        (2u32, r)
    } else if let Some(r) = cleaned.strip_prefix("0o").or(cleaned.strip_prefix("0O")) {
        (8u32, r)
    } else {
        (10u32, cleaned.as_str())
    };
    i128::from_str_radix(digits, radix).unwrap_or(0)
}

// ---------- IR 参考解释器（M3.1：唯一语义源的语义定义） ----------

#[derive(Debug, Clone, PartialEq)]
pub enum IrValue {
    Int(i128),
    Float(f64),
    Bool(bool),
    Str(Vec<u8>),
    Void,
    Null,
    Err(String),
}

#[derive(Debug, Clone)]
pub struct IrError {
    pub name: String,
    pub message: String,
}

impl IrError {
    pub fn msg(name: &str, message: impl Into<String>) -> Self {
        IrError {
            name: name.to_string(),
            message: message.into(),
        }
    }
}

type R<T> = std::result::Result<T, IrError>;

impl IrValue {
    fn as_bool(&self) -> bool {
        match self {
            IrValue::Bool(b) => *b,
            IrValue::Int(i) => *i != 0,
            IrValue::Float(f) => *f != 0.0,
            IrValue::Str(s) => !s.is_empty(),
            IrValue::Null => false,
            _ => true,
        }
    }
    fn is_err(&self) -> bool {
        matches!(self, IrValue::Err(_))
    }
    fn display(&self) -> String {
        match self {
            IrValue::Int(i) => i.to_string(),
            IrValue::Float(f) => f.to_string(),
            IrValue::Bool(b) => b.to_string(),
            IrValue::Str(s) => String::from_utf8_lossy(s).to_string(),
            IrValue::Void => "void".into(),
            IrValue::Null => "null".into(),
            IrValue::Err(n) => format!("error.{n}"),
        }
    }
    fn value_eq(&self, other: &IrValue) -> bool {
        match (self, other) {
            (IrValue::Int(a), IrValue::Int(b)) => a == b,
            (IrValue::Int(a), IrValue::Float(b)) => *a as f64 == *b,
            (IrValue::Float(a), IrValue::Int(b)) => *a == *b as f64,
            (IrValue::Float(a), IrValue::Float(b)) => a == b,
            (IrValue::Bool(a), IrValue::Bool(b)) => a == b,
            (IrValue::Str(a), IrValue::Str(b)) => a == b,
            (IrValue::Null, IrValue::Null) => true,
            (IrValue::Void, IrValue::Void) => true,
            (IrValue::Err(a), IrValue::Err(b)) => a == b,
            _ => false,
        }
    }
}

/// 执行模块中名为 entry 的函数（测试/入口），参数按 IrModule 函数签名传入
pub fn run_ir(module: &IrModule, entry: &str, args: &[IrValue]) -> R<IrValue> {
    let idx = *module
        .func_index
        .get(entry)
        .ok_or_else(|| IrError::msg("NoFunction", format!("no function `{entry}`")))?;
    exec_func(module, idx, args)
}

fn exec_func(module: &IrModule, idx: usize, args: &[IrValue]) -> R<IrValue> {
    let func = &module.funcs[idx];
    let mut slots: Vec<IrValue> = vec![IrValue::Void; func.n_slots];
    for (i, ps) in func.params.iter().enumerate() {
        if i < args.len() {
            slots[*ps] = args[i].clone();
        }
    }
    let mut pc = 0usize;
    let mut fail: Option<String> = None;
    loop {
        if pc >= func.body.len() {
            return Err(IrError::msg(
                "NoReturn",
                format!("function `{}` fell through", func.name),
            ));
        }
        match &func.body[pc] {
            IrInst::Const { temp, val } => {
                slots[*temp] = match val {
                    IrConst::Int(i) => IrValue::Int(*i),
                    IrConst::Float(f) => IrValue::Float(*f),
                    IrConst::Bool(b) => IrValue::Bool(*b),
                    IrConst::Str(s) => IrValue::Str(s.clone().into_bytes()),
                    IrConst::Void => IrValue::Void,
                    IrConst::Null => IrValue::Null,
                    IrConst::Err(n) => IrValue::Err(n.clone()),
                };
            }
            IrInst::Load { temp, slot } => {
                slots[*temp] = slots[*slot].clone();
            }
            IrInst::Store { slot, temp } => {
                slots[*slot] = slots[*temp].clone();
            }
            IrInst::Bin { op, temp, a, b } => {
                let (av, bv) = (slots[*a].clone(), slots[*b].clone());
                slots[*temp] = binop(*op, &av, &bv);
            }
            IrInst::Un { op, temp, a } => {
                let av = slots[*a].clone();
                slots[*temp] = match op {
                    IrUnOp::Neg => match av {
                        IrValue::Int(i) => IrValue::Int(-i),
                        IrValue::Float(f) => IrValue::Float(-f),
                        _ => return Err(IrError::msg("TypeError", "unary -")),
                    },
                    IrUnOp::Not => IrValue::Bool(!av.as_bool()),
                    IrUnOp::BitNot => match av {
                        IrValue::Int(i) => IrValue::Int(!i),
                        _ => return Err(IrError::msg("TypeError", "~")),
                    },
                };
            }
            IrInst::Jump { label } => {
                pc = find_label(func, *label)?;
                continue;
            }
            IrInst::JumpIf { temp, label } => {
                if slots[*temp].as_bool() {
                    pc = find_label(func, *label)?;
                    continue;
                }
            }
            IrInst::JumpIfNot { temp, label } => {
                if !slots[*temp].as_bool() {
                    pc = find_label(func, *label)?;
                    continue;
                }
            }
            IrInst::JumpIfErr { temp, label } => {
                if slots[*temp].is_err() {
                    pc = find_label(func, *label)?;
                    continue;
                }
            }
            IrInst::JumpIfNull { temp, label } => {
                if slots[*temp] == IrValue::Null {
                    pc = find_label(func, *label)?;
                    continue;
                }
            }
            IrInst::Label { .. } => {}
            IrInst::Call { name, args, temp } => {
                let arg_vals: Vec<IrValue> = args.iter().map(|a| slots[*a].clone()).collect();
                let callee_idx = *module
                    .func_index
                    .get(name)
                    .ok_or_else(|| IrError::msg("NoFunction", format!("no function `{name}`")))?;
                slots[*temp] = exec_func(module, callee_idx, &arg_vals)?;
            }
            IrInst::CallBuiltin { name, args, temp } => {
                let arg_vals: Vec<IrValue> = args.iter().map(|a| slots[*a].clone()).collect();
                slots[*temp] = call_assert_builtin(name, &arg_vals, &mut fail)?;
            }
            IrInst::Return { temp } => {
                let v = slots[*temp].clone();
                if let Some(f) = fail {
                    return Err(IrError::msg("AssertFailed", f));
                }
                return Ok(v);
            }
            IrInst::ReturnVoid => {
                if let Some(f) = fail {
                    return Err(IrError::msg("AssertFailed", f));
                }
                return Ok(IrValue::Void);
            }
        }
        pc += 1;
    }
}

fn find_label(func: &IrFunc, id: usize) -> R<usize> {
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

fn binop(op: IrBinOp, a: &IrValue, b: &IrValue) -> IrValue {
    match op {
        IrBinOp::Add
        | IrBinOp::Sub
        | IrBinOp::Mul
        | IrBinOp::Div
        | IrBinOp::Mod
        | IrBinOp::EucMod => {
            use IrValue::*;
            match (a, b) {
                (Int(x), Int(y)) => {
                    let r = match op {
                        IrBinOp::Add => x + y,
                        IrBinOp::Sub => x - y,
                        IrBinOp::Mul => x * y,
                        IrBinOp::Div => {
                            if *y == 0 {
                                return Int(0);
                            }
                            x / y
                        }
                        IrBinOp::Mod | IrBinOp::EucMod => {
                            if *y == 0 {
                                return Int(0);
                            }
                            x % y
                        }
                        _ => 0,
                    };
                    Int(r)
                }
                (Int(x), Float(y)) | (Float(y), Int(x)) => {
                    let (x, y) = (x.clone(), y.clone());
                    let r = match op {
                        IrBinOp::Add => x as f64 + y,
                        IrBinOp::Sub => x as f64 - y,
                        IrBinOp::Mul => x as f64 * y,
                        _ => 0.0,
                    };
                    Float(r)
                }
                (Float(x), Float(y)) => {
                    let r = match op {
                        IrBinOp::Add => x + y,
                        IrBinOp::Sub => x - y,
                        IrBinOp::Mul => x * y,
                        IrBinOp::Div => {
                            if *y == 0.0 {
                                return Float(0.0);
                            }
                            x / y
                        }
                        _ => 0.0,
                    };
                    Float(r)
                }
                _ => Int(0),
            }
        }
        IrBinOp::BitAnd | IrBinOp::BitOr | IrBinOp::BitXor | IrBinOp::Shl | IrBinOp::Shr => {
            match (a, b) {
                (IrValue::Int(x), IrValue::Int(y)) => {
                    let r = match op {
                        IrBinOp::BitAnd => x & y,
                        IrBinOp::BitOr => x | y,
                        IrBinOp::BitXor => x ^ y,
                        IrBinOp::Shl => x.wrapping_shl((*y % 128).max(0) as u32),
                        IrBinOp::Shr => x.wrapping_shr((*y % 128).max(0) as u32),
                        _ => 0,
                    };
                    IrValue::Int(r)
                }
                _ => IrValue::Int(0),
            }
        }
        IrBinOp::Eq | IrBinOp::Ne | IrBinOp::Lt | IrBinOp::Le | IrBinOp::Gt | IrBinOp::Ge => {
            let r = match op {
                IrBinOp::Eq => a.value_eq(b),
                IrBinOp::Ne => !a.value_eq(b),
                IrBinOp::Lt => value_lt(a, b),
                IrBinOp::Le => value_lt(a, b) || a.value_eq(b),
                IrBinOp::Gt => !value_lt(a, b) && !a.value_eq(b),
                IrBinOp::Ge => !value_lt(a, b),
                _ => false,
            };
            IrValue::Bool(r)
        }
    }
}

fn value_lt(a: &IrValue, b: &IrValue) -> bool {
    match (a, b) {
        (IrValue::Int(x), IrValue::Int(y)) => x < y,
        (IrValue::Int(x), IrValue::Float(y)) => (*x as f64) < *y,
        (IrValue::Float(x), IrValue::Int(y)) => *x < *y as f64,
        (IrValue::Float(x), IrValue::Float(y)) => x < y,
        (IrValue::Str(x), IrValue::Str(y)) => x < y,
        (IrValue::Bool(x), IrValue::Bool(y)) => x < y,
        _ => false,
    }
}

/// 断言内建（IR 参考语义：失败记 fail，返回时抛 AssertFailed）
fn call_assert_builtin(name: &str, args: &[IrValue], fail: &mut Option<String>) -> R<IrValue> {
    match name {
        "expect" => {
            if args.first().map_or(false, |v| v.as_bool()) {
                Ok(IrValue::Void)
            } else {
                *fail = Some("expect failed".into());
                Ok(IrValue::Void)
            }
        }
        "expect_eq" => {
            if args.len() >= 2 && args[0].value_eq(&args[1]) {
                Ok(IrValue::Void)
            } else {
                let got = args.first().map(|v| v.display()).unwrap_or_default();
                let want = args.get(1).map(|v| v.display()).unwrap_or_default();
                *fail = Some(format!("expect_eq failed: got {got}, want {want}"));
                Ok(IrValue::Void)
            }
        }
        "expect_neq" => {
            if args.len() >= 2 && !args[0].value_eq(&args[1]) {
                Ok(IrValue::Void)
            } else {
                *fail = Some("expect_neq failed".into());
                Ok(IrValue::Void)
            }
        }
        "expect_error" => {
            if args.len() >= 2 && args[0].is_err() && args[1].is_err() && args[0] == args[1] {
                Ok(IrValue::Void)
            } else {
                *fail = Some("expect_error failed".into());
                Ok(IrValue::Void)
            }
        }
        "expect_eq_slices" => {
            if args.len() >= 2 && args[0].value_eq(&args[1]) {
                Ok(IrValue::Void)
            } else {
                *fail = Some("expect_eq_slices failed".into());
                Ok(IrValue::Void)
            }
        }
        _ => Ok(IrValue::Void),
    }
}
