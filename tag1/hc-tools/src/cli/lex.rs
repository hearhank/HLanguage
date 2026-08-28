//! K1：`hc lex <file.hc>`——token 流转储命令

use std::path::Path;
use std::process::ExitCode;

use super::read_source::read_source;
use super::usage::USAGE;

/// K1：`hc lex <file.hc>`——Rust 参考 lexer 输出 token 流。
///
/// 每 token 一行，格式 `{start} {end} {line} {col} {kind:?}`（`kind:?` 为 Rust Debug 形态，
/// 如 `KwFn` / `Ident("main")` / `Str("hi\\n")` / `Char(120)`）。H 版 lexer（stage1/lexer.hc）
/// 输出同一格式，对照测试（hc-tools/tests/k1_lexer.rs）逐行 diff。
pub(crate) fn lex_command(args: &[String]) -> ExitCode {
    let Some(path_str) = args.first() else {
        eprintln!("error: `hc lex` requires a file\n\n{USAGE}");
        return ExitCode::from(2);
    };
    let source = match read_source(Path::new(path_str)) {
        Ok(s) => s,
        Err(code) => return code,
    };
    for tok in hc::lexer::lex(&source) {
        println!(
            "{} {} {} {} {:?}",
            tok.span.start, tok.span.end, tok.span.line, tok.span.col, tok.kind
        );
    }
    ExitCode::SUCCESS
}
