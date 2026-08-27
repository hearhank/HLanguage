//! hc 工具链入口：栈扩容与 CLI 命令分发

mod build;
mod cli;
mod comptime;
mod doc;
mod fmt;
mod lint;
mod pkg;
mod project;
mod run;
mod script;
mod test;

use std::process::ExitCode;

fn main() -> ExitCode {
    // 递归/深层 AST 求值需要更大栈（Windows 主线程默认 1MB；测试线程 8MB+）
    let handle = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(cli::run_cli)
        .expect("spawn worker thread");
    handle.join().unwrap_or(ExitCode::FAILURE)
}

#[cfg(test)]
mod tests;
