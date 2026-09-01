//! hc-lsp/src/completion/tests.rs

use super::*;

#[test]
fn test_keyword_completions() {
    let engine = CompletionEngine::new();
    let completions = engine.get_keyword_completions("f");

    // Should include "fn", "for", "false"（关键字表 01 §1.2.1）
    assert!(!completions.is_empty());
    assert!(completions.iter().any(|c| c.label == "fn"));
    assert!(completions.iter().any(|c| c.label == "for"));
    assert!(completions.iter().any(|c| c.label == "false"));
}

#[test]
fn test_keyword_completions_empty_prefix() {
    let engine = CompletionEngine::new();
    let completions = engine.get_keyword_completions("");

    // All keywords minus the blocklist (script removed)
    assert_eq!(completions.len(), KEYWORDS.len() - KEYWORD_BLOCKLIST.len());
}

#[test]
fn test_keyword_completions_no_match() {
    let engine = CompletionEngine::new();
    let completions = engine.get_keyword_completions("xyz");

    // Should be empty
    assert!(completions.is_empty());
}

/// 规范对齐（01 §1.2.1）：45 关键字全集——owned/tree/where/script 在列，
/// case/default/self/test 不是关键字，iSize/uSize/f128 不存在；
/// script 在 KEYWORDS 但被补全引擎过滤（块已移除）
#[test]
fn test_keywords_match_spec() {
    assert_eq!(KEYWORDS.len(), 45);
    for kw in [
        "owned", "tree", "where", "extern", "export", "union", "script",
    ] {
        assert!(KEYWORDS.contains(&kw), "missing keyword `{kw}`");
    }
    for kw in [
        "case", "default", "self", "test", "iSize", "uSize", "f128", "let", "using",
    ] {
        assert!(!KEYWORDS.contains(&kw), "non-keyword `{kw}` in list");
    }
    let engine = CompletionEngine::new();
    let all = engine.get_keyword_completions("");
    assert!(
        !all.iter().any(|c| c.label == "script"),
        "script must be filtered"
    );
    assert!(all.iter().any(|c| c.label == "owned"));
}

/// 类型表（03 §3.2）：isize/usize 小写、comptime_* 在列、f128 废弃
#[test]
fn test_types_match_spec() {
    for ty in ["isize", "usize", "comptime_int", "comptime_float", "String"] {
        assert!(TYPES.contains(&ty), "missing type `{ty}`");
    }
    assert!(!TYPES.contains(&"iSize"));
    assert!(!TYPES.contains(&"f128"), "f128 deprecated (F1)");
}

/// `@` 内建全集（13-builtins.md）：21 项， Panic/CompileError/原子/溢出在列
#[test]
fn test_builtins_match_spec() {
    assert_eq!(BUILTINS.len(), 21);
    for b in [
        "@sizeOf",
        "@intCast",
        "@panic",
        "@compileError",
        "@atomicRmw",
        "@addWithOverflow",
    ] {
        assert!(
            BUILTINS.iter().any(|(n, _)| *n == b),
            "missing builtin `{b}`"
        );
    }
}

/// 特性标注（04 §4.9）：test/Inline/Extension/Align
#[test]
fn test_attributes_match_spec() {
    let engine = CompletionEngine::new();
    let completions = engine.get_attribute_completions("");
    assert_eq!(completions.len(), ATTRIBUTES.len());
    for a in ["test", "Inline", "Extension", "Align"] {
        assert!(ATTRIBUTES.iter().any(|(n, _, _)| *n == a));
    }
}

/// 类型位置补全（03 §3.1）：前缀过滤 + builtin 类型在列
#[test]
fn test_type_completions() {
    let engine = CompletionEngine::new();
    let completions = engine.get_type_completions("f");
    assert!(completions.iter().any(|c| c.label == "f64"));
    assert!(completions.iter().all(|c| c.label.starts_with("f")));
}
