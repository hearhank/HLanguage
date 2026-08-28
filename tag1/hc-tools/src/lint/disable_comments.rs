//! 禁用注释解析：`// @lint(off rule_name)`

use std::collections::{HashMap, HashSet};

/// 解析源文件中的 `// @lint(off rule_name)` 注释，返回被禁用规则名 → 所在行号集合。
pub(crate) fn parse_lint_off_comments(source: &str) -> HashMap<String, HashSet<usize>> {
    let mut disabled: HashMap<String, HashSet<usize>> = HashMap::new();
    for (i, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("// @lint(off ") {
            if let Some(name) = rest.strip_suffix(')') {
                let name = name.trim();
                disabled.entry(name.to_string()).or_default().insert(i + 1);
            }
        }
    }
    disabled
}

pub(crate) fn is_disabled(
    disabled: &HashMap<String, HashSet<usize>>,
    rule: &str,
    line: usize,
) -> bool {
    disabled
        .get(rule)
        .map_or(false, |lines| lines.contains(&line))
}
