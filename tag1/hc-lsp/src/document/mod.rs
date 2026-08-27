//! LSP 文档管理：文档打开/关闭/变更/保存
//!
//! 定义：结构体：Document, DocumentManager

use std::collections::HashMap;
use tower_lsp::lsp_types::*;

/// Represents a single document managed by the LSP server
#[derive(Debug, Clone)]
pub struct Document {
    /// The URI of the document
    pub uri: Url,
    /// The content of the document
    pub content: String,
    /// The version of the document (incremented on each change)
    pub version: i32,
    /// The language ID of the document
    pub language_id: String,
}

impl Document {
    /// Create a new document
    pub fn new(uri: Url, content: String, version: i32, language_id: String) -> Self {
        Self {
            uri,
            content,
            version,
            language_id,
        }
    }

    /// Update the document content
    pub fn update(&mut self, content: String, version: i32) {
        self.content = content;
        self.version = version;
    }
}

/// Manages all open documents in the workspace
#[derive(Debug, Default)]
pub struct DocumentManager {
    /// Map from document URI to Document
    documents: HashMap<Url, Document>,
}

impl DocumentManager {
    /// Create a new document manager
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
        }
    }

    /// Open a document
    pub fn open(&mut self, uri: Url, content: String, version: i32, language_id: String) {
        let document = Document::new(uri.clone(), content, version, language_id);
        self.documents.insert(uri, document);
    }

    /// Close a document
    pub fn close(&mut self, uri: &Url) -> Option<Document> {
        self.documents.remove(uri)
    }

    /// Update a document
    pub fn update(&mut self, uri: &Url, content: String, version: i32) -> bool {
        if let Some(document) = self.documents.get_mut(uri) {
            document.update(content, version);
            true
        } else {
            false
        }
    }

    /// Get a document by URI
    pub fn get(&self, uri: &Url) -> Option<&Document> {
        self.documents.get(uri)
    }

    /// Get a mutable document by URI
    pub fn get_mut(&mut self, uri: &Url) -> Option<&mut Document> {
        self.documents.get_mut(uri)
    }

    /// Check if a document is open
    pub fn is_open(&self, uri: &Url) -> bool {
        self.documents.contains_key(uri)
    }

    /// Get all open documents
    pub fn all_documents(&self) -> Vec<&Document> {
        self.documents.values().collect()
    }

    /// Get the number of open documents
    pub fn count(&self) -> usize {
        self.documents.len()
    }
}

#[cfg(test)]
mod tests;
