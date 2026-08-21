use crate::compiler::{compile_document, to_lsp_diagnostic};
use crate::completion::CompletionEngine;
use crate::document::DocumentManager;
use crate::project::ProjectContext;
use crate::symbol::SymbolTable;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

#[derive(Debug)]
pub struct HcLspServer {
    client: Client,
    documents: Arc<Mutex<DocumentManager>>,
    project: Arc<Mutex<ProjectContext>>,
    /// Symbol tables for each document (URI -> SymbolTable)
    symbols: Arc<Mutex<HashMap<Url, SymbolTable>>>,
    /// Completion engine
    completion_engine: CompletionEngine,
}

impl HcLspServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(Mutex::new(DocumentManager::new())),
            project: Arc::new(Mutex::new(ProjectContext::new())),
            symbols: Arc::new(Mutex::new(HashMap::new())),
            completion_engine: CompletionEngine::new(),
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for HcLspServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Set project root
        if let Some(root_uri) = params.root_uri {
            let mut project = self.project.lock().await;
            project.set_root_uri(root_uri);

            // Try to parse build.zon
            if let Err(e) = project.parse_build_zon() {
                self.client
                    .log_message(
                        MessageType::WARNING,
                        format!("Failed to parse build.zon: {}", e),
                    )
                    .await;
            }
        } else if let Some(root_path) = params.root_path {
            let mut project = self.project.lock().await;
            project.set_root_path(std::path::PathBuf::from(root_path));

            // Try to parse build.zon
            if let Err(e) = project.parse_build_zon() {
                self.client
                    .log_message(
                        MessageType::WARNING,
                        format!("Failed to parse build.zon: {}", e),
                    )
                    .await;
            }
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(true),
                    trigger_characters: Some(vec![".".to_string()]),
                    work_done_progress_options: Default::default(),
                    all_commit_characters: None,
                    completion_item: None,
                }),
                definition_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "hc-lsp".to_string(),
                version: Some("0.1.0".to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "hc-lsp server initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let content = params.text_document.text;
        let version = params.text_document.version;
        let language_id = params.text_document.language_id;

        // Add document to manager
        let mut docs = self.documents.lock().await;
        docs.open(uri.clone(), content.clone(), version, language_id);

        self.client
            .log_message(MessageType::INFO, format!("file opened: {}", uri))
            .await;

        // Drop the lock before compiling
        drop(docs);

        // Compile and publish diagnostics
        let result = compile_document(&content);
        let diagnostics: Vec<_> = result.diagnostics.iter().map(to_lsp_diagnostic).collect();

        // Build symbol table if parsing succeeded
        if let Some(program) = &result.program {
            let symbol_table = SymbolTable::build_from_ast(program, uri.clone());
            let mut symbols = self.symbols.lock().await;
            symbols.insert(uri.clone(), symbol_table);
        }

        self.client
            .publish_diagnostics(uri, diagnostics, Some(version))
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;

        // Update document content and get the content
        let content = if let Some(change) = params.content_changes.first() {
            let mut docs = self.documents.lock().await;
            docs.update(&uri, change.text.clone(), version);

            // Get the updated content
            docs.get(&uri)
                .map(|doc| doc.content.clone())
                .unwrap_or_default()
        } else {
            return;
        };

        self.client
            .log_message(MessageType::INFO, format!("file changed: {}", uri))
            .await;

        // Compile and publish diagnostics
        let result = compile_document(&content);
        let diagnostics: Vec<_> = result.diagnostics.iter().map(to_lsp_diagnostic).collect();

        // Build symbol table if parsing succeeded
        if let Some(program) = &result.program {
            let symbol_table = SymbolTable::build_from_ast(program, uri.clone());
            let mut symbols = self.symbols.lock().await;
            symbols.insert(uri.clone(), symbol_table);
        }

        self.client
            .publish_diagnostics(uri, diagnostics, Some(version))
            .await;
    }

    async fn did_save(&self, _params: DidSaveTextDocumentParams) {
        self.client
            .log_message(MessageType::INFO, "file saved!")
            .await;

        // Get all open documents
        let docs = self.documents.lock().await;
        let all_docs: Vec<_> = docs
            .all_documents()
            .iter()
            .map(|doc| (doc.uri.clone(), doc.content.clone(), doc.version))
            .collect();
        drop(docs);

        // Compile and publish diagnostics for all documents
        for (uri, content, version) in all_docs {
            let result = compile_document(&content);
            let diagnostics: Vec<_> = result.diagnostics.iter().map(to_lsp_diagnostic).collect();

            self.client
                .publish_diagnostics(uri, diagnostics, Some(version))
                .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;

        // Remove document from manager
        let mut docs = self.documents.lock().await;
        docs.close(&uri);

        self.client
            .log_message(MessageType::INFO, format!("file closed: {}", uri))
            .await;
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        // Get document content
        let docs = self.documents.lock().await;
        let content = match docs.get(&uri) {
            Some(doc) => doc.content.clone(),
            None => return Ok(None),
        };
        drop(docs);

        // Get symbol table for this document
        let symbols = self.symbols.lock().await;
        let symbol_table = match symbols.get(&uri) {
            Some(table) => table,
            None => return Ok(None),
        };

        // Find identifier at cursor position
        // Simple approach: extract identifier from the line
        let line = content.lines().nth(position.line as usize);
        if let Some(line) = line {
            // Find identifier at cursor position
            // Simple approach: find word boundaries around cursor
            let col = position.character as usize;
            let chars: Vec<char> = line.chars().collect();

            // Find start of identifier (move left until non-identifier char)
            let mut start = col;
            while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
                start -= 1;
            }

            // Find end of identifier (move right until non-identifier char)
            let mut end = col;
            while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
                end += 1;
            }

            // Extract identifier
            if start < end {
                let identifier: String = chars[start..end].iter().collect();

                // Look up identifier in symbol table
                if let Some(symbols) = symbol_table.find(&identifier) {
                    // Return the first definition (could be improved to handle overloading)
                    if let Some(symbol) = symbols.first() {
                        return Ok(Some(GotoDefinitionResponse::Scalar(
                            symbol.location.clone(),
                        )));
                    }
                }
            }
        }

        Ok(None)
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        // Get document content
        let docs = self.documents.lock().await;
        let content = match docs.get(&uri) {
            Some(doc) => doc.content.clone(),
            None => return Ok(None),
        };
        drop(docs);

        // Get symbol table for this document
        let symbols = self.symbols.lock().await;
        let symbol_table = match symbols.get(&uri) {
            Some(table) => table,
            None => return Ok(None),
        };

        // Find identifier at cursor position
        // Simple approach: extract identifier from the line
        let line = content.lines().nth(position.line as usize);
        if let Some(line) = line {
            // Find identifier at cursor position
            // Simple approach: find word boundaries around cursor
            let col = position.character as usize;
            let chars: Vec<char> = line.chars().collect();

            // Find start of identifier (move left until non-identifier char)
            let mut start = col;
            while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
                start -= 1;
            }

            // Find end of identifier (move right until non-identifier char)
            let mut end = col;
            while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
                end += 1;
            }

            // Extract identifier
            if start < end {
                let identifier: String = chars[start..end].iter().collect();

                // Look up identifier in symbol table
                if let Some(symbols) = symbol_table.find(&identifier) {
                    // Return the first symbol (could be improved to handle overloading)
                    if let Some(symbol) = symbols.first() {
                        // Create hover content
                        let kind_str = match symbol.kind {
                            crate::symbol::SymbolKind::Function => "function",
                            crate::symbol::SymbolKind::Class => "class",
                            crate::symbol::SymbolKind::Enum => "enum",
                            crate::symbol::SymbolKind::Interface => "interface",
                            crate::symbol::SymbolKind::Variable => "variable",
                            crate::symbol::SymbolKind::Constant => "constant",
                            crate::symbol::SymbolKind::Namespace => "namespace",
                            crate::symbol::SymbolKind::Field => "field",
                            crate::symbol::SymbolKind::Method => "method",
                        };

                        let hover_text = format!(
                            "**{}** ({})\n\nDefined at {}:{}:{}",
                            symbol.name,
                            kind_str,
                            symbol.location.uri,
                            symbol.location.range.start.line + 1,
                            symbol.location.range.start.character + 1,
                        );

                        return Ok(Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: hover_text,
                            }),
                            range: None,
                        }));
                    }
                }
            }
        }

        Ok(None)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        // Get document content
        let docs = self.documents.lock().await;
        let content = match docs.get(&uri) {
            Some(doc) => doc.content.clone(),
            None => return Ok(None),
        };
        drop(docs);

        // Get symbol table for this document
        let symbols = self.symbols.lock().await;
        let symbol_table = symbols.get(&uri);

        // Find prefix at cursor position
        // Simple approach: extract identifier prefix from the line
        let line = content.lines().nth(position.line as usize);
        if let Some(line) = line {
            let col = position.character as usize;
            let chars: Vec<char> = line.chars().collect();

            // Find start of identifier (move left until non-identifier char)
            let mut start = col;
            while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
                start -= 1;
            }

            // Extract prefix
            let prefix: String = chars[start..col].iter().collect();

            // Get completions
            let completions = self
                .completion_engine
                .get_completions(symbol_table, &prefix);

            return Ok(Some(CompletionResponse::Array(completions)));
        }

        Ok(None)
    }
}
