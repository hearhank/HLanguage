//! 诊断系统：Diagnostic 类型、多错误收集与严重级别
//!
//! 定义：枚举：Severity
//! 定义：结构体：Diagnostic

use crate::token::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub span: Span,
    pub message: String,
}

impl Diagnostic {
    pub fn error(span: Span, message: impl Into<String>) -> Self {
        Self { severity: Severity::Error, span, message: message.into() }
    }
    pub fn warning(span: Span, message: impl Into<String>) -> Self {
        Self { severity: Severity::Warning, span, message: message.into() }
    }
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// 将诊断格式化为带行号的文本（简化：不展开源码行，仅输出位置）。
pub fn render(diags: &[Diagnostic], source: &str) -> String {
    let mut out = String::new();
    for d in diags {
        let sev = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
        };
        out.push_str(&format!(
            "{}:{}:{}: {}\n",
            sev, d.span.line, d.span.col, d.message,
        ));
        // 源码行 + 指示符
        let line_start = source[..d.span.start.min(source.len())]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let line_end = source[line_start..]
            .find('\n')
            .map(|i| line_start + i)
            .unwrap_or(source.len());
        let line = &source[line_start..line_end];
        out.push_str(line);
        out.push('\n');
        let pad = " ".repeat(d.span.col.saturating_sub(1) as usize);
        out.push_str(&pad);
        out.push_str("^\n");
    }
    out
}
