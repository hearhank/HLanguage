//! hc 工具链 CLI（M7.1：`hc build` / `hc run` / `hc test`——tag1 子集）
//!
//! - `hc run <file.hc>`：脚本模式（tree-walking 解释器，全语言）
//! - `hc run <file.hbc>`：字节码 VM（M3.2，装载 HBC2 字节码复用 IR 语义；全语言）
//! - `hc run --ir <file.hc>`：IR 参考解释器（全语言，与字节码 VM 同语义源；interp == IR）
//! - `hc test [file.hc|dir]`：收集并运行 `test fn`，输出 [PASS]/[FAIL]/[SKIP] + 汇总
//! - `hc build <file.hc>`：原生编译（M3.3 LLVM 后端，emit-.ll + `zig cc`）
//! - `hc check <file.hc>`：仅词法/语法/装载检查
//! - `hc init <name>`：创建新项目骨架（build.zon + main.hc，组 H1）

mod build;
mod buildzon;
mod cli;
mod comptimegen;
mod docgen;
mod fmtgen;
mod fsio;
mod lintgen;
mod package;
mod project;
mod run;
mod scriptgen;
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
