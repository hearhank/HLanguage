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

/// 检查是否包含指定消息片段的 error
fn has_error_containing(diags: &[hc::Diagnostic], fragment: &str) -> bool {
    diags
        .iter()
        .any(|d| d.severity == hc::Severity::Error && d.message.contains(fragment))
}

#[test]
fn owned_var_without_defer_or_move_errors() {
    let diags = check(
        "class Res { data: *mut i32, }
         fn test() void {
            var r: owned Res = null;
        }",
    );
    assert!(
        has_error_containing(&diags, "owned"),
        "expected error about owned variable without defer/move, got: {diags:?}"
    );
}

#[test]
fn owned_var_with_defer_no_error() {
    let diags = check(
        "class Res { data: *mut i32, }
         fn test() void {
            var r: owned Res = null;
            defer r;
        }",
    );
    assert!(
        !has_error_containing(&diags, "owned"),
        "expected no error when owned variable has defer, got: {diags:?}"
    );
}

#[test]
fn non_owned_var_no_error() {
    let diags = check(
        "fn test() void {
            var x: i32 = 42;
        }",
    );
    assert!(
        !has_error_containing(&diags, "owned"),
        "expected no error for non-owned variable, got: {diags:?}"
    );
}

#[test]
fn only_uncovered_owned_var_errors() {
    let diags = check(
        "class Res { data: *mut i32, }
         fn test() void {
            var a: owned Res = null;
            var b: owned Res = null;
            defer a;
        }",
    );
    // `b` is uncovered → error; `a` is covered by defer → no error
    let b_errs = has_error_containing(&diags, "`b`");
    let a_errs = has_error_containing(&diags, "`a`");
    assert!(
        b_errs && !a_errs,
        "expected error only for 'b' (uncovered), got: {diags:?}"
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
        !has_error_containing(&diags, "owned"),
        "expected no error when both owned vars have defer in correct scopes, got: {diags:?}"
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
    let y_errs = has_error_containing(&diags, "`y`");
    let x_errs = has_error_containing(&diags, "`x`");
    assert!(
        y_errs && !x_errs,
        "expected error only for 'y' (uncovered in nested block), got: {diags:?}"
    );
}
