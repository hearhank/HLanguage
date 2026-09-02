//! L004: upper_case_abbr——缩写应全大写检测

use std::collections::{HashMap, HashSet};

use hc::ast::{Decl, Program};
use hc::token::Span;

use super::disable_comments::is_disabled;
use super::models::{LintDiag, LintRule};
use super::rules::find_rule;

/// 常见应全大写的缩写
const ABBREVIATIONS: &[&str] = &[
    // 不含 id（恒等函数/后缀通用词）、utf8/utf16/utf32（带数字编码名通行小写）
    "json", "html", "http", "https", "url", "uri", "db", "csv", "xml", "yaml", "toml", "ini", "ssh",
    "ftp", "smtp", "imap", "pop", "tcp", "udp", "ip", "dns", "dhcp", "ssl", "tls", "api", "cli",
    "gui", "ui", "ux", "os", "io", "math", "regex", "uuid", "sha", "md5", "aes", "rsa", "ecdsa",
    "jwt", "oauth", "ldap", "sql", "orm", "rpc", "grpc", "rest", "soap", "html", "css", "png",
    "jpg", "gif", "svg", "pdf", "txt", "rtf", "async", "sync", "mutex", "sem", "fifo", "lifo",
    "mime", "base64", "ascii", "ansi", "ebcdic",
];

/// 检查标识符中是否包含应全大写的缩写。
/// 只在 snake_case 分段上做完全匹配（不区分大小写），避免子串误报：
/// `is_ident_start` 中的 "id"、`utf8_width` 中的 "id"、`dbg_escape` 中的 "db" 均不是缩写。
fn check_abbr(name: &str) -> Vec<(usize, &'static str)> {
    let mut results = Vec::new();
    let mut pos = 0;
    for segment in name.split('_') {
        let seg_lower = segment.to_lowercase();
        if let Some(&abbr) = ABBREVIATIONS.iter().find(|&&a| a == seg_lower) {
            // 检查该段是否确实含小写字符（已是全大写则跳过）
            let actual = &name[pos..pos + segment.len()];
            if actual.chars().any(|c| c.is_lowercase()) {
                results.push((pos, abbr));
            }
        }
        pos += segment.len() + 1;
    }
    results
}

pub(crate) fn lint_upper_case_abbr(
    program: &Program,
    source: &str,
    disabled: &HashMap<String, HashSet<usize>>,
    fix: bool,
    diags: &mut Vec<LintDiag>,
) {
    let rule = find_rule("upper_case_abbr").unwrap();
    for d in &program.decls {
        check_abbr_in_decl(d, source, disabled, fix, rule, diags);
    }
}

fn check_abbr_in_decl(
    decl: &Decl,
    source: &str,
    disabled: &HashMap<String, HashSet<usize>>,
    fix: bool,
    rule: &'static LintRule,
    diags: &mut Vec<LintDiag>,
) {
    let names = match decl {
        Decl::Fn { name, .. } => vec![name.clone()],
        Decl::Class { name, .. } => vec![name.clone()],
        Decl::Struct { name, .. } => vec![name.clone()],
        Decl::Enum { name, .. } => vec![name.clone()],
        Decl::Union { name, .. } => vec![name.clone()],
        Decl::Interface { name, .. } => vec![name.clone()],
        Decl::Namespace { name, .. } => vec![name.clone()],
        Decl::Global { name, .. } | Decl::Const { name, .. } => vec![name.clone()],
        Decl::Import { .. } | Decl::Comptime { .. } | Decl::Include { .. } => Vec::new(),
    };
    for name in names {
        let abbrs = check_abbr(&name);
        for (pos, abbr) in abbrs {
            let upper = abbr.to_uppercase();
            let fixed_name = format!("{}{}{}", &name[..pos], upper, &name[pos + abbr.len()..]);
            if !is_disabled(disabled, "upper_case_abbr", decl_span(decl).line as usize) {
                diags.push(LintDiag {
                    rule,
                    span: decl_span(decl),
                    message: format!("缩写 `{abbr}` 应全大写（`{name}` → `{fixed_name}`）"),
                    fix: if fix { Some(fixed_name) } else { None },
                });
            }
        }
    }
    // 检查命名空间内层
    if let Decl::Namespace { decls: inner, .. } = decl {
        for d in inner {
            check_abbr_in_decl(d, source, disabled, fix, rule, diags);
        }
    }
}

fn decl_span(decl: &Decl) -> Span {
    match decl {
        Decl::Fn { span, .. } => span.clone(),
        Decl::Class { span, .. } => span.clone(),
        Decl::Struct { span, .. } => span.clone(),
        Decl::Enum { span, .. } => span.clone(),
        Decl::Union { span, .. } => span.clone(),
        Decl::Interface { span, .. } => span.clone(),
        Decl::Namespace { span, .. } => span.clone(),
        Decl::Global { span, .. } => span.clone(),
        Decl::Const { span, .. } => span.clone(),
        Decl::Import { span, .. } => span.clone(),
        Decl::Comptime { span, .. } => span.clone(),
        Decl::Include { span, .. } => span.clone(),
    }
}
