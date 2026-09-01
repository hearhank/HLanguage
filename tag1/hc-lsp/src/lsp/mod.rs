//! LSP 服务器实现：tower-lsp 协议处理、诊断推送、配置管理
//!
//! 定义：结构体：HcLspServer

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
                    trigger_characters: Some(vec![
                        ".".to_string(),
                        "@".to_string(),
                        "[".to_string(),
                    ]),
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
                version: Some("0.2.0".to_string()),
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

        // Build symbol table from source (includes doc comments, signatures)
        let symbol_table = SymbolTable::build_from_source(&content, uri.clone());
        let mut symbols = self.symbols.lock().await;
        symbols.insert(uri.clone(), symbol_table);

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

        // Build symbol table from source (includes doc comments, signatures)
        let symbol_table = SymbolTable::build_from_source(&content, uri.clone());
        let mut symbols = self.symbols.lock().await;
        symbols.insert(uri.clone(), symbol_table);

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

        // Find identifier at cursor position
        let identifier = extract_identifier(&content, position);
        let identifier = match identifier {
            Some(id) => id,
            None => return Ok(None),
        };

        // Search across all symbol tables (cross-file support)
        let symbols = self.symbols.lock().await;
        for (_doc_uri, table) in symbols.iter() {
            if let Some(symbols) = table.find(&identifier) {
                if let Some(symbol) = symbols.first() {
                    return Ok(Some(GotoDefinitionResponse::Scalar(
                        symbol.location.clone(),
                    )));
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

        // Find identifier at cursor position
        let identifier = extract_identifier(&content, position);
        let identifier = match identifier {
            Some(id) => id,
            None => return Ok(None),
        };

        // Search across all symbol tables
        let symbols = self.symbols.lock().await;
        for (_doc_uri, table) in symbols.iter() {
            if let Some(symbols) = table.find(&identifier) {
                if let Some(symbol) = symbols.first() {
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

                    let mut parts = Vec::new();

                    // Name + kind
                    parts.push(format!("**{}** ({})", symbol.name, kind_str));

                    // Signature (if available)
                    if let Some(sig) = &symbol.signature {
                        parts.push(format!("```hc\n{}\n```", sig));
                    }

                    // Type info (if available and different from signature)
                    if let Some(ti) = &symbol.type_info {
                        if symbol.signature.as_deref() != Some(ti.as_str()) {
                            parts.push(format!("_Type_: {}", ti));
                        }
                    }

                    // Location
                    let loc = &symbol.location;
                    parts.push(format!(
                        "Defined at {}:{}:{}",
                        loc.uri.as_str(),
                        loc.range.start.line + 1,
                        loc.range.start.character + 1,
                    ));

                    // Doc comment (if available)
                    if let Some(doc) = &symbol.doc {
                        parts.push(format!("\n---\n{}", doc));
                    }

                    let hover_text = parts.join("\n\n");

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

        // Get symbol tables for all documents (for cross-file completions)
        let symbols = self.symbols.lock().await;
        let all_tables: Vec<&SymbolTable> = symbols.values().collect();
        let current_table = symbols.get(&uri);

        // Check if we're in a dot-qualified context (e.g. "io." or "vec.")
        let line = content.lines().nth(position.line as usize);
        if let Some(line) = line {
            let col = position.character as usize;
            let chars: Vec<char> = line.chars().collect();

            // Find the plain prefix at the cursor (identifier chars only)
            let mut start = col.min(chars.len());
            while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
                start -= 1;
            }
            let prefix: String = chars[start..col.min(chars.len())].iter().collect();

            // `@` context (13-builtins.md): `@na` → builtin completions
            if start > 0 && chars[start - 1] == '@' {
                let completions = self.completion_engine.get_builtin_completions(&prefix);
                return Ok(Some(CompletionResponse::Array(completions)));
            }

            // `[` context (04 §4.9): `[In` → attribute snippets
            if start > 0 && chars[start - 1] == '[' {
                let mut completions = self.completion_engine.get_attribute_completions(&prefix);
                // [test(...) is the dominant use — also allow keywords as fallback
                completions.extend(self.completion_engine.get_keyword_completions(&prefix));
                return Ok(Some(CompletionResponse::Array(completions)));
            }

            // Check for dot before cursor
            if col > 0 && col <= chars.len() && chars[col - 1] == '.' {
                // Find the namespace prefix before the dot
                let mut ns_start = col - 1;
                // Skip the dot
                if ns_start > 0 {
                    ns_start -= 1;
                }
                // Find start of namespace name
                while ns_start > 0
                    && (chars[ns_start - 1].is_alphanumeric() || chars[ns_start - 1] == '_')
                {
                    ns_start -= 1;
                }
                let namespace: String = chars[ns_start..col - 1].iter().collect();

                // Get dot-qualified completions
                let completions = self
                    .completion_engine
                    .get_dot_qualified_completions(&namespace, &all_tables);

                return Ok(Some(CompletionResponse::Array(completions)));
            }

            // Type-position heuristic (03 §3.1): prefix right after ':' or '<'
            // → builtin types first, then keywords
            let mut ws_back = start;
            while ws_back > 0 && chars[ws_back - 1].is_whitespace() {
                ws_back -= 1;
            }
            let in_type_position =
                ws_back > 0 && (chars[ws_back - 1] == ':' || chars[ws_back - 1] == '<');

            // Get completions
            let completions = if in_type_position {
                let mut c = self.completion_engine.get_type_completions(&prefix);
                c.extend(self.completion_engine.get_keyword_completions(&prefix));
                if let Some(table) = current_table {
                    c.extend(
                        self.completion_engine
                            .get_symbol_completions(table, &prefix),
                    );
                }
                c
            } else {
                self.completion_engine
                    .get_completions(current_table, &prefix)
            };

            return Ok(Some(CompletionResponse::Array(completions)));
        }

        Ok(None)
    }
}

/// Extract identifier at cursor position from source content
fn extract_identifier(content: &str, position: Position) -> Option<String> {
    let line = content.lines().nth(position.line as usize)?;
    let col = position.character as usize;
    let chars: Vec<char> = line.chars().collect();

    // Find start of identifier (move left until non-identifier char)
    let mut start = col.min(chars.len());
    while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
        start -= 1;
    }

    // Find end of identifier (move right until non-identifier char)
    let mut end = col.min(chars.len());
    while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
        end += 1;
    }

    if start < end {
        Some(chars[start..end].iter().collect())
    } else {
        None
    }
}
