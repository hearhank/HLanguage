//! 命令行参数解析辅助：`--mode` / `--dangle`

use std::process::ExitCode;

use super::models::{DangleMode, TestMode};

/// 解析 `--dangle` 取值，非法值报错退出。
pub(crate) fn parse_dangle_mode(v: &str) -> Result<DangleMode, ExitCode> {
    match v {
        "on" => Ok(DangleMode::On),
        "off" => Ok(DangleMode::Off),
        "auto" => Ok(DangleMode::Auto),
        other => {
            eprintln!("error: 未知 --dangle `{other}`（可选 on|off|auto）");
            Err(ExitCode::from(2))
        }
    }
}

/// 解析 `--mode` 取值，非法值报错退出。
pub(crate) fn parse_test_mode(v: &str) -> Result<TestMode, ExitCode> {
    match v {
        "interpret" => Ok(TestMode::Interpret),
        "compile" => Ok(TestMode::Compile),
        other => {
            eprintln!("error: 未知 --mode `{other}`（可选 interpret|compile）");
            Err(ExitCode::from(2))
        }
    }
}

/// C2（ADR-0016）：从参数列表提取 `--dangle=on|off|auto` 标志，返回模式与程序参数起始位置。
/// 不匹配则返回 `Auto` 默认，起始位置不变。
pub(crate) fn extract_dangle(args: &[String], start: usize) -> (DangleMode, usize) {
    for i in start..args.len() {
        if let Some(v) = args[i].strip_prefix("--dangle=") {
            match parse_dangle_mode(v) {
                Ok(m) => return (m, i + 1),
                Err(_) => return (DangleMode::Auto, start),
            }
        }
    }
    (DangleMode::Auto, start)
}
