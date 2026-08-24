//! 组 E E1：async fn / await 解析与语义（协作式 Future，ADR-0011/0008）
//!
//! 覆盖：`async fn` 解析（Decl::Fn.is_async）/ `await` 解析（Expr::Await）/
//! async 调用点返回 `Future(R)`（R = 声明返回类型，含错误联合）/
//! await 解包 Future(R)→R / 非 Future 的 await 诊断 / async 缺返回类型诊断。
//! E1 范围 = parse + semantic；运行时语义（协作式 join）留 E2。

use hc::ast::Decl;
use hc::check_semantics;
use hc::lexer::lex;
use hc::parse_source;
use hc::token::TokenKind;

/// 从已解析程序取具名函数（含 async）
fn find_fn<'a>(prog: &'a hc::ast::Program, name: &str) -> &'a Decl {
    for d in &prog.decls {
        if let Decl::Fn { name: n, .. } = d {
            if n == name {
                return d;
            }
        }
    }
    panic!("fn `{name}` 未找到");
}

#[test]
fn lex_async_await_keywords() {
    let toks = lex("async fn f() i32 { return 1; } await fut");
    assert!(toks.iter().any(|t| matches!(&t.kind, TokenKind::KwAsync)));
    assert!(toks.iter().any(|t| matches!(&t.kind, TokenKind::KwAwait)));
}

#[test]
fn parse_async_fn_decl() {
    let prog = parse_source(
        r#"
        async fn async_add(b: *i32, n: i32) i32 {
            return b.* + n;
        }
        fn main() void {}
        "#,
    )
    .unwrap();
    let d = find_fn(&prog, "async_add");
    assert!(
        matches!(d, Decl::Fn { is_async: true, .. }),
        "`async fn` 应解析为 is_async: true"
    );
    // 普通 fn 保持 is_async: false
    let m = find_fn(&prog, "main");
    assert!(matches!(m, Decl::Fn { is_async: false, .. }));
}

#[test]
fn parse_await_expr() {
    let prog = parse_source(
        r#"
        async fn fetch() i32 { return 42; }
        fn main() void {
            var fut: Future<i32> = fetch();
            var r = await fut;
            _ = r;
        }
        "#,
    )
    .unwrap();
    // 语义检查通过且无诊断 = `await fut` 已解析为 Expr::Await 并被定型
    let diags = check_semantics(&prog);
    assert!(
        diags.is_empty(),
        "async/await 程序应无诊断：{:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn semantic_async_call_types_future() {
    // async fn 调用点返回 `Future(R)`：赋给 `Future<i32>` 无诊断（类型精确）
    let prog = parse_source(
        r#"
        async fn fetch() i32 { return 42; }
        fn main() void {
            var fut: Future<i32> = fetch();
            _ = fut;
        }
        "#,
    )
    .unwrap();
    let diags = check_semantics(&prog);
    assert!(
        diags.is_empty(),
        "async 调用应定型为 Future<i32>：{:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn semantic_await_unpacks_future() {
    // await Future(R) → R：赋给 i32 无诊断；R 精确而非 Future 包装
    let prog = parse_source(
        r#"
        async fn fetch() i32 { return 42; }
        fn main() void {
            var fut: Future<i32> = fetch();
            var r: i32 = await fut;
            _ = r;
        }
        "#,
    )
    .unwrap();
    let diags = check_semantics(&prog);
    assert!(
        diags.is_empty(),
        "await 应解包 Future<i32> → i32：{:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn semantic_async_error_union_future() {
    // Q20：R = 完整返回类型含错误联合——`async fn fetch() !String` 调用点 = Future<!String>；
    // `try await fut` → String
    let prog = parse_source(
        r#"
        async fn fetch() !String { return "ok"; }
        fn main() void {
            var fut: Future<!String> = fetch();
            var s: String = try await fut;
            _ = s;
        }
        "#,
    )
    .unwrap();
    let diags = check_semantics(&prog);
    assert!(
        diags.is_empty(),
        "Future<!String> + try await 应无诊断：{:?}",
        diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn semantic_await_non_future_is_error() {
    // await 非 Future 值 → 诊断
    let prog = parse_source(
        r#"
        fn main() void {
            var x: i32 = 5;
            var y = await x;
            _ = y;
        }
        "#,
    )
    .unwrap();
    let diags = check_semantics(&prog);
    let rendered: Vec<String> = diags.iter().map(|d| d.message.as_str().to_string()).collect();
    assert!(
        rendered.iter().any(|s| s.contains("`await` requires a Future value")),
        "await 非 Future 应诊断：{rendered:?}"
    );
}

#[test]
fn semantic_async_missing_ret_is_error() {
    // async fn 必须声明返回类型（调用点需 Future(R) 包装）
    let prog = parse_source(
        r#"
        async fn f() {
        }
        fn main() void {}
        "#,
    )
    .unwrap();
    let diags = check_semantics(&prog);
    let rendered: Vec<String> = diags.iter().map(|d| d.message.as_str().to_string()).collect();
    assert!(
        rendered.iter().any(|s| s.contains("`async fn` must declare a return type")),
        "async fn 缺返回类型应诊断：{rendered:?}"
    );
}
