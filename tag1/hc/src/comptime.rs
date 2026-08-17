//! comptime 类型函数具体化（E1.2 组 D；ADR-0012）
//!
//! 类型函数（`fn List(T: type) type`）= 返回 `type` 的编译期函数：实例化（调用点在
//! 类型位置应用 `List(i32)`）= 名字 + 实参列表的具体化（monomorphization）+ 惰性缓存。
//!
//! D1 最小切片（2026-08-18）：类型函数体求值支持 `return struct { name: Type, ... };`
//! 与 `return T;`（透传类型参数）。`comptime { }` 块 / comptime_int / anytype 完整语义
//! 归 D2–D4。三后端共用本模块（interp / IR 各自登记具体化产物到自身类型表）。

use std::collections::HashMap;

use crate::ast;

/// 类型函数判定：**返回类型为 `type`**（元类型，无运行时表示）即触发编译期求值。
/// 注意：参数含 `anytype` 的普通运行时函数（`fn max_value(a: anytype, b: anytype) anytype`）
/// **不是**类型函数——它返回运行时值，按普通函数降级。ADR-0012「参数含 type/anytype 触发
/// 编译期执行」面向 comptime 值函数（D2），本模块只管返回 `type` 的类型函数。
pub fn is_type_fn(params: &[ast::Param], ret: &Option<ast::Type>) -> bool {
    // 返回 `type` = 类型函数
    if let Some(r) = ret {
        if matches!(r.strip(), ast::Type::Named(n, _) if n == "type") {
            return true;
        }
    }
    // 参数含 `T: type` 且返回类型缺失/推断——保守也按类型函数处理（调用方仅对
    // `Type::Named(name, args)` 触发，args 非空才进来）
    let _ = params;
    false
}

/// 具体化名（缓存键）：`Pair(i32)` → `Pair<@i32>`。`<@...>` 不会出现在用户类型名中
/// （标识符不含 `<`/`>`/`@`），保证与手写类型不冲突、可作类型表键。
pub fn concrete_name(name: &str, args: &[ast::Type]) -> String {
    let arg_str: Vec<String> = args.iter().map(|a| type_key(a)).collect();
    format!("{name}<@{}>", arg_str.join(","))
}

/// 类型 → 规范串（具体化名与 `types` 元数据用；`*mut T` / `?T` / `[N]T` 可读）。
pub fn type_key(t: &ast::Type) -> String {
    match t.strip() {
        ast::Type::Named(n, inner) => {
            if inner.is_empty() {
                n.clone()
            } else {
                format!(
                    "{n}({})",
                    inner.iter().map(type_key).collect::<Vec<_>>().join(",")
                )
            }
        }
        ast::Type::Ptr(inner, mut_) => {
            if *mut_ {
                format!("*mut {}", type_key(inner))
            } else {
                format!("*{}", type_key(inner))
            }
        }
        ast::Type::Slice(inner, _) => format!("&[{}]", type_key(inner)),
        ast::Type::Optional(inner) => format!("?{}", type_key(inner)),
        ast::Type::ErrorUnion(_, inner) => format!("{}!", type_key(inner)),
        ast::Type::Tuple(ts) => {
            format!("({})", ts.iter().map(type_key).collect::<Vec<_>>().join(", "))
        }
        ast::Type::Array(n, inner) => format!("[{n}]{}", type_key(inner)),
        ast::Type::ComptimeInt(v) => format!("{v}"),
        ast::Type::Infer => "anytype".to_string(),
        ast::Type::Owned(inner) => type_key(inner),
    }
}

/// 类型替换：把类型参数名（`T`）替换为实参类型。深度遍历，覆盖全部类型形态。
pub fn subst(ty: &ast::Type, bindings: &HashMap<String, ast::Type>) -> ast::Type {
    match ty {
        ast::Type::Named(n, args) if args.is_empty() => {
            if let Some(bound) = bindings.get(n) {
                return bound.clone();
            }
            ast::Type::Named(n.clone(), vec![])
        }
        ast::Type::Named(n, args) => ast::Type::Named(
            n.clone(),
            args.iter().map(|a| subst(a, bindings)).collect(),
        ),
        ast::Type::Ptr(inner, mut_) => {
            ast::Type::Ptr(Box::new(subst(inner, bindings)), *mut_)
        }
        ast::Type::Slice(inner, mut_) => {
            ast::Type::Slice(Box::new(subst(inner, bindings)), *mut_)
        }
        ast::Type::Optional(inner) => ast::Type::Optional(Box::new(subst(inner, bindings))),
        ast::Type::ErrorUnion(e, inner) => ast::Type::ErrorUnion(
            e.as_ref().map(|x| Box::new(subst(x, bindings))),
            Box::new(subst(inner, bindings)),
        ),
        ast::Type::Tuple(ts) => {
            ast::Type::Tuple(ts.iter().map(|t| subst(t, bindings)).collect())
        }
        ast::Type::Array(n, inner) => ast::Type::Array(*n, Box::new(subst(inner, bindings))),
        ast::Type::ComptimeInt(v) => ast::Type::ComptimeInt(*v),
        ast::Type::Infer => ast::Type::Infer,
        ast::Type::Owned(inner) => ast::Type::Owned(Box::new(subst(inner, bindings))),
    }
}

/// 具体化产物
#[derive(Debug)]
pub enum Instantiated {
    /// 透传类型（`return T;` 或 `return SomeType;`）——具体化 = 实参/给定类型本身
    Type(ast::Type),
    /// struct 具体化 → 伪 Class 声明（调用方登记类型表；`name` = 具体化名）
    Class(ast::Decl),
}

/// 对类型函数体做编译期求值：绑定类型实参 → 求值 return 表达式 → 具体化产物。
///
/// D1 支持：`return struct { name: Type, ... };`（字段类型经替换）与 `return T;`
/// （透传实参类型）。其它体形态 → Err（编译错误，带说明）。
pub fn instantiate(
    name: &str,
    params: &[ast::Param],
    body: &ast::Block,
    args: &[ast::Type],
) -> Result<Instantiated, String> {
    // 参数分类：`T: type` = 类型参数；`n: comptime_int` = 编译期整数值参数
    // （ADR-0012：惰性宽度字面量，实例化时按上下文收窄）。实参按全部参数对齐。
    let is_type_param = |p: &ast::Param| matches!(p.ty.strip(), ast::Type::Named(n, _) if n == "type");
    let is_value_param = |p: &ast::Param| {
        matches!(p.ty.strip(), ast::Type::Named(n, _) if n == "comptime_int")
    };
    let type_params: Vec<&ast::Param> = params.iter().filter(|p| is_type_param(p)).collect();
    let value_params: Vec<&ast::Param> = params.iter().filter(|p| is_value_param(p)).collect();
    let total = type_params.len() + value_params.len();
    if total != args.len() {
        return Err(if value_params.is_empty() {
            format!(
                "类型函数 `{name}` 需要 {} 个类型实参，给出 {} 个",
                type_params.len(),
                args.len()
            )
        } else {
            format!(
                "类型函数 `{name}` 需要 {total} 个实参（{} 个类型 + {} 个 comptime_int），给出 {} 个",
                type_params.len(),
                value_params.len(),
                args.len()
            )
        });
    }

    let mut bindings: HashMap<String, ast::Type> = HashMap::new();
    let mut value_bindings: HashMap<String, usize> = HashMap::new();
    let mut ai = 0;
    for p in params {
        if is_type_param(p) {
            // 类型参数须收类型实参（comptime_int 值是编译期整数值，非类型）
            if let ast::Type::ComptimeInt(_) = &args[ai] {
                return Err(format!(
                    "类型函数 `{name}`：类型参数 `{}` 需要类型实参，得 comptime_int 值",
                    p.name
                ));
            }
            bindings.insert(p.name.clone(), args[ai].clone());
            ai += 1;
        } else if is_value_param(p) {
            match &args[ai] {
                ast::Type::ComptimeInt(v) => {
                    value_bindings.insert(p.name.clone(), *v);
                }
                other => {
                    return Err(format!(
                        "类型函数 `{name}`：comptime_int 参数 `{}` 需要整数实参，得 {}",
                        p.name,
                        type_key(other)
                    ));
                }
            }
            ai += 1;
        }
    }

    // 求值 return 表达式（D1：取块内最后一个 return——类型函数体通常为单 return）
    let ret_expr = body
        .stmts
        .iter()
        .rev()
        .find_map(|s| match s {
            ast::Stmt::Return(Some(e), _) => Some(e),
            _ => None,
        })
        .ok_or_else(|| format!("类型函数 `{name}`：体不含 return（类型函数须返回类型）"))?;

    match ret_expr {
        // `return struct { name: Type, ... };` → 具体化 Class 声明
        ast::Expr::StructType { fields, span } => {
            let cname = concrete_name(name, args);
            let fdecls: Vec<ast::FieldDecl> = fields
                .iter()
                .map(|(fname, fty)| ast::FieldDecl {
                    name: fname.clone(),
                    ty: subst(fty, &bindings),
                    pub_: false,
                    span: span.clone(),
                })
                .collect();
            Ok(Instantiated::Class(ast::Decl::Class {
                name: cname,
                ifaces: vec![],
                traits: vec![],
                fields: fdecls,
                methods: vec![],
                pub_: false,
                span: span.clone(),
            }))
        }
        // `return T;`（透传）/ `return i32;`（固定别名）/ `return [n]T;`（数组类型）
        other => {
            let t = eval_type_expr(other, &bindings, &value_bindings)?;
            Ok(Instantiated::Type(t))
        }
    }
}

/// 类型值表达式求值（组 D）：`[n]T` 数组类型 / `T` 透传 / `i32` 固定类型。
/// `struct {...}` 作为顶层 return 单独走 Class 分支（数组元素为 struct 暂不支持）。
fn eval_type_expr(
    e: &ast::Expr,
    bindings: &HashMap<String, ast::Type>,
    value_bindings: &HashMap<String, usize>,
) -> Result<ast::Type, String> {
    match e {
        ast::Expr::ArrayType { len, elem, .. } => {
            let n = eval_array_len(len, value_bindings)?;
            let inner = eval_type_expr(elem, bindings, value_bindings)?;
            Ok(ast::Type::Array(n, Box::new(inner)))
        }
        // `T`（类型参数）→ 实参类型；`i32`（固定名）→ 类型名自身
        ast::Expr::Ident(id, _) => Ok(subst(&ast::Type::Named(id.clone(), vec![]), bindings)),
        other => Err(format!(
            "类型函数：体不支持该形态（得 {}）",
            expr_kind(other)
        )),
    }
}

/// 数组类型长度求值：`3` 字面量（收窄为 usize）/ `n` comptime_int 参数引用。
fn eval_array_len(
    e: &ast::Expr,
    value_bindings: &HashMap<String, usize>,
) -> Result<usize, String> {
    match e {
        ast::Expr::IntLit { text, .. } => text
            .trim_end_matches(|c: char| c.is_alphabetic())
            .replace('_', "")
            .parse::<usize>()
            .map_err(|_| format!("数组类型长度非法：`{text}`")),
        ast::Expr::Ident(name, _) => value_bindings
            .get(name)
            .copied()
            .ok_or_else(|| format!("数组类型长度引用未知的 comptime_int 参数 `{name}`")),
        other => Err(format!(
            "数组类型长度须为整数或 comptime_int 参数引用（得 {}）",
            expr_kind(other)
        )),
    }
}

/// 表达式种类简述（错误信息用）
fn expr_kind(e: &ast::Expr) -> &'static str {
    match e {
        ast::Expr::StructType { .. } => "struct 类型字面量",
        ast::Expr::ArrayType { .. } => "数组类型字面量",
        ast::Expr::Call { .. } => "调用",
        ast::Expr::IfExpr { .. } => "if 表达式",
        ast::Expr::SwitchExpr { .. } => "switch 表达式",
        ast::Expr::Block(..) => "块",
        ast::Expr::NamedLit { .. } => "字面量构造",
        ast::Expr::Binary(..) => "二元运算",
        ast::Expr::Ident(..) => "标识符",
        _ => "表达式",
    }
}
