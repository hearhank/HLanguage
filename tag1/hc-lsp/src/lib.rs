//! LSP 语言服务器根模块：服务器入口与公共 API

pub mod compiler;
pub mod completion;
pub mod document;
pub mod lsp;
pub mod project;
pub mod symbol;

// Re-export main types for convenience
pub use compiler::{compile_document, CompileResult};
pub use document::{Document, DocumentManager};
pub use lsp::HcLspServer;
pub use project::ProjectContext;

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
