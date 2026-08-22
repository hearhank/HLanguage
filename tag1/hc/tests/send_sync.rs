//! C3：Send/Sync 编译期诊断测试
//!
//! Send/Sync = 内建标记接口（编译器内建实现，不可自定义）。
//! 组合性验证：标量/值类型自动 Send+Sync、指针/切片看指向、内建容器看元素。
//! 用户 `class Foo: Send` 字段全满足才合法。

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

// ---------- 标量类型自动 Send/Sync ----------

#[test]
fn scalars_send_sync() {
    // 标量类型声明 Send/Sync 类应通过编译
    check_ok(
        r#"
class ScalarHolder: Send, Sync {
    x: i32,
    y: f64,
    z: bool,
    c: u8,
}
"#,
    );
}

// ---------- 集合类型 Send/Sync 依赖于元素类型 ----------

#[test]
fn collection_send_element() {
    // Vec<i32> 的元素 i32 是 Send → Vec<i32> 是 Send
    check_ok(
        r#"
class SendVec: Send {
    items: Vec<i32>,
}
"#,
    );
}

#[test]
fn collection_sync_element() {
    // Vec<i32> 的元素 i32 是 Sync → Vec<i32> 是 Sync
    check_ok(
        r#"
class SyncVec: Sync {
    items: Vec<i32>,
}
"#,
    );
}

// ---------- 指针类型 ----------

#[test]
fn immut_ptr_sync() {
    // *T 只读指针是 Sync（共享读安全）
    check_ok(
        r#"
class SyncPtr: Sync {
    data: *i32,
}
"#,
    );
}

#[test]
fn immut_ptr_send() {
    // *T 只读指针是 Send（指向类型是 Send 即可）
    check_ok(
        r#"
class SendPtr: Send {
    data: *i32,
}
"#,
    );
}

#[test]
fn mut_ptr_not_sync() {
    // *mut T 可变指针不是 Sync
    check_err(
        r#"
class NotSync: Sync {
    data: *mut i32,
}
"#,
        "Sync",
    );
}

#[test]
fn mut_ptr_is_send() {
    // *mut T 是 Send（指向类型是 Send 即可）
    check_ok(
        r#"
class SendMutPtr: Send {
    data: *mut i32,
}
"#,
    );
}

// ---------- 嵌套类型传递 ----------

#[test]
fn nested_send_chain() {
    // 嵌套 Vec<Vec<i32>> 中所有元素都是 Send → 传递
    check_ok(
        r#"
class NestedSend: Send {
    matrix: Vec<Vec<i32>>,
}
"#,
    );
}

// ---------- 具有非 Send 字段的类 ----------

#[test]
fn class_without_send_interface_ok() {
    // 未声明 Send 的类不检查字段
    check_ok(
        r#"
class NonSend {
    data: *mut i32,
}
"#,
    );
}

// ---------- 切片类型 ----------

#[test]
fn slice_send_sync() {
    // &[i32] 切片是 Send 和 Sync
    check_ok(
        r#"
class SliceSend: Send, Sync {
    data: &[i32],
}
"#,
    );
}

// ---------- 可选类型 ----------

#[test]
fn optional_send() {
    // ?i32 可选是 Send
    check_ok(
        r#"
class OptionalSend: Send {
    val: ?i32,
}
"#,
    );
}
