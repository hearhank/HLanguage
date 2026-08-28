//! 共享 IR 模块根：线性指令集、IrModule 结构与执行引擎
//!
//! 类型定义已按 ADR-0028 拆分至 models/（一个类型一个文件，文件名 = 类型名
//! snake_case），经 `pub use models::*` 重导出保持原路径；功能函数按功能分组拆分至
//! value_ops/pattern/iter/access/eq 子模块。本文件保留：模块声明、重导出、
//! `MAX_CALL_DEPTH` 与入口函数 `run_ir`。

use self::comptime::Instantiated;
use crate::ast::*;
use crate::lexer::token::Span;
use crate::runtime::errorcodes::ErrorCodeTable;
use crate::runtime::regex::{parse_regex, RegexMatcher};
use crate::runtime::rng::xorshift64;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Seek, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

// ---------- 子模块 ----------
mod builtin;
pub mod comptime;
mod json;
mod lower_impl;
mod method;
mod models;
mod ops;
mod runtime;
pub mod string;
mod types;

pub use self::builtin::*;
pub use self::json::*;
pub use self::lower_impl::*;
pub use self::method::*;
pub use self::ops::*;
pub use self::runtime::*;
pub use self::string::*;
pub use self::types::*;
pub use models::*;

// ---------- 功能函数模块（ADR-0028：按功能分组） ----------
mod access;
mod eq;
mod iter;
mod pattern;
mod value_ops;
pub(crate) use self::access::*;
pub(crate) use self::eq::*;
pub(crate) use self::iter::*;
pub(crate) use self::pattern::*;
pub(crate) use self::value_ops::*;

/// 递归深度上限（对齐 tree-walking `MAX_CALL_DEPTH`——双模式一致）
pub const MAX_CALL_DEPTH: usize = 1000;

/// 一次性执行模块中名为 entry 的函数（测试/入口）——建独立 [`IrRuntime`]（含全局初始化）。
pub fn run_ir(module: &IrModule, entry: &str, args: &[IrValue]) -> R<IrValue> {
    let mut rt = IrRuntime::new();
    rt.call(module, entry, args)
}
