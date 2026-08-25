//! hc-lsp/src/compiler/tests.rs

use super::*;

#[test]
fn test_compile_valid_document() {
    let source = r#"
        fn main() {
            var x: i32 = 42;
        }
    "#;

    let result = compile_document(source);

    // Should parse successfully
    assert!(result.program.is_some());

    // Should have no errors (or only warnings)
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "Unexpected errors: {:?}", errors);
}

#[test]
fn test_compile_syntax_error() {
    let source = r#"
        fn main() {
            var x: i32 =
        }
    "#;

    let result = compile_document(source);

    // Should fail to parse
    assert!(result.program.is_none());

    // Should have parse errors
    assert!(!result.diagnostics.is_empty());
}

#[test]
fn test_compile_type_error() {
    let source = r#"
        fn main() {
            var x: i32 = "hello";
        }
    "#;

    let result = compile_document(source);

    // Should parse successfully
    assert!(result.program.is_some());

    // Should have type errors
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(!errors.is_empty(), "Expected type errors");
}

#[test]
fn test_to_lsp_diagnostic_error() {
    use hc::token::Span;

    // Create a diagnostic at line 2, col 5, spanning 3 characters
    let span = Span::new(10, 13, 2, 5);
    let diag = Diagnostic::error(span, "test error");

    let lsp_diag = to_lsp_diagnostic(&diag);

    // Check severity
    assert_eq!(lsp_diag.severity, Some(DiagnosticSeverity::ERROR));

    // Check range (should be 0-based)
    assert_eq!(lsp_diag.range.start.line, 1); // 2 - 1 = 1
    assert_eq!(lsp_diag.range.start.character, 4); // 5 - 1 = 4
    assert_eq!(lsp_diag.range.end.line, 1);
    assert_eq!(lsp_diag.range.end.character, 7); // 4 + (13-10) = 7

    // Check message
    assert_eq!(lsp_diag.message, "test error");
}

#[test]
fn test_to_lsp_diagnostic_warning() {
    use hc::token::Span;

    let span = Span::new(0, 5, 1, 1);
    let diag = Diagnostic::warning(span, "test warning");

    let lsp_diag = to_lsp_diagnostic(&diag);

    assert_eq!(lsp_diag.severity, Some(DiagnosticSeverity::WARNING));
    assert_eq!(lsp_diag.range.start.line, 0);
    assert_eq!(lsp_diag.range.start.character, 0);
    assert_eq!(lsp_diag.message, "test warning");
}

#[test]
fn test_to_lsp_diagnostic_note() {
    use hc::token::Span;
    use hc::Severity;

    let span = Span::new(20, 25, 5, 10);
    let diag = Diagnostic {
        severity: Severity::Note,
        span,
        message: "test note".to_string(),
    };

    let lsp_diag = to_lsp_diagnostic(&diag);

    assert_eq!(lsp_diag.severity, Some(DiagnosticSeverity::INFORMATION));
    assert_eq!(lsp_diag.range.start.line, 4); // 5 - 1
    assert_eq!(lsp_diag.range.start.character, 9); // 10 - 1
    assert_eq!(lsp_diag.message, "test note");
}
