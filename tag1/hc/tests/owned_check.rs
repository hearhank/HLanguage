//! hc/tests/owned_check.rs
//!
//! 语义测试：`owned` 变量必须匹配 `defer` 或 `move`（2026-08-25）；
//! ADR-0030（2026-08-29）：指针形态转移 + use-after-move 冻结

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

// ---------- ADR-0030（2026-08-29）：指针形态转移 + 冻结 ----------

#[test]
fn adr30_move_addr_of_owned_ok() {
    // move &t：只读拥有转移 → 合法
    let diags = check(
        "fn take(r: owned *String) void {}
         fn test() void {
            var s: owned String = String.from(\"x\", alloc);
            take(move &s);
        }",
    );
    assert!(!diags.iter().any(|d| d.is_error()), "got: {diags:?}");
}

#[test]
fn adr30_move_addr_mut_ok() {
    // move &mut t：可写拥有转移（t 为 mut）→ 合法
    let diags = check(
        "fn take(r: owned *mut String) void {}
         fn test() void {
            var mut s: owned String = String.from(\"x\", alloc);
            take(move &mut s);
        }",
    );
    assert!(!diags.iter().any(|d| d.is_error()), "got: {diags:?}");
}

#[test]
fn adr30_move_alias_requires_mut() {
    // move t 是 move &mut t 的字面别名：非 mut 变量 → 编译错误
    let diags = check(
        "fn take(r: owned *mut String) void {}
         fn test() void {
            var s: owned String = String.from(\"x\", alloc);
            take(move s);
        }",
    );
    assert!(
        has_error_containing(&diags, "not declared `mut`"),
        "expected mut error on alias move, got: {diags:?}"
    );
}

#[test]
fn adr30_move_non_owned_errors() {
    // 未标注 owned 的变量不可 move（ADR-0030 裁决 6）
    let diags = check(
        "fn take(r: owned *String) void {}
         fn test() void {
            var s: String = String.from(\"x\", alloc);
            take(move &s);
        }",
    );
    assert!(
        has_error_containing(&diags, "not declared `owned`"),
        "expected non-owned error, got: {diags:?}"
    );
}

#[test]
fn adr30_use_after_move_errors() {
    // move 后原变量冻结：使用 → 编译错误
    let diags = check(
        "fn take(r: owned *String) void {}
         fn peek(r: *String) void {}
         fn test() void {
            var s: owned String = String.from(\"x\", alloc);
            take(move &s);
            peek(&s);
        }",
    );
    assert!(
        has_error_containing(&diags, "use of moved variable `s`"),
        "expected use-after-move error, got: {diags:?}"
    );
}

#[test]
fn adr30_assign_revives_moved_var() {
    // move 后重新赋值 → 复活，可再次转移
    let diags = check(
        "fn take(r: owned *mut String) void {}
         fn test() void {
            var mut s: owned String = String.from(\"x\", alloc);
            take(move &mut s);
            s = String.from(\"y\", alloc);
            take(move &mut s);
        }",
    );
    assert!(!diags.iter().any(|d| d.is_error()), "got: {diags:?}");
}

#[test]
fn adr30_move_global_rejected_alias() {
    // global 禁止 move（别名形态 move g 同样拒绝）
    let diags = check(
        "fn take(y: owned *mut String) void {}\n         global g: String = String.from(\"x\", alloc);\n         [test] fn t() !void {\n            take(move g);\n        }",
    );
    assert!(
        has_error_containing(&diags, "cannot move global"),
        "expected global move error, got: {diags:?}"
    );
}
