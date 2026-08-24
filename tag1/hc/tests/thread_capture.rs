//! hc/tests/thread_capture.rs

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
    assert!(
        diags.iter().any(|d| d.is_error() && d.message.contains(frag)),
        "expected an error containing `{frag}`, got: {:?}",
        diags
    );
}

#[test]
fn value_capture_escaping_ok() {
    // 值复制捕获（无引用）：线程逃逸安全（根回收运行，无悬垂）
    check_ok(
        r#"
fn add(a: i32, b: i32) i32 { return a + b; }
fn main() {
    var th = spawn(add, 6, 7);
}
"#,
    );
}

#[test]
fn global_ref_capture_escaping_ok() {
    // `&global` 捕获：全局程序期存活，逃逸安全
    check_ok(
        r#"
global g: i32 = 0;
fn touch(x: *i32) i32 { return 0; }
fn main() {
    var th = spawn(touch, &g);
}
"#,
    );
}

#[test]
fn bound_ref_capture_ok() {
    // `&局部` 捕获 + 声明作用域内 join（Q18 绑定）→ 允许
    check_ok(
        r#"
fn touch(x: *i32) i32 { return 0; }
fn main() {
    var v: i32 = 1;
    var th = spawn(touch, &v);
    var r = th.join();
}
"#,
    );
}

#[test]
fn detach_ref_capture_ok() {
    // detach：运行点 = 调用处（局部仍存活）→ 允许引用捕获
    check_ok(
        r#"
fn touch(x: *i32) i32 { return 0; }
fn main() {
    var v: i32 = 1;
    var th = spawn(touch, &v);
    th.detach();
}
"#,
    );
}

#[test]
fn escaping_ref_capture_rejected() {
    // Q18：`&局部` 捕获但未 join → 作用域退出线程逃逸 → 局部悬垂 → 编译错误
    check_err(
        r#"
fn touch(x: *i32) i32 { return 0; }
fn main() {
    var v: i32 = 1;
    var th = spawn(touch, &v);
}
"#,
        "Q18",
    );
}

#[test]
fn escaping_ref_capture_rejected_nested_block() {
    // 嵌套作用域：块退出即提升 → 同样逃逸错误
    check_err(
        r#"
fn touch(x: *i32) i32 { return 0; }
fn main() {
    var v: i32 = 1;
    {
        var th = spawn(touch, &v);
    }
}
"#,
        "Q18",
    );
}

#[test]
fn conditional_join_still_escaping() {
    // 条件体内 join 不保证执行（else 路径逃逸）→ 保守报逃逸错误
    check_err(
        r#"
fn touch(x: *i32) i32 { return 0; }
fn main() {
    var v: i32 = 1;
    var th = spawn(touch, &v);
    if (v > 0) {
        th.join();
    }
}
"#,
        "Q18",
    );
}

#[test]
fn freeze_write_before_join_rejected() {
    // Q19：spawn→join 之间写入被引用捕获目标 → 冻结违例
    check_err(
        r#"
fn touch(x: *i32) i32 { return 0; }
fn main() {
    var v: i32 = 1;
    var th = spawn(touch, &v);
    v = 2;
    var r = th.join();
}
"#,
        "Q19",
    );
}

#[test]
fn write_after_join_ok() {
    // join 闭合冻结窗口：之后写入允许
    check_ok(
        r#"
fn touch(x: *i32) i32 { return 0; }
fn main() {
    var v: i32 = 1;
    var th = spawn(touch, &v);
    var r = th.join();
    v = 2;
}
"#,
    );
}
