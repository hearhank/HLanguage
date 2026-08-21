//! comptime 类型函数具体化引擎单元测试（E1.2 组 D；ADR-0012）
//!
//! 覆盖：类型函数判定 / 具体化名 / 类型规范串 / 类型替换 / 体求值（struct → Class、
//! 透传 → Type）/ 错误路径（实参个数、体形态）。

use std::collections::HashMap;

use hc::ast::{Block, Decl, Expr, Param, Program, Stmt, Type};
use hc::check_semantics;
use hc::comptime::{
    concrete_name, expr_to_type, has_anytype, instantiate, is_comptime_value_fn, is_type_fn,
    map_type_apps, subst, type_key, Instantiated,
};
use hc::parse_source;
use hc::token::Span;

/// 从已解析程序取具名函数的三要素（params, ret, body）
fn find_fn<'a>(prog: &'a Program, name: &str) -> (&'a [Param], &'a Option<Type>, &'a Block) {
    for d in &prog.decls {
        if let Decl::Fn {
            name: n,
            params,
            ret,
            body,
            ..
        } = d
        {
            if n == name {
                return (params, ret, body);
            }
        }
    }
    panic!("fn `{name}` 未找到");
}

fn t_i32() -> Type {
    Type::Named("i32".into(), vec![])
}

fn t_str() -> Type {
    Type::Named("String".into(), vec![])
}

fn t_f64() -> Type {
    Type::Named("f64".into(), vec![])
}

#[test]
fn is_type_fn_returns_type_only() {
    let prog = parse_source(
        r#"
        fn Pair(T: type) type { return struct { first: T, second: T }; }
        fn max_value(a: anytype, b: anytype) anytype { return a; }
        fn normal(x: i32) i32 { return x; }
        "#,
    )
    .unwrap();
    let (p_params, p_ret, _) = find_fn(&prog, "Pair");
    assert!(is_type_fn(p_params, p_ret), "返回 `type` 应为类型函数");

    let (m_params, m_ret, _) = find_fn(&prog, "max_value");
    assert!(
        !is_type_fn(m_params, m_ret),
        "返回 `anytype` 的普通运行时函数不是类型函数"
    );

    let (n_params, n_ret, _) = find_fn(&prog, "normal");
    assert!(!is_type_fn(n_params, n_ret));
}

#[test]
fn has_anytype_detects_infer_params() {
    let prog = parse_source(
        r#"
        fn max_value(a: anytype, b: anytype) anytype { return a; }
        fn Pair(T: type) type { return struct { a: T }; }
        fn normal(x: i32) i32 { return x; }
        "#,
    )
    .unwrap();
    let (m_params, _, _) = find_fn(&prog, "max_value");
    assert!(has_anytype(m_params), "anytype 参数应被识别");
    let (p_params, _, _) = find_fn(&prog, "Pair");
    assert!(!has_anytype(p_params), "类型参数不是 anytype");
    let (n_params, _, _) = find_fn(&prog, "normal");
    assert!(!has_anytype(n_params), "普通参数不是 anytype");
}

#[test]
fn is_comptime_value_fn_detects_type_params_but_not_type_fns() {
    // D4c：参数含 `T: type` 且非返回 `type` 的普通函数 = comptime 值函数（调用点编译期求值）
    let prog = parse_source(
        r#"
        fn array_len(T: type) comptime_int { return 4; }
        fn make(T: type, n: comptime_int) comptime_int { return n; }
        fn Pair(T: type) type { return struct { a: T }; }
        fn max_value(a: anytype, b: anytype) anytype { return a; }
        fn normal(x: i32) i32 { return x; }
        "#,
    )
    .unwrap();
    let (a_params, a_ret, _) = find_fn(&prog, "array_len");
    assert!(
        is_comptime_value_fn(a_params, a_ret),
        "含 `T: type` 且返回 comptime_int 应为 comptime 值函数"
    );
    let (m_params, m_ret, _) = find_fn(&prog, "make");
    assert!(
        is_comptime_value_fn(m_params, m_ret),
        "混合类型参数 + 值参数也应为 comptime 值函数"
    );
    let (p_params, p_ret, _) = find_fn(&prog, "Pair");
    assert!(
        !is_comptime_value_fn(p_params, p_ret),
        "返回 `type` 的类型函数不是 comptime 值函数（归 D1 is_type_fn）"
    );
    let (v_params, v_ret, _) = find_fn(&prog, "max_value");
    assert!(
        !is_comptime_value_fn(v_params, v_ret),
        "anytype 普通运行时函数（D4b）不是 comptime 值函数"
    );
    let (n_params, n_ret, _) = find_fn(&prog, "normal");
    assert!(!is_comptime_value_fn(n_params, n_ret));
}

#[test]
fn expr_to_type_converts_type_exprs_only() {
    // D4c：调用点实参表达式 → 类型（`i32` → Named；`Vec(i32)` → 嵌套应用；值 → None）
    let sp = Span::new(0, 0, 0, 0);
    assert_eq!(
        expr_to_type(&Expr::Ident("i32".into(), sp.clone())),
        Some(Type::Named("i32".into(), vec![]))
    );
    let nested = Expr::Call {
        callee: Box::new(Expr::Ident("Vec".into(), sp.clone())),
        args: vec![Expr::Ident("i32".into(), sp.clone())],
        span: sp.clone(),
    };
    assert_eq!(
        expr_to_type(&nested),
        Some(Type::Named(
            "Vec".into(),
            vec![Type::Named("i32".into(), vec![])]
        ))
    );
    // 值形态（整数字面量 / 算术）→ None
    assert_eq!(
        expr_to_type(&Expr::IntLit {
            text: "3".into(),
            span: sp.clone()
        }),
        None
    );
    let arith = Expr::Binary(
        hc::ast::BinOp::Add,
        Box::new(Expr::Ident("x".into(), sp.clone())),
        Box::new(Expr::IntLit {
            text: "1".into(),
            span: sp.clone(),
        }),
        sp,
    );
    assert_eq!(expr_to_type(&arith), None);
}

#[test]
fn concrete_name_anytype_instance() {
    // anytype 调用点具体化键：`max_value(i32, i32)` → `max_value<@i32,i32>`
    // （对齐类型函数 `Pair(i32)` → `Pair<@i32>`；ADR-0012 #5 调用点按实参类型实例化）
    assert_eq!(
        concrete_name("max_value", &[t_i32(), t_i32()]),
        "max_value<@i32,i32>"
    );
    assert_eq!(
        concrete_name("max_value", &[t_f64(), t_f64()]),
        "max_value<@f64,f64>"
    );
    assert_eq!(
        concrete_name("pick", &[t_i32(), t_str()]),
        "pick<@i32,String>",
        "异构实参也按类型组合具体化"
    );
}

#[test]
fn concrete_name_single_and_multi() {
    assert_eq!(
        concrete_name("Pair", &[t_i32()]),
        "Pair<@i32>",
        "单类型实参"
    );
    assert_eq!(
        concrete_name("KV", &[t_i32(), t_str()]),
        "KV<@i32,String>",
        "多类型实参（逗号分隔）"
    );
}

#[test]
fn concrete_name_nested_args() {
    let args = vec![Type::Named("List".into(), vec![t_str()])];
    assert_eq!(
        concrete_name("Wrapper", &args),
        "Wrapper<@List<String>>",
        "嵌套泛型实参按 type_key 展开"
    );
}

#[test]
fn type_key_covers_shapes() {
    assert_eq!(type_key(&t_i32()), "i32");
    assert_eq!(type_key(&Type::Array(8, Box::new(t_i32()))), "[8]i32");
    assert_eq!(type_key(&Type::Ptr(Box::new(t_i32()), false)), "*i32");
    assert_eq!(type_key(&Type::Ptr(Box::new(t_i32()), true)), "*mut i32");
    assert_eq!(type_key(&Type::Slice(Box::new(t_i32()), false)), "&[i32]");
    assert_eq!(type_key(&Type::Optional(Box::new(t_i32()))), "?i32");
    assert_eq!(type_key(&Type::Infer), "anytype");
}

#[test]
fn subst_replaces_type_params() {
    let bindings = HashMap::from([("T".to_string(), t_i32())]);
    // 裸类型参数 → 实参
    assert_eq!(subst(&Type::Named("T".into(), vec![]), &bindings), t_i32());
    // 非参数命名类型不受影响
    assert_eq!(
        subst(&Type::Named("String".into(), vec![]), &bindings),
        t_str()
    );
    // 嵌套形态（Vec(T) → Vec(i32)；T 仅在内部替换）
    assert_eq!(
        subst(
            &Type::Named("Vec".into(), vec![Type::Named("T".into(), vec![])]),
            &bindings
        ),
        Type::Named("Vec".into(), vec![t_i32()])
    );
    // 指针 / 可选 / 数组递推
    assert_eq!(
        subst(
            &Type::Ptr(Box::new(Type::Named("T".into(), vec![])), false),
            &bindings
        ),
        Type::Ptr(Box::new(t_i32()), false)
    );
    assert_eq!(
        subst(
            &Type::Optional(Box::new(Type::Named("T".into(), vec![]))),
            &bindings
        ),
        Type::Optional(Box::new(t_i32()))
    );
    assert_eq!(
        subst(
            &Type::Array(4, Box::new(Type::Named("T".into(), vec![]))),
            &bindings
        ),
        Type::Array(4, Box::new(t_i32()))
    );
}

#[test]
fn instantiate_struct_returns_class() {
    let prog =
        parse_source("fn Pair(T: type) type { return struct { first: T, second: T }; }").unwrap();
    let (params, _, body) = find_fn(&prog, "Pair");
    let args = vec![t_i32()];
    match instantiate("Pair", params, body, &args).unwrap() {
        Instantiated::Class(Decl::Class { name, fields, .. }) => {
            assert_eq!(name, "Pair<@i32>");
            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0].name, "first");
            assert_eq!(fields[0].ty, t_i32());
            assert_eq!(fields[1].name, "second");
            assert_eq!(fields[1].ty, t_i32());
        }
        other => panic!("期望 Class 具体化，得 {other:?}"),
    }
}

#[test]
fn instantiate_struct_substs_multiple_params() {
    let prog =
        parse_source("fn Box(T: type, U: type) type { return struct { a: T, b: U }; }").unwrap();
    let (params, _, body) = find_fn(&prog, "Box");
    let args = vec![t_i32(), t_str()];
    match instantiate("Box", params, body, &args).unwrap() {
        Instantiated::Class(Decl::Class { name, fields, .. }) => {
            assert_eq!(name, "Box<@i32,String>");
            assert_eq!(fields[0].ty, t_i32());
            assert_eq!(fields[1].ty, t_str());
        }
        other => panic!("期望 Class 具体化，得 {other:?}"),
    }
}

#[test]
fn instantiate_passthrough_returns_arg_type() {
    let prog = parse_source("fn Identity(T: type) type { return T; }").unwrap();
    let (params, _, body) = find_fn(&prog, "Identity");
    let args = vec![t_i32()];
    match instantiate("Identity", params, body, &args).unwrap() {
        Instantiated::Type(t) => assert_eq!(t, t_i32()),
        other => panic!("期望 Type 透传，得 {other:?}"),
    }
}

#[test]
fn instantiate_arity_mismatch_errors() {
    let prog = parse_source("fn Pair(T: type) type { return struct { a: T }; }").unwrap();
    let (params, _, body) = find_fn(&prog, "Pair");
    let err = instantiate("Pair", params, body, &[t_i32(), t_str()]).unwrap_err();
    assert!(
        err.contains("需要 1 个类型实参"),
        "错误信息含个数说明：{err}"
    );
}

#[test]
fn instantiate_unsupported_body_errors() {
    let prog = parse_source("fn Bad(T: type) type { return T.nope(); }").unwrap();
    let (params, _, body) = find_fn(&prog, "Bad");
    let err = instantiate("Bad", params, body, &[t_i32()]).unwrap_err();
    assert!(err.contains("体不支持该形态"), "错误信息含形态说明：{err}");
}

#[test]
fn instantiate_no_return_errors() {
    let prog = parse_source("fn Bad(T: type) type { var x: i32 = 1; }").unwrap();
    let (params, _, body) = find_fn(&prog, "Bad");
    let err = instantiate("Bad", params, body, &[t_i32()]).unwrap_err();
    assert!(err.contains("不含 return"), "错误信息含 return 说明：{err}");
}

// ---------- 组 D：comptime_int 值参数 + 数组类型函数（示例 35） ----------

fn t_comptime_int(v: usize) -> Type {
    Type::ComptimeInt(v)
}

#[test]
fn type_key_comptime_int() {
    assert_eq!(type_key(&t_comptime_int(3)), "3");
    assert_eq!(
        type_key(&Type::Array(3, Box::new(t_i32()))),
        "[3]i32",
        "数组具体化名 = `[N]T` 规范串"
    );
}

#[test]
fn concrete_name_with_comptime_int() {
    let args = vec![t_i32(), t_comptime_int(3)];
    assert_eq!(
        concrete_name("ArrayLen", &args),
        "ArrayLen<@i32,3>",
        "类型 + comptime_int 混合实参"
    );
}

#[test]
fn instantiate_array_type_returns_array() {
    let prog = parse_source(
        r#"
        fn ArrayLen(T: type, n: comptime_int) type {
            return [n]T;
        }
        "#,
    )
    .unwrap();
    let (params, _, body) = find_fn(&prog, "ArrayLen");
    let args = vec![t_i32(), t_comptime_int(3)];
    match instantiate("ArrayLen", params, body, &args).unwrap() {
        Instantiated::Type(t) => assert_eq!(t, Type::Array(3, Box::new(t_i32()))),
        other => panic!("期望 Type::Array 具体化，得 {other:?}"),
    }
}

#[test]
fn instantiate_array_len_literal() {
    // 长度直接写字面量（`[3]T`），不引用 comptime_int 参数
    let prog = parse_source(
        r#"
        fn Fixed(T: type) type {
            return [3]T;
        }
        "#,
    )
    .unwrap();
    let (params, _, body) = find_fn(&prog, "Fixed");
    match instantiate("Fixed", params, body, &[t_i32()]).unwrap() {
        Instantiated::Type(t) => assert_eq!(t, Type::Array(3, Box::new(t_i32()))),
        other => panic!("期望 Type::Array，得 {other:?}"),
    }
}

#[test]
fn instantiate_array_nested_elem() {
    let prog = parse_source(
        r#"
        fn Grid(T: type, n: comptime_int) type {
            return [n][2]T;
        }
        "#,
    )
    .unwrap();
    let (params, _, body) = find_fn(&prog, "Grid");
    let args = vec![t_i32(), t_comptime_int(2)];
    match instantiate("Grid", params, body, &args).unwrap() {
        Instantiated::Type(t) => assert_eq!(
            t,
            Type::Array(2, Box::new(Type::Array(2, Box::new(t_i32()))))
        ),
        other => panic!("期望嵌套数组，得 {other:?}"),
    }
}

#[test]
fn instantiate_value_arity_mismatch_errors() {
    let prog = parse_source(
        r#"
        fn ArrayLen(T: type, n: comptime_int) type {
            return [n]T;
        }
        "#,
    )
    .unwrap();
    let (params, _, body) = find_fn(&prog, "ArrayLen");
    let err = instantiate("ArrayLen", params, body, &[t_i32()]).unwrap_err();
    assert!(err.contains("需要 2 个实参"), "错误信息含个数：{err}");
}

#[test]
fn instantiate_value_type_mismatch_errors() {
    let prog = parse_source(
        r#"
        fn ArrayLen(T: type, n: comptime_int) type {
            return [n]T;
        }
        "#,
    )
    .unwrap();
    let (params, _, body) = find_fn(&prog, "ArrayLen");
    // comptime_int 参数收到类型实参 → 错误
    let err = instantiate("ArrayLen", params, body, &[t_i32(), t_str()]).unwrap_err();
    assert!(err.contains("需要整数实参"), "错误信息含类型说明：{err}");
}

#[test]
fn instantiate_type_param_gets_comptime_int_errors() {
    let prog = parse_source(
        r#"
        fn ArrayLen(T: type, n: comptime_int) type {
            return [n]T;
        }
        "#,
    )
    .unwrap();
    let (params, _, body) = find_fn(&prog, "ArrayLen");
    // 类型参数收到 comptime_int 值 → 错误（类型参数须收类型实参）
    let args = vec![t_comptime_int(3), t_comptime_int(3)];
    let err = instantiate("ArrayLen", params, body, &args).unwrap_err();
    assert!(err.contains("需要类型实参"), "错误信息含类型说明：{err}");
}

// ---------- 组 D D3：嵌套/递归实例化 ----------

#[test]
fn parser_nested_type_application() {
    // 组 D D3 parser 回归：`L(L(i32))` 嵌套类型实参按 `Named` 树保留
    // （内层先于外层——具体化键由后端在登记期递归计算，见 `map_type_apps`）。
    let prog = parse_source(
        r#"
        fn L(T: type) type { return struct { x: T }; }
        fn main() void { var a: L<L<i32>>; }
        "#,
    )
    .unwrap();
    let nested = Type::Named("L".into(), vec![Type::Named("L".into(), vec![t_i32()])]);
    for d in &prog.decls {
        if let Decl::Fn { name, body, .. } = d {
            if name == "main" {
                let mut found = false;
                for s in &body.stmts {
                    if let Stmt::VarDecl { ty: Some(t), .. } = s {
                        assert_eq!(t, &nested, "嵌套类型实参按 Named 树保留");
                        found = true;
                    }
                }
                assert!(found, "main 内应有 var 声明");
                return;
            }
        }
    }
    panic!("fn main 未找到");
}

#[test]
fn map_type_apps_resolves_nested_apps() {
    // 组 D D3：`map_type_apps` 深度遍历，把嵌套类型函数应用替换为具体化键。
    // 假 resolver 模拟后端 `concrete_type_name` 的预解析实参步骤：内层类型函数
    // 应用先具体化（`Pair(i32)` → `Pair<@i32>`），外层以已解析实参生成键。
    fn fake_resolve(n: &str, args: &[Type]) -> Result<String, String> {
        let mut resolved: Vec<Type> = Vec::new();
        for a in args {
            resolved.push(map_type_apps(a, &mut |n2, a2| {
                Ok(format!(
                    "{n2}<@{}>",
                    a2.iter().map(type_key).collect::<Vec<_>>().join(",")
                ))
            })?);
        }
        Ok(format!(
            "{n}<@{}>",
            resolved.iter().map(type_key).collect::<Vec<_>>().join(",")
        ))
    }

    // `Pair(Pair(i32))` → 外层具体化键 `Pair<@Pair<@i32>>`
    let root = Type::Named(
        "Pair".into(),
        vec![Type::Named("Pair".into(), vec![t_i32()])],
    );
    assert_eq!(
        map_type_apps(&root, &mut fake_resolve).unwrap(),
        Type::Named("Pair<@Pair<@i32>>".into(), vec![]),
        "嵌套类型函数应用 → 深层具体化键"
    );

    // 复合形态递推：`?Pair(i32)` / `[2]Pair(i32)` / `*mut Pair(i32)` / `(Pair(i32), i32)`
    let opt = Type::Optional(Box::new(Type::Named("Pair".into(), vec![t_i32()])));
    assert_eq!(
        map_type_apps(&opt, &mut fake_resolve).unwrap(),
        Type::Optional(Box::new(Type::Named("Pair<@i32>".into(), vec![]))),
        "?T 递推"
    );
    let arr = Type::Array(2, Box::new(Type::Named("Pair".into(), vec![t_i32()])));
    assert_eq!(
        map_type_apps(&arr, &mut fake_resolve).unwrap(),
        Type::Array(2, Box::new(Type::Named("Pair<@i32>".into(), vec![]))),
        "[N]T 递推"
    );
    let ptr = Type::Ptr(Box::new(Type::Named("Pair".into(), vec![t_i32()])), true);
    assert_eq!(
        map_type_apps(&ptr, &mut fake_resolve).unwrap(),
        Type::Ptr(Box::new(Type::Named("Pair<@i32>".into(), vec![])), true),
        "*mut T 递推"
    );
    let tup = Type::Tuple(vec![Type::Named("Pair".into(), vec![t_i32()]), t_i32()]);
    assert_eq!(
        map_type_apps(&tup, &mut fake_resolve).unwrap(),
        Type::Tuple(vec![Type::Named("Pair<@i32>".into(), vec![]), t_i32()]),
        "Tuple 递推"
    );
}

// ---------- 组 D D4：comptime_int 常量折叠（类型层） ----------

#[test]
fn check_semantics_recognizes_comptime_int_type() {
    // `comptime_int` = 惰性宽度整数：`ty_of` 识别为 `Int { width: Comptime }`，
    // 与整数字面量初始化兼容（Comptime 宽度跳过收窄检查）
    let prog = parse_source(
        r#"
        fn main() void {
            var x: comptime_int = 5;
            _ = x;
        }
        "#,
    )
    .unwrap();
    let diags = check_semantics(&prog);
    assert!(
        diags.is_empty(),
        "comptime_int 类型应被识别且无诊断：{:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn check_semantics_rejects_string_assigned_to_comptime_int() {
    // 类型不匹配：`comptime_int = "hello"` → 诊断（compatible 无 (Str, Int) 臂 → false）
    let prog = parse_source(
        r#"
        fn main() void {
            var x: comptime_int = "hello";
            _ = x;
        }
        "#,
    )
    .unwrap();
    let diags = check_semantics(&prog);
    let rendered: Vec<String> = diags
        .iter()
        .map(|d| d.message.as_str().to_string())
        .collect();
    assert!(
        rendered.iter().any(|s| s.contains("cannot assign")),
        "comptime_int 应拒绝 String 初始化：{rendered:?}"
    );
}

#[test]
fn check_semantics_recognizes_comptime_float_type() {
    // `comptime_float` = 惰性宽度浮点（H 浮点单一 f64 表示）：识别为 SType::Float，
    // 与浮点字面量初始化兼容
    let prog = parse_source(
        r#"
        fn main() void {
            var x: comptime_float = 1.5;
            _ = x;
        }
        "#,
    )
    .unwrap();
    let diags = check_semantics(&prog);
    assert!(
        diags.is_empty(),
        "comptime_float 类型应被识别且无诊断：{:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn check_semantics_rejects_string_assigned_to_comptime_float() {
    // 类型不匹配：`comptime_float = "hello"` → 诊断（compatible 无 (Str, Float) 臂 → false）
    let prog = parse_source(
        r#"
        fn main() void {
            var x: comptime_float = "hello";
            _ = x;
        }
        "#,
    )
    .unwrap();
    let diags = check_semantics(&prog);
    let rendered: Vec<String> = diags
        .iter()
        .map(|d| d.message.as_str().to_string())
        .collect();
    assert!(
        rendered.iter().any(|s| s.contains("cannot assign")),
        "comptime_float 应拒绝 String 初始化：{rendered:?}"
    );
}

// ---------- 组 D D4b：anytype 完整语义（调用点具体化） ----------

#[test]
fn semantic_anytype_ret_resolves_concrete() {
    // anytype 调用点具体化：`max_value(2.5, 1.5)` 返回类型解析为 `f64`（非 anytype
    // 通配）——赋给 f64 变量无诊断（具体类型匹配）
    let prog = parse_source(
        r#"
        fn max_value(a: anytype, b: anytype) anytype {
            return if (a > b) a else b;
        }
        fn main() void {
            var m: f64 = max_value(2.5, 1.5);
            _ = m;
        }
        "#,
    )
    .unwrap();
    let diags = check_semantics(&prog);
    assert!(
        diags.is_empty(),
        "anytype 返回类型应解析为 f64：{:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn semantic_anytype_ret_mismatch_is_error() {
    // anytype 具体化后类型精确：`max_value(2.5, 1.5)` = f64，赋给 String → 编译错误。
    // 判别场景：具体化前返回类型为 anytype 通配（`Infer` 与一切兼容 → 静默放行）；
    // int↔float 在本语言 mutual-compatible（compatible 有 (Int, Float) 臂），故用
    // 非数值 String 作目标——只有返回类型具体化为 f64 才会触发诊断。
    let prog = parse_source(
        r#"
        fn max_value(a: anytype, b: anytype) anytype {
            return if (a > b) a else b;
        }
        fn main() void {
            var s: String = max_value(2.5, 1.5);
            _ = s;
        }
        "#,
    )
    .unwrap();
    let diags = check_semantics(&prog);
    let rendered: Vec<String> = diags
        .iter()
        .map(|d| d.message.as_str().to_string())
        .collect();
    assert!(
        rendered.iter().any(|s| s.contains("cannot assign")),
        "f64 结果赋给 String 应报类型不匹配：{rendered:?}"
    );
}

#[test]
fn semantic_anytype_int_args_resolve_comptime_width() {
    // 整型实参：`max_value(3, 7)` 返回惰性宽度整数（comptime_int 宽度）——
    // 赋给 i32 变量按上下文收窄，无诊断
    let prog = parse_source(
        r#"
        fn max_value(a: anytype, b: anytype) anytype {
            return if (a > b) a else b;
        }
        fn main() void {
            var n: i32 = max_value(3, 7);
            _ = n;
        }
        "#,
    )
    .unwrap();
    let diags = check_semantics(&prog);
    assert!(
        diags.is_empty(),
        "comptime 宽度结果赋给 i32 应无诊断：{:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}
