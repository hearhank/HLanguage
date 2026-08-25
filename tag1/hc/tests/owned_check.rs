//! hc/tests/owned_check.rs
//!
//! 语义测试：`owned` 变量必须匹配 `defer` 或 `move`

use hc::check_semantics;
use hc::parse_source;

/// 辅助函数：解析源码并运行语义检查，返回诊断列表
fn check(src: &str) -> Vec<hc::Diagnostic> {
    let program = parse_source(src).expect("parse should succeed");
    check_semantics(&program)
}

/// 检查是否包含指定消息片段的 warning
fn has_warning_containing(diags: &[hc::Diagnostic], fragment: &str) -> bool {
    diags
        .iter()
        .any(|d| d.severity == hc::Severity::Warning && d.message.contains(fragment))
}

/// 检查是否包含指定消息片段的 error
fn has_error_containing(diags: &[hc::Diagnostic], fragment: &str) -> bool {
    diags
        .iter()
        .any(|d| d.severity == hc::Severity::Error && d.message.contains(fragment))
}

#[test]
fn owned_var_without_defer_or_move_warns() {
    let diags = check(
        "class Res { data: *mut i32, }
         fn test() void {
            var r: owned Res = null;
        }",
    );
    assert!(
        has_warning_containing(&diags, "owned"),
        "expected warning about owned variable without defer/move, got: {diags:?}"
    );
}

#[test]
fn owned_var_with_defer_no_warning() {
    let diags = check(
        "class Res { data: *mut i32, }
         fn test() void {
            var r: owned Res = null;
            defer r;
        }",
    );
    assert!(
        !has_warning_containing(&diags, "owned"),
        "expected no warning when owned variable has defer, got: {diags:?}"
    );
}

#[test]
fn non_owned_var_no_warning() {
    let diags = check(
        "fn test() void {
            var x: i32 = 42;
        }",
    );
    assert!(
        !has_warning_containing(&diags, "owned"),
        "expected no warning for non-owned variable, got: {diags:?}"
    );
}

#[test]
fn only_uncovered_owned_var_warns() {
    let diags = check(
        "class Res { data: *mut i32, }
         fn test() void {
            var a: owned Res = null;
            var b: owned Res = null;
            defer a;
        }",
    );
    // `b` is uncovered → warning; `a` is covered by defer → no warning
    let b_warns = has_warning_containing(&diags, "`b`");
    let a_warns = has_warning_containing(&diags, "`a`");
    assert!(
        b_warns && !a_warns,
        "expected warning only for 'b' (uncovered), got: {diags:?}"
    );
}

#[test]
fn owned_var_in_nested_block_covered() {
    let diags = check(
        "class Res { data: *mut i32, }
         fn test() void {
            var x: owned Res = null;
            {
                var y: owned Res = null;
                defer y;
            }
            defer x;
        }",
    );
    assert!(
        !has_warning_containing(&diags, "owned"),
        "expected no warning when both owned vars have defer in correct scopes, got: {diags:?}"
    );
}

#[test]
fn owned_var_in_nested_block_uncovered() {
    let diags = check(
        "class Res { data: *mut i32, }
         fn test() void {
            var x: owned Res = null;
            {
                var y: owned Res = null;
            }
            defer x;
        }",
    );
    let y_warns = has_warning_containing(&diags, "`y`");
    let x_warns = has_warning_containing(&diags, "`x`");
    assert!(
        y_warns && !x_warns,
        "expected warning only for 'y' (uncovered in nested block), got: {diags:?}"
    );
}
