//! L001: unused_var——未使用变量检测

use std::collections::{HashMap, HashSet};

use hc::ast::Program;

use super::collect_decls::collect_decls;
use super::collect_refs::collect_refs;
use super::disable_comments::is_disabled;
use super::models::LintDiag;
use super::rules::find_rule;

pub(crate) fn lint_unused_var(
    program: &Program,
    disabled: &HashMap<String, HashSet<usize>>,
    diags: &mut Vec<LintDiag>,
) {
    let decls = collect_decls(program);
    let refs: HashSet<String> = collect_refs(program).into_iter().collect();
    let rule = find_rule("unused_var").unwrap();

    for (name, span) in &decls {
        // 跳过 `_` 前缀约定（intentionally unused）
        if name.starts_with('_') || name == "_" {
            continue;
        }
        // 跳过函数名（函数名在 Decl::Fn 的 name 中，不在 param 中）
        // 跳过全局变量和常量
        let is_param_or_local = span.start != 0 || span.end != 0;
        if !is_param_or_local {
            continue;
        }
        if !refs.contains(name) {
            if !is_disabled(disabled, "unused_var", span.line as usize) {
                diags.push(LintDiag {
                    rule,
                    span: span.clone(),
                    message: format!("未使用变量 `{name}`"),
                    fix: None,
                });
            }
        }
    }
}
