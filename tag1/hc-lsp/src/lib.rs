pub mod compiler;
pub mod completion;
pub mod document;
pub mod handlers;
pub mod project;
pub mod server;
pub mod symbol;

// Re-export main types for convenience
pub use compiler::{compile_document, CompileResult};
pub use document::{Document, DocumentManager};
pub use project::ProjectContext;
pub use server::HcLspServer;

/// Run the LSP server over stdin/stdout.
/// Creates a tokio runtime internally, so this is a blocking call.
pub fn run_server() {
    tracing_subscriber::fmt::init();
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async {
        let (service, socket) = tower_lsp::LspService::new(|client| HcLspServer::new(client));
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        tower_lsp::Server::new(stdin, stdout, socket)
            .serve(service)
            .await;
    });
}
