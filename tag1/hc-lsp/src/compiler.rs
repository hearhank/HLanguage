use hc::{check_semantics, parse_source, Diagnostic, Program};
use tower_lsp::lsp_types::{Diagnostic as LspDiagnostic, DiagnosticSeverity, Position, Range};

/// Result of compiling a document
#[derive(Debug)]
pub struct CompileResult {
    /// The parsed program (if successful)
    pub program: Option<Program>,
    /// All diagnostics (errors and warnings)
    pub diagnostics: Vec<Diagnostic>,
}

/// Compile a single document
///
/// This function:
/// 1. Parses the source code
/// 2. Performs semantic checking
/// 3. Returns the result with all diagnostics
pub fn compile_document(source: &str) -> CompileResult {
    // Parse the source code
    let parse_result = parse_source(source);

    match parse_result {
        Ok(program) => {
            // Parse successful, perform semantic checking
            let diagnostics = check_semantics(&program);

            CompileResult {
                program: Some(program),
                diagnostics,
            }
        }
        Err(parse_diagnostics) => {
            // Parse failed, return parse errors
            CompileResult {
                program: None,
                diagnostics: parse_diagnostics,
            }
        }
    }
}

/// Convert hc::Diagnostic to LSP Diagnostic
///
/// Key conversions:
/// - Severity: Error/Warning/Note → DiagnosticSeverity
/// - Span (1-based) → Range (0-based)
/// - message: String → String
pub fn to_lsp_diagnostic(diag: &Diagnostic) -> LspDiagnostic {
    // Convert severity
    let severity = match diag.severity {
        hc::Severity::Error => DiagnosticSeverity::ERROR,
        hc::Severity::Warning => DiagnosticSeverity::WARNING,
        hc::Severity::Note => DiagnosticSeverity::INFORMATION,
    };

    // Convert span (1-based) to range (0-based)
    // LSP uses 0-based line and character positions
    let start = Position::new(
        diag.span.line.saturating_sub(1),
        diag.span.col.saturating_sub(1),
    );

    // For end position, we need to calculate it from the span
    // Since we only have start line/col, we'll estimate the end position
    // A better approach would be to track end line/col in Span, but for now:
    // - If it's a single-line span, end_col = start_col + (end - start)
    // - For multi-line spans, we'd need more info
    //
    // Simplified: assume single-line, use start position + length
    let end = Position::new(
        diag.span.line.saturating_sub(1),
        diag.span.col.saturating_sub(1) + (diag.span.end - diag.span.start) as u32,
    );

    let range = Range::new(start, end);

    LspDiagnostic::new(
        range,
        Some(severity),
        None,
        None,
        diag.message.clone(),
        None,
        None,
    )
}

#[cfg(test)]
mod tests {
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
        use hc::Span;

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
        use hc::Span;

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
        use hc::{Severity, Span};

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
}
