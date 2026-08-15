//! 语义检查（M2：宽度检查 / 引用赋值禁止 / definite assignment 基础）
//!
//! tag1 静态 pass：在解释器 load 之前运行，基于 AST + 显式类型标注做
//! 保守检查。动态类型（运行时推断）部分留后续。

use crate::ast::*;
use crate::diag::Diagnostic;
use crate::token::Span;
use std::collections::HashMap;

/// 类型信息（编译时元数据）
#[derive(Clone)]
pub struct TypeInfo {
    pub kind: TypeKind,
    pub continuous: bool,
}

#[derive(Clone)]
pub enum TypeKind {
    Class { fields: Vec<FieldDecl> },
    Enum,
    Interface,
}

/// 错误集（M2.6）：error{ NotFound, ... } 成员集合
pub type ErrorSet = std::collections::HashSet<String>;

/// 变量声明类型（静态推断：显式标注 / NamedLit / 字面量）
#[derive(Clone)]
struct VarInfo {
    ty: Option<Type>,
    /// definite assignment（C7）：alloc.init(T) 无参构造的待初始化字段集
    pending_fields: Option<std::collections::HashSet<String>>,
}

pub fn check(program: &Program) -> Vec<Diagnostic> {
    let mut checker = Checker {
        types: HashMap::new(),
        funcs: HashMap::new(),
        globals: HashMap::new(),
        error_sets: HashMap::new(),
        diags: Vec::new(),
    };
    checker.collect(program);
    checker.check_program(program);
    checker.diags
}

struct Checker {
    types: HashMap<String, TypeInfo>,
    funcs: HashMap<String, Vec<Vec<Type>>>, // 函数名 → 参数类型列表
    globals: HashMap<String, Type>,
    /// 错误集：const 名 → 成员（M2.6）
    error_sets: HashMap<String, ErrorSet>,
    diags: Vec<Diagnostic>,
}

impl Checker {
    /// 收集类型/函数/错误集元数据（第一遍）
    fn collect(&mut self, program: &Program) {
        for d in &program.decls {
            self.collect_decl(d);
        }
    }

    fn collect_decl(&mut self, d: &Decl) {
        match d {
            Decl::Class {
                name,
                traits,
                fields,
                methods,
                ..
            } => {
                let continuous = traits.iter().any(|t| matches!(t, Trait::Continuous));
                self.types.insert(
                    name.clone(),
                    TypeInfo {
                        kind: TypeKind::Class {
                            fields: fields.clone(),
                        },
                        continuous,
                    },
                );
                for m in methods {
                    let params = m.params.iter().map(|p| p.ty.clone()).collect();
                    self.funcs
                        .entry(format!("{name}.{}", m.name))
                        .or_default()
                        .push(params);
                }
            }
            Decl::Enum { name, .. } => {
                self.types.insert(
                    name.clone(),
                    TypeInfo {
                        kind: TypeKind::Enum,
                        continuous: false,
                    },
                );
            }
            Decl::Interface { name, .. } => {
                self.types.insert(
                    name.clone(),
                    TypeInfo {
                        kind: TypeKind::Interface,
                        continuous: false,
                    },
                );
            }
            Decl::Fn { name, params, .. } => {
                let ps = params.iter().map(|p| p.ty.clone()).collect();
                self.funcs.entry(name.clone()).or_default().push(ps);
            }
            Decl::Global { name, ty, .. } => {
                if let Some(t) = ty {
                    self.globals.insert(name.clone(), t.clone());
                }
            }
            Decl::Const { name, ty, .. } => {
                // 错误集别名：const FileError = error{ NotFound, ... }
                if let Some(Type::Named(tn, _)) = ty {
                    if let Some(rest) = tn.strip_prefix("error_set:") {
                        let members: ErrorSet =
                            rest.split(',').map(|s| s.trim().to_string()).collect();
                        self.error_sets.insert(name.clone(), members);
                    }
                }
            }
            Decl::Namespace { decls, .. } => {
                for inner in decls {
                    self.collect_decl(inner);
                }
            }
            _ => {}
        }
    }

    /// 当前函数返回的错误集约束（Some(集合名)）；None = anyerror/无约束
    fn fn_error_constraint(&self, ret: &Option<Type>) -> Option<String> {
        match ret {
            Some(Type::ErrorUnion(Some(err), _)) => match err.strip() {
                Type::Named(n, _) => Some(n.clone()),
                _ => None,
            },
            Some(Type::ErrorUnion(None, _)) => None, // anyerror：不检查
            _ => None,
        }
    }

    fn check_program(&mut self, program: &Program) {
        for d in &program.decls {
            self.check_decl(d);
        }
    }

    fn check_decl(&mut self, d: &Decl) {
        match d {
            Decl::Fn { body, ret, .. } => {
                let constraint = self.fn_error_constraint(ret);
                self.check_block(body, &mut Vec::new(), constraint);
            }
            Decl::Namespace { decls, .. } => {
                for inner in decls {
                    self.check_decl(inner);
                }
            }
            _ => {}
        }
    }

    /// 作用域链检查；返回是否正常（tag1：收集诊断但继续）
    fn check_block(
        &mut self,
        b: &Block,
        scopes: &mut Vec<HashMap<String, VarInfo>>,
        err_constraint: Option<String>,
    ) {
        scopes.push(HashMap::new());
        for stmt in &b.stmts {
            self.check_stmt(stmt, scopes, err_constraint.clone());
        }
        scopes.pop();
    }

    fn check_stmt(
        &mut self,
        s: &Stmt,
        scopes: &mut Vec<HashMap<String, VarInfo>>,
        err_constraint: Option<String>,
    ) {
        match s {
            Stmt::Block(inner) => self.check_block(inner, scopes, err_constraint),
            Stmt::VarDecl {
                name,
                ty,
                init,
                span,
                ..
            } => {
                let inferred = self.infer_init_type(init.as_ref(), scopes);
                let declared = ty.clone().or(inferred);
                // 宽度检查：var x: u8 = 256
                if let (Some(Type::Named(tn, _)), Some(Expr::IntLit { text, .. })) =
                    (&declared, init)
                {
                    self.check_int_width(tn, text, span);
                }
                // 引用赋值禁止（保守）：var x: 引用类型 = 直接变量复制（非 copy）
                if let Some(init) = init {
                    if let Expr::Ident(src, _) = init {
                        let src_ty = self.lookup_var_ty(src, scopes);
                        let is_ref = match (&src_ty, &declared) {
                            (Some(st), _) => self.type_is_ref(st),
                            (None, Some(dt)) => self.type_is_ref(dt),
                            _ => false,
                        };
                        if is_ref {
                            let _ = ty;
                            self.diags.push(Diagnostic::error(
                                span.clone(),
                                format!(
                                    "cannot assign reference type `{src}` by value; \
                                     use `copy(&{src})` for explicit copy or a pointer"
                                ),
                            ));
                        }
                    }
                }
                // definite assignment（C7）：alloc.init(T) 无参构造 → 跟踪待初始化字段
                let pending = self.alloc_init_pending(init.as_ref());
                if let Some(t) = declared {
                    scopes.last_mut().unwrap().insert(
                        name.clone(),
                        VarInfo {
                            ty: Some(t),
                            pending_fields: pending,
                        },
                    );
                } else {
                    scopes.last_mut().unwrap().insert(
                        name.clone(),
                        VarInfo {
                            ty: None,
                            pending_fields: pending,
                        },
                    );
                }
            }
            Stmt::ConstDecl { name, init, span } => {
                let t = self.infer_init_type(Some(init), scopes);
                scopes.last_mut().unwrap().insert(
                    name.clone(),
                    VarInfo {
                        ty: t,
                        pending_fields: None,
                    },
                );
                let _ = span;
            }
            Stmt::If(ifs) => {
                self.check_block(&ifs.then_b, scopes, err_constraint.clone());
                if let Some(else_b) = &ifs.else_b {
                    self.check_stmt(else_b, scopes, err_constraint);
                }
            }
            Stmt::While(w) => self.check_block(&w.body, scopes, err_constraint),
            Stmt::For(f) => {
                scopes.push(HashMap::new());
                scopes.last_mut().unwrap().insert(
                    f.capture_name.clone(),
                    VarInfo {
                        ty: None,
                        pending_fields: None,
                    },
                );
                self.check_block(&f.body, scopes, err_constraint);
                scopes.pop();
            }
            Stmt::Switch(sw) => {
                for arm in &sw.arms {
                    scopes.push(HashMap::new());
                    if let Some((_, n)) = &arm.capture {
                        scopes.last_mut().unwrap().insert(
                            n.clone(),
                            VarInfo {
                                ty: None,
                                pending_fields: None,
                            },
                        );
                    }
                    self.check_block(&arm.body, scopes, err_constraint.clone());
                    scopes.pop();
                }
            }
            Stmt::Return(e, span) => {
                // M2.6：错误集成员检查——return error.X 必须属于函数返回的错误集
                if let Some(constraint) = &err_constraint {
                    if let Some(Expr::ErrorLit(ename, _)) = e {
                        let members = self.error_sets.get(constraint);
                        match members {
                            Some(set) if set.contains(ename) => {}
                            Some(_) => {
                                self.diags.push(Diagnostic::error(
                                    span.clone(),
                                    format!(
                                        "error `error.{ename}` not in declared error set `{constraint}`"
                                    ),
                                ));
                            }
                            None => {
                                // 错误集未收集到（如内建/别名未解析）——不拦截
                            }
                        }
                    }
                }
                // definite assignment（C7 保守版）：返回未完全初始化的 alloc.init(T) 实例
                if let Some(Expr::Ident(name, _)) = e {
                    let missing = self.missing_fields(name, scopes);
                    if let Some(fields) = missing {
                        if !fields.is_empty() {
                            self.diags.push(Diagnostic::error(
                                span.clone(),
                                format!(
                                    "cannot return partially-initialized `{name}`; \
                                     missing field(s): {}",
                                    fields.iter().cloned().collect::<Vec<_>>().join(", ")
                                ),
                            ));
                        }
                    }
                }
            }
            Stmt::Expr(Expr::Assign { target, .. }) => {
                // 字段赋值 x.field = v → 消除 definite assignment 待初始化字段
                // （第一个 `.` 访问解析为 Dot，链式访问为 Field——两者都处理）
                let (x, field): (Option<&str>, Option<&str>) = match target.as_ref() {
                    Expr::Dot { base, field, .. } | Expr::Field { base, field, .. } => {
                        match base.as_ref() {
                            Expr::Ident(x, _) => (Some(x), Some(field)),
                            _ => (None, None),
                        }
                    }
                    _ => (None, None),
                };
                if let (Some(x), Some(field)) = (x, field) {
                    for s in scopes.iter_mut().rev() {
                        if let Some(info) = s.get_mut(x) {
                            if let Some(pending) = &mut info.pending_fields {
                                pending.remove(field);
                            }
                            break;
                        }
                    }
                }
            }
            Stmt::Expr(Expr::ErrorLit(name, span)) => {
                // 独立 error 字面量不检查（值上下文）
                let _ = (name, span);
            }
            _ => {}
        }
    }

    /// alloc.init(T) 无参构造检测（C7）：返回 T 的待初始化字段集
    fn alloc_init_pending(&self, init: Option<&Expr>) -> Option<std::collections::HashSet<String>> {
        match init {
            Some(Expr::Call { callee, args, .. }) => {
                // callee 形如 alloc.init；args = [Ident(T)]（无字段形态）
                // 注意：`alloc.init` 第一个 `.` 解析为 Dot（parse_primary），非 Field
                if let Expr::Dot { base, field, .. } = callee.as_ref() {
                    if field == "init"
                        && matches!(base.as_ref(), Expr::Ident(b, _) if b == "alloc")
                        && args.len() == 1
                    {
                        if let Expr::Ident(tname, _) = &args[0] {
                            if let Some(TypeInfo {
                                kind: TypeKind::Class { fields },
                                continuous,
                            }) = self.types.get(tname)
                            {
                                if *continuous {
                                    return None; // 连续类型：字面量构造/值语义
                                }
                                return Some(fields.iter().map(|f| f.name.clone()).collect());
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// 变量待初始化字段（无该变量或无待初始化要求 → None）
    fn missing_fields(
        &self,
        name: &str,
        scopes: &[HashMap<String, VarInfo>],
    ) -> Option<std::collections::HashSet<String>> {
        for s in scopes.iter().rev() {
            if let Some(info) = s.get(name) {
                return info.pending_fields.clone();
            }
        }
        None
    }

    /// 推断初始化表达式的静态类型（显式字面量 / NamedLit / 集合）
    fn infer_init_type(
        &self,
        init: Option<&Expr>,
        _scopes: &[HashMap<String, VarInfo>],
    ) -> Option<Type> {
        match init {
            Some(Expr::IntLit { .. }) => Some(Type::Named("i32".into(), vec![])),
            Some(Expr::FloatLit { .. }) => Some(Type::Named("f64".into(), vec![])),
            Some(Expr::BoolLit(..)) => Some(Type::Named("bool".into(), vec![])),
            Some(Expr::StrLit { .. }) => Some(Type::Named("&[u8]".into(), vec![])),
            Some(Expr::ArrayLit(..)) => Some(Type::Named("Vec".into(), vec![])),
            Some(Expr::NamedLit { ty, .. }) => Some(Type::Named(ty.clone(), vec![])),
            _ => None,
        }
    }

    fn lookup_var_ty(&self, name: &str, scopes: &[HashMap<String, VarInfo>]) -> Option<Type> {
        for s in scopes.iter().rev() {
            if let Some(v) = s.get(name) {
                return v.ty.clone();
            }
        }
        self.globals.get(name).cloned()
    }

    /// 类型是否为引用类型（不可值赋值；连续类型/标量除外）
    fn type_is_ref(&self, t: &Type) -> bool {
        match t.strip() {
            Type::Named(n, _) => match self.types.get(n) {
                Some(info) => !info.continuous,
                None => {
                    // 内建集合/String 为引用类型
                    matches!(n.as_str(), "Vec" | "Map" | "Deque" | "String")
                }
            },
            Type::Slice(_, _) | Type::Ptr(_, _) => false, // 指针/切片可复制（指针自由）
            _ => false,
        }
    }

    /// 宽度检查：字面量是否超出目标标量类型范围
    fn check_int_width(&mut self, ty: &str, text: &str, span: &Span) {
        // 去掉后缀（i32/u8 等）与下划线
        let cleaned: String = text
            .chars()
            .take_while(|c| {
                c.is_ascii_digit()
                    || matches!(c, 'x' | 'X' | 'b' | 'B' | 'o' | 'O' | 'a'..='f' | 'A'..='F' | '_')
            })
            .collect();
        let cleaned = cleaned.replace('_', "");
        let (radix, digits) =
            if let Some(r) = cleaned.strip_prefix("0x").or(cleaned.strip_prefix("0X")) {
                (16u32, r)
            } else if let Some(r) = cleaned.strip_prefix("0b").or(cleaned.strip_prefix("0B")) {
                (2u32, r)
            } else if let Some(r) = cleaned.strip_prefix("0o").or(cleaned.strip_prefix("0O")) {
                (8u32, r)
            } else {
                (10u32, cleaned.as_str())
            };
        let Ok(v) = i128::from_str_radix(digits, radix) else {
            return; // 非法字面量由运行时/解析层处理
        };
        let (min, max) = match ty {
            "i8" => (i8::MIN as i128, i8::MAX as i128),
            "i16" => (i16::MIN as i128, i16::MAX as i128),
            "i32" => (i32::MIN as i128, i32::MAX as i128),
            "i64" => (i64::MIN as i128, i64::MAX as i128),
            "i128" => (i128::MIN, i128::MAX),
            "isize" => (isize::MIN as i128, isize::MAX as i128),
            "u8" => (0, u8::MAX as i128),
            "u16" => (0, u16::MAX as i128),
            "u32" => (0, u32::MAX as i128),
            "u64" => (0, u64::MAX as i128),
            "u128" => (0, u128::MAX as i128),
            "usize" => (0, usize::MAX as i128),
            _ => return,
        };
        if v < min || v > max {
            self.diags.push(Diagnostic::error(
                span.clone(),
                format!("integer literal `{text}` out of range for `{ty}` ({v} ∉ [{min}, {max}])"),
            ));
        }
    }
}
