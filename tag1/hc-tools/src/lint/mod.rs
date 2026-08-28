//! hc lint 命令：静态检查器（9 规则 + --json 输出）
//!
//! 类型定义在 models/ 下（一个类型一个文件，ADR-0028）；
//! 规则实现与辅助函数按功能拆分到同名功能文件。

mod models;

mod collect_decls;
mod collect_imports;
mod collect_refs;
mod disable_comments;
mod json;
mod redundant_eq_false;
mod rules;
mod simplifiable_construct;
mod simplifiable_if_else;
mod unused_import;
mod unused_var;
mod upper_case_abbr;

pub use models::*;

pub use json::diags_to_json;

// 保留 hc_tools::lint::all_rules / find_rule 公开路径（ADR-0028 拆分前的 pub API）
#[allow(unused_imports)]
pub use rules::{all_rules, find_rule};

use hc::ast::Program;

use disable_comments::parse_lint_off_comments;
use redundant_eq_false::lint_redundant_eq_false;
use simplifiable_construct::lint_simplifiable_construct;
use simplifiable_if_else::lint_simplifiable_if_else;
use unused_import::lint_unused_import;
use unused_var::lint_unused_var;
use upper_case_abbr::lint_upper_case_abbr;

// ---------- 主 lint 函数 ----------

/// 对单个源文件执行 lint 检查
pub fn lint_source(source: &str, program: &Program, fix: bool) -> Vec<LintDiag> {
    let disabled = parse_lint_off_comments(source);
    let mut diags = Vec::new();

    // L001: unused_var
    lint_unused_var(program, &disabled, &mut diags);

    // L002: unused_import
    lint_unused_import(program, &disabled, &mut diags);

    // L003: simplifiable_construct
    lint_simplifiable_construct(program, source, &disabled, fix, &mut diags);

    // L004: upper_case_abbr
    lint_upper_case_abbr(program, source, &disabled, fix, &mut diags);

    // L005: simplifiable_if_else
    lint_simplifiable_if_else(program, source, &disabled, fix, &mut diags);

    // L006: redundant_eq_false
    lint_redundant_eq_false(program, source, &disabled, fix, &mut diags);

    diags
}
