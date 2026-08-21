use hc_lsp::HcLspServer;
use tower_lsp::{LspService, Server};

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Create LSP service
    let (service, socket) = LspService::new(|client| HcLspServer::new(client));

    // Use tokio's async IO
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    // Run the server
    Server::new(stdin, stdout, socket).serve(service).await;
}
