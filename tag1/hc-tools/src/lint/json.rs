//! lint 诊断 JSON 输出

use super::models::LintDiag;

pub fn diags_to_json(diags: &[LintDiag], file: &str) -> String {
    let mut items = Vec::new();
    for d in diags {
        items.push(format!(
            r#"{{"file":"{}","rule":"{}","line":{},"col":{},"message":"{}"}}"#,
            file,
            d.rule.name,
            d.span.line,
            d.span.col,
            d.message.replace('"', "\\\""),
        ));
    }
    format!("[{}]", items.join(",\n"))
}
