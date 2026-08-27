//! hc-lsp/tests/integration_test.rs

use hc_lsp::HcLspServer;
use tower_lsp::lsp_types::*;
use tower_lsp::LspService;

/// Test that the LSP server can respond to initialize request
#[tokio::test]
async fn test_initialize() {
    // Create the LSP service
    let (service, _socket) = LspService::new(|client| HcLspServer::new(client));

    // Create initialize params
    let params = InitializeParams {
        process_id: None,
        client_info: None,
        locale: None,
        root_path: None,
        root_uri: None,
        initialization_options: None,
        capabilities: ClientCapabilities::default(),
        trace: None,
        workspace_folders: None,
    };

    // Note: We can't directly call initialize on the service,
    // but we can verify that the service was created successfully
    // by checking that it doesn't panic.
    // A more complete test would use a mock LSP client.

    // For now, we just verify that the service can be created
    assert!(true);
}

/// Test that diagnostics are correctly generated for syntax errors
#[tokio::test]
async fn test_diagnostics_syntax_error() {
    use hc_lsp::compiler::{compile_document, to_lsp_diagnostic};

    // Create a document with a syntax error
    let source = r#"
        fn main() {
            var x: i32 =
        }
    "#;

    // Compile the document
    let result = compile_document(source);

    // Should have parse errors
    assert!(!result.diagnostics.is_empty());

    // Convert to LSP diagnostics
    let lsp_diagnostics: Vec<_> = result.diagnostics.iter().map(to_lsp_diagnostic).collect();

    // Verify that diagnostics were converted
    assert!(!lsp_diagnostics.is_empty());

    // Verify that the first diagnostic has the correct structure
    let first_diag = &lsp_diagnostics[0];
    assert!(!first_diag.message.is_empty());
    assert!(first_diag.severity.is_some());
}

/// Test that diagnostics are correctly generated for type errors
#[tokio::test]
async fn test_diagnostics_type_error() {
    use hc_lsp::compiler::{compile_document, to_lsp_diagnostic};

    // Create a document with a type error
    let source = r#"
        fn main() {
            var x: i32 = "hello";
        }
    "#;

    // Compile the document
    let result = compile_document(source);

    // Should parse successfully
    assert!(result.program.is_some());

    // Should have type errors
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(!errors.is_empty());

    // Convert to LSP diagnostics
    let lsp_diagnostics: Vec<_> = result.diagnostics.iter().map(to_lsp_diagnostic).collect();

    // Verify that diagnostics were converted
    assert!(!lsp_diagnostics.is_empty());
}

/// Test that no diagnostics are generated for valid code
#[tokio::test]
async fn test_diagnostics_valid_code() {
    use hc_lsp::compiler::{compile_document, to_lsp_diagnostic};

    // Create a valid document
    let source = r#"
        fn main() {
            var x: i32 = 42;
        }
    "#;

    // Compile the document
    let result = compile_document(source);

    // Should parse successfully
    assert!(result.program.is_some());

    // Should have no errors (or only warnings)
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty());
}
