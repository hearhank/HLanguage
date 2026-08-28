//! L002: unused_import——未使用导入检测

use std::collections::{HashMap, HashSet};

use hc::ast::Program;

use super::collect_imports::collect_imports;
use super::collect_refs::collect_refs;
use super::disable_comments::is_disabled;
use super::models::LintDiag;
use super::rules::find_rule;

pub(crate) fn lint_unused_import(
    program: &Program,
    disabled: &HashMap<String, HashSet<usize>>,
    diags: &mut Vec<LintDiag>,
) {
    let imports = collect_imports(program);
    let refs: HashSet<String> = collect_refs(program).into_iter().collect();
    let rule = find_rule("unused_import").unwrap();

    for (name, span) in &imports {
        // 跳过 `H.std` 标准库导入（通常隐式使用）
        if name == "std" || name.starts_with('H') {
            continue;
        }
        if !refs.contains(name) {
            if !is_disabled(disabled, "unused_import", span.line as usize) {
                diags.push(LintDiag {
                    rule,
                    span: span.clone(),
                    message: format!("未使用导入 `{name}`"),
                    fix: None,
                });
            }
        }
    }
}
