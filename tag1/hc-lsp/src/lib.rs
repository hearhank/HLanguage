pub mod compiler;
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
