//! comptime 类型函数具体化引擎单元测试（E1.2 组 D；ADR-0012）
//!
//! 覆盖：类型函数判定 / 具体化名 / 类型规范串 / 类型替换 / 体求值（struct → Class、
//! 透传 → Type）/ 错误路径（实参个数、体形态）。

use std::collections::HashMap;

use hc::ast::{Block, Decl, Param, Program, Type};
use hc::comptime::{concrete_name, instantiate, is_type_fn, subst, type_key, Instantiated};
use hc::parse_source;

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
    let args = vec![Type::Named(
        "List".into(),
        vec![t_str()],
    )];
    assert_eq!(
        concrete_name("Wrapper", &args),
        "Wrapper<@List(String)>",
        "嵌套泛型实参按 type_key 展开"
    );
}

#[test]
fn type_key_covers_shapes() {
    assert_eq!(type_key(&t_i32()), "i32");
    assert_eq!(
        type_key(&Type::Array(8, Box::new(t_i32()))),
        "[8]i32"
    );
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
    assert_eq!(subst(&Type::Named("String".into(), vec![]), &bindings), t_str());
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
        subst(&Type::Ptr(Box::new(Type::Named("T".into(), vec![])), false), &bindings),
        Type::Ptr(Box::new(t_i32()), false)
    );
    assert_eq!(
        subst(&Type::Optional(Box::new(Type::Named("T".into(), vec![]))), &bindings),
        Type::Optional(Box::new(t_i32()))
    );
    assert_eq!(
        subst(&Type::Array(4, Box::new(Type::Named("T".into(), vec![]))), &bindings),
        Type::Array(4, Box::new(t_i32()))
    );
}

#[test]
fn instantiate_struct_returns_class() {
    let prog = parse_source("fn Pair(T: type) type { return struct { first: T, second: T }; }")
        .unwrap();
    let (params, _, body) = find_fn(&prog, "Pair");
    let args = vec![t_i32()];
    match instantiate("Pair", params, body, &args).unwrap() {
        Instantiated::Class(Decl::Class {
            name, fields, ..
        }) => {
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
    assert!(err.contains("需要 1 个类型实参"), "错误信息含个数说明：{err}");
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
