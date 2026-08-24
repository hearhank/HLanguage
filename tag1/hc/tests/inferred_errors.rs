//! hc/tests/inferred_errors.rs

use hc::{check_semantics, inferred_error_sets, parse_source};

fn infer(src: &str) -> hc::InferredErrorSets {
    let program = parse_source(src).unwrap_or_else(|d| panic!("parse: {:?}", d));
    inferred_error_sets(&program)
}

fn has(set: &std::collections::HashSet<String>, name: &str) -> bool {
    set.contains(name)
}

#[test]
fn infer_direct_error_literals() {
    let res = infer(
        r#"
fn read() !i32 {
    if (false) { return error.NotFound; }
    return error.Perm;
}
"#,
    );
    let set = &res.sets["read"];
    assert!(has(set, "NotFound"), "direct set = {:?}", set);
    assert!(has(set, "Perm"));
    assert!(res.incomplete.is_empty());
}

#[test]
fn infer_propagation_try_and_return() {
    // try low() 与 return low() 均将 low 的推断集并入调用方
    let res = infer(
        r#"
fn low() !i32 {
    if (false) { return error.Low; }
    return 1;
}
fn mid() !i32 {
    var x = try low();
    return x;
}
fn wrap() !i32 {
    return low();
}
"#,
    );
    assert!(has(&res.sets["low"], "Low"));
    assert!(has(&res.sets["mid"], "Low"), "try 传播失败: {:?}", res.sets["mid"]);
    assert!(has(&res.sets["wrap"], "Low"), "return 传播失败: {:?}", res.sets["wrap"]);
    assert!(res.incomplete.is_empty());
}

#[test]
fn infer_propagation_from_explicit_set() {
    // !T 函数 try 调用显式 E!T 函数 → 并入其 const 错误集
    let res = infer(
        r#"
const FileError = error{ NotFound, Perm };
fn f() FileError!i32 {
    if (false) { return error.NotFound; }
    return 1;
}
fn g() !i32 {
    var x = try f();
    return x;
}
"#,
    );
    let g = &res.sets["g"];
    assert!(has(g, "NotFound"));
    assert!(has(g, "Perm"));
    // 显式 E!T 不在推断集中
    assert!(!res.sets.contains_key("f"));
    assert!(res.incomplete.is_empty());
}

#[test]
fn infer_skips_explicit_anyerror_and_plain() {
    let res = infer(
        r#"
const E = error{ A };
fn explicit() E!i32 { return error.A; }
fn infered() !i32 { if (false) { return error.B; } return 1; }
fn any() anyerror!i32 { if (false) { return error.C; } return 1; }
fn plain() i32 { return 1; }
"#,
    );
    assert!(res.sets.contains_key("infered"));
    assert!(has(&res.sets["infered"], "B"));
    assert!(!res.sets.contains_key("explicit"));
    assert!(!res.sets.contains_key("any"));
    assert!(!res.sets.contains_key("plain"));
}

#[test]
fn infer_recursive_incomplete_and_warning() {
    let src = r#"
fn countdown(n: i32) !i32 {
    if (n == 0) { return error.Done; }
    var x = try countdown(n - 1);
    return x;
}
"#;
    let program = parse_source(src).unwrap();
    let res = inferred_error_sets(&program);
    assert!(
        res.incomplete.contains(&"countdown".to_string()),
        "递归应标记 incomplete: {:?}",
        res.incomplete
    );
    // 递归 → 退化为 anyerror，不提供有限集
    assert!(!res.sets.contains_key("countdown"));

    // 语义检查：递归 !T → warning（非 error，不阻断 load）
    let diags = check_semantics(&program);
    assert!(
        !diags.iter().any(|d| d.is_error()),
        "递归 !T 不应产生 error: {:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
    assert!(
        diags.iter().any(|d| d.message.contains("annotate explicitly")),
        "应提示显式标注: {:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn infer_mutual_recursion_detected() {
    // 间接递归（a ↔ b）同样无法收集 → 双双 incomplete
    let res = infer(
        r#"
fn a(n: i32) !i32 {
    if (n == 0) { return error.Done; }
    var x = try b(n - 1);
    return x;
}
fn b(n: i32) !i32 {
    if (n == 0) { return error.Done; }
    var x = try a(n - 1);
    return x;
}
"#,
    );
    assert!(res.incomplete.contains(&"a".to_string()));
    assert!(res.incomplete.contains(&"b".to_string()));
    assert!(!res.sets.contains_key("a"));
    assert!(!res.sets.contains_key("b"));
}
