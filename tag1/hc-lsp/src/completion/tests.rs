//! hc-lsp/src/completion/tests.rs

use super::*;

#[test]
fn test_keyword_completions() {
    let engine = CompletionEngine::new();
    let completions = engine.get_keyword_completions("f");

    // Should include "fn", "for", "f16", "f32", "f64", "f128", "false"
    assert!(!completions.is_empty());
    assert!(completions.iter().any(|c| c.label == "fn"));
    assert!(completions.iter().any(|c| c.label == "for"));
    assert!(completions.iter().any(|c| c.label == "false"));
}

#[test]
fn test_keyword_completions_empty_prefix() {
    let engine = CompletionEngine::new();
    let completions = engine.get_keyword_completions("");

    // Should include all keywords
    assert_eq!(completions.len(), KEYWORDS.len());
}

#[test]
fn test_keyword_completions_no_match() {
    let engine = CompletionEngine::new();
    let completions = engine.get_keyword_completions("xyz");

    // Should be empty
    assert!(completions.is_empty());
}
