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
mod tests;
