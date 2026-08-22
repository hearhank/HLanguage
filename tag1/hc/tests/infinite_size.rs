//! C5-2：无限大小类型语言层拒绝
//!
//! 值内嵌自引用/互递归（无间接层）= 编译错误（报类型名 + 循环链位置）。
//! 合法间接层 = 指针/装箱/堆容器/?T。

use hc::{check_semantics, parse_source};

/// 断言源码通过语义检查（无 error 诊断）
fn check_ok(src: &str) {
    let program = parse_source(src).unwrap_or_else(|d| panic!("parse: {:?}", d));
    let diags = check_semantics(&program);
    let errors: Vec<_> = diags.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "expected no errors, got {}: {:?}",
        errors.len(),
        errors
    );
}

/// 断言源码语义检查报错（消息含 frag）
fn check_err(src: &str, frag: &str) {
    let program = parse_source(src).unwrap_or_else(|d| panic!("parse: {:?}", d));
    let diags = check_semantics(&program);
    let errors: Vec<_> = diags.iter().filter(|d| d.is_error()).collect();
    assert!(
        !errors.is_empty(),
        "expected error containing `{frag}`, but got no errors"
    );
    for e in &errors {
        let msg = format!("{:?}", e);
        if msg.contains(frag) {
            return;
        }
    }
    panic!(
        "expected error containing `{frag}`, got:\n{}",
        errors
            .iter()
            .map(|d| format!("  {:?}", d))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// ---------- 非法：直接自引用 ----------

#[test]
fn direct_self_ref() {
    // class Node 的字段 next 直接引用 Node 自身，无间接层
    check_err(
        r#"
class Node {
    next: Node,
}
"#,
        "infinite size",
    );
}

// ---------- 非法：互递归 ----------

#[test]
fn mutual_recursion() {
    // A 包含 B，B 包含 A（互递归）
    check_err(
        r#"
class A {
    b: B,
}
class B {
    a: A,
}
"#,
        "infinite size",
    );
}

// ---------- 非法：三层互递归 ----------

#[test]
fn three_way_recursion() {
    // A → B → C → A
    check_err(
        r#"
class A {
    b: B,
}
class B {
    c: C,
}
class C {
    a: A,
}
"#,
        "infinite size",
    );
}

// ---------- 合法：指针间接层 ----------

#[test]
fn pointer_indirection() {
    // *T 指针是间接层，固定大小
    check_ok(
        r#"
class Node {
    next: *Node,
}
"#,
    );
}

// ---------- 合法：可变指针间接层 ----------

#[test]
fn mut_ptr_indirection() {
    // *mut T 指针也是间接层
    check_ok(
        r#"
class Node {
    next: *mut Node,
}
"#,
    );
}

// ---------- 合法：集合容器 ----------

#[test]
fn collection_indirection() {
    // Vec<T> 是堆分配，固定大小
    check_ok(
        r#"
class Tree {
    children: Vec<Tree>,
}
"#,
    );
}

// ---------- 合法：可选类型 ----------

#[test]
fn optional_indirection() {
    // ?T 是可选类型，固定大小（类似指针）
    check_ok(
        r#"
class Node {
    next: ?Node,
}
"#,
    );
}

// ---------- 合法：既有递归数据结构（LinkedList） ----------

#[test]
fn linked_list_ok() {
    // 经典链表，使用指针间接层
    check_ok(
        r#"
class LinkedList {
    head: *Node,
}
class Node {
    value: i32,
    next: *Node,
}
"#,
    );
}

// ---------- 合法：嵌套集合 ----------

#[test]
fn nested_collection_ok() {
    // Vec<Vec<T>> 多层堆分配
    check_ok(
        r#"
class Matrix {
    rows: Vec<Vec<i32>>,
}
"#,
    );
}

// ---------- 合法：非自引用类 ----------

#[test]
fn non_recursive_ok() {
    // 普通类，无自引用
    check_ok(
        r#"
class Point {
    x: i32,
    y: i32,
}
class Line {
    start: Point,
    end: Point,
}
"#,
    );
}

// ---------- 非法：数组嵌套自引用 ----------

#[test]
fn array_self_ref() {
    // 定长数组 [N]T 直接嵌入值，所以 [N]Node 也嵌入 Node
    check_err(
        r#"
class Node {
    children: [2]Node,
}
"#,
        "infinite size",
    );
}

// ---------- 合法：数组通过指针间接层 ----------

#[test]
fn array_with_ptr_ok() {
    // 指针间接层打破循环
    check_ok(
        r#"
class Node {
    children: [2]*Node,
}
"#,
    );
}

// ---------- 合法：多个指针字段 ----------

#[test]
fn multi_ptr_fields_ok() {
    // 多个指针字段，都安全
    check_ok(
        r#"
class TreeNode {
    left: *TreeNode,
    right: *TreeNode,
    value: i32,
}
"#,
    );
}

// ---------- 非法：类本身无循环但字段类型有循环 ----------

#[test]
fn field_type_has_cycle() {
    // Container 本身无循环，但字段类型 Inner 有自引用
    check_err(
        r#"
class Container {
    inner: Inner,
}
class Inner {
    self_ref: Inner,
}
"#,
        "infinite size",
    );
}

