use hc::token::Span;

use super::lint_rule::LintRule;

#[derive(Debug, Clone)]
pub struct LintDiag {
    pub rule: &'static LintRule,
    pub span: Span,
    pub message: String,
    pub fix: Option<String>,
}

impl LintDiag {
    pub fn render(&self, _source: &str) -> String {
        format!(
            "{}:{}:{}: [{}] {} ({})",
            self.rule.code,
            self.span.line,
            self.span.col,
            self.rule.name,
            self.message,
            self.rule.desc,
        )
    }
}
